use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use std::thread;

pub struct TextReplaceComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for TextReplaceComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        TextReplaceComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("TextReplace is now run()ning!");
        let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        trace!("read config IPs");
        let mut replacements: Vec<(String, String)> = Vec::new();
        // NOTE: this condition ensures that when the inport is abandoned, we still process the remaining replacements
        // and any remaining single one is ignored, eg. the usual last line in a text file on Unix-like systems
        while !conf.is_abandoned() || conf.slots() >= 2 {
            // wait for packet
            while conf.is_empty() {
                thread::yield_now();
            }
            // read pairs
            while conf.slots() >= 2 {
                let from = conf.pop().expect("failed to pop CONF replacement from IP");
                let to = conf.pop().expect("failed to pop CONF replacement to IP");
                let entry = (String::from_utf8(from).unwrap(), String::from_utf8(to).unwrap());
                trace!("got replacement pair: from={} to={}", entry.0, entry.1);
                replacements.push(entry);
            }
        }
        trace!("got text replacements: {}", replacements.len());

        // configure
        // NOTE: nothing to be done here

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
                    // read packet - expecting UTF-8 string
                    //TODO optimize - go through these lines and check for clones
                    let mut text = String::from_utf8(ip).expect("non utf-8 data");
                    debug!("got a text to process: {}", text);

                    // apply text replacements
                    //TODO feature - add version using regular expressions
                    //TODO feature - add search and replacement of \r, \n, \t etc. as well
                    for replacement in &replacements {
                        text = text.replace(replacement.0.as_str(), replacement.1.as_str());    //TODO optimize - better &String or String.as_str() ?
                    }

                    // send it
                    debug!("forwarding...");
                    //TODO optimize .to_vec() copies the contents - is Vec::from faster?
                    out.push(text.into_bytes()).expect("could not push into OUT");
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
            name: String::from("TextReplace"),
            description: String::from("Reads IPs as UTF-8 strings, applies text replacements and forwards the processed string IPs."),
            icon: String::from("cut"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs in a multiple of two with text replacements, first line to search for, second to replace it with"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("string IPs to process"),
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
                    description: String::from("IPs with strings, replacements applied"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}