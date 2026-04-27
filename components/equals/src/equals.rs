use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

pub struct EqualsComponent {
    cmp: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out_true: ProcessEdgeSink,
    out_false: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    cmp_value: Option<FbpMessage>,
    pending_packets: std::collections::VecDeque<(FbpMessage, bool)>, // (packet, route_to_true)
}

impl Component for EqualsComponent {
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
        EqualsComponent {
            cmp: inports
                .remove("CMP")
                .expect("found no CMP inport")
                .pop()
                .unwrap(),
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            out_true: outports
                .remove("TRUE")
                .expect("found no TRUE outport")
                .pop()
                .unwrap(),
            out_false: outports
                .remove("FALSE")
                .expect("found no FALSE outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
            cmp_value: None,
            pending_packets: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Equals is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration if not yet configured
        if self.cmp_value.is_none() {
            if let Ok(cmp_data) = self.cmp.pop() {
                trace!("got cmp value");
                self.cmp_value = Some(cmp_data);
            } else {
                trace!("no cmp value available yet");
                return ProcessResult::NoWork;
            }
        }

        let cmp_value = self.cmp_value.as_ref().unwrap();

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

        // First, try to send any pending packets that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_packets.is_empty() {
            if let Some((pending_ip, route_to_true)) = self.pending_packets.front() {
                let result = if *route_to_true {
                    self.out_true.push(pending_ip.clone())
                } else {
                    self.out_false.push(pending_ip.clone())
                };

                match result {
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

            if let Ok(ip) = self.inn.pop() {
                trace!("checking if equals...");
                let route_to_true = ip == *cmp_value;

                if route_to_true {
                    trace!("equality packet");
                } else {
                    trace!("inequality packet");
                }

                // Try to push to appropriate output
                let result = if route_to_true {
                    self.out_true.push(ip.clone())
                } else {
                    self.out_false.push(ip.clone())
                };

                match result {
                    Ok(()) => {
                        work_units += 1;
                        context.remaining_budget -= 1;
                        trace!("routed packet successfully");
                    }
                    Err(PushError::Full(returned_ip)) => {
                        // Output buffer full, buffer internally for later retry
                        trace!("output buffer full, buffering packet internally");
                        self.pending_packets.push_back((returned_ip, route_to_true));
                        work_units += 1; // We did process the packet, just couldn't forward
                        context.remaining_budget -= 1;
                        // Continue processing more packets that might fit
                    }
                }
            } else {
                break;
            }
        }

        // Check for new cmp value (runtime updates)
        if let Ok(new_cmp) = self.cmp.pop() {
            trace!("got new cmp value");
            self.cmp_value = Some(new_cmp);
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
            name: String::from("Equals"),
            description: String::from("Sends each IP from IN port to TRUE or FALSE port, depending on whether it is equal to the latest IP from CMP port"),
            icon: String::from(""),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CMP"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to compare against - can be updated at runtime"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be routed to TRUE or FALSE outport"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("TRUE"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("repeated data from IN port if it is equal to the latest IP from CMP port"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("FALSE"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("repeated data from IN port if it is not equal to the latest IP from CMP port"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
            ],
            ..Default::default()
        }
    }
}
