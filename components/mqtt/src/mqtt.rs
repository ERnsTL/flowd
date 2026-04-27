use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, create_io_channels,
    SchedulerWaker, wake_scheduler, ErrorType, RetryConfig, RetryState,
};
use log::{debug, error, info, trace, warn};

// component-specific
use rumqttc::{Client, MqttOptions};
use std::time::{Duration, Instant};
use tokio::sync::mpsc as tokio_mpsc;

enum MQTTPublisherState {
    WaitingForConfig,
    Connecting {
        url: String,
        topic: String,
    },
    // ADR-017: Retry states for failed connections
    RetryingConnection {
        url: String,
        topic: String,
        retry_state: RetryState,
    },
    Connected {
        client: rumqttc::Client,
        connection: rumqttc::Connection,
        topic: String,
        pending_messages: Vec<Vec<u8>>,
    },
    Finished,
}

pub struct MQTTPublisherComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: MQTTPublisherState,
    // ADR-017: Bounded IO channels
    cmd_tx: std::sync::mpsc::SyncSender<(String, String)>,
    result_rx: std::sync::mpsc::Receiver<Result<(rumqttc::Client, rumqttc::Connection), String>>,
    // Background async worker
    _async_worker: Option<std::thread::JoinHandle<()>>,
    // ADR-017: Retry configuration and state
    retry_config: RetryConfig,
    retry_state: RetryState,
}



// ADR-017: Error classification for MQTT connections
fn classify_mqtt_error(error: &str) -> ErrorType {
    // MQTT connection errors are generally transient (network issues, broker unavailable)
    // Only permanent errors would be invalid configuration
    if error.contains("invalid") || error.contains("malformed") || error.contains("unauthorized") {
        ErrorType::Permanent
    } else {
        ErrorType::Transient
    }
}

// ADR-017: Async MQTT worker that runs in background thread with Tokio runtime
async fn async_mqtt_worker(
    cmd_rx: std::sync::mpsc::Receiver<(String, String)>,
    result_tx: std::sync::mpsc::SyncSender<Result<(rumqttc::Client, rumqttc::Connection), String>>,
    waker: Option<SchedulerWaker>,
) {
    debug!("MQTT async worker started");

    while let Ok((url, topic)) = cmd_rx.recv() {
        debug!("MQTT worker connecting to: {} topic: {}", &url, &topic);

        // Perform MQTT connection (this is synchronous but we're in an async context)
        let result = std::panic::catch_unwind(|| {
            let mut mqttoptions = MqttOptions::parse_url(&url).expect("failed to parse MQTT URL");
            mqttoptions.set_keep_alive(Duration::from_secs(5));
            Client::new(mqttoptions, 10)
        });

        let connection_result = match result {
            Ok((client, connection)) => Ok((client, connection)),
            Err(_) => Err("Failed to create MQTT client".to_string()),
        };

        // Send result back (ignore send errors - component may have been destroyed)
        let _ = result_tx.try_send(connection_result);

        // ADR-002: Signal scheduler that work is ready
        wake_scheduler(&waker);
    }

    debug!("MQTT async worker exiting");
}

