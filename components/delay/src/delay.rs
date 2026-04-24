use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, ProcessEdgeSink,
    ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessSignalSink, ProcessSignalSource,
    PushError,
};
use log::{debug, info, trace, warn};

// component-specific
use std::thread;
use std::time::Duration;

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
    let num: f64 = num_str.parse().map_err(|_| "invalid number")?;
    let multiplier = match unit.as_str() {
        "us" => 1.0,
        "ms" => 1000.0,
        "s" => 1_000_000.0,
        _ => return Err("unknown unit, use us, ms, or s".to_string()),
    };
    Ok(Duration::from_micros((num * multiplier) as u64))
}

pub struct DelayComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
}

impl Component for DelayComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
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
        }
    }

    fn run(self) {
        debug!("Delay is now run()ning!");
        let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let wake = self.out.wakeup.expect("delay OUT wake missing");

        // read config
        let delay = match conf.pop() {
            Ok(ip) => {
                let config_str = std::str::from_utf8(&ip).expect("invalid utf-8 config");
                match parse_delay(config_str) {
                    Ok(d) => d,
                    Err(e) => {
                        info!(
                            "invalid delay config '{}': {}, using default 50us",
                            config_str, e
                        );
                        Duration::from_micros(50)
                    }
                }
            }
            Err(_) => {
                info!("no config received, using default 50us");
                Duration::from_micros(50)
            }
        };
        info!("using delay {:?}", delay);

        loop {
            if let Ok(signal) = self.signals_in.try_recv() {
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&signal).expect("invalid utf-8")
                );
                if signal == b"stop" {
                    info!("got stop signal, exiting");
                    break;
                }
                if signal == b"ping" {
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                }
            }

            let mut stop_requested = false;
            while let Ok(ip) = inn.pop() {
                thread::sleep(delay);
                if push_blocking(&mut out, &wake, ip, &self.signals_in, &self.signals_out) {
                    stop_requested = true;
                    break;
                }
            }
            if stop_requested {
                info!("stop requested while waiting on full outport, exiting");
                break;
            }

            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
                drop(out);
                wake.unpark();
                break;
            }

            thread::park();
        }
        info!("exiting");
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
