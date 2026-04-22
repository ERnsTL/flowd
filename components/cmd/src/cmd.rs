use flowd_component_api::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHandle, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};
use log::{debug, info, warn, trace};

//component-specific
use std::ffi::{OsStr,OsString};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Error, ErrorKind, Write};
use std::sync::mpsc;
use std::time::Duration;
use lexopt::prelude::*;

pub struct CmdComponent {
    //TODO differentiate between trigger inport and STDIN inport?
    inn: ProcessEdgeSource,
    cmd: ProcessEdgeSource,
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

enum Mode { One, Each }
const STDOUT_THREAD_JOIN_GRACE: Duration = Duration::from_secs(2);

impl Component for CmdComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        CmdComponent {
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            cmd: inports.remove("CMD").expect("found no CMD inport").pop().unwrap(),
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("Cmd is now run()ning!");
        let mut inn = self.inn;
        let mut cmd = self.cmd;
        let mut conf = self.conf;
        let mut out = self.out.sink;
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
        let mut retry = false;
        //TODO must split the configuration into words with shell_words::split() and then parse the arguments with lexopt, like already done in SSH client component
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
                    let _ = self.signals_out.try_send(b"pong".to_vec());
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
                    match mode {
                        Mode::Each => {
                            let mut child = Command::new(cmd_program)   //TODO optimize pre-construct Command once
                                .args(&cmd_args)    // NOTE: adding a grep or similar has its own buffering so you will not see immediate output on child STDOUT
                                .stdin(Stdio::piped())
                                .stdout(Stdio::piped())
                                .spawn()
                                .expect("could not start sub-process");
                            if retry {
                                //TODO implement
                                unimplemented!();
                            }

                            // set up reader from child STDOUT
                            let child_stdout = child
                                .stdout
                                .take()
                                .ok_or_else(|| Error::new(ErrorKind::Other,"Could not capture standard output."))
                                .expect("could not get standard output");   //TODO optimize looks like duplication from above line
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

                            // read child STDOUT in a dedicated thread so this component can keep handling stop/ping.
                            let (stdout_tx, stdout_rx) = mpsc::channel::<Vec<u8>>();
                            let stdout_thread = std::thread::spawn(move || {
                                let reader = BufReader::new(child_stdout);
                                for line in reader.lines().map_while(Result::ok) {
                                    if stdout_tx.send(line.into_bytes()).is_err() {
                                        break;
                                    }
                                }
                            });

                            let mut stop_component = false;
                            let mut stdout_closed = false;
                            while !stop_component {
                                // child output
                                match stdout_rx.recv_timeout(Duration::from_millis(50)) {
                                    Ok(line) => {
                                        out.push(line).expect("could not push into OUT");
                                        out_wakeup.unpark();
                                    }
                                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                                        stdout_closed = true;
                                    }
                                }

                                // process signals while child may still be running
                                if let Ok(sig) = self.signals_in.try_recv() {
                                    trace!("received signal ip: {}", std::str::from_utf8(&sig).expect("invalid utf-8"));
                                    if sig == b"stop" {
                                        info!("got stop signal while child process is running, terminating child");
                                        let _ = child.kill();
                                        let _ = child.wait();
                                        stop_component = true;
                                        break;
                                    } else if sig == b"ping" {
                                        trace!("got ping signal, responding");
                                        let _ = self.signals_out.try_send(b"pong".to_vec());
                                    } else {
                                        warn!("received unknown signal ip: {}", std::str::from_utf8(&sig).expect("invalid utf-8"))
                                    }
                                }

                                // child exit
                                if let Some(_status) = child.try_wait().expect("failed to query child process status") {
                                    if stdout_closed {
                                        break;
                                    }
                                }
                            }

                            let stdout_join_started = std::time::Instant::now();
                            while !stdout_thread.is_finished()
                                && stdout_join_started.elapsed() < STDOUT_THREAD_JOIN_GRACE
                            {
                                std::thread::sleep(Duration::from_millis(10));
                            }
                            if stdout_thread.is_finished() {
                                if let Err(err) = stdout_thread.join() {
                                    warn!("failed to join child stdout reader thread: {:?}", err);
                                }
                            } else {
                                warn!(
                                    "child stdout reader thread did not exit within {:?}, detaching",
                                    STDOUT_THREAD_JOIN_GRACE
                                );
                                drop(stdout_thread);
                            }

                            if stop_component {
                                drop(out);
                                out_wakeup.unpark();
                                info!("exiting");
                                return;
                            }
                        },
                        Mode::One => { unimplemented!(); }  //TODO implement
                    }

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
