use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, error, info, trace};

use imap::extensions::idle::WaitOutcome;
use std::collections::VecDeque;
use std::net::ToSocketAddrs;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const IMAP_CONNECT_TIMEOUT: Duration = Duration::from_secs(7);
const IDLE_POLL_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug)]
enum AppendState {
    WaitingForConfig,
    Connecting,
    Active,
    Finished,
}

#[derive(Debug)]
enum AppendCommand {
    Connect(String),
    Append(Vec<u8>),
    Shutdown,
}

#[derive(Debug)]
enum AppendResult {
    Connected,
    Appended,
    Error(String),
    Stopped,
}

pub struct IMAPAppendComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: AppendState,
    cmd_tx: Sender<AppendCommand>,
    result_rx: Receiver<AppendResult>,
    worker_handle: Option<JoinHandle<()>>,
    pending_input: VecDeque<Vec<u8>>,
    inflight_appends: usize,
}

impl Component for IMAPAppendComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let worker_handle = Some(thread::spawn(move || {
            append_worker_loop(cmd_rx, result_tx, scheduler_waker)
        }));

        IMAPAppendComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            signals_in,
            signals_out,
            state: AppendState::WaitingForConfig,
            cmd_tx,
            result_rx,
            worker_handle,
            pending_input: VecDeque::new(),
            inflight_appends: 0,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("IMAPAppend process() called, state: {:?}", self.state);
        let mut work_units = 0u32;

        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                let _ = self.cmd_tx.send(AppendCommand::Shutdown);
                self.state = AppendState::Finished;
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                let _ = self.signals_out.try_send(b"pong".to_vec());
            }
        }

        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                AppendResult::Connected => {
                    self.state = AppendState::Active;
                    work_units += 1;
                }
                AppendResult::Appended => {
                    self.inflight_appends = self.inflight_appends.saturating_sub(1);
                    work_units += 1;
                }
                AppendResult::Stopped => {
                    self.state = AppendState::Finished;
                    return ProcessResult::Finished;
                }
                AppendResult::Error(err) => {
                    error!("IMAP append worker failed: {}", err);
                    self.state = AppendState::Finished;
                    return ProcessResult::Finished;
                }
            }
        }

        match self.state {
            AppendState::WaitingForConfig => {
                if let Ok(url_vec) = self.conf.pop() {
                    let url = std::str::from_utf8(&url_vec)
                        .expect("invalid utf-8")
                        .to_owned();
                    if self.cmd_tx.send(AppendCommand::Connect(url)).is_err() {
                        self.state = AppendState::Finished;
                        return ProcessResult::Finished;
                    }
                    self.state = AppendState::Connecting;
                    work_units += 1;
                }
            }
            AppendState::Connecting => {}
            AppendState::Active => {
                if let Ok(chunk) = self.inn.read_chunk(self.inn.slots()) {
                    for ip in chunk {
                        self.pending_input.push_back(ip);
                    }
                }

                while context.remaining_budget > 0 {
                    let Some(packet) = self.pending_input.front().cloned() else {
                        break;
                    };
                    if self.cmd_tx.send(AppendCommand::Append(packet)).is_err() {
                        self.state = AppendState::Finished;
                        return ProcessResult::Finished;
                    }
                    self.pending_input.pop_front();
                    self.inflight_appends += 1;
                    context.remaining_budget -= 1;
                    work_units += 1;
                }

                if self.pending_input.is_empty()
                    && self.inn.is_abandoned()
                    && self.inflight_appends == 0
                {
                    info!("EOF on inport, shutting down");
                    let _ = self.cmd_tx.send(AppendCommand::Shutdown);
                    self.state = AppendState::Finished;
                    return ProcessResult::Finished;
                }

                if !self.pending_input.is_empty() {
                    context.signal_ready();
                }
            }
            AppendState::Finished => return ProcessResult::Finished,
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("IMAPAppend"),
            description: String::from(
                "Appends data as-is from IN port to the mailbox given in CONF.",
            ),
            icon: String::from("cloud-upload"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL which includes encryption, server, username, password, mailbox name"),
                    values_allowed: vec![],
                    value_default: String::from("imaps://username:password@example.com:993/INBOX"),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be appended to given mailbox"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

impl Drop for IMAPAppendComponent {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(AppendCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Debug)]
enum FetchState {
    WaitingForConfig,
    Connecting,
    Active,
    Finished,
}

#[derive(Debug)]
enum FetchCommand {
    Connect(String),
    Poll,
    Shutdown,
}

#[derive(Debug)]
enum FetchResult {
    Connected,
    Messages(Vec<Vec<u8>>),
    IdleTimeout,
    Error(String),
    Stopped,
}

pub struct IMAPFetchIdleComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: FetchState,
    cmd_tx: Sender<FetchCommand>,
    result_rx: Receiver<FetchResult>,
    worker_handle: Option<JoinHandle<()>>,
    pending_messages: VecDeque<Vec<u8>>,
    poll_inflight: bool,
}

impl Component for IMAPFetchIdleComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let worker_handle = Some(thread::spawn(move || {
            fetch_worker_loop(cmd_rx, result_tx, scheduler_waker)
        }));

        IMAPFetchIdleComponent {
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
            signals_in,
            signals_out,
            state: FetchState::WaitingForConfig,
            cmd_tx,
            result_rx,
            worker_handle,
            pending_messages: VecDeque::new(),
            poll_inflight: false,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("IMAPFetchIdle process() called, state: {:?}", self.state);
        let mut work_units = 0u32;

        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                let _ = self.cmd_tx.send(FetchCommand::Shutdown);
                self.state = FetchState::Finished;
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                let _ = self.signals_out.try_send(b"pong".to_vec());
            }
        }

        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                FetchResult::Connected => {
                    self.state = FetchState::Active;
                    self.poll_inflight = false;
                    work_units += 1;
                }
                FetchResult::Messages(messages) => {
                    for msg in messages {
                        self.pending_messages.push_back(msg);
                    }
                    self.poll_inflight = false;
                    work_units += 1;
                }
                FetchResult::IdleTimeout => {
                    self.poll_inflight = false;
                    work_units += 1;
                }
                FetchResult::Stopped => {
                    self.state = FetchState::Finished;
                    return ProcessResult::Finished;
                }
                FetchResult::Error(err) => {
                    error!("IMAP fetch worker failed: {}", err);
                    self.state = FetchState::Finished;
                    return ProcessResult::Finished;
                }
            }
        }

        while context.remaining_budget > 0 {
            let Some(msg) = self.pending_messages.front().cloned() else {
                break;
            };
            match self.out.push(msg) {
                Ok(()) => {
                    self.pending_messages.pop_front();
                    context.remaining_budget -= 1;
                    work_units += 1;
                }
                Err(PushError::Full(_)) => break,
            }
        }

        match self.state {
            FetchState::WaitingForConfig => {
                if let Ok(url_vec) = self.conf.pop() {
                    let url = std::str::from_utf8(&url_vec)
                        .expect("invalid utf-8")
                        .to_owned();
                    if self.cmd_tx.send(FetchCommand::Connect(url)).is_err() {
                        self.state = FetchState::Finished;
                        return ProcessResult::Finished;
                    }
                    self.state = FetchState::Connecting;
                    self.poll_inflight = true;
                    work_units += 1;
                }
            }
            FetchState::Connecting => {}
            FetchState::Active => {
                if !self.poll_inflight {
                    if self.cmd_tx.send(FetchCommand::Poll).is_err() {
                        self.state = FetchState::Finished;
                        return ProcessResult::Finished;
                    }
                    self.poll_inflight = true;
                    work_units += 1;
                }
            }
            FetchState::Finished => return ProcessResult::Finished,
        }

        if !self.pending_messages.is_empty() {
            context.signal_ready();
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("IMAPFetchIdle"),
            description: String::from("Fetches and then idles on the IMAP mailbox given in CONF and forwards received messages to the OUT outport."),
            icon: String::from("cloud-download"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("CONF"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("connection URL which includes encryption, server, username, password, mailbox name"),
                values_allowed: vec![],
                value_default: String::from("imaps://username:password@example.com:993/INBOX"),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("contents of received message in given mailbox"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}

impl Drop for IMAPFetchIdleComponent {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(FetchCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

fn append_worker_loop(
    cmd_rx: Receiver<AppendCommand>,
    result_tx: Sender<AppendResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let mut session: Option<imap::Session<native_tls::TlsStream<std::net::TcpStream>>> = None;
    let mut mailbox = String::new();

    while let Ok(cmd) = cmd_rx.recv() {
        let result = match cmd {
            AppendCommand::Connect(url) => match login_and_connect(&url) {
                Ok((sess, mb)) => {
                    session = Some(sess);
                    mailbox = mb;
                    AppendResult::Connected
                }
                Err(e) => AppendResult::Error(e),
            },
            AppendCommand::Append(packet) => {
                if let Some(sess) = session.as_mut() {
                    match sess.append(mailbox.as_str(), packet) {
                        Ok(_) => AppendResult::Appended,
                        Err(e) => AppendResult::Error(e.to_string()),
                    }
                } else {
                    AppendResult::Error("append requested before connection".to_string())
                }
            }
            AppendCommand::Shutdown => {
                if let Some(mut sess) = session.take() {
                    let _ = close(&mut sess);
                }
                let _ = result_tx.send(AppendResult::Stopped);
                if let Some(waker) = scheduler_waker.as_ref() {
                    waker();
                }
                break;
            }
        };

        if result_tx.send(result).is_err() {
            break;
        }
        if let Some(waker) = scheduler_waker.as_ref() {
            waker();
        }
    }
}

fn fetch_worker_loop(
    cmd_rx: Receiver<FetchCommand>,
    result_tx: Sender<FetchResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let mut session: Option<imap::Session<native_tls::TlsStream<std::net::TcpStream>>> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        let result = match cmd {
            FetchCommand::Connect(url) => match login_and_connect(&url) {
                Ok((mut sess, _mailbox)) => {
                    // fetch unseen once immediately after connect
                    let initial = fetch_unseen_messages(&mut sess).unwrap_or_default();
                    session = Some(sess);
                    if !initial.is_empty() {
                        let _ = result_tx.send(FetchResult::Messages(initial));
                    }
                    FetchResult::Connected
                }
                Err(e) => FetchResult::Error(e),
            },
            FetchCommand::Poll => {
                if let Some(sess) = session.as_mut() {
                    match sess.idle().and_then(|idle| idle.wait_with_timeout(IDLE_POLL_TIMEOUT)) {
                        Ok(WaitOutcome::MailboxChanged) => match fetch_unseen_messages(sess) {
                            Ok(messages) => FetchResult::Messages(messages),
                            Err(e) => FetchResult::Error(e),
                        },
                        Ok(WaitOutcome::TimedOut) => FetchResult::IdleTimeout,
                        Err(e) => FetchResult::Error(e.to_string()),
                    }
                } else {
                    FetchResult::Error("poll requested before connection".to_string())
                }
            }
            FetchCommand::Shutdown => {
                if let Some(mut sess) = session.take() {
                    let _ = close(&mut sess);
                }
                let _ = result_tx.send(FetchResult::Stopped);
                if let Some(waker) = scheduler_waker.as_ref() {
                    waker();
                }
                break;
            }
        };

        if result_tx.send(result).is_err() {
            break;
        }
        if let Some(waker) = scheduler_waker.as_ref() {
            waker();
        }
    }
}

fn login_and_connect(
    url: &str,
) -> Result<(
    imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
    String,
), String> {
    let url_parsed = url::Url::parse(url).map_err(|e| e.to_string())?;

    let protocol = url_parsed.scheme();
    if protocol != "imaps" {
        return Err("only imaps:// protocol is supported".to_string());
    }

    let user = url_parsed.username().replace("%40", "@");
    let password = url_parsed
        .password()
        .ok_or_else(|| "no password given in connection URL".to_string())?;
    let host = url_parsed
        .host_str()
        .ok_or_else(|| "no host given in connection URL".to_string())?;
    let port = url_parsed.port().unwrap_or(993);

    let mut mailbox = url_parsed.path().to_string();
    if mailbox.is_empty() || mailbox == "/" {
        return Err("no mailbox given in connection URL path".to_string());
    }
    mailbox = mailbox.trim_start_matches('/').to_string();

    debug!(
        "user={} pass={}... host={} mailbox={}",
        user,
        password.chars().take(3).collect::<String>(),
        host,
        mailbox
    );

    let tls = native_tls::TlsConnector::builder()
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .map_err(|e| e.to_string())?;

    let socket_addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| "IMAP server host resolved to no address".to_string())?;

    let tcp = std::net::TcpStream::connect_timeout(&socket_addr, IMAP_CONNECT_TIMEOUT)
        .map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;

    let tls_stream = native_tls::TlsConnector::connect(&tls, host, tcp).map_err(|e| e.to_string())?;
    let mut client = imap::Client::new(tls_stream);
    client.read_greeting().map_err(|e| e.to_string())?;

    let mut imap_session = client
        .login(user, password)
        .map_err(|e| e.0.to_string())?;

    imap_session
        .select(mailbox.as_str())
        .map_err(|e| e.to_string())?;

    Ok((imap_session, mailbox))
}

fn close(
    imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
) -> Result<(), String> {
    let _ = imap_session.expunge();
    imap_session.logout().map_err(|e| e.to_string())
}

fn fetch_unseen_messages(
    imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
) -> Result<Vec<Vec<u8>>, String> {
    let uids_set = imap_session
        .uid_search("UNSEEN")
        .map_err(|e| e.to_string())?;

    if uids_set.is_empty() {
        return Ok(Vec::new());
    }

    let mut uids: Vec<&u32> = uids_set.iter().collect();
    uids.sort_by(|a, b| a.cmp(b));

    let uid_set = uids
        .iter()
        .map(|val| val.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let messages = imap_session
        .uid_fetch(&uid_set, "BODY[]")
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for message in messages.iter() {
        if let Some(body) = message.body() {
            out.push(body.to_vec());
        }
    }

    imap_session
        .uid_store(&uid_set, "+FLAGS (\\Deleted)")
        .map_err(|e| e.to_string())?;

    Ok(out)
}
