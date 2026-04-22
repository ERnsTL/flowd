use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, ProcessEdgeSink,
    ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessSignalSink, ProcessSignalSource,
    PushError,
};
use log::{debug, info, trace};
use std::thread;
use std::time::Duration;

fn parse_delay(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if let Ok(num) = s.parse::<u64>() {
        return Ok(Duration::from_micros(num));
    }
    let mut num_str = String::new();
    let mut unit = String::new();
    for c in s.chars() {
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
            conf: inports.remove("CONF").expect("delay missing CONF").pop().unwrap(),
            inn: inports.remove("IN").expect("delay missing IN").pop().unwrap(),
            out: outports.remove("OUT").expect("delay missing OUT").pop().unwrap(),
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
                        info!("invalid delay config '{}': {}, using default 50us", config_str, e);
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
                    self.signals_out
                        .send(b"pong".to_vec())
                        .expect("could not send pong");
                }
            }

            while let Ok(ip) = inn.pop() {
                thread::sleep(delay);
                push_blocking(&mut out, &wake, ip);
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
                    description: "Configuration packet with delay, e.g. '50us', '1ms', '2s'".to_string(),
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

fn push_blocking(sink: &mut flowd_component_api::ProcessEdgeSinkConnection, wake: &thread::Thread, mut packet: Vec<u8>) {
    loop {
        match sink.push(packet) {
            Ok(()) => {
                wake.unpark();
                return;
            }
            Err(PushError::Full(returned)) => {
                packet = returned;
                wake.unpark();
                thread::yield_now();
            }
        }
    }
}
