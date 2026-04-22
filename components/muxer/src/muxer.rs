use flowd_component_api::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHandle, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort, PushError};
use log::{debug, trace, info, warn};
use std::thread;

pub struct MuxerComponent {
    inn: Vec<ProcessEdgeSource>,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for MuxerComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        MuxerComponent {
            inn: inports.remove("IN").expect("found no IN inport"),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("Muxer is now run()ning!");
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");
        loop {
            trace!("begin of iteration");

            // check signals
            //TODO optimize, there is also try_recv() and recv_timeout()
            if let Ok(ip) = self.signals_in.try_recv() {
                //TODO optimize string conversions
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {   //TODO optimize comparison
                    info!("got stop signal, exiting");
                    break;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                }
            }

            // check in port(s)
            for inport in inn.iter_mut() {
                loop {
                    if let Ok(ip) = inport.pop() {
                        debug!("repeating packet...");
                        if push_blocking(&mut out, &out_wakeup, ip, &self.signals_in, &self.signals_out) {
                            info!("stop requested while waiting on full outport, exiting");
                            return;
                        }
                        debug!("done");
                    } else {
                        break;
                    }
                }
            }

            // are we done? = all inports have exited
            if inn.iter().all(|x| x.is_abandoned()) {
                // inputs closed, nothing more to do
                info!("EOF on all inports, shutting down");
                drop(out);
                out_wakeup.unpark();
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("Muxer"),
            description: String::from("Copies data as-is from IN port(s) to single OUT port."),
            icon: String::from("dedent"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: true,
                    description: String::from("IPs to be multiplexed into outport"),
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
                    description: String::from("multiplexed IPs from IN port"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
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
}

impl Component for Demux3Component {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self {
        Demux3Component {
            inn: inports.remove("IN").expect("demux missing IN").pop().unwrap(),
            out_a: outports.remove("A").expect("demux missing A").pop().unwrap(),
            out_b: outports.remove("B").expect("demux missing B").pop().unwrap(),
            out_c: outports.remove("C").expect("demux missing C").pop().unwrap(),
            signals_in,
            signals_out,
        }
    }

    fn run(self) {
        debug!("Demux3 is now run()ning!");
        let mut inn = self.inn;
        let mut out_a = self.out_a.sink;
        let mut out_b = self.out_b.sink;
        let mut out_c = self.out_c.sink;
        let wake_a = self.out_a.wakeup.expect("demux A wake missing");
        let wake_b = self.out_b.wakeup.expect("demux B wake missing");
        let wake_c = self.out_c.wakeup.expect("demux C wake missing");

        let mut branch = 0usize;
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
                match branch % 3 {
                    0 => {
                        if push_blocking(&mut out_a, &wake_a, ip, &self.signals_in, &self.signals_out) {
                            stop_requested = true;
                            break;
                        }
                    }
                    1 => {
                        if push_blocking(&mut out_b, &wake_b, ip, &self.signals_in, &self.signals_out) {
                            stop_requested = true;
                            break;
                        }
                    }
                    _ => {
                        if push_blocking(&mut out_c, &wake_c, ip, &self.signals_in, &self.signals_out) {
                            stop_requested = true;
                            break;
                        }
                    }
                }
                branch += 1;
            }
            if stop_requested {
                info!("stop requested while waiting on full outport, exiting");
                break;
            }

            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
                drop(out_a);
                drop(out_b);
                drop(out_c);
                wake_a.unpark();
                wake_b.unpark();
                wake_c.unpark();
                break;
            }

            thread::park();
        }
        info!("exiting");
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
