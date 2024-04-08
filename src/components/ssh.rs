use std::sync::{Arc, Mutex};
use std::vec;
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

//component-specific
use std::ffi::OsString;
use lexopt::prelude::*;
use ssh::TerminalSize;
use std::time::Duration;

/*
Ability to remotely execute a command and forward data to its STDIN and receive data from its STDOUT.
TODO Streaming of data from stdout to transfer data from the remote command to the next component.
TODO scp file transfer
TODO use custom identity file

Usage example: https://crates.io/crates/ssh-rs#how-to-use
TODO what does https://crates.io/crates/ssh-key do? maybe useful for key management
*/

pub struct SSHClientComponent {
    //TODO differentiate between trigger inport and STDIN inport?
    inn: ProcessEdgeSource,
    cmd: ProcessEdgeSource,
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

const CONNECT_TIMEOUT: Option<Duration> = Some(Duration::from_secs(7));
const READ_WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(2000));     //TODO should be set to 500ms for streaming mode

enum Mode { One, Each }

impl Component for SSHClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        SSHClientComponent {
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
        debug!("SSHClient is now run()ning!");
        let mut inn = self.inn;
        let mut cmd = self.cmd;
        let mut conf = self.conf;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read sub-process program and args
        let cmd_ip = cmd.pop().expect("could not read IP from CMD configuration inport");
        let cmd_line = std::str::from_utf8(cmd_ip.as_slice()).expect("invalid utf-8");
        // NOTE: ssh does the parsing of the command-line itself, we just pass it as a str
        /*
        let mut cmd_words = shell_words::split(cmd_line).expect("failed to parse command-line of sub-process");
        let mut cmd_args: Vec<OsString> = vec![];
        if cmd_words.len() > 0 {
            let cmd_args1: Vec<String> = cmd_words.drain(1..).collect();
            cmd_args = cmd_args1.iter().map(|x| OsString::from(x)).collect();
        }
        let cmd_program = cmd_words.pop().expect("could not pop program name");
        debug!("got program {} with arguments {:?}", cmd_program, cmd_args);
        */

