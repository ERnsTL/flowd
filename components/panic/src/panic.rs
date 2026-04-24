use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::warn;

pub struct PanicComponent {
    inn: ProcessEdgeSource,
    _out: ProcessEdgeSink,
    _signals_in: ProcessSignalSource,
    _signals_out: ProcessSignalSink,
}

impl Component for PanicComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
    ) -> Self {
        PanicComponent {
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            _out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            _signals_in: signals_in,
            _signals_out: signals_out,
        }
    }

    fn process(&mut self, _context: &mut NodeContext) -> ProcessResult {
        if self.inn.pop().is_ok() {
            // Intentionally panic for runtime failure-path validation.
            panic!("intentional panic from PanicComponent");
        }

        if self.inn.is_abandoned() {
            return ProcessResult::Finished;
        }

        warn!("PanicComponent idle: no message received");
        ProcessResult::NoWork
    }

    fn get_metadata() -> ComponentComponentPayload {
        ComponentComponentPayload {
            name: String::from("Panic"),
            description: String::from(
                "Testing component that intentionally panics when it receives input.",
            ),
            icon: String::from("warning"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("IN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("input that triggers an intentional panic"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: false,
                is_arrayport: false,
                description: String::from("unused output"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}
