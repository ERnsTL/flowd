use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHandle, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use std::time::Duration;
use rumqttc::{Client, Event::Incoming, MqttOptions, Packet::Publish, QoS};
use std::thread;

pub struct MQTTPublisherComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

const RETAIN_MSG: bool = false; // "sticky" message to the topic, there can be only one retained message, and this message is delivered to new subscribers immediately
const MQTT_QOS: QoS = QoS::AtMostOnce;  //TODO find out what is best for FBP

impl Component for MQTTPublisherComponent {
    fn new(mut inports: ProcessInports, _outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        MQTTPublisherComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("MQTTPublisher is now run()ning!");
        let mut conf = self.conf;
        let mut inn = self.inn;

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url = std::str::from_utf8(&url_vec).expect("invalid utf-8");

        // prepare connection arguments
        let mut mqttoptions = MqttOptions::parse_url(url).expect("failed to parse MQTT URL");
        mqttoptions.set_keep_alive(Duration::from_secs(5));
        let (mut client, mut connection) = Client::new(mqttoptions, 10);
        // get topic from URL
        let url_parsed = url::Url::parse(&url).expect("failed to parse URL");
        let mut topic = url_parsed.path();
        if topic.is_empty() || topic == "/" {
            error!("no topic given in MQTT URL path, exiting");
            return;
        }
        // remove leading slash
        topic = topic.trim_start_matches('/');
        debug!("topic: {}", topic);

        // signal handling
        //TODO optimize - currently not used because we use connection error (connection aborted) to stop the thread
        // but there can be other reasons for connection close or an error...
        //let (eventthread_signalsink, eventthread_signalsource) = std::sync::mpsc::sync_channel::<()>(0);

        // handle connection events
        //TODO automatic reconnection
        let event_handler_thread = thread::Builder::new().name(format!("{}/EV", thread::current().name().expect("failed to get current thread name"))).spawn(move || {
            // Iterate to poll the eventloop for connection progress
            //TODO or change to recv_timeout() like in MQTTSubscriber and then use signals channel to check for stop signal
            //TODO optimize - dont know how to install a signal handler on thread level, otherwise we could save the channel and polling for shutdown
            /*
            for event in connection.iter() {
                match event {
                    _ => {
                        trace!("Event = {:?}", event);
                        // nothing to do, not interested in any events
                        //TODO really?
                    }
                }
            }
            */
            // alternative with recv_timeout()
            /*
            loop {
                // check for closed shutdown signal channel
                match eventthread_signalsource.try_recv() {
                    Ok(_) => {
                        // there will never anything be sent
                    },
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // nothing to do, lets continue receiving MQTT events
                    },
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        trace!("eventloop listener got close on signal channel, shutting down");
                        break;
                    }
                }
                // block on MQTT events
                while let Ok(event) = connection.recv_timeout(RECV_TIMEOUT) {
                        match event {
                        _ => {
                            debug!("Event = {:?}", event);
                            // nothing to do, not interested in any events
                            //TODO really?
                        }
                    }
                }
            }
            */
            //TODO optimize - we dont want to poll for shutdown, so we currently use the Ok or Err distinction - maybe it can be more efficient with an iter() based solution?
            // but there can be other reasons for connection close or an error...
            while let Ok(event) = connection.recv() {
                match event {
                    Err(err) => {
                        trace!("Event Err = {:?}", err);
                        match err {
                            rumqttc::ConnectionError::MqttState(err) => {
                                match err {
                                    rumqttc::StateError::Io(err) => {
                                        if err.kind() == std::io::ErrorKind::ConnectionAborted {
                                            debug!("MQTT connection aborted, shutting down");
                                            break;
                                        } else {
                                            error!("MQTT state error: {:?}", err);
                                            //TODO optimize - maybe send a signal to the main thread to stop the component
                                        }
                                    },
                                    _ => {
                                        error!("MQTT connection error: {:?}", err);
                                        //TODO optimize - maybe send a signal to the main thread to stop the component
                                    }
                                }
                            },
                            _ => {
                                // will be handled by the eventloop?
                                //TODO make sure
                            }
                        }
                    },
                    _ => {
                        trace!("Event = {:?}", event);
                        // nothing to do, not interested in any events
                        //TODO really?
                    }
                }
            }
            debug!("exiting");
        }).expect("failed to spawn eventloop listener thread");

        // FBP main loop
        loop {
            trace!("begin of iteration");

            // check signals
            //TODO optimize, there is also try_recv() and recv_timeout()
            if let Ok(ip) = self.signals_in.try_recv() {
                //TODO optimize string conversions
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {   //TODO optimize comparison
                    info!("got stop signal, exiting");
                    break;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    self.signals_out.send(b"pong".to_vec()).expect("could not send pong");
                }
            }

            // check in port
            /*
            loop {
                if let Ok(_ip) = inn.pop() {
                    debug!("got a packet, dropping it.");
                } else {
                    break;
                }
            }
            loop {
                if let Ok(ip) = inn.pop() {
                    debug!("repeating packet...");
                    out.push(ip).expect("could not push into OUT");
                    out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }
            */
            while !inn.is_empty() {
                //_ = inn.pop().ok();
                //debug!("got a packet, dropping it.");

                debug!("got {} packets, forwarding to MQTT topic.", inn.slots());
                let chunk = inn.read_chunk(inn.slots()).expect("receive as chunk failed");
                for ip in chunk.into_iter() {   //TODO is iterator faster or as_slices() or as_mut_slices() ?
                    client.publish(topic, MQTT_QOS, RETAIN_MSG, ip).expect("failed to publish");
                }
                // NOTE: no commit_all() necessary, because into_iter() does that automatically
            }

            // are we done?
            if inn.is_abandoned() {
                // input closed, nothing more to do
                info!("EOF on inport, shutting down");
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }

        // stop MQTT event thread - close channel
        //drop(eventthread_signalsink);
        // close MQTT connection -> MQTT event thread will exit from special connection error ConnectionAborted
        client.disconnect().expect("failed to disconnect");
        // wait for event thread to exit
        event_handler_thread.join().expect("failed to join eventloop listener thread");

        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("MQTTPublisher"),
            description: String::from("Publishes data as-is from IN port to the MQTT topic given in CONF."),
            icon: String::from("cloud-upload"), // or arrow-circle-down
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL which includes options, see rumqttc crate documentation"),
                    values_allowed: vec![],
                    value_default: String::from("mqtts://test.mosquitto.org:8886/hello/flowd?client_id=flowd123")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be published on given MQTT topic"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

pub struct MQTTSubscriberComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

// how often the MQTT subscriber receive loop should check for signals from FBP network
const RECV_TIMEOUT: Duration = Duration::from_millis(500);

impl Component for MQTTSubscriberComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        MQTTSubscriberComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("MQTTSubscriber is now run()ning!");
        let conf = &mut self.conf;    //TODO optimize
        let out = &mut self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url = std::str::from_utf8(&url_vec).expect("invalid utf-8");

        // prepare connection arguments
        let mut mqttoptions = MqttOptions::parse_url(url).expect("failed to parse MQTT URL");
        mqttoptions.set_keep_alive(Duration::from_secs(5));
        let (mut client, mut connection) = Client::new(mqttoptions, 10);

        // get topic from URL
        let url_parsed = url::Url::parse(&url).expect("failed to parse URL");
        let mut topic = url_parsed.path();
        if topic.is_empty() || topic == "/" {
            error!("no topic given in MQTT URL path, exiting");
            return;
        }
        // remove leading slash
        topic = topic.trim_start_matches('/');
        debug!("topic: {}", topic);

        // subscribe to given topic
        //TODO enable reconnection - or is this done automatically via .iter()?
        client.subscribe(topic, QoS::AtMostOnce).unwrap();

        loop {
            trace!("begin of iteration");
            // check signals
            //TODO optimize, there is also try_recv() and recv_timeout()
            if let Ok(ip) = self.signals_in.try_recv() {
                //TODO optimize string conversions
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {   //TODO optimize comparison
                    info!("got stop signal, exiting");
                    break;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    self.signals_out.send(b"pong".to_vec()).expect("cloud not send pong");
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }

            // iterate to poll the eventloop for connection progress
            //TODO optimize is recv(), recv_timeout() or iter() better?
            while let Ok(event) = connection.recv_timeout(RECV_TIMEOUT) {
                match event {
                    Ok(Incoming(Publish(packet))) => {
                        debug!("Received payload from MQTT topic: {:?}", packet.payload);

                        // send it
                        debug!("forwarding MQTT payload...");
                        out.push(packet.payload.to_vec()).expect("could not push into OUT");    //TODO optimize conversion
                        out_wakeup.unpark();
                        debug!("done");
                    }
                    _ => {
                        trace!("Event = {:?}", event);
                    }
                }
            }

            // are we done?
            //TODO handle EOF on MQTT connection? or does it automatically reconnect? what if it fails to reconnect and we better shut down?
            /*
            if inn.is_abandoned() {
                //TODO EOF on MQTT connection
                info!("EOF on inport NAMES, shutting down");
                drop(out);
                out_wakeup.unpark();
                break;
            }
            */

            trace!("-- end of iteration");
            //NOTE: dont park thread here - this is done by recv_timeout()
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
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