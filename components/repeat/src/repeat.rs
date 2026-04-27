use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult, PushError,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, info, trace};

pub struct RepeatComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    pending_out: std::collections::VecDeque<FbpMessage>,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for RepeatComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self {
        RepeatComponent {
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
            signals_in,
            signals_out,
            pending_out: std::collections::VecDeque::new(),
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Repeat is now process()ing!");
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
            let signal_text = ip.as_text().unwrap_or("");
            trace!("received signal ip: {}", signal_text);
            // stop signal
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(FbpMessage::from_str("pong"));
            }
        }

        // check in port within budget
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
                debug!("repeating packet...");
                match self.out.push(ip) {
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
            // input closed, nothing more to do
            info!("EOF on inport, finishing");
            return ProcessResult::Finished;
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload {
        ComponentComponentPayload {
            name: String::from("Repeat"),
            description: String::from("Copies data as-is from IN port to OUT port."),
            icon: String::from("arrow-right"), // or "copy"
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("IN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("data to be repeated on outport"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("repeated data from IN port"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}
