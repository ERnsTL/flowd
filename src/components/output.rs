use std::sync::{Condvar, Arc, Mutex};
use crate::{condvar_block, condvar_notify, ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

pub struct OutputComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
    wakeup_notify: Arc<(Mutex<bool>, Condvar)>,
}

impl Component for OutputComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, graph_inout: Arc<Mutex<GraphInportOutportHolder>>, wakeup_notify: Arc<(Mutex<bool>, Condvar)>) -> Self where Self: Sized {
        OutputComponent {
            inn: inports.remove("IN").expect("found no IN inport"),
            out: outports.remove("OUT").expect("found no OUT outport"),
            signals_in: signals_in,
            signals_out: signals_out,
            graph_inout: graph_inout,
            wakeup_notify: wakeup_notify,
        }
    }

    fn run(mut self) {
        debug!("Output is now run()ning!");
        let inn = &mut self.inn;    //TODO optimize
        let out = &mut self.out.sink;
        let out_wakeup = self.out.wake_notify;
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
                    self.signals_out.send(b"pong".to_vec()).expect("could not send pong");
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }
            // check in port
            //TODO while !inn.is_empty() {
            loop {
                if let Ok(ip) = inn.pop() {
                    // output the packet data with newline
                    debug!("got a packet, printing:");
                    println!("{}", std::str::from_utf8(&ip).expect("non utf-8 data")); //TODO optimize avoid clone here

                    // repeat
                    debug!("repeating packet...");
                    out.push(ip).expect("could not push into OUT");
                    condvar_notify!(&*out_wakeup);
                    debug!("done");
                } else {
                    break;
                }
            }
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
                condvar_notify!(&*out_wakeup);
                break;
            }

            trace!("-- end of iteration");
            //###thread::park();
            condvar_block!(&*self.wakeup_notify);
        }
        info!("exiting");
    }

    // modeled after https://github.com/noflo/noflo-core/blob/master/components/Output.js
    // what node.js console.log() does:  https://nodejs.org/api/console.html#consolelogdata-args
    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("Output"),
            description: String::from("Prints packet data to stdout and repeats packet."),
            icon: String::from("eye"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be printed and repeated on outport"),
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
                    description: String::from("repeated data from IN port"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}