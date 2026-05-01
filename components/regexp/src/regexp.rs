use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, info, trace, warn};

// component-specific
use regex::Regex;

pub struct RegexpExtractComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    regexp: Option<Regex>,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for RegexpExtractComponent {
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
        RegexpExtractComponent {
            conf: inports
                .remove("REGEXP")
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
            regexp: None,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("RegexpExtract process() called");

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        // Check if we have configuration
        if self.regexp.is_none() {
            if let Ok(regexp_vec) = self.conf.pop() {
                let regexp_str = regexp_vec.as_text().expect("regexp must be text");
                debug!("received regexp: {}", regexp_str);
                match Regex::new(regexp_str) {
                    Ok(regexp) => {
                        self.regexp = Some(regexp);
                        return ProcessResult::DidWork(1); // Configuration processed
                    }
                    Err(err) => {
                        info!("failed to compile regexp: {}", err);
                        return ProcessResult::Finished; // Invalid config, finish
                    }
                }
            } else {
                // No config yet
                return ProcessResult::NoWork;
            }
        }

        let regexp = self.regexp.as_ref().unwrap();

        let mut work_units = 0;

        // Process available input packets within remaining budget
        while context.remaining_budget > 0 && !self.inn.is_empty() {
            if let Ok(ip) = self.inn.pop() {
                debug!("got packet, processing...");

                // Apply regexp
                let input_str = ip.as_text().unwrap_or("");
                if let Some(captures) = regexp.captures(input_str) {
                    // Get first capture
                    if let Some(capture) = captures.get(1) {
                        let capture_str = capture.as_str();

                        // Send results
                        debug!("sending...");
                        let msg = FbpMessage::from_str(capture_str);
                        if let Ok(()) = self.out.push(msg) {
                            debug!("done");
                            work_units += 1;
                            context.remaining_budget -= 1;
                        } else {
                            // Output buffer full, stop processing for now
                            break;
                        }
                    } else {
                        // No first capture, send empty
                        debug!("no first capture, sending empty");
                        let msg = FbpMessage::from_bytes(vec![]);
                        if let Ok(()) = self.out.push(msg) {
                            work_units += 1;
                            context.remaining_budget -= 1;
                        } else {
                            break;
                        }
                    }
                } else {
                    // No match, send empty value
                    info!("no match");
                    let msg = FbpMessage::from_bytes(vec![]);
                    if let Ok(()) = self.out.push(msg) {
                        work_units += 1;
                        context.remaining_budget -= 1;
                    } else {
                        break;
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
