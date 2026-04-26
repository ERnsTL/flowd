use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, info, trace, warn};

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
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        HTMLStripComponent {
            //conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("HTMLStrip process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!(
                    "received unknown signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                )
            }
        }

        let mut work_units = 0;

        // Process available input packets within remaining budget
        while context.remaining_budget > 0 && !self.inn.is_empty() {
            if let Ok(ip) = self.inn.pop() {
                debug!("got a packet, stripping...");

                // Strip HTML tags
                let ip_out = HTMLStripComponent::remove_html_tags(ip);

                // Send it
                debug!("sending...");
                if let Ok(()) = self.out.push(ip_out) {
                    debug!("done");
                    work_units += 1;
                    context.remaining_budget -= 1;
                } else {
                    // Output buffer full, stop processing for now
                    break;
                }
            } else {
                break;
            }
        }

        // Check if input is abandoned
        if self.inn.is_abandoned() {
            info!("EOF on inport, shutting down");
            return ProcessResult::Finished;
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
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
    query: Option<xpath::Xpath>,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for HTMLQueryComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        HTMLQueryComponent {
            conf: inports
                .remove("QUERY")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            query: None,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("HTMLQuery process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!(
                    "received unknown signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                )
            }
        }

        // Check if we have configuration
        if self.query.is_none() {
            if let Ok(query_vec) = self.conf.pop() {
                let query_str =
                    std::str::from_utf8(&query_vec).expect("invalid utf-8 in config IP");
                debug!("received XPath query: {}", query_str);
                match xpath::parse(query_str) {
                    Ok(xpath_query) => {
                        self.query = Some(xpath_query);
                        return ProcessResult::DidWork(1); // Configuration processed
                    }
                    Err(err) => {
                        warn!("failed to parse XPath query: {}", err);
                        return ProcessResult::Finished; // Invalid config, finish
                    }
                }
            } else {
                // No config yet
                return ProcessResult::NoWork;
            }
        }

        let xpath_query = self.query.as_ref().unwrap();

        let mut work_units = 0;

        // Process available input packets within remaining budget
        while context.remaining_budget > 0 && !self.inn.is_empty() {
            if let Ok(ip) = self.inn.pop() {
                debug!("got a packet, processing...");

                // Process packet
                match html::parse(std::str::from_utf8(&ip).expect("failed to use IP data as UTF-8"))
                {
                    Ok(document) => {
                        let xpath_item_tree = XpathItemTree::from(&document);
                        match xpath_query.apply(&xpath_item_tree) {
                            Ok(item_set) => {
                                // Prepare output
                                if !item_set.is_empty() {
                                    for item in item_set.into_iter() {
                                        // Prepare packet
                                        let vec_out = item.to_string().into_bytes();

                                        // Send it
                                        debug!("sending...");
                                        if let Ok(()) = self.out.push(vec_out) {
                                            debug!("done");
                                            work_units += 1;
                                            context.remaining_budget -= 1;
                                        } else {
                                            // Output buffer full, stop processing for now
                                            break;
                                        }
                                    }
                                } else {
                                    debug!("XPath query did not match any elements - skipping IP");
                                }
                            }
                            Err(err) => {
                                warn!("failed to apply XPath query: {}", err);
                            }
                        }
                    }
                    Err(err) => {
                        warn!("failed to parse IP as HTML: {}", err);
                    }
                }

                if context.remaining_budget == 0 {
                    break;
                }
            } else {
                break;
            }
        }

        // Check if input is abandoned
        if self.inn.is_abandoned() {
            info!("EOF on inport, shutting down");
            return ProcessResult::Finished;
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
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
