use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult, ProcessSignalSink,
    ProcessSignalSource,
};
use log::{debug, info, trace, warn};

pub struct FlowdVisComponent {
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    graph_inout: GraphInportOutportHandle,
}

impl Component for FlowdVisComponent {
    fn new(
        mut inports: ProcessInports,
        _: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        FlowdVisComponent {
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("FlowdVis process() called");

        let mut work_units = 0u32;

        // Check signals first (signals are handled regardless of budget)
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

        // Process input within budget
        while context.remaining_budget > 0 {
            if let Ok(_ip) = self.inn.pop() {
                debug!("got a packet, sending preview URL");

                // Send network output with preview URL
                flowd_component_api::send_network_previewurl_comfortable(
                    &self.graph_inout,
                    "https://placehold.co/150x150".to_string(),
                );

                work_units += 1;
                context.remaining_budget -= 1;
            } else {
                break;
            }
        }

        // Check if we're done
        if self.inn.is_abandoned() {
            info!("EOF on inport, finishing");
            return ProcessResult::Finished;
        }

        // Signal readiness if we have pending input work
        if self.inn.slots() > 0 {
            context.signal_ready();
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
            name: String::from("Visualization"),
            description: String::from("Sends preview URLs when receiving packets."),
            icon: String::from("eye"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("IN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from(
                    "packets to trigger preview URL sending to graph -> clients",
                ),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![],
            ..Default::default()
        }
    }
}