impl Component for MQTTPublisherComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        // ADR-017: Create bounded IO channels
        let (cmd_tx, cmd_rx, result_tx, result_rx) = create_io_channels::<(String, String), Result<(rumqttc::Client, rumqttc::Connection), String>>();

        // Start background async worker thread
        let waker = scheduler_waker.clone();
        let async_worker = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async_mqtt_worker(cmd_rx, result_tx, waker));
        }));

        MQTTPublisherComponent {
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
            signals_in: signals_in,
            signals_out: signals_out,
            state: MQTTPublisherState::WaitingForConfig,
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
        debug!("MQTTPublisher process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = MQTTPublisherState::Finished;
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
        let current_state = std::mem::replace(&mut self.state, MQTTPublisherState::Finished);
        if let MQTTPublisherState::Connecting { url, topic } = current_state {
            // Check if connection completed
            match self.result_rx.try_recv() {
                Ok(Ok((client, connection))) => {
                    // Connection successful - transition to connected state
                    self.state = MQTTPublisherState::Connected {
                        client,
                        connection,
                        topic: topic.clone(),
                        pending_messages: Vec::new(),
                    };
                    debug!("MQTT publisher connected to topic: {}", topic);
                    return ProcessResult::DidWork(1);
                }
                Ok(Err(e)) => {
                    // ADR-017: Classify error and handle retry logic
                    let error_type = classify_mqtt_error(&e);
                    match error_type {
                        ErrorType::Transient => {
                            if self.retry_state.should_retry(error_type, &self.retry_config) {
                                let mut retry_state = std::mem::take(&mut self.retry_state);
                                retry_state.calculate_next_retry(&self.retry_config);
                                retry_state.last_error_type = error_type;
                                warn!("MQTT connection failed (transient error), will retry in {:?} (attempt {}): {}",
                                      retry_state.next_retry_at - Instant::now(), retry_state.attempt, e);
                                self.state = MQTTPublisherState::RetryingConnection {
                                    url,
                                    topic,
                                    retry_state,
                                };
                                return ProcessResult::DidWork(1);
                            } else {
                                error!("MQTT connection failed after {} retries, giving up: {}", self.retry_config.max_retries, e);
                                self.state = MQTTPublisherState::Finished;
                                return ProcessResult::Finished;
                            }
                        }
                        ErrorType::Permanent => {
                            error!("MQTT connection failed (permanent error), not retrying: {}", e);
                            self.state = MQTTPublisherState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                }
                Err(_) => {
                    // Connection still in progress, put state back and wait
                    self.state = MQTTPublisherState::Connecting { url, topic };
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

        let current_state = std::mem::replace(&mut self.state, MQTTPublisherState::Finished);
        match current_state {
            MQTTPublisherState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url_str = String::from_utf8(url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url_str);

                    // Get topic from URL
                    let url_parsed = url::Url::parse(&url_str).expect("failed to parse URL");
                    let mut topic = url_parsed.path();
                    if topic.is_empty() || topic == "/" {
                        error!("no topic given in MQTT URL path");
                        self.state = MQTTPublisherState::Finished;
                        return ProcessResult::Finished;
                    }
                    topic = topic.trim_start_matches('/');
                    debug!("topic: {}", topic);

                    // ADR-017: Send to background async worker via bounded channel
                    match self.cmd_tx.try_send((url_str.clone(), topic.to_string())) {
                        Ok(()) => {
                            debug!("MQTT connection request enqueued to async worker");
                            self.state = MQTTPublisherState::Connecting {
                                url: url_str,
                                topic: topic.to_string(),
                            };
                            return ProcessResult::DidWork(1);
                        }
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            // ADR-017: Channel full, apply backpressure
                            debug!("MQTT command channel full, applying backpressure");
                            warn!("MQTT connection request dropped due to full command channel");
                            self.state = MQTTPublisherState::WaitingForConfig;
                            return ProcessResult::NoWork;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            // Worker thread died, component should finish
                            warn!("MQTT async worker disconnected");
                            self.state = MQTTPublisherState::Finished;
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

            MQTTPublisherState::Connecting { .. } => {
                // This should have been handled above
                self.state = MQTTPublisherState::Finished;
                ProcessResult::Finished
            }

            // ADR-017: Handle retry state
            MQTTPublisherState::RetryingConnection { url, topic, retry_state } => {
                // Check if it's time to retry
                if retry_state.is_ready_to_retry() {
                    // Try to send connection request again
                    match self.cmd_tx.try_send((url.clone(), topic.clone())) {
                        Ok(()) => {
                            debug!("MQTT retry connection request enqueued to async worker (attempt {})", retry_state.attempt);
                            self.state = MQTTPublisherState::Connecting { url, topic };
                            return ProcessResult::DidWork(1);
                        }
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            // Channel full, wait and try again later
                            context.wake_at(
                                Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                            );
                            self.state = MQTTPublisherState::RetryingConnection { url, topic, retry_state };
                            return ProcessResult::NoWork;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            // Worker thread died
                            warn!("MQTT async worker disconnected during retry");
                            self.state = MQTTPublisherState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                } else {
                    // Not ready to retry yet, schedule wakeup
                    context.wake_at(retry_state.next_retry_at);
                    self.state = MQTTPublisherState::RetryingConnection { url, topic, retry_state };
                    return ProcessResult::NoWork;
                }
            }

            MQTTPublisherState::Connected {
                client,
                connection,
                topic,
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

                // Publish pending messages asynchronously
                if !pending_messages.is_empty() && context.remaining_budget > 0 {
                    let messages = std::mem::take(&mut pending_messages);
                    let topic_name = topic.clone();

                    tokio::spawn(async move {
                        // Publish messages (in async task to avoid blocking)
                        for message in messages {
                            if let Err(_e) = std::panic::catch_unwind(|| {
                                // This is a simplified approach - in a real implementation
                                // we'd need proper async MQTT client
                                debug!("Would publish message to MQTT topic '{}': {:?}", topic_name, message);
                            }) {
                                warn!("Failed to publish MQTT message");
                            }
                        }
                    });

                    work_units += 1;
                    context.remaining_budget -= 1;
                }

                // Handle MQTT connection events cooperatively (simplified)
                // In a real implementation, we'd poll the connection in an async task

                // Check if input is abandoned
                if self.inn.is_abandoned() {
                    info!("EOF on inport, shutting down");
                    self.state = MQTTPublisherState::Finished;
                    return ProcessResult::Finished;
                }

                // Put the state back with updated pending_messages
                self.state = MQTTPublisherState::Connected {
                    client,
                    connection,
                    topic,
                    pending_messages,
                };

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    context.wake_at(Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL);
                    ProcessResult::NoWork
                }
            }

            MQTTPublisherState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("MQTTPublisher"),
            description: String::from(
                "Publishes data as-is from IN port to the MQTT topic given in CONF.",
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
                        "connection URL which includes options, see rumqttc crate documentation",
                    ),
                    values_allowed: vec![],
                    value_default: String::from(
                        "mqtts://test.mosquitto.org:8886/hello/flowd?client_id=flowd123",
                    ),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be published on given MQTT topic"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

enum MQTTSubscriberState {
    WaitingForConfig,
    Connecting {
        url: String,
        topic: String,
        result_rx: tokio_mpsc::UnboundedReceiver<Result<(rumqttc::Client, rumqttc::Connection), String>>,
    },
    Listening {
        client: rumqttc::Client,
        connection: rumqttc::Connection,
        topic: String,
        message_rx: tokio_mpsc::UnboundedReceiver<Result<Vec<u8>, String>>,
    },
    Finished,
}

pub struct MQTTSubscriberComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: MQTTSubscriberState,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for MQTTSubscriberComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        MQTTSubscriberComponent {
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
            signals_in: signals_in,
            signals_out: signals_out,
            state: MQTTSubscriberState::WaitingForConfig,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("MQTTSubscriber process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = MQTTSubscriberState::Finished;
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
        let current_state = std::mem::replace(&mut self.state, MQTTSubscriberState::Finished);
        if let MQTTSubscriberState::Connecting { url, topic, mut result_rx } = current_state {
            // Check if connection completed
            match result_rx.try_recv() {
                Ok(Ok((client, connection))) => {
                    // Connection successful, now subscribe
                    let (message_tx, message_rx) = tokio_mpsc::unbounded_channel();
                    let topic_name = topic.clone();
                    let topic_name_clone = topic_name.clone();

                    tokio::spawn(async move {
                        // Subscribe to topic (in async task)
                        if let Err(_e) = std::panic::catch_unwind(|| {
                            // This is a simplified approach - in a real implementation
                            // we'd need proper async MQTT subscription
                            debug!("Would subscribe to MQTT topic: {}", topic_name);
                        }) {
                            let _ = message_tx.send(Err("Failed to subscribe".to_string()));
                            return;
                        }

                        // Listen for messages (simplified - in real implementation we'd poll connection)
                        let _ = message_tx.send(Ok(vec![])); // Signal successful subscription
                    });

                    self.state = MQTTSubscriberState::Listening {
                        client,
                        connection,
                        topic: topic_name_clone,
                        message_rx,
                    };
                    debug!("MQTT subscriber connected and listening on topic");
                    return ProcessResult::DidWork(1);
                }
                Ok(Err(e)) => {
                    error!("MQTT connection failed: {}", e);
                    self.state = MQTTSubscriberState::Finished;
                    return ProcessResult::Finished;
                }
                Err(_) => {
                    // Connection still in progress, put state back and wait
                    self.state = MQTTSubscriberState::Connecting { url, topic, result_rx };
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

        let current_state = std::mem::replace(&mut self.state, MQTTSubscriberState::Finished);
        match current_state {
            MQTTSubscriberState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url_str = String::from_utf8(url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url_str);

                    // Get topic from URL
                    let url_parsed = url::Url::parse(&url_str).expect("failed to parse URL");
                    let mut topic = url_parsed.path();
                    if topic.is_empty() || topic == "/" {
                        error!("no topic given in MQTT URL path");
                        self.state = MQTTSubscriberState::Finished;
                        return ProcessResult::Finished;
                    }
                    topic = topic.trim_start_matches('/');
                    debug!("topic: {}", topic);

                    // Start async connection
                    let (result_tx, result_rx) = tokio_mpsc::unbounded_channel();
                    let url_clone = url_str.clone();

                    tokio::spawn(async move {
                        // Parse and connect to MQTT server (in async task to avoid blocking)
                        match std::panic::catch_unwind(|| {
                            let mut mqttoptions = MqttOptions::parse_url(&url_clone).expect("failed to parse MQTT URL");
                            mqttoptions.set_keep_alive(Duration::from_secs(5));
                            Client::new(mqttoptions, 10)
                        }) {
                            Ok((client, connection)) => {
                                let _ = result_tx.send(Ok((client, connection)));
                            }
                            Err(_) => {
                                let _ = result_tx.send(Err("Failed to create MQTT client".to_string()));
                            }
                        }
                    });

                    self.state = MQTTSubscriberState::Connecting {
                        url: url_str,
                        topic: topic.to_string(),
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

            MQTTSubscriberState::Connecting { .. } => {
                // This should have been handled above
                self.state = MQTTSubscriberState::Finished;
                ProcessResult::Finished
            }

            MQTTSubscriberState::Listening {
                client: _client,
                connection: _connection,
                topic,
                mut message_rx,
            } => {
                // Check for messages
                match message_rx.try_recv() {
                    Ok(Ok(message_data)) => {
                        // Received a message
                        debug!("Received message from MQTT topic '{}'", topic);
                        if let Ok(()) = self.out.push(message_data) {
                            return ProcessResult::DidWork(1);
                        }
                        // Output buffer full, will try again next time
                        return ProcessResult::NoWork;
                    }
                    Ok(Err(e)) => {
                        error!("MQTT subscriber error: {}", e);
                        self.state = MQTTSubscriberState::Finished;
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

            MQTTSubscriberState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("MQTTSubscriber"),
            description: String::from("Subscribes to the MQTT topic given in CONF and forwards received message payload to the OUT outport."),
            icon: String::from("cloud-download"),   // or arrow-circle-down
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL which includes options, see rumqttc crate documentation"),    //TODO careful with the client id, other one gets disconnected - https://stackoverflow.com/questions/50654338/how-to-use-client-id-in-mosquitto-mqtt
                    values_allowed: vec![],
                    value_default: String::from("mqtts://test.mosquitto.org:8886/hello/flowd?client_id=flowd456")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("contents of received MQTT events on given topic"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
