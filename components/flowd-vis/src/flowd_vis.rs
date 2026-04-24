use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle,
    ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessSignalSink, ProcessSignalSource,
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

    fn run(self) {
        debug!("FlowdVis is now run()ning!");
        let mut inn = self.inn;
        loop {
            trace!("begin of iteration");

            // check signals
            if let Ok(ip) = self.signals_in.try_recv() {
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                );
                // stop signal
                if ip == b"stop" {
                    info!("got stop signal, exiting");
                    break;
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

            // check in port
            while !inn.is_empty() {
                if let Ok(_ip) = inn.pop() {
                    debug!("got a packet, sending preview URL");

                    // Send network output with preview URL
                    flowd_component_api::send_network_previewurl_comfortable(
                        &self.graph_inout,
                        "https://placehold.co/150x150".to_string(),
                    );
                } else {
                    break;
                }
            }

            // are we done?
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
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
