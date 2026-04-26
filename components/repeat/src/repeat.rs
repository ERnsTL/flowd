use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, info, trace};

pub struct RepeatComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
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
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Repeat is now process()ing!");
        let mut work_units = 0u32;

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
                debug!("repeating packet...");
                self.out.push(ip).expect("could not push into OUT");
                debug!("done");
                work_units += 1;
                context.remaining_budget -= 1;
            } else {
                break;
            }
        }

        // are we done?
        if self.inn.is_abandoned() {
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
