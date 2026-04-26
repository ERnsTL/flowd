use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace};

// component-specific
use std::collections::VecDeque;
use std::time::{Duration, Instant};

fn parse_delay(s: &str) -> Result<Duration, String> {
    let s = s.trim();

    // Handle URL format: ?delay=50us
    let delay_value = if s.starts_with('?') {
        if let Some(delay_part) = s.strip_prefix("?delay=") {
            delay_part
        } else {
            return Err("invalid URL format, expected ?delay=<value>".to_string());
        }
    } else {
        s
    };

    if let Ok(num) = delay_value.parse::<u64>() {
        return Ok(Duration::from_micros(num));
    }
    let mut num_str = String::new();
    let mut unit = String::new();
    for c in delay_value.chars() {
        if c.is_digit(10) || c == '.' {
            num_str.push(c);
        } else {
            unit.push(c);
        }
    }
    let num: f64 = num_str.parse().map_err(|_| "invalid number".to_string())?;
    let multiplier = match unit.as_str() {
        "us" => 1.0,
        "ms" => 1000.0,
        "s" => 1_000_000.0,
        _ => return Err("unknown unit, use us, ms, or s".to_string()),
    };
    Ok(Duration::from_micros((num * multiplier) as u64))
}

#[derive(Clone)]
struct DelayedPacket {
    data: Vec<u8>,
    ready_time: Instant,
}

pub struct DelayComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    delay: Option<Duration>,
    pending_packets: VecDeque<DelayedPacket>,
}

impl Component for DelayComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self {
        DelayComponent {
            conf: inports
                .remove("CONF")
                .expect("delay missing CONF")
                .pop()
                .unwrap(),
            inn: inports
                .remove("IN")
                .expect("delay missing IN")
                .pop()
                .unwrap(),
            out: outports
                .remove("OUT")
                .expect("delay missing OUT")
                .pop()
                .unwrap(),
            signals_in,
            signals_out,
            delay: None,
            pending_packets: VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Delay is now process()ing!");
        let mut work_units = 0u32;

        // Read config if not already read
        if self.delay.is_none() {
            if let Ok(config_data) = self.conf.pop() {
                let config_str = std::str::from_utf8(&config_data).expect("invalid utf-8 config");
                match parse_delay(config_str) {
                    Ok(d) => {
                        self.delay = Some(d);
                        info!("using delay {:?}", d);
                    }
                    Err(e) => {
                        info!(
                            "invalid delay config '{}': {}, using default 50us",
                            config_str, e
                        );
                        self.delay = Some(Duration::from_micros(50));
                    }
                }
            } else {
                // No config yet, use default
                self.delay = Some(Duration::from_micros(50));
            }
        }

        let delay = self.delay.unwrap();

        // Check signals
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

        // Process within budget
        while context.remaining_budget > 0 {
            // Check for stop signals during processing
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

            // First, release packets only when scheduler indicates the timer fired.
            if let Some(fired_at) = context.take_timer_fired() {
                while !self.pending_packets.is_empty() {
                    let front_ready_time = self.pending_packets.front().unwrap().ready_time;
                    if front_ready_time <= fired_at {
                        let packet = self.pending_packets.pop_front().unwrap();
                        if let Err(PushError::Full(returned_packet)) = self.out.push(packet.data) {
                            // If output is full, put it back at the front
                            self.pending_packets.push_front(DelayedPacket {
                                data: returned_packet,
                                ready_time: front_ready_time,
                            });
                            context.wake_at(
                                Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                            );
                            break; // Can't send more until output is free
                        }
                        work_units += 1;
                        context.remaining_budget -= 1;
                    } else {
                        break;
                    }
                }
            }

            // Then, accept new input packets
            if let Ok(ip) = self.inn.pop() {
                let ready_time = Instant::now() + delay;
                self.pending_packets.push_back(DelayedPacket {
                    data: ip,
                    ready_time,
                });
                work_units += 1;
                context.remaining_budget -= 1;
            } else {
                break; // No more input
            }
        }

        // Check if we're done
        if self.inn.is_abandoned() && self.pending_packets.is_empty() {
            // Input closed and no pending packets, nothing more to do
            info!("EOF on inport and no pending packets, finishing");
            return ProcessResult::Finished;
        }

        // If we have pending packets, set ready signal to be re-queued when time comes
        if !self.pending_packets.is_empty() {
            if let Some(next_ready) = self.pending_packets.front().map(|pkt| pkt.ready_time) {
                context.wake_at(next_ready);
            }
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload {
        ComponentComponentPayload {
            name: "Delay".to_string(),
            description: "Introduces a configurable delay to packets".to_string(),
            icon: "clock-o".to_string(),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: "CONF".to_string(),
                    allowed_type: "any".to_string(),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: "Configuration packet with delay, e.g. '50us', '1ms', '2s' in URL format ?delay=1ms".to_string(),
                    values_allowed: vec![],
                    value_default: "50us".to_string(),
                },
                ComponentPort {
                    name: "IN".to_string(),
                    allowed_type: "any".to_string(),
                    ..ComponentPort::default_in()
                },
            ],
            out_ports: vec![ComponentPort {
                name: "OUT".to_string(),
                allowed_type: "any".to_string(),
                ..ComponentPort::default_out()
            }],
            ..Default::default()
        }
    }
}
