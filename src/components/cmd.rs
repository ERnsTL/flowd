use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

//component-specific
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Error, ErrorKind};

pub struct CmdComponent {
    //TODO differentiate between trigger inport and STDIN inport?
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for CmdComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        CmdComponent {
            inn: inports.remove("IN").expect("found no IN inport"),
            out: outports.remove("OUT").expect("found no OUT outport"),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("Cmd is now run()ning!");
        let inn = &mut self.inn;    //TODO optimize
        let out = &mut self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup notify handle for outport OUT");
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
            loop {
                if let Ok(ip) = inn.pop() {
                    // output the packet data with newline
                    debug!("got a packet, starting sub-process:");
                    //println!("{}", std::str::from_utf8(&ip).expect("non utf-8 data")); //TODO optimize avoid clone here

                    let stdout = Command::new("bash")
                        .args(["-c", "a=1 ; while [ true ] ; do echo bla$a; sleep 2s ; ((a=a+1)) ; done"])
                    //let stdout = Command::new("recsel")
                    //    .args(["-p","name", "/dev/shm/test.rec"])
                        .stdout(Stdio::piped())
                        .spawn()
                        .expect("could not start sub-process")
                        .stdout
                        .ok_or_else(|| Error::new(ErrorKind::Other,"Could not capture standard output."))
                        .expect("could not construct Command");

                    let reader = BufReader::new(stdout);

                    reader
                        .lines()
                        .filter_map(|line| line.ok())
                        //.filter(|line| line.find("usb").is_some())
                        //.for_each(|line| println!("{}", line));
                        .for_each(|line| {
                            out.push(line.into_bytes()).expect("could not push into OUT");
                            out_wakeup.unpark();
                        });

                    // repeat
                    //debug!("repeating packet...");
                    //out.push(ip).expect("could not push into OUT");
                    //condvar_notify!(out_wakeup);
                    //out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
                drop(out);
                //condvar_notify!(out_wakeup);
                out_wakeup.unpark();
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
            //condvar_block!(self.wakeup_notify);
        }
        info!("exiting");
    }

    // modeled after https://github.com/noflo/noflo-core/blob/master/components/Output.js
    // what node.js console.log() does:  https://nodejs.org/api/console.html#consolelogdata-args
    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("Cmd"),
            description: String::from("Runs an external program and forwards STDIN, STDERR and STDOUT."),
            icon: String::from("terminal"),
            subgraph: false,
            //TODO config inport - flag mode operating mode: one (command instance handling all IPs) or each (IP handled by new instance
	        //TODO config inport - flag bool framing  true = frame mode, false = send frame body to command STDIN, frame the data from command STDOUT")
	        //TODO config inport - flag bool retry retry/restart command on non-zero return code
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be sent to the sub-process STDIN"),
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
                    description: String::from("STDOUT output data coming from the sub-process"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            //TODO STDERR
            ..Default::default()
        }
    }
}