use std::sync::{Arc, Mutex};
use crate::{Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHolder, MessageBuf, ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessSignalSink, ProcessSignalSource};

// component-specific imports
//use std::io::BufRead;

pub struct SplitLinesComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for SplitLinesComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        SplitLinesComponent {
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("SplitLines is now run()ning!");
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");
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
                    self.signals_out.send(b"pong".to_vec()).expect("cloud not send pong");
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }
            // check in port
            loop {
                if let Ok(ip) = inn.pop() {
                    // read packet - expecting UTF-8 string
                    debug!("got a text to split");

                    // split into lines and send them
                    debug!("forwarding lines...");

                    //TODO check which variant is more efficient

                    // variant 1 - blocks only on first line

                    //let mut cur = std::io::Cursor::new(ip);
                    //let ip_out: MessageBuf = vec![];
                    //cur.read_until(b'\n', &mut ip_out);
                    let mut line_iter = ip.split(|&x| x == b'\n').map(|x| x.to_vec());
                    // check if there are more lines to write - unfortunately no peek available so we have to write the first line here and then write as much as possible in big chunk
                    while let Some(line) = line_iter.next() {
                        // write first line
                        match out.push(line) {
                            Ok(_) => {},
                            Err(rtrb::PushError::Full(line)) => {
                                // full, so wake up output-side component
                                out_wakeup.unpark();
                                while out.is_full() {
                                    out_wakeup.unpark();
                                    // wait
                                    std::thread::yield_now();
                                }
                                // send first line now that outport is !full
                                out.push(line).expect("could not push into OUT - but said !is_full");
                            }
                        }
                        // write as much as possible from the iterator into outport ringbuffer
                        if let Ok(chunk) = out.write_chunk_uninit(out.slots()) {
                            chunk.fill_from_iter(&mut line_iter);
                        }
                    }

                    // variant 2 - always blocks since we usually fill the outport fully
                    /*
                    let mut line_iter = ip.split(|&x| x == b'\n').map(|x| x.to_vec());
                    loop {
                        if let Ok(chunk) = out.write_chunk_uninit(out.slots()) {
                            if chunk.fill_from_iter(&mut line_iter) == 0 {
                                // no more lines to write = iterator sucked empty
                                break;
                            }
                            //NOTE: problem is that it can return 0 because the target buffer is full, but the iterator is not empty - so we have to make sure that when we try again the ringbuffer is !full
                            while out.is_full() {
                                // we have filled the chunk, receiver has to work now so wake it up
                                out_wakeup.unpark();
                                // wait for receiver to free up space in the ringbuffer
                                std::thread::yield_now();
                            }
                        }
                    }
                    */

                    out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }

            // are we done?
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
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
            name: String::from("SplitLines"),
            description: String::from("Splits IP contents by newline (\\n) and forwards the parts in separate IPs."),
            icon: String::from("cut"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs with text to split"),
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
                    description: String::from("split lines"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}