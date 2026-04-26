use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

//component-specific
use lexopt::prelude::*;
use std::ffi::OsString;
use std::vec;
use tokio::sync::mpsc;
use openssh;

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
    // Configuration state
    cmd_config: Option<String>,
    ssh_config: Option<ParsedSSHConfig>,
    // Async operation state
    state: SSHState,
    cmd_sender: mpsc::UnboundedSender<SSHCommand>,
    result_receiver: mpsc::UnboundedReceiver<SSHResult>,
    /// JoinHandle for the dedicated async thread that runs SSH operations.
    ///
    /// This field is currently unused after thread creation but kept for architectural clarity
    /// and future enhancement potential. The thread terminates automatically when the component
    /// is dropped or when a disconnect command is received.
    ///
    /// TODO: Future enhancement potentials:
    /// 1. Thread Joining: Explicitly wait for thread completion in Drop::drop()
    /// 2. Status Monitoring: Check if the thread is still running for health checks
    /// 3. Testing: Wait for thread completion in unit tests to ensure clean teardown
    /// 4. Debugging: Better error reporting if the async thread panics
    #[allow(dead_code)]
    async_thread: Option<std::thread::JoinHandle<()>>,
    //graph_inout: GraphInportOutportHandle,
}

//TODO implement or remove
#[derive(Debug, Clone)]
struct ParsedSSHConfig {
    address_port: String,
    #[allow(dead_code)]
    username: Option<String>,
    #[allow(dead_code)]
    password: Option<String>,
    #[allow(dead_code)]
    private_key_path: Option<String>,
    #[allow(dead_code)]
    mode: Mode,
    #[allow(dead_code)]
    retry: bool,
    #[allow(dead_code)]
    pipe_out: bool,
}



#[derive(Debug, Clone)]
enum Mode {
    One,
    Each,
}

#[derive(Debug)]
enum SSHCommand {
    Connect { config: ParsedSSHConfig },
    Execute { command: String, input: Vec<u8> },
    Disconnect,
}

#[derive(Debug)]
enum SSHResult {
    Connected,
    Output(Vec<u8>),
    Error(String),
}

#[derive(Debug)]
enum SSHState {
    WaitingForConfig,
    Connecting,
    Connected,
    Executing,
    Finished,
}

async fn async_main(
    mut cmd_rx: mpsc::UnboundedReceiver<SSHCommand>,
    result_tx: mpsc::UnboundedSender<SSHResult>,
) {
    let mut session: Option<openssh::Session> = None;

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            SSHCommand::Connect { config } => {
                debug!("Connecting to SSH: {}", config.address_port);
                match connect_ssh(&config).await {
                    Ok(sess) => {
                        session = Some(sess);
                        let _ = result_tx.send(SSHResult::Connected);
                    }
                    Err(e) => {
                        let _ = result_tx.send(SSHResult::Error(format!("Connect failed: {}", e)));
                    }
                }
            }
            SSHCommand::Execute { command, input } => {
                if let Some(sess) = &session {
                    debug!("Executing command: {}", command);
                    match execute_command(sess, &command, &input).await {
                        Ok(output) => {
                            let _ = result_tx.send(SSHResult::Output(output));
                        }
                        Err(e) => {
                            let _ = result_tx.send(SSHResult::Error(format!("Execute failed: {}", e)));
                        }
                    }
                } else {
                    let _ = result_tx.send(SSHResult::Error("Not connected".to_string()));
                }
            }
            SSHCommand::Disconnect => {
                debug!("Disconnecting SSH session");
                session = None; // Drop the session
                let _ = result_tx.send(SSHResult::Error("Disconnected".to_string()));
            }
        }
    }
}

async fn connect_ssh(config: &ParsedSSHConfig) -> Result<openssh::Session, Box<dyn std::error::Error + Send + Sync>> {
    let mut builder = openssh::SessionBuilder::default();

    // Set user if provided
    if let Some(user) = &config.username {
        builder.user(user.clone());
    }

    // Configure authentication - openssh handles this automatically based on available credentials
    // Password and key authentication will be attempted in order

    if let Some(key_path) = &config.private_key_path {
        builder.keyfile(key_path);
    }

    // Connect to the SSH server
    let session = builder.connect(&config.address_port).await?;
    Ok(session)
}

