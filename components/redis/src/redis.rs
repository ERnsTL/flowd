use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, SchedulerWaker,
};
use log::{debug, error, info, trace, warn};

// component-specific
use redis_async::client::connect::RespConnectionInner;
use tokio_util::codec::Framed;
use redis_async::resp::RespCodec;
use std::time::{Instant};
use tokio::sync::mpsc as tokio_mpsc;



enum RedisPublisherState {
    WaitingForConfig,
    Connecting {
        url: String,
        channel: String,
        result_rx: tokio_mpsc::UnboundedReceiver<Result<Framed<RespConnectionInner, RespCodec>, String>>,
    },
    Connected {
        url: String,
        connection: Framed<RespConnectionInner, RespCodec>,
        channel: String,
        pending_messages: Vec<Vec<u8>>,
    },
    Finished,
}

pub struct RedisPublisherComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: RedisPublisherState,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for RedisPublisherComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
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
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("RedisPublisher process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = RedisPublisherState::Finished;
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!(
                    "received unknown signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                )
            }
        }

        // Handle state transitions that require borrowing
        let current_state = std::mem::replace(&mut self.state, RedisPublisherState::Finished);
        if let RedisPublisherState::Connecting { url, channel, mut result_rx } = current_state {
            // Check if connection completed
            match result_rx.try_recv() {
                Ok(Ok(connection)) => {
                    // Connection successful - transition to connected state
                    let new_state = RedisPublisherState::Connected {
                        url,
                        connection,
                        channel: channel.clone(),
                        pending_messages: Vec::new(),
                    };
                    self.state = new_state;
                    debug!("Redis publisher connected to channel: {}", channel);
                    return ProcessResult::DidWork(1);
                }
                Ok(Err(e)) => {
                    error!("Redis connection failed: {}", e);
                    self.state = RedisPublisherState::Finished;
                    return ProcessResult::Finished;
                }
                Err(_) => {
                    // Connection still in progress, put state back and wait
                    self.state = RedisPublisherState::Connecting { url, channel, result_rx };
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
                if let Ok(url_vec) = self.conf.pop() {
                    let url_str = String::from_utf8(url_vec).expect("invalid utf-8");
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

                    // Start async connection
                    let (result_tx, result_rx) = tokio_mpsc::unbounded_channel();
                    let url_clone = url_str.clone();

                    tokio::spawn(async move {
                        match redis_async::client::connect(&url_clone, 6379, None, None).await {
                            Ok(connection) => {
                                let _ = result_tx.send(Ok(connection));
                            }
                            Err(e) => {
                                let _ = result_tx.send(Err(format!("Failed to connect: {}", e)));
                            }
                        }
                    });

                    self.state = RedisPublisherState::Connecting {
                        url: url_str,
                        channel: channel.to_string(),
                        result_rx,
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

            RedisPublisherState::Connecting { url, channel, mut result_rx } => {
                // Check if connection completed
                match result_rx.try_recv() {
                    Ok(Ok(connection)) => {
                        // Connection successful - transition to connected state
                        let new_state = RedisPublisherState::Connected {
                            url: url.clone(),
                            connection,
                            channel: channel.clone(),
                            pending_messages: Vec::new(),
                        };
                        self.state = new_state;
                        debug!("Redis publisher connected to channel: {}", channel);
                        return ProcessResult::DidWork(1);
                    }
                    Ok(Err(e)) => {
                        error!("Redis connection failed: {}", e);
                        self.state = RedisPublisherState::Finished;
                        return ProcessResult::Finished;
                    }
                    Err(_) => {
                        // Connection still in progress, wait
                        context.wake_at(
                            Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                        );
                        return ProcessResult::NoWork;
                    }
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
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = RedisSubscriberState::Finished;
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!(
                    "received unknown signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                )
            }
        }

        match &mut self.state {
            RedisSubscriberState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url_str = String::from_utf8(url_vec).expect("invalid utf-8");
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
                        if let Ok(()) = self.out.push(message_data) {
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
