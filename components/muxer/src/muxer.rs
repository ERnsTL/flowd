use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};
use std::thread;

pub struct MuxerComponent {
    inn: Vec<ProcessEdgeSource>,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for MuxerComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
    ) -> Self
    where
        Self: Sized,
    {
        MuxerComponent {
            inn: inports.remove("IN").expect("found no IN inport"),
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Muxer process() called");

        // Check signals first
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
            }
        }

        let mut work_units = 0;

        // Process available packets from all input ports within remaining budget
        for inport in self.inn.iter_mut() {
            while context.remaining_budget > 0 && !inport.is_empty() {
                if let Ok(ip) = inport.pop() {
                    debug!("multiplexing packet...");

                    // Try to send to output
                    match self.out.push(ip) {
                        Ok(()) => {
                            debug!("done");
                            work_units += 1;
                            context.remaining_budget -= 1;
                        }
                        Err(PushError::Full(_)) => {
                            // Output buffer full, stop processing for now
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

            if context.remaining_budget == 0 {
                break;
            }
        }

        // Check if all inports are abandoned
        if self.inn.iter().all(|x| x.is_abandoned()) {
            info!("EOF on all inports, shutting down");
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
            name: String::from("Muxer"),
            description: String::from("Copies data as-is from IN port(s) to single OUT port."),
            icon: String::from("dedent"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("IN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: true,
                description: String::from("IPs to be multiplexed into outport"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("multiplexed IPs from IN port"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}

pub struct Demux3Component {
    inn: ProcessEdgeSource,
    out_a: ProcessEdgeSink,
    out_b: ProcessEdgeSink,
    out_c: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    branch: usize,
}

impl Component for Demux3Component {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
    ) -> Self {
        Demux3Component {
            inn: inports
                .remove("IN")
                .expect("demux missing IN")
                .pop()
                .unwrap(),
            out_a: outports
                .remove("A")
                .expect("demux missing A")
                .pop()
                .unwrap(),
            out_b: outports
                .remove("B")
                .expect("demux missing B")
                .pop()
                .unwrap(),
            out_c: outports
                .remove("C")
                .expect("demux missing C")
                .pop()
                .unwrap(),
            signals_in,
            signals_out,
            branch: 0,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Demux3 process() called");

        // Check signals first
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
            }
        }

        let mut work_units = 0;

        // Process available packets within remaining budget
        while context.remaining_budget > 0 && !self.inn.is_empty() {
            if let Ok(ip) = self.inn.pop() {
                debug!("demultiplexing packet...");

                // Route to appropriate output based on round-robin
                let result = match self.branch % 3 {
                    0 => self.out_a.push(ip),
                    1 => self.out_b.push(ip),
                    _ => self.out_c.push(ip),
                };

                match result {
                    Ok(()) => {
                        debug!("done");
                        work_units += 1;
                        context.remaining_budget -= 1;
                        self.branch += 1;
                    }
                    Err(PushError::Full(_)) => {
                        // Output buffer full, stop processing for now
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

    fn get_metadata() -> ComponentComponentPayload {
        ComponentComponentPayload {
            name: "Demux3".to_string(),
            description: "Demux router (IN -> A|B|C round-robin)".to_string(),
            icon: "random".to_string(),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: "IN".to_string(),
                allowed_type: "any".to_string(),
                ..ComponentPort::default_in()
            }],
            out_ports: vec![
                ComponentPort {
                    name: "A".to_string(),
                    allowed_type: "any".to_string(),
                    ..ComponentPort::default_out()
                },
                ComponentPort {
                    name: "B".to_string(),
                    allowed_type: "any".to_string(),
                    ..ComponentPort::default_out()
                },
                ComponentPort {
                    name: "C".to_string(),
                    allowed_type: "any".to_string(),
                    ..ComponentPort::default_out()
                },
            ],
            ..Default::default()
        }
    }
}

fn push_blocking(
    sink: &mut flowd_component_api::ProcessEdgeSinkConnection,
    wake: &thread::Thread,
    mut packet: Vec<u8>,
    signals_in: &ProcessSignalSource,
    signals_out: &ProcessSignalSink,
) -> bool {
    loop {
        match sink.push(packet) {
            Ok(()) => {
                wake.unpark();
                return false;
            }
            Err(PushError::Full(returned)) => {
                packet = returned;
                wake.unpark();
                if let Ok(signal) = signals_in.try_recv() {
                    trace!(
                        "received signal ip while waiting for outport: {}",
                        std::str::from_utf8(&signal).expect("invalid utf-8")
                    );
                    if signal == b"stop" {
                        return true;
                    } else if signal == b"ping" {
                        let _ = signals_out.try_send(b"pong".to_vec());
                    } else {
                        warn!(
                            "received unknown signal ip: {}",
                            std::str::from_utf8(&signal).expect("invalid utf-8")
                        );
                    }
                }
                thread::yield_now();
            }
        }
    }
}
