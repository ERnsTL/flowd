use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

// component-specific
//use std::io::BufRead;

pub struct SplitLinesComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // State for partial processing
    line_iter: Option<std::vec::IntoIter<FbpMessage>>,
}

impl Component for SplitLinesComponent {
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
        SplitLinesComponent {
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
            line_iter: None,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("SplitLines is now process()ing!");
        let mut work_units = 0u32;

        // check signals
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            // stop signal
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

        // check in port within budget
        if context.remaining_budget > 0 {
            // stay responsive to stop/ping even while draining a busy input buffer
            if let Ok(sig) = self.signals_in.try_recv() {
                let sig_text = sig.as_text()
                    .or_else(|| sig.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                    .unwrap_or("");
                trace!("received signal: {}", sig_text);
                if sig_text == "stop" {
                    info!("got stop signal while processing, finishing");
                    return ProcessResult::Finished;
                } else if sig_text == "ping" {
                    trace!("got ping signal, responding");
                    let pong_msg = FbpMessage::from_str("pong");
                    let _ = self.signals_out.try_send(pong_msg);
                }
            }

            // If we don't have an active iterator, try to get new input
            if self.line_iter.is_none() {
                if let Ok(ip) = self.inn.pop() {
                    // read packet - expecting UTF-8 string
                    debug!("got a text to split");

                    // split into lines
                    let ip_bytes = ip.as_bytes().unwrap_or(&[]);
                    let lines: Vec<FbpMessage> =
                        ip_bytes.split(|&x| x == b'\n').map(|x| FbpMessage::from_bytes(x.to_vec())).collect();
                    self.line_iter = Some(lines.into_iter());
                    debug!(
                        "created iterator with {} lines",
                        self.line_iter.as_ref().unwrap().len()
                    );
                }
            }

            // Send lines from current iterator within remaining budget
            if let Some(ref mut line_iter) = self.line_iter {
                let mut lines_sent = 0u32;

                while context.remaining_budget > 0 {
                    if let Some(line) = line_iter.next() {
                        match self.out.push(line) {
                            Ok(_) => {
                                lines_sent += 1;
                                context.remaining_budget -= 1;
                            }
                            Err(PushError::Full(_)) => {
                                // Output buffer full, can't send more this cycle
                                break;
                            }
                        }
                    } else {
                        // No more lines from current input
                        self.line_iter = None;
                        break;
                    }
                }

                if lines_sent > 0 {
                    work_units += 1;
                    debug!("sent {} lines", lines_sent);
                }
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
            name: String::from("SplitLines"),
            description: String::from(
                "Splits IP contents by newline (\\n) and forwards the parts in separate IPs.",
            ),
            icon: String::from("cut"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("IN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("IPs with text to split"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("split lines"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}
