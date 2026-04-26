use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult, PushError,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, info, trace, warn};

pub struct TrimComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    pending_out: std::collections::VecDeque<Vec<u8>>,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for TrimComponent {
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
        TrimComponent {
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
            pending_out: std::collections::VecDeque::new(),
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Trim is now process()ing!");
        let mut work_units = 0u32;

        while context.remaining_budget > 0 && !self.pending_out.is_empty() {
            if let Some(ip) = self.pending_out.front().cloned() {
                match self.out.push(ip) {
                    Ok(()) => {
                        self.pending_out.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(PushError::Full(_)) => break,
                }
            }
        }

        // check signals
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            // stop signal
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

        // check in port within budget
        while context.remaining_budget > 0 {
            // stay responsive to stop/ping even while draining a busy input buffer
            if let Ok(sig) = self.signals_in.try_recv() {
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&sig).expect("invalid utf-8")
                );
                if sig == b"stop" {
                    info!("got stop signal while processing, finishing");
                    return ProcessResult::Finished;
                } else if sig == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                }
            }

            if let Ok(ip) = self.inn.pop() {
                // read packet - expecting UTF-8 string
                let mut text = std::str::from_utf8(&ip).expect("non utf-8 data");
                debug!("got a text to trim: {}", &text);

                // trim string
                debug!("len before trim: {}", text.len());
                text = text.trim();
                debug!("len after trim: {}", text.len());

                // send it
                debug!("forwarding trimmed string...");
                match self.out.push(Vec::from(text)) {
                    Ok(()) => {
                        debug!("done");
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(PushError::Full(returned)) => {
                        self.pending_out.push_back(returned);
                        break;
                    }
                }
            } else {
                break;
            }
        }

        // are we done?
        if self.inn.is_abandoned() && self.pending_out.is_empty() {
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
            name: String::from("Trim"),
            description: String::from("Reads IPs as UTF-8 strings and trims whitespace at beginning and end, forwarding the trimmed string."),
            icon: String::from("cut"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs with strings to trim, one string per IP"),
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
                    description: String::from("trimmed strings"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
