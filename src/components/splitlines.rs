use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

pub struct SplitLinesComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for SplitLinesComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        SplitLinesComponent {
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("SplitLines is now run()ning!");
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");
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
                    self.signals_out.send(b"pong".to_vec()).expect("cloud not send pong");
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }
            // check in port
            loop {
                if let Ok(ip) = inn.pop() {
                    // read packet - expecting UTF-8 string
                    let text = std::str::from_utf8(&ip).expect("non utf-8 data");
                    debug!("got a text to split");

                    // split into lines and send them
                    //TODO split by \r\n as well?
                    //let split = text.split("\n"); //TODO optimize what is faster - this or text.lines() ?

                    // send it
                    debug!("forwarding lines...");
                    let mut iterations: usize = 0;
                    //for line in split {
                    for line in text.lines() {
                        //TODO optimize - next process gets woken up only once outport is full
                        //TODO optimize handover handling - maybe unpark every x lines?
                        //TODO optimize error handling, all these Ok, or_else() seem unefficient
                        /*
                        out.push(Vec::from(line)).or_else(|_| {
                            // wake up output component
                            out_wakeup.unpark();
                            while out.is_full() {
                                // wait
                            }
                            // send nao
                            out.push(Vec::from(line)).expect("could not push into OUT - but said !is_full");
                            Ok::<(), rtrb::PushError<MessageBuf>>(())
                        }).expect("could not push into OUT");
                        */
                        if let Err(_) = out.push(Vec::from(line)) {
                            // full, so wake up output-side component
                            out_wakeup.unpark();
                            //println!("out_wakeup has name: {}", out_wakeup.name().unwrap());
                            while out.is_full() {
                                out_wakeup.unpark();
                                // wait     //TODO optimize
                                std::thread::yield_now();
                            }
                            // send nao
                            out.push(Vec::from(line)).expect("could not push into OUT - but said !is_full");
                        }

                        // wake up the output-side process once there is some data to work on
                        // NOTE: This is an important optimization as otherwise the process on OUT port will only be woken up when the connection becomes full (see above)
                        //TODO optimize - but increment and bitwise equality should be cheap?
                        iterations += 1;
                        if iterations == 50 {
                            out_wakeup.unpark();
                        }
                    }
                    out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }

            // are we done?
            if inn.is_abandoned() {
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
            name: String::from("SplitLines"),
            description: String::from("Splits IP contents by newline (\\n) and forwards the parts in separate IPs."),
            icon: String::from("cut"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs with text to split"),
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
                    description: String::from("split lines"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}