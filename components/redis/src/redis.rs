use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, SchedulerWaker, create_io_channels,
    wake_scheduler, ErrorType, RetryConfig, RetryState,
};
use log::{debug, error, info, trace, warn};

// component-specific
use redis_async::client::connect::RespConnectionInner;
use tokio_util::codec::Framed;
use redis_async::resp::RespCodec;
use std::time::Instant;
use tokio::sync::mpsc as tokio_mpsc;

// ADR-017: Error classification for Redis connections
fn classify_redis_error(error: &str) -> ErrorType {
    // Redis connection errors are generally transient (network issues, server unavailable)
    // Only permanent errors would be invalid configuration or authentication
    if error.contains("invalid") || error.contains("auth") || error.contains("unauthorized") {
        ErrorType::Permanent
    } else {
        ErrorType::Transient
    }
}



enum RedisPublisherState {
    WaitingForConfig,
    Connecting {
        url: String,
        channel: String,
    },
    // ADR-017: Retry states for failed connections
    RetryingConnection {
        url: String,
        channel: String,
        retry_state: RetryState,
    },
    Connected {
        url: String,
        connection: Framed<RespConnectionInner, RespCodec>,
        channel: String,
        pending_messages: Vec<FbpMessage>,
    },
    Finished,
}

pub struct RedisPublisherComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: RedisPublisherState,
    // ADR-017: Bounded IO channels
    cmd_tx: std::sync::mpsc::SyncSender<(String, String)>,
    result_rx: std::sync::mpsc::Receiver<Result<Framed<RespConnectionInner, RespCodec>, String>>,
    // Background async worker
    _async_worker: Option<std::thread::JoinHandle<()>>,
    // ADR-017: Retry configuration and state
    retry_config: RetryConfig,
    retry_state: RetryState,
}

// ADR-017: Async Redis worker that runs in background thread with Tokio runtime
async fn async_redis_worker(
    cmd_rx: std::sync::mpsc::Receiver<(String, String)>,
    result_tx: std::sync::mpsc::SyncSender<Result<Framed<RespConnectionInner, RespCodec>, String>>,
    waker: Option<SchedulerWaker>,
) {
    debug!("Redis async worker started");

    while let Ok((url, channel)) = cmd_rx.recv() {
        debug!("Redis worker connecting to: {} channel: {}", &url, &channel);

        // Perform Redis connection (this is asynchronous)
        let result = redis_async::client::connect(&url, 6379, None, None).await;

        let connection_result = match result {
            Ok(connection) => Ok(connection),
            Err(e) => Err(format!("Failed to connect: {}", e)),
        };

        // Send result back (ignore send errors - component may have been destroyed)
        let _ = result_tx.try_send(connection_result);

        // ADR-002: Signal scheduler that work is ready
        wake_scheduler(&waker);
    }

    debug!("Redis async worker exiting");
}

