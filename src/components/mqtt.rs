use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

use std::time::Duration;
use rumqttc::{MqttOptions, Client, Event::Incoming, Packet::Publish, QoS};
use std::thread;

pub struct MQTTPublisherComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for MQTTPublisherComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        MQTTPublisherComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("MQTTPublisher is now run()ning!");
        let conf = &mut self.conf;
        let inn = &mut self.inn;    //TODO optimize these references, not really needed for them to be referenes, can just consume?

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url = std::str::from_utf8(&url_vec).expect("invalid utf-8");
        
        // prepare connection arguments
        let mut mqttoptions = MqttOptions::parse_url(url).expect("failed to parse MQTT URL");
        mqttoptions.set_keep_alive(Duration::from_secs(5));
        let (mut client, mut connection) = Client::new(mqttoptions, 10);

        // handle connection events
        //TODO automatic reconnection
        //TODO integration with main component thread signalling (MQTT -> FBP signalling and FBP signalling -> MQTT)
        //###
        //client.subscribe("hello/rumqtt", QoS::AtMostOnce).unwrap();
        thread::spawn(move || {
            // Iterate to poll the eventloop for connection progress
            //thread::sleep(Duration::from_millis(2000));
            for (i, notification) in connection.iter().enumerate() {
                println!("Notification = {:?}", notification);
                match notification {
                    Ok(Incoming(Publish(packet))) => {
                        println!("Received payload: {:?}", packet.payload);
                    }
                    _ => {}
                }
            }
        });

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
            */
            while !inn.is_empty() {
                //_ = inn.pop().ok();
                //debug!("got a packet, dropping it.");

                debug!("got {} packets, forwarding to MQTT topic.", inn.slots());
                let chunk = inn.read_chunk(inn.slots()).expect("receive as chunk failed");
                for ip in chunk.into_iter() {   //TODO is iterator faster or as_slices() or as_mut_slices() ?
                    //TODO make topic configurable
                    //###
                    client.publish("hello/rumqtt", QoS::AtLeastOnce, false, ip).expect("failed to publish");
                }
                // NOTE: no commit_all() necessary, because into_iter() does that automatically
            }

            // check in port
            /*
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

            // are we done?
            if inn.is_abandoned() {
                // input closed, nothing more to do
                info!("EOF on inport, shutting down");
                //###
                //TODO close MQTT connection
                //TODO signal MQTT event thread
                //drop(out);
                //out_wakeup.unpark();
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("MQTTPublisher"),
            description: String::from("Publishes data as-is from IN port to the MQTT topic given in CONF."),
            icon: String::from("arrow-right"),  //###
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
                    value_default: String::from("mqtts://test.mosquitto.org:8886?client_id=flowd")
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
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

const RECV_TIMEOUT: Duration = Duration::from_millis(250);

impl Component for MQTTSubscriberComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
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

        // subscribe to given topic
        //###
        //TODO enable reconnection - or is this done automatically via .iter()?
        client.subscribe("hello/rumqtt", QoS::AtMostOnce).unwrap();

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
            /*
            for event in connection.iter() {
                println!("Event = {:?}", event);
                match event {
                    Ok(Incoming(Publish(packet))) => {
                        println!("Received payload: {:?}", packet.payload);
                    }
                    _ => {}
                }
            }
            */
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

            // check in port
            //TODO while !inn.is_empty() {
            /*
            loop {
                if let Ok(ip) = conf.pop() {
                    // read filename on inport
                    let file_path = std::str::from_utf8(&ip).expect("non utf-8 data");
                    debug!("got a filename: {}", &file_path);

                    // read whole file
                    //TODO may be big file - add chunking
                    //TODO enclose files in brackets to know where its stream of chunks start and end
                    debug!("reading file...");
                    let contents = std::fs::read(file_path).expect("should have been able to read the file");

                    // send it
                    debug!("forwarding file contents...");
                    out.push(contents).expect("could not push into OUT");
                    out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }
            */

            // are we done?
            //### EOF on MQTT connection
            /*
            if conf.is_abandoned() {
                //TODO EOF on MQTT connection
                info!("EOF on inport NAMES, shutting down");
                drop(out);
                out_wakeup.unpark();
                break;
            }
            */

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("MQTTSubscriber"),
            description: String::from("Reads the contents of the given files and sends the contents."),
            icon: String::from("file"), //###
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
                    value_default: String::from("mqtts://test.mosquitto.org:8886?client_id=flowd")
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