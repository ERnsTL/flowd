use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use serde_json::Value;
use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};   //Error};
use jaq_parse;
//use jaq_core::core;
//use jaq_std::std;

/*
Ability to extract a value from a JSON data structure.
Use case is for example getting sensor value out of a JSON data structure loaded via HTTP from a CPS/IoT device.

structure from https://github.com/01mf02/jaq/blob/101619985b3b99707be8863727fa1c08f350559c/jaq/src/main.rs#L418
and https://docs.rs/jaq-interpret/1.2.1/jaq_interpret/

TODO add options from jaq, for example if result is a string if it should be quoted or not
*/

pub struct JSONQueryComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for JSONQueryComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        JSONQueryComponent {
            conf: inports.remove("QUERY").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("JSONQuery is now run()ning!");
        let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        trace!("read config IP");
        /*
        while conf.is_empty() {
            thread::yield_now();
        }
        */
        let Ok(filter_vec) = conf.pop() else { trace!("no config IP received - exiting"); return; };
        let filter_str = std::str::from_utf8(&filter_vec).expect("invalid utf-8 in config IP");

        // configure
        // start out only from core filters,
        // which do not include filters in the standard library
        // such as `map`, `select` etc.
        //TODO add support for standard library filters
        let mut parse_context = ParseCtx::new(Vec::new());

        // parse the filter
        let (filter, errors) = jaq_parse::parse(filter_str, jaq_parse::main());
        if errors.len() > 0 {
            for err in errors {
                error!("filter parse error: {:?} - exiting", err);
            }
            return;
        }

        // compile the filter in the context of the given definitions
        let filter = parse_context.compile(filter.unwrap());
        if parse_context.errs.len() > 0 {
            for (err, range) in parse_context.errs {
                error!("filter compile error: {:?} in character {} to {} - exiting", err.to_string(), range.start, range.end);
            }
            return;
        }

        let inputs = RcIter::new(core::iter::empty());

        // main loop
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
                    // prepare packet
                    debug!("got a packet, processing...");
                    let input: Value = serde_json::from_slice(&ip).expect("could not parse JSON from input IP");

                    // iterator over the output values
                    //TODO support for chunks via open bracket, closing bracket
                    let result = filter.run((Ctx::new([], &inputs), Val::from(input)));

                    //TODO send open bracket
                    /*
                    assert_eq!(out.next(), Some(Ok(Val::from(json!("Hello")))));
                    assert_eq!(out.next(), Some(Ok(Val::from(json!("world")))));
                    assert_eq!(out.next(), None);
                    */
                    for value in result {
                        match value {
                            Ok(val) => {
                                // prepare packet
                                let vec_out = format!("{}", val).into_bytes();  //TODO optimize - get rid of format!() maybe possible using .to_string() but what if result is a number or an object etc.?

                                // send it
                                debug!("sending...");
                                out.push(vec_out).expect("could not push into OUT");
                                out_wakeup.unpark();
                                debug!("done");
                            },
                            Err(err) => {
                                let vec_out = err.to_string().into_bytes();
                                error!("error while filtering: {} - discarding", std::str::from_utf8(&vec_out).expect("invalid utf-8"));
                            }
                        };
                    }
                    //TODO send closing bracket
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
            name: String::from("JSONQuery"),
            description: String::from("Reads IPs containing JSON data, filters them using jaq/jq and sends the processed resp. filtered data to the OUT port."),
            icon: String::from("filter"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("QUERY"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("filter to apply to the JSON data, in jaq/jq filter syntax"),
                    values_allowed: vec![],
                    value_default: String::from(".[]")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs to process, expected to contain JSON data"),
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
                    description: String::from("processed IPs with filtered JSON data"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}