impl Component for RedisPublisherComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        // ADR-017: Create bounded IO channels
        let (cmd_tx, cmd_rx, result_tx, result_rx) = create_io_channels::<(String, String), Result<Framed<RespConnectionInner, RespCodec>, String>>();

        // Start background async worker thread
        let waker = scheduler_waker.clone();
        let async_worker = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async_redis_worker(cmd_rx, result_tx, waker));
        }));

        RedisPublisherComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            signals_in,
            signals_out,
            state: RedisPublisherState::WaitingForConfig,
            // ADR-017: Bounded IO channels
            cmd_tx,
            result_rx,
            // Background async worker
            _async_worker: async_worker,
            // ADR-017: Retry configuration
            retry_config: RetryConfig::default(),
            retry_state: RetryState::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("RedisPublisher process() called");

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                self.state = RedisPublisherState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        // Handle state transitions that require borrowing
        let current_state = std::mem::replace(&mut self.state, RedisPublisherState::Finished);
        if let RedisPublisherState::Connecting { url, channel } = current_state {
            // Check if connection completed
            match self.result_rx.try_recv() {
                Ok(Ok(connection)) => {
                    // Connection successful - transition to connected state
                    self.state = RedisPublisherState::Connected {
                        url,
                        connection,
                        channel: channel.clone(),
                        pending_messages: Vec::new(),
                    };
                    debug!("Redis publisher connected to channel: {}", channel);
                    return ProcessResult::DidWork(1);
                }
                Ok(Err(e)) => {
                    // ADR-017: Classify error and handle retry logic
                    let error_type = classify_redis_error(&e);
                    match error_type {
                        ErrorType::Transient => {
                            if self.retry_state.should_retry(error_type, &self.retry_config) {
                                let mut retry_state = std::mem::take(&mut self.retry_state);
                                retry_state.calculate_next_retry(&self.retry_config);
                                retry_state.last_error_type = error_type;
                                warn!("Redis connection failed (transient error), will retry in {:?} (attempt {}): {}",
                                      retry_state.next_retry_at - Instant::now(), retry_state.attempt, e);
                                self.state = RedisPublisherState::RetryingConnection {
                                    url,
                                    channel,
                                    retry_state,
                                };
                                return ProcessResult::DidWork(1);
                            } else {
                                error!("Redis connection failed after {} retries, giving up: {}", self.retry_config.max_retries, e);
                                self.state = RedisPublisherState::Finished;
                                return ProcessResult::Finished;
                            }
                        }
                        ErrorType::Permanent => {
                            error!("Redis connection failed (permanent error), not retrying: {}", e);
                            self.state = RedisPublisherState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                }
                Err(_) => {
                    // Connection still in progress, put state back and wait
                    self.state = RedisPublisherState::Connecting { url, channel };
                    context.wake_at(
                        Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                    );
                    return ProcessResult::NoWork;
                }
            }
        } else {
            // Put the state back since we didn't handle it
            self.state = current_state;
        }

        let current_state = std::mem::replace(&mut self.state, RedisPublisherState::Finished);
        match current_state {
            RedisPublisherState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_msg) = self.conf.pop() {
                    let url_str = url_msg.as_text().expect("invalid text").to_string();
                    debug!("got config URL: {}", url_str);

                    // Parse connection arguments and get channel from URL
                    let url = url::Url::parse(&url_str).expect("failed to parse URL");
                    let channel_queryparam = url
                        .query_pairs()
                        .find(|(key, _)| key.eq("channel"))
                        .expect("failed to get channel name from connection URL channel parameter");
                    let channel_bytes = channel_queryparam.1.as_bytes();
                    let channel = std::str::from_utf8(channel_bytes)
                        .expect("failed to convert channel name to str");
                    debug!("channel: {}", channel);

                    // ADR-017: Send to background async worker via bounded channel
                    match self.cmd_tx.try_send((url_str.to_string(), channel.to_string())) {
                        Ok(()) => {
                            debug!("Redis connection request enqueued to async worker");
                            self.state = RedisPublisherState::Connecting {
                                url: url_str.to_string(),
                                channel: channel.to_string(),
                            };
                            return ProcessResult::DidWork(1);
                        }
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            // ADR-017: Channel full, apply backpressure
                            debug!("Redis command channel full, applying backpressure");
                            warn!("Redis connection request dropped due to full command channel");
                            self.state = RedisPublisherState::WaitingForConfig;
                            return ProcessResult::NoWork;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            // Worker thread died, component should finish
                            warn!("Redis async worker disconnected");
                            self.state = RedisPublisherState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                }
                // No config yet, but check if we should yield budget
                if context.remaining_budget == 0 {
                    return ProcessResult::NoWork;
                }
                context.remaining_budget -= 1;
                return ProcessResult::NoWork;
            }

            RedisPublisherState::Connecting { .. } => {
                // This should have been handled above
                self.state = RedisPublisherState::Finished;
                ProcessResult::Finished
            }

            // ADR-017: Handle retry state
            RedisPublisherState::RetryingConnection { url, channel, retry_state } => {
                // Check if it's time to retry
                if retry_state.is_ready_to_retry() {
                    // Try to send connection request again
                    match self.cmd_tx.try_send((url.clone(), channel.clone())) {
                        Ok(()) => {
                            debug!("Redis retry connection request enqueued to async worker (attempt {})", retry_state.attempt);
                            self.state = RedisPublisherState::Connecting { url, channel };
                            return ProcessResult::DidWork(1);
                        }
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            // Channel full, wait and try again later
                            context.wake_at(
                                Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                            );
                            self.state = RedisPublisherState::RetryingConnection { url, channel, retry_state };
                            return ProcessResult::NoWork;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            // Worker thread died
                            warn!("Redis async worker disconnected during retry");
                            self.state = RedisPublisherState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                } else {
                    // Not ready to retry yet, schedule wakeup
                    context.wake_at(retry_state.next_retry_at);
                    self.state = RedisPublisherState::RetryingConnection { url, channel, retry_state };
                    return ProcessResult::NoWork;
                }
            }

            RedisPublisherState::Connected {
                url,
                connection,
                channel,
                mut pending_messages,
            } => {
                let mut work_units = 0;

                // Collect available messages
                while context.remaining_budget > 0 && !self.inn.is_empty() {
                    let chunk = self.inn.read_chunk(1).expect("receive as chunk failed");
                    for ip in chunk {
                        pending_messages.push(ip);
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                }

                // Send pending messages asynchronously
                if !pending_messages.is_empty() && context.remaining_budget > 0 {
                    let messages = std::mem::take(&mut pending_messages);
                    let _channel_name = channel.clone();
                    let url_clone = url.clone(); // We need the URL to reconnect

                    tokio::spawn(async move {
                        // Reconnect for each batch of messages (simplified approach)
                        match redis_async::client::connect(&url_clone, 6379, None, None).await {
                            Ok(_conn) => {
                                for message in messages {
                                    // TODO: Implement proper Redis PUBLISH command
                                    // For now, just log that we would publish
                                    debug!("Would publish message to channel '{}': {:?}", _channel_name, message);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to reconnect for publishing: {}", e);
                            }
                        }
                    });

                    work_units += 1;
                    context.remaining_budget -= 1;
                }

                // Check if input is abandoned
                if self.inn.is_abandoned() {
                    info!("EOF on inport, shutting down");
                    self.state = RedisPublisherState::Finished;
                    return ProcessResult::Finished;
                }

                // Put the state back with updated pending_messages
                self.state = RedisPublisherState::Connected {
                    url,
                    connection,
                    channel,
                    pending_messages,
                };

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    context.wake_at(
                        Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                    );
                    ProcessResult::NoWork
                }
            }

            RedisPublisherState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("RedisPublisher"),
            description: String::from(
                "Publishes data as-is from IN port to the Redis MQ pub/sub channel given in CONF.",
            ),
            icon: String::from("cloud-upload"), // or arrow-circle-down
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from(
                        "connection URL which includes options, see redis crate documentation",
                    ), // see https://github.com/redis-rs/redis-rs/blob/45973d30c70c3817856095dda0c20401a8327207/redis/src/connection.rs#L282
                    values_allowed: vec![],
                    value_default: String::from(
                        "rediss://user:pass@server.com/db_number?channel=channel_name",
                    ),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be published on given Redis MQ channel"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

enum RedisSubscriberState {
    WaitingForConfig,
    Listening {
        channel: String,
        message_rx: tokio_mpsc::UnboundedReceiver<Result<Vec<u8>, String>>,
    },
    Finished,
}

pub struct RedisSubscriberComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: RedisSubscriberState,
    //graph_inout: GraphInportOutportHandle,
}



impl Component for RedisSubscriberComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        RedisSubscriberComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in,
            signals_out,
            state: RedisSubscriberState::WaitingForConfig,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("RedisSubscriber process() called");

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                self.state = RedisSubscriberState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        match &mut self.state {
            RedisSubscriberState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_msg) = self.conf.pop() {
                    let url_str = url_msg.as_text().expect("invalid text").to_string();
                    debug!("got config URL: {}", url_str);

                    // Parse connection arguments and get channel from URL
                    let url = url::Url::parse(&url_str).expect("failed to parse URL");
                    let channel_queryparam = url
                        .query_pairs()
                        .find(|(key, _)| key.eq("channel"))
                        .expect("failed to get channel name from connection URL channel parameter");
                    let channel_bytes = channel_queryparam.1.as_bytes();
                    let channel = std::str::from_utf8(channel_bytes)
                        .expect("failed to convert channel name to str");
                    debug!("channel: {}", channel);

                    // Start async connection and subscription
                    let (result_tx, result_rx) = tokio_mpsc::unbounded_channel();
                    let _channel_name = channel.to_string();

                    tokio::spawn(async move {
                        match redis_async::client::connect(&url_str, 6379, None, None).await {
                            Ok(_connection) => {
                                // For now, just signal successful connection
                                // TODO: Implement proper pub/sub subscription
                                let _ = result_tx.send(Ok(vec![])); // Empty message to indicate connection success
                            }
                            Err(e) => {
                                let _ = result_tx.send(Err(format!("Failed to connect: {}", e)));
                            }
                        }
                    });

                    self.state = RedisSubscriberState::Listening {
                        channel: channel.to_string(),
                        message_rx: result_rx,
                    };
                    return ProcessResult::DidWork(1);
                }
                // No config yet, but check if we should yield budget
                if context.remaining_budget == 0 {
                    return ProcessResult::NoWork;
                }
                context.remaining_budget -= 1;
                return ProcessResult::NoWork;
            }

            RedisSubscriberState::Listening { channel, message_rx } => {
                // Check for messages
                match message_rx.try_recv() {
                    Ok(Ok(message_data)) => {
                        // Received a message
                        debug!("Received message from Redis channel '{}'", channel);
                        let message_msg = FbpMessage::from_bytes(message_data);
                        if let Ok(()) = self.out.push(message_msg) {
                            return ProcessResult::DidWork(1);
                        }
                        // Output buffer full, will try again next time
                        return ProcessResult::NoWork;
                    }
                    Ok(Err(e)) => {
                        error!("Redis subscriber error: {}", e);
                        self.state = RedisSubscriberState::Finished;
                        return ProcessResult::Finished;
                    }
                    Err(_) => {
                        // No message available, wait
                        context.wake_at(
                            Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                        );
                        return ProcessResult::NoWork;
                    }
                }
            }



            RedisSubscriberState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("RedisSubscriber"),
            description: String::from("Subscribes to the Redis MQ pub/sub channel given in CONF and forwards received message data to the OUT outport."),
            icon: String::from("cloud-download"),   // or arrow-circle-down
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL which includes options, see redis crate documentation"),  // see https://github.com/redis-rs/redis-rs/blob/45973d30c70c3817856095dda0c20401a8327207/redis/src/connection.rs#L282
                    values_allowed: vec![],
                    value_default: String::from("rediss://user:pass@server.com/db_number?channel=channel_name")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("contents of received messages on given Redis MQ channel"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
