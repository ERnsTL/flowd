use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

pub struct TextReplaceComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    replacements: Vec<(String, String)>,
    config_complete: bool,
    pending_packets: std::collections::VecDeque<FbpMessage>,
}

impl Component for TextReplaceComponent {
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
        TextReplaceComponent {
            conf: inports
                .remove("CONF")
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
            //graph_inout: graph_inout,
            replacements: Vec::new(),
            config_complete: false,
            pending_packets: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("TextReplace is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration incrementally if not complete
        if !self.config_complete {
            // Read replacement pairs from CONF port
            while self.conf.slots() >= 2 {
                if let (Ok(from), Ok(to)) = (self.conf.pop(), self.conf.pop()) {
                    let from_str = from.as_text().expect("invalid text in replacement from");
                    let to_str = to.as_text().expect("invalid text in replacement to");
                    trace!("got replacement pair: from={} to={}", from_str, to_str);
                    self.replacements.push((from_str.to_string(), to_str.to_string()));
                } else {
                    break;
                }
            }

            // Check if configuration is complete (CONF port abandoned)
            if self.conf.is_abandoned() {
                self.config_complete = true;
                trace!(
                    "configuration complete, got {} replacements",
                    self.replacements.len()
                );
            } else if self.replacements.is_empty() {
                // No configuration available yet
                trace!("no configuration available yet");
                return ProcessResult::NoWork;
            }
        }

        // check signals
        if let Ok(ip) = self.signals_in.try_recv() {
            let signal_text = ip.as_text().unwrap_or("");
            trace!("received signal ip: {}", signal_text);
            // stop signal
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(FbpMessage::from_str("pong"));
            } else {
                warn!("received unknown signal ip: {}", signal_text)
            }
        }

        // First, try to send any pending packets that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_packets.is_empty() {
            if let Some(pending_ip) = self.pending_packets.front() {
                match self.out.push(pending_ip.clone()) {
                    Ok(()) => {
                        self.pending_packets.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        trace!("sent pending packet");
                    }
                    Err(PushError::Full(_)) => {
                        // Still can't send, stop trying for now
                        break;
                    }
                }
            }
        }

        // Then, check in port within remaining budget
        while context.remaining_budget > 0 {
            // stay responsive to stop/ping even while draining a busy input buffer
            if let Ok(sig) = self.signals_in.try_recv() {
                let signal_text = sig.as_text().unwrap_or("");
                trace!("received signal ip: {}", signal_text);
                if signal_text == "stop" {
                    info!("got stop signal while processing, finishing");
                    return ProcessResult::Finished;
                } else if signal_text == "ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(FbpMessage::from_str("pong"));
                }
            }

            if let Ok(ip) = self.inn.pop() {
                // read packet - expecting UTF-8 string
                let mut text = ip.as_text().expect("non-text data").to_string();
                debug!("got a text to process: {}", text);

                // apply text replacements
                for replacement in &self.replacements {
                    text = text.replace(replacement.0.as_str(), replacement.1.as_str());
                }

                // Try to send the processed packet
                debug!("forwarding...");
                let processed_msg = FbpMessage::from_str(&text);
                match self.out.push(processed_msg) {
                    Ok(()) => {
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("done");
                    }
                    Err(PushError::Full(returned_ip)) => {
                        // Output buffer full, buffer internally for later retry
                        debug!("output buffer full, buffering packet internally");
                        self.pending_packets.push_back(returned_ip);
                        work_units += 1; // We did process the packet, just couldn't forward
                        context.remaining_budget -= 1;
                        // Continue processing more packets that might fit
                    }
                }
            } else {
                break;
            }
        }

        // are we done?
        if self.inn.is_abandoned() {
            info!("EOF on inport, finishing");
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
