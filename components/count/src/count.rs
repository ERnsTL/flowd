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

        self.out
            .push(final_count.clone().into_bytes())
            .expect("could not push into OUT");

        // Send network output with the final count
        flowd_component_api::send_network_output_comfortable(&self.graph_inout, final_count.clone());

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
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Count is now process()ing!");

        // Try to read configuration if not yet configured
        if self.mode.is_none() {
            trace!("reading config IP");
            if let Ok(url_vec) = self.conf.pop() {
                let raw_conf = std::str::from_utf8(&url_vec).expect("invalid utf-8");
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

        // check signals
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            // stop signal
            if ip == b"stop" {
                if let Some(mode) = self.mode {
                    let _ = self.emit_final_count(mode);
                }
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

        // check in port within budget
        if self.packets == 0 && !self.inn.is_empty() {
            self.start_1st = Some(chrono::Utc::now());
        }

        while context.remaining_budget > 0 && !self.inn.is_empty() {
            // stay responsive to stop/ping even while draining a busy input buffer
            if let Ok(sig) = self.signals_in.try_recv() {
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&sig).expect("invalid utf-8")
                );
                if sig == b"stop" {
                    if let Some(mode) = self.mode {
                        let _ = self.emit_final_count(mode);
                    }
                    info!("got stop signal while processing, finishing");
                    return ProcessResult::Finished;
                } else if sig == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                }
            }

            if let Ok(chunk) = self.inn.read_chunk(self.inn.slots()) {
                //debug!("got {} packets", chunk.len());
                //TODO optimize - instead of if on every IP, prepare Fn and apply it without branching
                match mode {
                    Mode::Packets => {
                        self.packets += chunk.len();
                        chunk.commit_all();
                    }
                    Mode::Size => {
                        for ip in chunk.into_iter() {
                            self.packetsize += ip.len();
                        }
                        // NOTE: no need for commit_all(), because into_iter() does that automatically
                    }
                    Mode::Sum => {
                        for ip in chunk {
                            //TODO optimize - is into_iter() optimal?
                            // convert ip value to numeric type and add it
                            //TODO optimize - there are multiple ways to do it, some much more performant - https://users.rust-lang.org/t/parse-number-from-u8/104487/8
                            //TODO optimize - atoi_simd crate
                            if let Some(value) = atoi::atoi::<u64>(&ip) {
                                self.sum += value;
                            } else {
                                error!("value if IP cannot be summed up: {:?} - skipping", ip);
                            }
                        }
                        // NOTE: no need for commit_all(), because into_iter() does that automatically
                    }
                }
                work_units += 1;
                context.remaining_budget -= 1;
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
        if self.inn.is_abandoned() {
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
