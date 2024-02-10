use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

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
        if let Ok(ip) = conf.pop() {
            //TODO the cron crate has a non-standard 7-parameter form ranging down to seconds and up to years, is that good? cron-parser has POSIX 5-parameter format
            schedule = Schedule::from_str(std::str::from_utf8(&ip).expect("invalid utf-8")).unwrap().upcoming_owned(Local); //TODO error handling
        } else {
            error!("no config IP received - exiting");
            return;
        }

        let mut mqttoptions = MqttOptions::parse_url("mqtts://test.mosquitto.org:8886?client_id=flowd").expect("failed to parse MQTT URL");	//new("rumqtt-sync", "test.mosquitto.org", 8886);	//1883);
        mqttoptions.set_keep_alive(Duration::from_secs(5));
    
        let (mut client, mut connection) = Client::new(mqttoptions, 10);
    
        client.subscribe("hello/rumqtt", QoS::AtMostOnce).unwrap();
    
        thread::spawn(move || for i in 0..10 {
            client.publish("hello/rumqtt", QoS::AtLeastOnce, false, format!("msg {}", i)).unwrap();
            thread::sleep(Duration::from_millis(1000));
        });
    
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

            debug!("got {} packets, dropping them.", inn.slots());
            inn.read_chunk(inn.slots()).expect("receive as chunk failed").commit_all();
        }





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

            // are we done?
            if inn.is_abandoned() {
                // input closed, nothing more to do
                info!("EOF on inport, shutting down");
                drop(out);
                out_wakeup.unpark();
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("Repeat"),
            description: String::from("Copies data as-is from IN port to OUT port."),
            icon: String::from("arrow-right"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be repeated on outport"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("repeated data from IN port"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

pub struct MQTTSubscriberComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for MQTTSubscriberComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        MQTTSubscriberComponent {
            inn: inports.remove("NAMES").expect("found no NAMES inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("MQTTSubscriber is now run()ning!");
        let filenames = &mut self.inn;    //TODO optimize
        let out = &mut self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        if let Ok(ip) = when.pop() {
            //TODO the cron crate has a non-standard 7-parameter form ranging down to seconds and up to years, is that good? cron-parser has POSIX 5-parameter format
            schedule = Schedule::from_str(std::str::from_utf8(&ip).expect("invalid utf-8")).unwrap().upcoming_owned(Local); //TODO error handling
        } else {
            error!("no config IP received - exiting");
            return;
        }

        let mut mqttoptions = MqttOptions::parse_url("mqtts://test.mosquitto.org:8886?client_id=flowd").expect("failed to parse MQTT URL");	//new("rumqtt-sync", "test.mosquitto.org", 8886);	//1883);
        mqttoptions.set_keep_alive(Duration::from_secs(5));
    
        let (mut client, mut connection) = Client::new(mqttoptions, 10);
    
        client.subscribe("hello/rumqtt", QoS::AtMostOnce).unwrap();
    
        thread::spawn(move || for i in 0..10 {
            client.publish("hello/rumqtt", QoS::AtLeastOnce, false, format!("msg {}", i)).unwrap();
            thread::sleep(Duration::from_millis(1000));
        });
    
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
            // check in port
            //TODO while !inn.is_empty() {
            loop {
                if let Ok(ip) = filenames.pop() {
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

            // are we done?
            if filenames.is_abandoned() {
                info!("EOF on inport NAMES, shutting down");
                drop(out);
                out_wakeup.unpark();
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("MQTTSubscriber"),
            description: String::from("Reads the contents of the given files and sends the contents."),
            icon: String::from("file"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("filenames, one per IP"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("conents of the given files"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}