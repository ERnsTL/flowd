use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

// component-specific
use rumqttc::{Client, Event::Incoming, MqttOptions, Packet::Publish, QoS};
use std::time::Duration;

enum MQTTPublisherState {
    WaitingForConfig,
    Connected {
        client: rumqttc::Client,
        connection: rumqttc::Connection,
        topic: String,
    },
    Finished,
}

pub struct MQTTPublisherComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: MQTTPublisherState,
    //graph_inout: GraphInportOutportHandle,
}

const RETAIN_MSG: bool = false; // "sticky" message to the topic, there can be only one retained message, and this message is delivered to new subscribers immediately
const MQTT_QOS: QoS = QoS::AtMostOnce; //TODO find out what is best for FBP

impl Component for MQTTPublisherComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
    ) -> Self
    where
        Self: Sized,
    {
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
            //graph_inout: graph_inout,
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

        match &mut self.state {
            MQTTPublisherState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url = std::str::from_utf8(&url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url);

                    // Parse and connect to MQTT server
                    let mut mqttoptions = MqttOptions::parse_url(url).expect("failed to parse MQTT URL");
                    mqttoptions.set_keep_alive(Duration::from_secs(5));
                    let (client, connection) = Client::new(mqttoptions, 10);

                    // Get topic from URL
                    let url_parsed = url::Url::parse(&url).expect("failed to parse URL");
                    let mut topic = url_parsed.path();
                    if topic.is_empty() || topic == "/" {
                        error!("no topic given in MQTT URL path");
                        self.state = MQTTPublisherState::Finished;
                        return ProcessResult::Finished;
                    }
                    topic = topic.trim_start_matches('/');
                    debug!("topic: {}", topic);

                    self.state = MQTTPublisherState::Connected {
                        client,
                        connection,
                        topic: topic.to_owned(),
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

            MQTTPublisherState::Connected { client, connection, topic } => {
                let mut work_units = 0;

                // Handle MQTT connection events cooperatively
                if context.remaining_budget > 0 {
                    match connection.recv_timeout(Duration::from_millis(10)) {
                        Ok(event) => {
                            match event {
                                Err(err) => {
                                    trace!("Event Err = {:?}", err);
                                    match err {
                                        rumqttc::ConnectionError::MqttState(err) => {
                                            match err {
                                                rumqttc::StateError::Io(err) => {
                                                    if err.kind() == std::io::ErrorKind::ConnectionAborted {
                                                        debug!("MQTT connection aborted, shutting down");
                                                        self.state = MQTTPublisherState::Finished;
                                                        return ProcessResult::Finished;
                                                    } else {
                                                        error!("MQTT state error: {:?}", err);
                                                    }
                                                }
                                                _ => {
                                                    error!("MQTT connection error: {:?}", err);
                                                }
                                            }
                                        }
                                        _ => {
                                            // Will be handled by the eventloop
                                        }
                                    }
                                }
                                _ => {
                                    trace!("Event = {:?}", event);
                                    // Nothing to do, not interested in events
                                }
                            }
                            work_units += 1;
                            context.remaining_budget -= 1;
                        }
                        Err(_) => {
                            // No events available, continue with publishing
                        }
                    }
                }

                // Publish available messages within remaining budget
                while context.remaining_budget > 0 && !self.inn.is_empty() {
                    debug!("got {} packets, forwarding to MQTT topic.", self.inn.slots());
                    let chunk = self.inn
                        .read_chunk(self.inn.slots())
                        .expect("receive as chunk failed");

                    for ip in chunk.into_iter() {
                        debug!("publishing message to MQTT topic: {}", topic);
                        if let Err(err) = client.publish(topic.as_str(), MQTT_QOS, RETAIN_MSG, ip) {
                            error!("failed to publish to MQTT: {}", err);
                            self.state = MQTTPublisherState::Finished;
                            return ProcessResult::Finished;
                        }
                        work_units += 1;
                        context.remaining_budget -= 1;

                        if context.remaining_budget == 0 {
                            break;
                        }
                    }

                    if context.remaining_budget == 0 {
                        break;
                    }
                }

                // Check if input is abandoned
                if self.inn.is_abandoned() {
                    info!("EOF on inport, shutting down");
                    // Disconnect MQTT client
                    if let Err(err) = client.disconnect() {
                        warn!("failed to disconnect MQTT client cleanly: {:?}", err);
                    }
                    self.state = MQTTPublisherState::Finished;
                    return ProcessResult::Finished;
                }

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
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
    Connected {
        client: rumqttc::Client,
        connection: rumqttc::Connection,
        topic: String,
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

        match &mut self.state {
            MQTTSubscriberState::WaitingForConfig => {
                // Check signals
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
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url = std::str::from_utf8(&url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url);

                    // Parse and connect to MQTT server
                    let mut mqttoptions = MqttOptions::parse_url(url).expect("failed to parse MQTT URL");
                    mqttoptions.set_keep_alive(Duration::from_secs(5));
                    let (mut client, connection) = Client::new(mqttoptions, 10);

                    // Get topic from URL
                    let url_parsed = url::Url::parse(&url).expect("failed to parse URL");
                    let mut topic = url_parsed.path();
                    if topic.is_empty() || topic == "/" {
                        error!("no topic given in MQTT URL path");
                        self.state = MQTTSubscriberState::Finished;
                        return ProcessResult::Finished;
                    }
                    topic = topic.trim_start_matches('/');
                    debug!("topic: {}", topic);

                    // Subscribe to the topic
                    if let Err(err) = client.subscribe(topic, QoS::AtMostOnce) {
                        error!("failed to subscribe to MQTT topic: {}", err);
                        self.state = MQTTSubscriberState::Finished;
                        return ProcessResult::Finished;
                    }

                    info!("MQTT subscriber connected and subscribed to topic: {}", topic);
                    self.state = MQTTSubscriberState::Connected {
                        client,
                        connection,
                        topic: topic.to_owned(),
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

            MQTTSubscriberState::Connected { client, connection, topic } => {
                // Check signals
                if let Ok(ip) = self.signals_in.try_recv() {
                    trace!(
                        "received signal ip: {}",
                        std::str::from_utf8(&ip).expect("invalid utf-8")
                    );
                    if ip == b"stop" {
                        info!("got stop signal, finishing");
                        info!("Disconnecting MQTT subscriber from topic: {}", topic);
                        if let Err(err) = client.disconnect() {
                            warn!("failed to disconnect MQTT client cleanly: {:?}", err);
                        }
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

                debug!("MQTT subscriber active - Topic: {}", topic);

                let mut work_units = 0;

                // Process MQTT events cooperatively within remaining budget
                while context.remaining_budget > 0 {
                    match connection.recv_timeout(Duration::from_millis(10)) {
                        Ok(event) => {
                            match event {
                                Ok(Incoming(Publish(packet))) => {
                                    debug!("Received payload from MQTT topic: {:?}", packet.payload);

                                    // Try to send it to output
                                    match self.out.push(packet.payload.to_vec()) {
                                        Ok(()) => {
                                            debug!("forwarded MQTT payload");
                                            work_units += 1;
                                            context.remaining_budget -= 1;
                                        }
                                        Err(_) => {
                                            // Output buffer full, stop processing for now
                                            break;
                                        }
                                    }
                                }
                                Err(err) => {
                                    trace!("MQTT event error: {:?}", err);
                                    // Continue processing other events
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                }
                                _ => {
                                    trace!("Other MQTT event: {:?}", event);
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                }
                            }
                        }
                        Err(_) => {
                            // No more events available, yield to scheduler
                            break;
                        }
                    }
                }

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    ProcessResult::NoWork
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
