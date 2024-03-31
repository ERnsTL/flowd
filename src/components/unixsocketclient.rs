use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use std::os::unix::net::UnixDatagram;
use std::io::prelude::*;
use std::os::unix::net::UnixStream;

pub struct UnixSocketClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for UnixSocketClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        UnixSocketClientComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("UnixSocketClient is now run()ning!");
        let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        trace!("read config IPs");
        /*
        while conf.is_empty() {
            thread::yield_now();
        }
        */
        let Ok(socket_addr) = conf.pop() else { trace!("no config IP received - exiting"); return; };
        //TODO abstract or path-based?
        //TODO buffer size?
        //TODO stream or datagram?
        // -> needs to be an URL

        // configure
        // NOTE: nothing to be done here

        // main loop
        loop {
            trace!("begin of iteration");

            // check signals
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
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
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

                debug!("got {} packets, dropping them.", inn.slots());  //###
                inn.read_chunk(inn.slots()).expect("receive as chunk failed").commit_all();

                //TODO automatic reconnection - reconnect timeout of 30s, then error out.

                //###
                //let sock = match UnixDatagram::unbound()
                //TODO bind
                //TODO connect = bind + connect_addr
                // unbound -> connect
                let socket = UnixDatagram::bind("/path/to/my/socket").expect("failed to bind");
                socket.send_to(b"hello world", "/path/to/other/socket").expect("failed to send");
                let mut buf = [0; 100];
                let (count, address) = socket.recv_from(&mut buf).expect("failed to receive");
                println!("socket {:?} sent {:?}", address, &buf[..count]);

                //TODO stream
                /*
                let mut stream = UnixStream::connect("/path/to/my/socket")?;
                stream.write_all(b"hello world")?;
                let mut response = String::new();
                stream.read_to_string(&mut response)?;
                println!("{response}");
                 */
            }

            // are we done?
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
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
            name: String::from("UnixSocketClient"),
            description: String::from("Drops all packets received on IN port."),    //###
            icon: String::from("trash-o"),  //###
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be dropped"),    //###
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be dropped"),
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
                    description: String::from("data to be dropped"),    //###
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}