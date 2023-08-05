use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

pub struct CountComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for CountComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        CountComponent {
            inn: inports.remove("IN").expect("found no IN inport"),
            out: outports.remove("OUT").expect("found no OUT outport"),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout,
        }
    }

    fn run(mut self) {
        debug!("Count is now run()ning!");
        let inn = &mut self.inn;
        let out = &mut self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");
        let mut packets: usize = 0;
        let start = chrono::Utc::now();
        let mut start_1st = chrono::Utc::now();
        loop {
            trace!("begin of iteration");

            // check signals
            if let Ok(ip) = self.signals_in.try_recv() {
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
            //TODO add reset port
            //TODO add triggered report by sending something into REPORT port
            //TODO add ability to forward as well (output count on separate port?)
            //TODO add counting of packet sizes, certain metadata etc.
            if packets == 0 {
                start_1st = chrono::Utc::now();
            }
            while !inn.is_empty() {
                // drop IP and count it
                if let Ok(chunk) = inn.read_chunk(inn.slots()) {
                    //debug!("got {} packets", chunk.len());
                    packets += chunk.len();
                    chunk.commit_all();
                    // TODO optimize, when we got a full buffer, we could assume there is more coming and wait a bit longer
                } else {
                    break;
                }
            }

            // check for EOF on input
            if inn.is_abandoned() {
                // send final report
                info!("EOF on inport, shutting down");
                let end = chrono::Utc::now();
                info!("received {} packets, total time: {}, since 1st packet: {}", packets, end - start, end - start_1st);
                out.push(format!("{}", packets).into_bytes()).expect("could not push into OUT");   //TODO optimize https://docs.rs/itoa/latest/itoa/
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
            name: String::from("Count"),
            description: String::from("Counts the number of packets, discarding them, and sending the packet count every 1M packets."), //TODO
            icon: String::from("bar-chart"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs to count"),
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
                    description: String::from("reports count on this outport"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}