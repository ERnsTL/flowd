use flowd_component_api::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHandle, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};
use log::{trace, debug, info};

// component-specific
use regex::Regex;

pub struct RegexpExtractComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for RegexpExtractComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        RegexpExtractComponent {
            conf: inports.remove("REGEXP").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("RegexpExtract is now run()ning!");
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
        let Ok(regexp_vec) = conf.pop() else { trace!("no config IP received - exiting"); return; };

        // configure
        let regexp = Regex::new(std::str::from_utf8(&regexp_vec).unwrap()).expect("failed to compile given regexp");

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
                    debug!("got packet, processing...");

                    // process
                    //TODO send each match to the according index on OUT port - number of match groups must be number of connections on OUT port
                    //TODO option for the future: send the matches as sexp object structure to the outport
                    // apply regexp
                    if let Some(captures) = regexp.captures(std::str::from_utf8(&ip).expect("failed to parse IP as UTF-8")) {
                        // get first capture
                        //captures.len() - TODO get all captures? put into object structure?
                        let capture = captures.get(1).expect("failed to get first capture");
                        // get the capture as string
                        let capture_str = capture.as_str();

                        // send results
                        out.push(capture_str.as_bytes().to_vec()).expect("could not push into OUT");
                        out_wakeup.unpark();
                        debug!("done");
                    } else {
                        // no match, send empty value
                        info!("no match");
                        out.push(vec![]).expect("could not push into OUT");
                        out_wakeup.unpark();
                        debug!("done");
                    }

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
            name: String::from("RegexpExtract"),
            description: String::from("Applies given regexp to IPs from IN port and sends the matched results to OUT port."),
            icon: String::from("search"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("REGEXP"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("the regular expression to apply"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to apply the given regexp to"),
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
                    description: String::from("extracted match data"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}