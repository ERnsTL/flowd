use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, info, trace, warn};

//component-specific
use lexopt::prelude::*;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

enum SubprocessState {
    Idle,
    Running {
        child: std::process::Child,
        stdin_thread: Option<std::thread::JoinHandle<()>>,
        stdout_rx: mpsc::Receiver<Vec<u8>>,
        stdout_thread: Option<std::thread::JoinHandle<()>>,
        start_time: Instant,
    },
    Completed,
}

pub struct CmdComponent {
    // Ports
    inn: ProcessEdgeSource,
    cmd: ProcessEdgeSource,
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,

    // Configuration (lazy-loaded)
    cmd_program: Option<OsString>,
    cmd_args: Vec<OsString>,
    mode: Mode,
    retry: bool,
    config_loaded: bool,

    // Runtime state
    state: SubprocessState,
}

#[derive(Debug)]
enum Mode {
    One,
    Each,
}
const STDOUT_THREAD_JOIN_GRACE: Duration = Duration::from_secs(2);
const STDIN_THREAD_JOIN_GRACE: Duration = Duration::from_secs(2);

fn handoff_join(handle: std::thread::JoinHandle<()>, label: &'static str) {
    std::thread::Builder::new()
        .name(format!("cmd-join-{}", label))
        .spawn(move || {
            if let Err(err) = handle.join() {
                warn!("{}: deferred thread join returned error: {:?}", label, err);
            }
        })
        .expect("failed to spawn cmd deferred join thread");
}

