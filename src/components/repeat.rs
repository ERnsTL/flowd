use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

pub struct RepeatComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for RepeatComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        RepeatComponent {
            inn: inports.remove("IN").expect("found no IN inport"),
            out: outports.remove("OUT").expect("found no OUT outport"),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("Repeat is now run()ning!");
        let inn = &mut self.inn;    //TODO optimize these references, not really needed for them to be referenes, can just consume?
        let out = &mut self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");
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

                    // small benchmark
                    // (2022-08-28) at commit 561927 currently at least 2x as fast as latest Go flowd, with perfect scheduling situation even 6x as fast
                    /*
                    info!("sending 1M packets...");
                    let now1 = chrono::Utc::now();
                    for n in 1..1000000 {
                        while out.is_full() {
                            // wait
                            //out_wakeup.unpark();
                            trace!("waiting");
                            //thread::yield_now();
                        }
                        out.push(Vec::from("bla")).unwrap();
                        if n % 100 == 0 { out_wakeup.unpark(); }
                    }
                    let now2 = chrono::Utc::now();
                    info!("done");
                    info!("time: {}ms", (now2 - now1).num_milliseconds());
                    */
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
            icon: String::from("arrow-right"),  // or "copy"
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