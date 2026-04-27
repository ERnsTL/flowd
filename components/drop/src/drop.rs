use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult, ProcessSignalSink,
    ProcessSignalSource,
};
use log::{debug, info, trace, warn};

pub struct DropComponent {
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for DropComponent {
    fn new(
        mut inports: ProcessInports,
        _: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        DropComponent {
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Drop is now process()ing!");
        let mut work_units = 0u32;

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

        // check in port within budget
        while context.remaining_budget > 0 && !self.inn.is_empty() {
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

            let available = self.inn.slots();
            let to_process = available.min(context.remaining_budget as usize);
            if to_process > 0 {
                if let Ok(chunk) = self.inn.read_chunk(to_process) {
                    let num = chunk.len() as u32;
                    chunk.commit_all();
                    work_units += num;
                    context.remaining_budget -= num;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            // are we done?
            if self.inn.is_abandoned() {
                info!("EOF on inport, finishing");
                return ProcessResult::Finished;
            }
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("Drop"),
            description: String::from("Drops all packets received on IN port."),
            icon: String::from("trash-o"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("IN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("data to be dropped"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![],
            ..Default::default()
        }
    }
}