impl Component for CmdComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
    ) -> Self
    where
        Self: Sized,
    {
        CmdComponent {
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            cmd: inports
                .remove("CMD")
                .expect("found no CMD inport")
                .pop()
                .unwrap(),
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
            cmd_program: None,
            cmd_args: Vec::new(),
            mode: Mode::Each,
            retry: false,
            config_loaded: false,
            state: SubprocessState::Idle,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Cmd is now process()ing!");

        // Load configuration if not loaded
        if !self.config_loaded {
            if let Ok(cmd_ip) = self.cmd.pop() {
                let cmd_line = std::str::from_utf8(cmd_ip.as_slice()).expect("invalid utf-8");
                let mut cmd_words = shell_words::split(cmd_line).expect("failed to parse command-line");
                if cmd_words.len() > 0 {
                    let cmd_args1: Vec<String> = cmd_words.drain(1..).collect();
                    self.cmd_args = cmd_args1.iter().map(|x| OsString::from(x)).collect();
                }
                let cmd_program1 = cmd_words.pop().expect("could not pop program name");
                self.cmd_program = Some(OsStr::new(&cmd_program1).to_os_string());
                debug!("loaded program {:?} with args {:?}", self.cmd_program, self.cmd_args);
            } else {
                trace!("no CMD config yet");
                return ProcessResult::NoWork;
            }

            if let Ok(conf_ip) = self.conf.pop() {
                let mut parser = lexopt::Parser::from_args(vec![OsString::from(
                    std::str::from_utf8(&conf_ip).expect("invalid utf-8"),
                )]);
                while let Some(arg) = parser.next().expect("could not call next()") {
                    match arg {
                        Long("retry") => {
                            self.retry = parser.value().expect("could not get parser value").parse().expect("could not parse value");
                        }
                        Long("mode") => {
                            let mode_str: OsString = parser.value().expect("could not get parser value").parse::<OsString>().expect("could not parse value");
                            match mode_str.to_str().expect("could not convert mode_str to str") {
                                "one" => self.mode = Mode::One,
                                "each" => self.mode = Mode::Each,
                                _ => unreachable!(),
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                self.config_loaded = true;
                debug!("loaded config: mode={:?}, retry={}", self.mode, self.retry);
            } else {
                trace!("no CONF config yet");
                return ProcessResult::NoWork;
            }
        }

        // Check signals
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
            if ip == b"stop" {
                info!("got stop signal, finishing");
                // Cleanup any running subprocess
                if let SubprocessState::Running { child, stdin_thread, stdout_thread, .. } = &mut self.state {
                    let _ = child.kill();
                    let _ = child.wait();
                    if let Some(thread) = stdin_thread.take() {
                        handoff_join(thread, "CmdComponent-stdin-stop");
                    }
                    if let Some(thread) = stdout_thread.take() {
                        handoff_join(thread, "CmdComponent-stdout-stop");
                    }
                }
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
            }
        }

        // Process based on state
        match &mut self.state {
            SubprocessState::Idle => {
                if context.remaining_budget > 0 {
                    if let Ok(ip) = self.inn.pop() {
                        debug!("got input packet, starting subprocess");
                        match self.mode {
                            Mode::Each => {
                                let mut child = Command::new(self.cmd_program.as_ref().unwrap())
                                    .args(&self.cmd_args)
                                    .stdin(Stdio::piped())
                                    .stdout(Stdio::piped())
                                    .spawn()
                                    .expect("could not start sub-process");

                                let child_stdout = child.stdout.take().expect("could not get stdout");
                                let writer = child.stdin.take().expect("could not get stdin");

                                // Start stdin thread
                                let stdin_thread = std::thread::spawn(move || {
                                    let mut writer = writer;
                                    writer.write_all(ip.as_slice()).expect("could not write to stdin");
                                    drop(writer);
                                });

                                // Start stdout thread
                                let (stdout_tx, stdout_rx) = mpsc::channel();
                                let stdout_thread = std::thread::spawn(move || {
                                    let reader = BufReader::new(child_stdout);
                                    for line in reader.lines().map_while(Result::ok) {
                                        if stdout_tx.send(line.into_bytes()).is_err() {
                                            break;
                                        }
                                    }
                                });

                                self.state = SubprocessState::Running {
                                    child,
                                    stdin_thread: Some(stdin_thread),
                                    stdout_rx,
                                    stdout_thread: Some(stdout_thread),
                                    start_time: Instant::now(),
                                };
                                context.remaining_budget -= 1;
                                return ProcessResult::DidWork(1);
                            }
                            Mode::One => unimplemented!(),
                        }
                    }
                }
            }
            SubprocessState::Running { child, stdin_thread, stdout_rx, stdout_thread, start_time: _ } => {
                let mut work_done = 0u32;

                // Check for output
                if context.remaining_budget > 0 {
                    match stdout_rx.try_recv() {
                        Ok(line) => {
                            self.out.push(line).expect("could not push to OUT");
                            work_done += 1;
                            context.remaining_budget -= 1;
                        }
                        Err(mpsc::TryRecvError::Empty) => {}
                        Err(mpsc::TryRecvError::Disconnected) => {
                            // stdout closed
                        }
                    }
                }

                // Check if child exited
                if let Some(_status) = child.try_wait().expect("failed to query child status") {
                    // Child finished, cleanup threads
                    if let Some(thread) = stdin_thread.take() {
                        if thread.is_finished() {
                            if let Err(err) = thread.join() {
                                warn!("failed to join stdin thread: {:?}", err);
                            }
                        } else {
                            handoff_join(thread, "CmdComponent-stdin");
                        }
                    }
                    if let Some(thread) = stdout_thread.take() {
                        if thread.is_finished() {
                            if let Err(err) = thread.join() {
                                warn!("failed to join stdout thread: {:?}", err);
                            }
                        } else {
                            handoff_join(thread, "CmdComponent-stdout");
                        }
                    }
                    self.state = SubprocessState::Completed;
                }

                if work_done > 0 {
                    return ProcessResult::DidWork(work_done);
                }
            }
            SubprocessState::Completed => {
                // Reset to idle for next input
                self.state = SubprocessState::Idle;
            }
        }

        // Check for EOF
        if self.inn.is_abandoned() {
            info!("EOF on inport, finishing");
            return ProcessResult::Finished;
        }

        ProcessResult::NoWork
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    // Component-level unit tests for cmd component logic
    // These tests focus on isolated logic testing without the full runtime

    #[test]
    fn test_mode_enum_debug() {
        // Test that Mode enum implements Debug
        assert_eq!(format!("{:?}", Mode::One), "One");
        assert_eq!(format!("{:?}", Mode::Each), "Each");
    }

    #[test]
    fn test_subprocess_state_enum() {
        // Test that SubprocessState enum variants exist
        let _idle = SubprocessState::Idle;
        let _completed = SubprocessState::Completed;
        // Running variant requires complex setup, just verify it compiles
    }

    #[test]
    fn test_get_metadata() {
        // Test component metadata
        let metadata = CmdComponent::get_metadata();
        assert_eq!(metadata.name, "Cmd");
        assert!(metadata.description.contains("external program"));
        assert_eq!(metadata.icon, "terminal");
        assert!(!metadata.subgraph);

        // Check ports
        assert_eq!(metadata.in_ports.len(), 3);
        assert_eq!(metadata.out_ports.len(), 1);

        let in_port_names: Vec<&str> = metadata.in_ports.iter().map(|p| p.name.as_str()).collect();
        assert!(in_port_names.contains(&"IN"));
        assert!(in_port_names.contains(&"CMD"));
        assert!(in_port_names.contains(&"CONF"));

        let out_port_names: Vec<&str> = metadata.out_ports.iter().map(|p| p.name.as_str()).collect();
        assert!(out_port_names.contains(&"OUT"));
    }

    #[test]
    fn test_constants() {
        // Test that constants are defined and reasonable
        assert!(STDOUT_THREAD_JOIN_GRACE.as_secs() > 0);
        assert!(STDIN_THREAD_JOIN_GRACE.as_secs() > 0);
    }

    #[test]
    fn test_shell_words_parsing_logic() {
        // Test the shell parsing logic that the component uses
        // This tests the logic without requiring full component setup

        let test_cases = vec![
            ("echo hello", vec!["echo", "hello"]),
            ("ls -la", vec!["ls", "-la"]),
            ("cmd", vec!["cmd"]),
        ];

        for (input, expected) in test_cases {
            let result = shell_words::split(input).unwrap();
            assert_eq!(result, expected, "Failed to parse: {}", input);
        }
    }

    #[test]
    fn test_invalid_shell_words() {
        // Test error handling in shell parsing
        // Unclosed quotes should fail
        assert!(shell_words::split("echo 'unclosed").is_err());
    }
}


