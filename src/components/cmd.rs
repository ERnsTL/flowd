use std::ffi::{OsStr,OsString};
use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

//component-specific
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Error, ErrorKind, Write};
use lexopt::prelude::*;

pub struct CmdComponent {
    //TODO differentiate between trigger inport and STDIN inport?
    inn: ProcessEdgeSource,
    cmd: ProcessEdgeSource,
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

enum Mode { One, Each }

impl Component for CmdComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        CmdComponent {
            inn: inports.remove("IN").expect("found no IN inport"),
            cmd: inports.remove("CMD").expect("found no CMD inport"),
            conf: inports.remove("CONF").expect("found no CONF inport"),
            out: outports.remove("OUT").expect("found no OUT outport"),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("Cmd is now run()ning!");
        let inn = &mut self.inn;    //TODO optimize
        let cmd = &mut self.cmd;
        let conf = &mut self.conf;
        let out = &mut self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read sub-process program and args
        let cmd_ip = cmd.pop().expect("could not read IP from CMD configuration inport");
        let cmd_line = std::str::from_utf8(cmd_ip.as_slice()).expect("invalid utf-8");
        let mut cmd_words = shell_words::split(cmd_line).expect("failed to parse command-line of sub-process");
        let mut cmd_args: Vec<OsString> = vec![];
        if cmd_words.len() > 0 {
            let cmd_args1: Vec<String> = cmd_words.drain(1..).collect();
            cmd_args = cmd_args1.iter().map(|x| OsString::from(x)).collect();
        }
        let cmd_program1 = cmd_words.pop().expect("could not pop program name");
        let cmd_program = OsStr::new(cmd_program1.as_str());
        debug!("got program {} with arguments {:?}", cmd_program.to_str().expect("could not convert OsStr to &str"), cmd_args);

        // read configuration
        let mut mode = Mode::Each;
        let mut retry: bool;
        let mut parser = lexopt::Parser::from_args(vec![OsString::from(std::str::from_utf8(&conf.pop().expect("could not read IP from CONF configuration inport")).expect("invalid utf-8"))]);
        while let Some(arg) = parser.next().expect("could not call next()") {
            match arg {
                Long("retry") => {
                    retry = parser.value().expect("could not get parser value").parse().expect("could not parse value");
                }
                Long("mode") => {
                    let mode_str: OsString = parser.value().expect("could not get parser value").parse::<OsString>().expect("could not parse value");
                    match mode_str.to_str().expect("could not convert mode_str to str") {
                        "one" => { mode = Mode::One; },
                        "each" => { mode = Mode::Each; },
                        _ => { unreachable!(); }
                    }
                }
                _ => { unreachable!(); }
            }
        }

        // main loop
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

                    //TODO this runs the sub-process but for longer-running or hanging processes the Cmd component is unresponsive for signals
                    //FIXME be responsive during longer-running sub-processes or for server sub-processes
                    //TODO add ability to send signal to sub-process (HUP, KILL, TERM etc.)
                    // NOTE: Alternative to IFS for reading all of STDIN:
                    //   input=$(cat)
                    //let mut child = Command::new("bash")
                    //    .args(["-c", "IFS= read -r -d '' input ; echo stdin is \"$input\" ; a=1 ; while [ true ] ; do echo bla$a; sleep 2s ; ((a=a+1)) ; done"])
                    //let mut child = Command::new("recsel")
                    //    .args(["-p","name", "/dev/shm/test.rec"])
                    //let mut child = Command::new("bash")
                    //    .args(["-c", "nc -l -n 127.0.0.1 8080"])    // NOTE: adding a grep or similar has its own buffering so you will not see immediate output on child STDOUT
                    let mut child = Command::new(cmd_program)   //TODO optimize pre-construct Command once
                        .args(&cmd_args)    // NOTE: adding a grep or similar has its own buffering so you will not see immediate output on child STDOUT
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                        .expect("could not start sub-process");

                    // set up reader from child STDOUT
                    let reader = BufReader::new(child
                        .stdout
                        .ok_or_else(|| Error::new(ErrorKind::Other,"Could not capture standard output."))
                        .expect("could not get standard output")    //TODO optimize looks like duplication from above line
                    );
                    // set up writer into child STDIN
                    let mut writer = child
                        .stdin
                        //.ok_or_else(|| Error::new(ErrorKind::Other,"Could not capture standard input."))
                        //.expect("could not get standard input")    //TODO optimize looks like duplication from above line
                        .take()
                        .expect("could not get standard input");

                    // deliver the trigger IP contents into child STDIN
                    writer.write(ip.as_slice()).expect("could not write into child STDIN");
                    drop(writer);   // close STDIN

                    // read child STDOUT until closed
                    reader
                        .lines()
                        .filter_map(|line| line.ok())
                        //.filter(|line| line.find("usb").is_some())
                        //.for_each(|line| println!("{}", line));
                        .for_each(|line| {
                            //debug!("repeating packet...");
                            out.push(line.into_bytes()).expect("could not push into OUT");
                            out_wakeup.unpark();
                        });

                    debug!("done");
                } else {
                    break;
                }
            }
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
            name: String::from("Cmd"),
            description: String::from("Runs an external program and forwards STDIN, STDERR and STDOUT."),
            icon: String::from("terminal"),
            subgraph: false,
            //TODO config inport - flag mode operating mode: one (command instance handling all IPs) or each (IP handled by new instance)
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
                },
                ComponentPort {
                    name: String::from("CMD"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("POSIX shell-compatible path and arguments for the sub-process"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("configuration parameters: --retry default false retry/restart command on non-zero return code  --mode=<one|each> where one (command instance handling all IPs) or each (IP handled by new instance)"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
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
            //TODO implement STDERR
            ..Default::default()
        }
    }
}