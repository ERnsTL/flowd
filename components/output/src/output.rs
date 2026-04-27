use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

pub struct OutputComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Internal buffer for handling backpressure
    pending_packets: std::collections::VecDeque<FbpMessage>,
}

impl Component for OutputComponent {
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
        OutputComponent {
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
            pending_packets: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Output is now process()ing!");
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

        // First, try to send any pending packets that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_packets.is_empty() {
            if let Some(pending_ip) = self.pending_packets.front() {
                match self.out.push(pending_ip.clone()) {
                    Ok(()) => {
                        self.pending_packets.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("sent pending packet");
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
                // output the packet data with newline
                debug!("got a packet, printing:");
                let text = ip.as_text().expect("non text data");
                println!("{}", text);

                // repeat - handle backpressure
                debug!("repeating packet...");
                match self.out.push(ip.clone()) {
                    Ok(()) => {
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("done");
                    }
                    Err(PushError::Full(returned_ip)) => {
                        // Output buffer full, buffer internally for later retry
                        debug!("output buffer full, buffering packet internally");
                        self.pending_packets.push_back(returned_ip);
                        work_units += 1; // We did process the packet (printed it), just couldn't forward
                        context.remaining_budget -= 1;
                        // Continue processing more packets that might fit
                    }
                }
            } else {
                break;
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

    // modeled after https://github.com/noflo/noflo-core/blob/master/components/Output.js
    // what node.js console.log() does:  https://nodejs.org/api/console.html#consolelogdata-args
    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("Output"),
            description: String::from("Prints packet data to stdout and repeats packet."),
            icon: String::from("eye"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("IN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("data to be printed and repeated on outport"),
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
