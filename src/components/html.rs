use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHandle, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use skyscraper::html;
use skyscraper::xpath::{self, XpathItemTree};

/*
CSS selectors seemingly can only select elements, not the contents of elements. Understandable, as CSS is for styling, not for selecting content.
XPath can select content, but is more complex and less intuitive than CSS selectors, but CSS selectors can be converted to XPath queries.
So this supports only XPath queries.
*/

pub struct HTMLStripComponent {
    //conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for HTMLStripComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        HTMLStripComponent {
            //conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("HTMLStrip is now run()ning!");
        //let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        //trace!("read config IPs");
        /*
        while conf.is_empty() {
            thread::yield_now();
        }
        */
        //let Ok(opts) = conf.pop() else { trace!("no config IP received - exiting"); return; };

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
                    // prepare packet
                    debug!("got a packet, stripping...");
                    // nothing to do

                    // strip HTML tags
                    let ip_out = HTMLStripComponent::remove_html_tags(ip);

                    // send it
                    debug!("sending...");
                    out.push(ip_out).expect("could not push into OUT");
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
            name: String::from("HTMLStrip"),
            description: String::from("Reads data IPs, strips all HTML tags and sends the cleaned, content-only data to the OUT port."),
            icon: String::from("trash"),
            subgraph: false,
            in_ports: vec![
                /*
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("TODO"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                */
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs with HTML code"),
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
                    description: String::from("HTML-stripped IPs"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

impl HTMLStripComponent {
    fn remove_html_tags(html: Vec<u8>) -> Vec<u8> {
        let mut result = Vec::new();
        let mut inside_tag = false;
        let mut inside_quotes = false;

        for &byte in html.iter() {
            if byte == b'<' {
                inside_tag = true;
            } else if byte == b'>' && !inside_quotes {
                inside_tag = false;
            } else if byte == b'"' {
                inside_quotes = !inside_quotes;
            } else if !inside_tag {
                result.push(byte);
            }
        }

        result
    }
}

pub struct HTMLQueryComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for HTMLQueryComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        HTMLQueryComponent {
            conf: inports.remove("QUERY").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("HTMLQuery is now run()ning!");
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
        let Ok(query_vec) = conf.pop() else { trace!("no config IP received - exiting"); return; };
        let query_str = std::str::from_utf8(&query_vec).expect("invalid utf-8 in config IP");

        // configure
        let xpath_query = xpath::parse(query_str).expect("failed to parse XPath query");

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
                if let Ok(ip) = inn.pop() { //TODO optimize - add support for chunks
                    // prepare packet
                    debug!("got a packet, processing...");

                    // process packet
                    let document = html::parse(std::str::from_utf8(&ip).expect("failed to use IP data as UTF-8")).expect("failed to parse IP as HTML document");
                    let xpath_item_tree = XpathItemTree::from(&document);
                    let item_set = xpath_query.apply(&xpath_item_tree).expect("failed to apply XPath query to HTML document");

                    // prepare output
                    if !item_set.is_empty() {
                        for item in item_set.into_iter() {
                            // prepare packet
                            let vec_out = item.to_string().into_bytes();    //TODO currently, this quotes output strings - add option to disable quoting and other output paramaters, or add un-quote component :-)

                            // send it
                            debug!("sending...");
                            out.push(vec_out).expect("could not push into OUT");
                            out_wakeup.unpark();
                            debug!("done");
                        }
                    } else {
                        debug!("XPath query did not match any elements - skipping IP");
                    }
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
            name: String::from("HTMLQuery"),
            description: String::from("Reads IPs containing HTML data, filters them using the given XPath query and sends the processed resp. filtered data to the OUT port."),
            icon: String::from("filter"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("QUERY"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("XPath query to apply to the input data"),
                    values_allowed: vec![],
                    value_default: String::from("//div/text()")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("HTML data to process"),
                    values_allowed: vec![],
                    value_default: String::from("<html><body><div>Hello world</div></body></html>")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("processed resp. filtered HTML data"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}