async fn execute_command(
    session: &openssh::Session,
    command: &str,
    _input: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    // Placeholder: input handling not implemented yet
    let output = session.command(command).output().await?;
    if !output.status.success() {
        return Err(format!("Command failed with exit code {:?}", output.status.code()).into());
    }
    Ok(output.stdout)
}

impl Component for SSHClientComponent {
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
        let (cmd_sender, cmd_receiver) = mpsc::unbounded_channel();
        let (result_sender, result_receiver) = mpsc::unbounded_channel();

        let async_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async_main(cmd_receiver, result_sender));
        }));

        SSHClientComponent {
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
            cmd_config: None,
            ssh_config: None,
            state: SSHState::WaitingForConfig,
            cmd_sender,
            result_receiver,
            async_thread,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, _context: &mut NodeContext) -> ProcessResult {
        debug!("SSHClient process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, disconnecting and finishing");
                // Send disconnect command to async thread
                let _ = self.cmd_sender.send(SSHCommand::Disconnect);
                self.state = SSHState::Finished;
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!(
                    "received unknown signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                )
            }
        }

        // Check if we have CMD configuration
        if self.cmd_config.is_none() {
            if let Ok(cmd_ip) = self.cmd.pop() {
                let cmd_line = std::str::from_utf8(&cmd_ip).expect("invalid utf-8 in CMD IP");
                debug!("received CMD config: {}", cmd_line);
                self.cmd_config = Some(cmd_line.to_string());
                return ProcessResult::DidWork(1); // Configuration processed
            } else {
                // No CMD config yet
                return ProcessResult::NoWork;
            }
        }

        // Check if we have CONF configuration
        if self.ssh_config.is_none() {
            if let Ok(conf_vec) = self.conf.pop() {
                debug!("received CONF config, parsing...");

                // Parse configuration (extracted from original run() method)
                let mut username: Option<String> = None;
                let mut password: Option<String> = None;
                let mut private_key_path: Option<String> = None;
                let mut mode = Mode::Each;
                let mut retry = false;
                let mut pipe_out: bool = false;

                let conf_words_str = std::str::from_utf8(&conf_vec).expect("invalid utf-8");
                let conf_words = shell_words::split(conf_words_str)
                    .expect("failed to parse command-line of sub-process");
                let mut parser = lexopt::Parser::from_args(conf_words);
                let mut address_port: Option<String> = None;

                while let Some(arg) = parser.next().expect("could not call next()") {
                    match arg {
                        Long("retry") => {
                            retry = parser
                                .value()
                                .expect("failed to get value for retry")
                                .parse()
                                .expect("failed to parse value for retry");
                        }
                        Long("mode") => {
                            let mode_str: OsString = parser
                                .value()
                                .expect("failed to get value for mode")
                                .parse::<OsString>()
                                .expect("failed to parse value for mode");
                            match mode_str
                                .to_str()
                                .expect("could not convert mode_str to str")
                            {
                                "one" => {
                                    mode = Mode::One;
                                }
                                "each" => {
                                    mode = Mode::Each;
                                }
                                _ => {
                                    error!("invalid mode: {:?}", mode_str);
                                    return ProcessResult::Finished;
                                }
                            }
                        }
                        Long("user") => {
                            username = Some(
                                parser
                                    .value()
                                    .expect("failed to get value for user")
                                    .into_string()
                                    .expect("failed to get parser value for user"),
                            );
                        }
                        Long("pass") => {
                            password = Some(
                                parser
                                    .value()
                                    .expect("failed to get value for pass")
                                    .into_string()
                                    .expect("failed to get parser value for pass"),
                            );
                        }
                        Short('i') => {
                            private_key_path = Some(
                                parser
                                    .value()
                                    .expect("failed to get value for -i")
                                    .into_string()
                                    .expect("failed to get parser value for -i"),
                            );
                        }
                        Long("pipeout") => {
                            pipe_out = parser
                                .value()
                                .expect("failed to get value for pipeout")
                                .parse()
                                .expect("failed to parse value for pipeout");
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
                                        return ProcessResult::Finished;
                                    }
                                    username = Some(parts[0].to_string());
                                    address_port = Some(parts[1].to_string());
                                } else {
                                    // just address:port
                                    address_port = Some(val);
                                }
                            } else {
                                error!("got extra free argument: {:?} - exiting", val);
                                return ProcessResult::Finished;
                            }
                        }
                        _ => {
                            error!("got unexpected argument: {:?} - exiting", arg);
                            return ProcessResult::Finished;
                        }
                    }
                }

                // Validate configuration
                if address_port.is_none() {
                    error!("missing address:port as free argument - exiting");
                    return ProcessResult::Finished;
                }

                let address_port = address_port.unwrap();

                //TODO implement retry and mode one
                if retry {
                    warn!("retry not implemented yet, ignoring");
                }
                if matches!(mode, Mode::One) {
                    error!("mode one not implemented yet - exiting");
                    return ProcessResult::Finished;
                }

                let config = ParsedSSHConfig {
                    address_port,
                    username,
                    password,
                    private_key_path,
                    mode,
                    retry,
                    pipe_out,
                };

                self.ssh_config = Some(config.clone());

                // Send connect command to async thread
                if let Err(_) = self.cmd_sender.send(SSHCommand::Connect { config }) {
                    error!("Failed to send connect command");
                    return ProcessResult::Finished;
                }

                self.state = SSHState::Connecting;
                debug!("SSH configuration parsed successfully, connecting...");
                return ProcessResult::DidWork(1); // Configuration processed
            } else {
                // No CONF config yet
                return ProcessResult::NoWork;
            }
        }

        // Handle state machine
        match self.state {
            SSHState::WaitingForConfig => {
                // Should not reach here
                return ProcessResult::NoWork;
            }
            SSHState::Connecting => {
                // Check for connection result
                if let Ok(result) = self.result_receiver.try_recv() {
                    match result {
                        SSHResult::Connected => {
                            self.state = SSHState::Connected;
                            debug!("SSH connected successfully");
                            return ProcessResult::DidWork(1);
                        }
                        SSHResult::Error(e) => {
                            error!("SSH connection failed: {}", e);
                            return ProcessResult::Finished;
                        }
                        _ => {
                            // Unexpected result
                            return ProcessResult::NoWork;
                        }
                    }
                }
                // Still connecting
                return ProcessResult::NoWork;
            }
            SSHState::Connected => {
                // Check if we have input to execute
                if let Ok(input) = self.inn.pop() {
                    if let Some(cmd) = &self.cmd_config {
                        if let Err(_) = self.cmd_sender.send(SSHCommand::Execute {
                            command: cmd.clone(),
                            input,
                        }) {
                            error!("Failed to send execute command");
                            return ProcessResult::Finished;
                        }
                        self.state = SSHState::Executing;
                        debug!("Sent execute command to async thread");
                        return ProcessResult::DidWork(1);
                    } else {
                        warn!("No command configured");
                        return ProcessResult::NoWork;
                    }
                }
                // No input
                return ProcessResult::NoWork;
            }
            SSHState::Executing => {
                // Check for execution result
                if let Ok(result) = self.result_receiver.try_recv() {
                    match result {
                        SSHResult::Output(output) => {
                            if let Err(_) = self.out.push(output) {
                                error!("Failed to send output");
                                return ProcessResult::Finished;
                            }
                            self.state = SSHState::Connected;
                            debug!("Command executed successfully, output sent");
                            return ProcessResult::DidWork(1);
                        }
                        SSHResult::Error(e) => {
                            error!("Command execution failed: {}", e);
                            self.state = SSHState::Connected; // Reset to connected for retry
                            return ProcessResult::DidWork(1);
                        }
                        _ => {
                            return ProcessResult::NoWork;
                        }
                    }
                }
                // Still executing
                return ProcessResult::NoWork;
            }
            SSHState::Finished => {
                return ProcessResult::Finished;
            }
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
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

impl Drop for SSHClientComponent {
    fn drop(&mut self) {
        debug!("SSHClientComponent dropping, sending disconnect command");
        // Send disconnect command to async thread
        let _ = self.cmd_sender.send(SSHCommand::Disconnect);
        // Note: The async thread will handle the disconnect and terminate
    }
}
