use std::sync::{Arc, Mutex};
use rtrb::PushError;

use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

pub struct EqualsComponent {
    cmp: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out_true: ProcessEdgeSink,
    out_false: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for EqualsComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        EqualsComponent {
            cmp: inports.remove("CMP").expect("found no CMP inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out_true: outports.remove("TRUE").expect("found no TRUE outport").pop().unwrap(),
            out_false: outports.remove("FALSE").expect("found no FALSE outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("Equals is now run()ning!");
        let cmp = &mut self.cmp;
        let inn = &mut self.inn;    //TODO optimize these references, not really needed for them to be referenes, can just consume?
        let out_true = &mut self.out_true.sink;
        let out_true_wakeup = self.out_true.wakeup.expect("got no wakeup handle for outport TRUE");
        let out_false = &mut self.out_false.sink;
        let out_false_wakeup = self.out_false.wakeup.expect("got no wakeup handle for outport FALSE");

        // read cmp until we get a value
        //TODO maybe support s-exprs here to support more complex comparisons
        trace!("read config IIP");
        let mut cmp_value = match cmp.pop() {
            Ok(cmp) => cmp,
            Err(_) => {
                error!("no config IP received - exiting");
                return;
            }
        };

        // main loop
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
                    trace!("checking if equals...");
                    if ip == cmp_value {
                        trace!("equality packet");
                        // try to push into TRUE
                        if let Err(PushError::Full(ip_ret)) = out_true.push(ip) {
                            // full, so wake up output-side component
                            out_true_wakeup.unpark();
                            while out_true.is_full() {
                                out_true_wakeup.unpark();
                                // wait     //TODO optimize
                                std::thread::yield_now();
                            }
                            // send nao
                            out_true.push(ip_ret).expect("could not push into TRUE - but said !is_full");
                        }
                        out_true_wakeup.unpark();
                    } else {
                        trace!("inequality packet");
                        // try to push into FALSE
                        if let Err(PushError::Full(ip_ret)) = out_false.push(ip) {
                            // full, so wake up output-side component
                            out_false_wakeup.unpark();
                            while out_false.is_full() {
                                out_false_wakeup.unpark();
                                // wait     //TODO optimize
                                std::thread::yield_now();
                            }
                            // send nao
                            out_false.push(ip_ret).expect("could not push into FALSE - but said !is_full");
                        }
                        out_false_wakeup.unpark();
                    }
                    trace!("done");
                } else {
                    break;
                }
            }

            // are we done?
            if inn.is_abandoned() {
                // input closed, nothing more to do
                info!("EOF on inport, shutting down");
                drop(out_true);
                out_true_wakeup.unpark();
                drop(out_false);
                out_false_wakeup.unpark();
                break;
            }

            // check for new cmp value
            if let Ok(cmp) = cmp.pop() {
                trace!("got new cmp value: {}", std::str::from_utf8(&cmp).expect("invalid utf-8"));
                cmp_value = cmp;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("Equals"),
            description: String::from("Sends each IP from IN port to TRUE or FALSE port, depending on whether it is equal to the latest IP from CMP port"),
            icon: String::from(""),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CMP"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to compare against - can be updated at runtime"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be routed to TRUE or FALSE outport"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("TRUE"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("repeated data from IN port if it is equal to the latest IP from CMP port"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("FALSE"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("repeated data from IN port if it is not equal to the latest IP from CMP port"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
            ],
            ..Default::default()
        }
    }
}