        // read configuration
        let mut username: Option<String> = None;
        let mut password: Option<String> = None;
        let mut private_key_path: Option<String> = None;
        let mut mode = Mode::Each;
        let mut retry = false;
        let mut pipe_out: bool = false;
        let conf_vec = conf.pop().expect("could not read IP from CONF configuration inport");
        let conf_words_str = std::str::from_utf8(&conf_vec).expect("invalid utf-8");
        let conf_words = shell_words::split(conf_words_str).expect("failed to parse command-line of sub-process");
        let mut parser = lexopt::Parser::from_args(conf_words);
        let mut address_port: Option<String> = None;
        while let Some(arg) = parser.next().expect("could not call next()") {
            match arg {
                Long("retry") => {
                    retry = parser.value().expect("failed to get value for retry").parse().expect("failed to parse value for retry");
                }
                Long("mode") => {
                    let mode_str: OsString = parser.value().expect("failed to get value for mode").parse::<OsString>().expect("failed to parse value for mode");
                    match mode_str.to_str().expect("could not convert mode_str to str") {
                        "one" => { mode = Mode::One; },
                        "each" => { mode = Mode::Each; },
                        _ => { unreachable!(); }
                    }
                }
                Long("user") => {
                    username = Some(parser.value().expect("failed to get value for user").into_string().expect("failed to get parser value for user"));
                }
                Long("pass") => {
                    password = Some(parser.value().expect("failed to get value for pass").into_string().expect("failed to get parser value for pass"));
                }
                Short('i') => {
                    private_key_path = Some(parser.value().expect("failed to get value for -i").into_string().expect("failed to get parser value for -i"));
                }
                Long("pipeout") => {
                    pipe_out = parser.value().expect("failed to get value for pipeout").parse().expect("failed to parse value for pipeout");
                }
                Value(val) => {
                    // check for address:port
                    if address_port.is_none() {
                        // check if the format is address:port or user@address:port
                        let val = val.into_string().expect("could not convert val to string");
                        if val.contains('@') {
                            // split user@address:port
                            let parts: Vec<&str> = val.split('@').collect();
                            if parts.len() != 2 {
                                error!("invalid address:port format: {}", val);
                                return;
                            }
                            username = Some(parts[0].to_string());
                            address_port = Some(parts[1].to_string());
                            //TODO add check if there is a port included, but because of IPv6 with its ":" this is not trivial
                        } else {
                            // just address:port
                            address_port = Some(val);
                        }
                    } else {
                        error!("got extra free argument: {:?} - exiting", val);
                        return;
                    }
                }
                _ => {
                    error!("got unexpected argument: {:?} - exiting", arg);
                    return;
                }
            }
        }
        // check configuration
        if address_port.is_none() {
            error!("missing address:port as free argument - exiting");
            return;
        }
        let address_port = address_port.unwrap();
        //TODO implement
        if retry {
            unimplemented!("retry not implemented yet");
        }
        match mode {
            Mode::One => {
                error!("mode one not implemented yet - exiting");
                return;
            }
            Mode::Each => {}
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

                    // build session
                    let mut session_builder = ssh::create_session();
                    if let Some(ref username) = username {
                        session_builder = session_builder.username(&username);
                    };
                    if let Some(ref password) = password {
                        session_builder = session_builder.password(&password);
                    };
                    if let Some(ref private_key_path) = private_key_path {
                        session_builder = session_builder.private_key_path(private_key_path);
                    };
                    session_builder = session_builder.timeout(READ_WRITE_TIMEOUT);

                    // open session
                    let mut session = session_builder
                        // connect does not support SSH-tpyical user@host:port syntax but only the basic from TcpStream.connect("host:port")
                        .connect_with_timeout(&address_port, CONNECT_TIMEOUT)   //TODO optimize - avoid unwrap, could do unwrap_unchecked because of previous check
                        .unwrap()
                        .run_local();

                    // run remote command
                    //TODO funtional overlap with mode = one
                    // NOTE: in send_command() mode, the read_write_timeout is applied to the command execution time (how unexpected)
                    //  but in exec_shell resp. streaming mode it is read from the channel, and then there is nothing available yet,
                    //  then we just do another iteration, able to handle singals
                    let out_vec: Vec<u8>;
                    if pipe_out {   // stream data from SSH command to next component
                        // open and execute command in SSH channel in session
                        let channel = session.open_channel().unwrap();
                        // start shell in the channel
                        let mut channel_shell = channel.shell(TerminalSize::from_type(80, 25, ssh::TerminalSizeType::Character)).unwrap();
                        // write trigger IP into the remote shell STDIN
                        channel_shell.write(&ip).expect("failed to write into channel_shell");
                        // read from that command
                        out_vec = channel_shell.read().expect("failed to read from channel_shell");
                        //TODO implement this mode - currently does only 1 read()
                    } else { // one command execution per inport packet
                        // capture of output desired
                        let exec = session.open_exec().expect("failed to open exec");
                        out_vec = exec.send_command(cmd_line).expect("failed to send command");

                        // no capture of output needed
                        //TODO make configurable
                        //TODO deliver the trigger IP contents to the sub-process STDIN, if possible
                        /*
                        let mut exec = session.open_exec().unwrap();
                        exec.exec_command("no_output_command").unwrap();
                        out_vec = exec.get_output().unwrap();
                        println!("exit status: {}", exec.exit_status().unwrap());
                        println!("terminated msg: {}", exec.terminate_msg().unwrap());
                        let _ = exec.close();   //TODO optimize - reuse?
                        */
                    }

                    debug!("repeating remote process output...");
                    out.push(out_vec).expect("could not push into OUT");
                    out_wakeup.unpark();
                    
                    // close session
                    //TODO optimize - reuse?
                    session.close();

                    debug!("done");
                } else {
                    break;
                }
            }

            // are we done yet?
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
            name: String::from("SSHClient"),
            description: String::from("Runs a program remotely and forwards STDIN, STDERR and STDOUT."),
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
                    description: String::from("data to be sent to the remote process STDIN"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("CMD"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("POSIX shell-compatible path and arguments for the remote process"),
                    values_allowed: vec![],
                    value_default: String::from("ls -alh /dev/shm/")
                },
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("configuration parameters: --retry default false retry/restart command on non-zero return code  --mode=<one|each> where one (command instance handling all IPs) or each (IP handled by new instance) - most parameters are optional"),
                    values_allowed: vec![],
                    value_default: String::from("--mode=each|one --pipeout=true --user=username --pass=password -i /home/user/.ssh/id_ed25519 username@address:port")
                },
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("STDOUT output data coming from the remote process"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            //TODO implement STDERR
            ..Default::default()
        }
    }
}