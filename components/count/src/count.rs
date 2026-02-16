use flowd_component_api::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHandle, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};
use log::{debug, trace, info, warn, error};

pub struct CountComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

enum Mode {
    Packets,
    Size,
    Sum,
}

impl Component for CountComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        CountComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout,
        }
    }

    fn run(self) {
        debug!("Count is now run()ning!");
        let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        trace!("read config IP");
        /*
        while conf.is_empty() {
            thread::yield_now();
        }
        */
        let Ok(url_vec) = conf.pop() else { trace!("no config IP received - exiting"); return; };
        let url_str = "https://makeurlhappy/?".to_owned() + std::str::from_utf8(&url_vec).expect("invalid utf-8");
        let url = url::Url::parse(url_str.as_str()).expect("failed to parse configuration URL");
        //let mut query_pairs = url.query_pairs();    // TODO optimize why mut?
        //TODO optimize ^ re-use the query_pairs iterator? wont find anything after first .find() call
        // get API key
        let mode: Mode;
        if let Some((_key, value)) = url.query_pairs().find(|(key, _)| key == "mode") {
            match value.to_string().as_str() {   //TODO optimize
                "packets" => { mode = Mode::Packets; }
                "size" => { mode = Mode::Size; }
                "sum" => { mode = Mode::Sum; }
                _ => { error!("unexpected mode, expected packets|size|sum - exiting"); return; }
            }
        } else {
            error!("no mode found in configuration URL - exiting");
            return;
        }

        // configure
        // mode already configured

        // main loop
        let mut packets: usize = 0;
        let mut packetsize: usize = 0;
        let mut sum: u64 = 0; //TODO count u64 or f64?
        let start = chrono::Utc::now();
        let mut start_1st = chrono::Utc::now();
        loop {
            trace!("begin of iteration");

            // check signals
            if let Ok(ip) = self.signals_in.try_recv() {
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {   //TODO optimize comparison
                    info!("got stop signal, exiting");
                    break;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    self.signals_out.send(b"pong".to_vec()).expect("could not send pong");
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }

            // check in port
            //TODO add reset port
            //TODO add triggered report by sending something into REPORT port
            //TODO add ability to forward as well (output count on separate port?)
            //TODO add counting of packet sizes, certain metadata etc.
            if packets == 0 {
                start_1st = chrono::Utc::now();
            }
            while !inn.is_empty() {
                // drop IP and count it
                if let Ok(chunk) = inn.read_chunk(inn.slots()) {
                    //debug!("got {} packets", chunk.len());
                    //TODO optimize - instead of if on every IP, prepare Fn and apply it without branching
                    match mode {
                        Mode::Packets => {
                            packets += chunk.len();
                            chunk.commit_all();
                        }
                        Mode::Size => {
                            for ip in chunk.into_iter() {
                                packetsize += ip.len();
                            }
                            // NOTE: no need for commit_all(), because into_iter() does that automatically
                        },
                        Mode::Sum => {
                            for ip in chunk {  //TODO optimize - is into_iter() optimal?
                                // convert ip value to numeric type and add it
                                //TODO optimize - there are multiple ways to do it, some much more performant - https://users.rust-lang.org/t/parse-number-from-u8/104487/8
                                //TODO optimize - atoi_simd crate
                                if let Some(value) = atoi::atoi::<u64>(&ip) {
                                    sum += value;
                                } else {
                                    error!("value if IP cannot be summed up: {:?} - skipping", ip);
                                }
                            }
                            // NOTE: no need for commit_all(), because into_iter() does that automatically
                        },
                    }
                    // TODO optimize, when we got a full buffer, we could assume there is more coming and wait a bit longer
                } else {
                    break;
                }
            }

            // check for EOF on input
            if inn.is_abandoned() {
                // send final report
                info!("EOF on inport, shutting down");
                let end = chrono::Utc::now();

                info!("total time: {}, since 1st packet: {}", end - start, end - start_1st);
                match mode {
                    //TODO optimize - instead of format try https://docs.rs/itoa/latest/itoa/
                    Mode::Packets => { out.push(format!("{}", packets).into_bytes()).expect("could not push into OUT"); },
                    Mode::Size => { out.push(format!("{}", packetsize).into_bytes()).expect("could not push into OUT"); },
                    Mode::Sum => { out.push(format!("{}", sum).into_bytes()).expect("could not push into OUT"); },
                };
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