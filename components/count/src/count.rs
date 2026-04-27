use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

pub struct CountComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    graph_inout: GraphInportOutportHandle,
    // Runtime state
    mode: Option<Mode>,
    packets: usize,
    packetsize: usize,
    sum: u64,
    start: Option<chrono::DateTime<chrono::Utc>>,
    start_1st: Option<chrono::DateTime<chrono::Utc>>,
    pending_reports: std::collections::VecDeque<flowd_component_api::FbpMessage>,
}

#[derive(Clone, Copy)]
enum Mode {
    Packets,
    Size,
    Sum,
}

impl CountComponent {
    fn emit_final_count(&mut self, mode: Mode) -> String {
        let final_count = match mode {
            //TODO optimize - instead of format try https://docs.rs/itoa/latest/itoa/
            Mode::Packets => format!("{}", self.packets),
            Mode::Size => format!("{}", self.packetsize),
            Mode::Sum => format!("{}", self.sum),
        };

        let message = flowd_component_api::FbpMessage::from_text(final_count.clone());
        if let Err(flowd_component_api::PushError::Full(returned)) =
            self.out.push(message)
        {
            // Store the returned message in pending_reports
            self.pending_reports.push_back(returned);
        }

        // Send network output with the final count
        flowd_component_api::send_network_output_comfortable(
            &self.graph_inout,
            final_count.clone(),
        );

        final_count
    }
}

impl Component for CountComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        CountComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
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
            graph_inout,
            mode: None,
            packets: 0,
            packetsize: 0,
            sum: 0,
            start: None,
            start_1st: None,
            pending_reports: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Count is now process()ing!");

        // Try to read configuration if not yet configured
        if self.mode.is_none() {
            trace!("reading config IP");
            if let Ok(config_msg) = self.conf.pop() {
                let raw_conf = config_msg.as_text()
                    .or_else(|| config_msg.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                    .unwrap_or("");
                let query = raw_conf.strip_prefix('?').unwrap_or(raw_conf);
                let url_str = "https://makeurlhappy/?".to_owned() + query;
                let url =
                    url::Url::parse(url_str.as_str()).expect("failed to parse configuration URL");

                if let Some((_key, value)) = url.query_pairs().find(|(key, _)| key == "mode") {
                    self.mode = match value.to_string().as_str() {
                        "packets" => Some(Mode::Packets),
                        "size" => Some(Mode::Size),
                        "sum" => Some(Mode::Sum),
                        _ => {
                            error!("unexpected mode, expected packets|size|sum - finishing");
                            return ProcessResult::Finished;
                        }
                    };
                    self.start = Some(chrono::Utc::now());
                } else {
                    error!("no mode found in configuration URL - finishing");
                    return ProcessResult::Finished;
                }
            }
        }

        // If not configured yet, can't process
        let Some(mode) = self.mode else {
            trace!("not configured yet - no work");
            return ProcessResult::NoWork;
        };
        let mut work_units = 0u32;

        while context.remaining_budget > 0 && !self.pending_reports.is_empty() {
            if let Some(report) = self.pending_reports.front().cloned() {
                match self.out.push(report) {
                    Ok(()) => {
                        self.pending_reports.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(flowd_component_api::PushError::Full(_)) => break,
                }
            }
        }

        // check signals
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);

            // stop signal
            if signal_text == "stop" {
                if let Some(mode) = self.mode {
                    let _ = self.emit_final_count(mode);
                }
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = flowd_component_api::FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        // check in port within budget
        if self.packets == 0 && !self.inn.is_empty() {
            self.start_1st = Some(chrono::Utc::now());
        }

        while context.remaining_budget > 0 && !self.inn.is_empty() {
            // stay responsive to stop/ping even while draining a busy input buffer
            if let Ok(sig) = self.signals_in.try_recv() {
                let sig_text = sig.as_text()
                    .or_else(|| sig.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                    .unwrap_or("");
                trace!("received signal during processing: {}", sig_text);
                if sig_text == "stop" {
                    if let Some(mode) = self.mode {
                        let _ = self.emit_final_count(mode);
                    }
                    info!("got stop signal while processing, finishing");
                    return ProcessResult::Finished;
                } else if sig_text == "ping" {
                    trace!("got ping signal during processing, responding");
                    let pong_msg = flowd_component_api::FbpMessage::from_str("pong");
                    let _ = self.signals_out.try_send(pong_msg);
                }
            }

            let available = self.inn.slots();
            let to_process = available.min(context.remaining_budget as usize);
            if to_process > 0 {
                if let Ok(chunk) = self.inn.read_chunk(to_process) {
                    let num = chunk.len() as u32;
                    //debug!("got {} packets", chunk.len());
                    //TODO optimize - instead of if on every IP, prepare Fn and apply it without branching
                    match mode {
                        Mode::Packets => {
                            self.packets += chunk.len();
                            chunk.commit_all();
                        }
                        Mode::Size => {
                            for msg in chunk.into_iter() {
                                // For size mode, count bytes in the message
                                let msg_size = match &msg {
                                    flowd_component_api::FbpMessage::Bytes(data) => data.len(),
                                    flowd_component_api::FbpMessage::Text(text) => text.len(),
                                    flowd_component_api::FbpMessage::Value(_) => 0, // Values don't have inherent size
                                    flowd_component_api::FbpMessage::Control(_) => 0, // Controls don't have inherent size
                                };
                                self.packetsize += msg_size;
                            }
                            // NOTE: no need for commit_all(), because into_iter() does that automatically
                        }
                        Mode::Sum => {
                            for msg in chunk {
                                // For sum mode, try to parse numeric values from messages
                                let value_str = msg.as_text()
                                    .or_else(|| msg.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                                    .unwrap_or("");
                                if let Some(value) = atoi::atoi::<u64>(value_str.as_bytes()) {
                                    self.sum += value;
                                } else {
                                    error!("value '{}' cannot be summed up - skipping", value_str);
                                }
                            }
                            // NOTE: no need for commit_all(), because into_iter() does that automatically
                        }
                    }
                    work_units += num;
                    context.remaining_budget -= num;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Emit current totals when input goes idle so downstream observers can
        // see progress without requiring an EOF/stop race to flush.
        if work_units > 0 && self.inn.is_empty() && !self.inn.is_abandoned() {
            let _ = self.emit_final_count(mode);
        }

        // check for EOF on input
        if self.inn.is_abandoned() && self.pending_reports.is_empty() {
            // send final report
            info!("EOF on inport, finishing");
            let end = chrono::Utc::now();
            let start = self.start.unwrap();
            let start_1st = self.start_1st.unwrap_or(start);

            info!(
                "total time: {}, since 1st packet: {}",
                end - start,
                end - start_1st
            );
            let _ = self.emit_final_count(mode);

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
            name: String::from("Count"),
            description: String::from("Counts the number of packets, total size of IPs or sums the amounts contained in IPs, discards them and sends the count every 1M packets."), //TODO make configurable
            icon: String::from("bar-chart"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs to count"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs to count"),
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
                    description: String::from("reports count on this outport"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
