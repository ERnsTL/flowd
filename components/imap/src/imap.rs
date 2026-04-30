use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, error, info, trace, warn};

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
    Append(FbpMessage),
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
    pending_input: VecDeque<FbpMessage>,
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

        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                let _ = self.cmd_tx.send(AppendCommand::Shutdown);
                self.state = AppendState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
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
                    let url = url_vec.as_text().expect("invalid text").to_owned();
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
    Connect(FetchConfig),
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

#[derive(Debug, Clone)]
struct FetchConfig {
    url: String,
    delete: bool,
    include_full: bool,
    include_uid: bool,
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

        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                let _ = self.cmd_tx.send(FetchCommand::Shutdown);
                self.state = FetchState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
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
            let msg_fbp = FbpMessage::from_bytes(msg);
            match self.out.push(msg_fbp) {
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
                    let url = url_vec.as_text().expect("invalid text").to_owned();
                    match parse_fetch_config(&url) {
                        Ok(fetch_conf) => {
                            if self.cmd_tx.send(FetchCommand::Connect(fetch_conf)).is_err() {
                                self.state = FetchState::Finished;
                                return ProcessResult::Finished;
                            }
                            self.state = FetchState::Connecting;
                            self.poll_inflight = true;
                            work_units += 1;
                        }
                        Err(e) => {
                            error!("invalid IMAPFetchIdle CONF: {}", e);
                            self.state = FetchState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
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
            description: String::from("Fetches and then idles on the IMAP mailbox given in CONF and forwards received messages to the OUT outport. Supports CONF query params delete=true|false and retrieve=full|uid|full,uid."),
            icon: String::from("cloud-download"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("CONF"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("connection URL with optional query params: delete=true|false and retrieve=full|uid|full,uid"),
                values_allowed: vec![],
                value_default: String::from("imaps://username:password@example.com:993/INBOX?delete=false&retrieve=full"),
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
                    let bytes = packet.as_bytes().unwrap_or(&[]);
                    match sess.append(mailbox.as_str(), bytes) {
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
    let mut fetch_config = FetchConfig {
        url: String::new(),
        delete: false,
        include_full: true,
        include_uid: false,
    };

    while let Ok(cmd) = cmd_rx.recv() {
        let result = match cmd {
            FetchCommand::Connect(config) => match login_and_connect(&config.url) {
                Ok((mut sess, _mailbox)) => {
                    fetch_config = config;
                    // fetch unseen once immediately after connect
                    let initial: Vec<Vec<u8>> =
                        fetch_unseen_messages_configurable(&mut sess, fetch_config.delete)
                        .map(|messages| {
                            messages
                                .into_iter()
                                .map(|(uid, body)| {
                                    encode_fetch_output(
                                        &uid,
                                        &body,
                                        fetch_config.include_full,
                                        fetch_config.include_uid,
                                    )
                                })
                                .collect::<Vec<Vec<u8>>>()
                        })
                        .unwrap_or_default();
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
                        Ok(WaitOutcome::MailboxChanged) => match fetch_unseen_messages_configurable(
                            sess,
                            fetch_config.delete,
                        ) {
                            Ok(messages) => FetchResult::Messages(
                                messages
                                    .into_iter()
                                    .map(|(uid, body)| {
                                        encode_fetch_output(
                                            &uid,
                                            &body,
                                            fetch_config.include_full,
                                            fetch_config.include_uid,
                                        )
                                    })
                                    .collect(),
                            ),
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

fn parse_fetch_config(conf_url: &str) -> Result<FetchConfig, String> {
    let parsed = url::Url::parse(conf_url).map_err(|e| e.to_string())?;

    let mut delete = false;
    let mut include_full = true;
    let mut include_uid = false;

    for (key, value) in parsed.query_pairs() {
        if key.eq_ignore_ascii_case("delete") {
            delete = value
                .parse::<bool>()
                .map_err(|_| format!("invalid delete value '{}', expected true or false", value))?;
        } else if key.eq_ignore_ascii_case("retrieve") {
            include_full = false;
            include_uid = false;
            for token in value.split(',').map(|t| t.trim().to_ascii_lowercase()) {
                match token.as_str() {
                    "full" => include_full = true,
                    "uid" => include_uid = true,
                    "" => {}
                    _ => {
                        return Err(format!(
                            "invalid retrieve token '{}', expected full and/or uid",
                            token
                        ));
                    }
                }
            }
            if !include_full && !include_uid {
                return Err("retrieve must include at least one of full or uid".to_string());
            }
        }
    }

    Ok(FetchConfig {
        url: conf_url.to_string(),
        delete,
        include_full,
        include_uid,
    })
}

fn encode_fetch_output(uid: &str, body: &[u8], include_full: bool, include_uid: bool) -> Vec<u8> {
    match (include_full, include_uid) {
        (true, false) => body.to_vec(),
        (false, true) => uid.as_bytes().to_vec(),
        (true, true) => serde_json::to_vec(&serde_json::json!({
            "uid": uid,
            "body": body
        }))
        .unwrap_or_else(|_| {
            br#"{"error":"failed to serialize fetched message","uid":"","body":[]}"#.to_vec()
        }),
        (false, false) => Vec::new(),
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

// ===== NEW IMAP COMPONENTS =====

#[derive(Debug)]
enum NewFetchState {
    WaitingForConfig,
    Connecting,
    Active,
    Finished,
}

#[derive(Debug)]
enum NewFetchCommand {
    Connect(String),
    Poll,
    Shutdown,
}

#[derive(Debug)]
enum NewFetchResult {
    Connected,
    Messages(Vec<serde_json::Value>),
    IdleTimeout,
    Error(String),
    Stopped,
}

pub struct IMAPFetchComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: NewFetchState,
    cmd_tx: Sender<NewFetchCommand>,
    result_rx: Receiver<NewFetchResult>,
    worker_handle: Option<JoinHandle<()>>,
    poll_inflight: bool,
}

impl Component for IMAPFetchComponent {
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
            fetch_worker_loop_new(cmd_rx, result_tx, scheduler_waker)
        }));

        IMAPFetchComponent {
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
            state: NewFetchState::WaitingForConfig,
            cmd_tx,
            result_rx,
            worker_handle,
            poll_inflight: false,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("IMAPFetch process() called, state: {:?}", self.state);
        let mut work_units = 0u32;

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                let _ = self.cmd_tx.send(NewFetchCommand::Shutdown);
                self.state = NewFetchState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text);
            }
        }

        // Handle results from worker
        if let Ok(result) = self.result_rx.try_recv() {
            match result {
                NewFetchResult::Connected => {
                    info!("IMAP connection established");
                    self.state = NewFetchState::Active;
                    work_units += 1;
                }
                NewFetchResult::Messages(messages) => {
                    for message_json in messages {
                        let json_str = serde_json::to_string(&message_json)
                            .unwrap_or_else(|_| r#"{"error": "failed to serialize message"}"#.to_string());
                        let output_msg = FbpMessage::from_bytes(json_str.into_bytes());
                        if let Err(_) = self.out.push(output_msg) {
                            warn!("output buffer full, dropping message");
                        }
                    }
                    self.poll_inflight = false;
                    work_units += 1;
                }
                NewFetchResult::IdleTimeout => {
                    self.poll_inflight = false;
                    // Continue polling
                }
                NewFetchResult::Error(e) => {
                    error!("IMAP error: {}", e);
                    self.state = NewFetchState::Finished;
                    return ProcessResult::Finished;
                }
                NewFetchResult::Stopped => {
                    info!("IMAP worker stopped");
                    self.state = NewFetchState::Finished;
                    return ProcessResult::Finished;
                }
            }
        }

        match self.state {
            NewFetchState::WaitingForConfig => {
                if let Ok(conf_msg) = self.conf.pop() {
                    let conf_text = conf_msg.as_text().expect("config must be text");
                    debug!("received config: {}", conf_text);
                    if self.cmd_tx.send(NewFetchCommand::Connect(conf_text.to_string())).is_err() {
                        self.state = NewFetchState::Finished;
                        return ProcessResult::Finished;
                    }
                    self.state = NewFetchState::Connecting;
                    work_units += 1;
                } else {
                    return ProcessResult::NoWork;
                }
            }
            NewFetchState::Connecting => {
                // Wait for connection result
                return ProcessResult::NoWork;
            }
            NewFetchState::Active => {
                if !self.poll_inflight {
                    if self.cmd_tx.send(NewFetchCommand::Poll).is_err() {
                        self.state = NewFetchState::Finished;
                        return ProcessResult::Finished;
                    }
                    self.poll_inflight = true;
                    work_units += 1;
                }
            }
            NewFetchState::Finished => return ProcessResult::Finished,
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
            name: String::from("IMAPFetch"),
            description: String::from("Fetches new messages from IMAP mailbox without deleting them, outputs UID and body for each message."),
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
                description: String::from("JSON with message UID and raw body bytes"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}

impl Drop for IMAPFetchComponent {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(NewFetchCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

// ===== IMAP MOVECOPY COMPONENT =====

#[derive(Debug)]
enum MoveCopyState {
    WaitingForConfig,
    Connecting,
    Active,
    Finished,
}

#[derive(Debug)]
enum MoveCopyCommand {
    Connect(String),
    MoveCopy { uid: String, mailbox: String, copy: bool },
    Shutdown,
}

#[derive(Debug)]
enum MoveCopyResult {
    Connected,
    MovedCopied,
    Error(String),
    Stopped,
}

pub struct IMAPMoveCopyComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: MoveCopyState,
    cmd_tx: Sender<MoveCopyCommand>,
    result_rx: Receiver<MoveCopyResult>,
    worker_handle: Option<JoinHandle<()>>,
    pending_input: VecDeque<FbpMessage>,
    inflight_operations: usize,
    copy_mode: bool,
}

impl Component for IMAPMoveCopyComponent {
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
            movecopy_worker_loop(cmd_rx, result_tx, scheduler_waker)
        }));

        IMAPMoveCopyComponent {
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
            state: MoveCopyState::WaitingForConfig,
            cmd_tx,
            result_rx,
            worker_handle,
            pending_input: VecDeque::new(),
            inflight_operations: 0,
            copy_mode: false, // Will be set from config
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("IMAPMoveCopy process() called, state: {:?}", self.state);
        let mut work_units = 0u32;

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                let _ = self.cmd_tx.send(MoveCopyCommand::Shutdown);
                self.state = MoveCopyState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text);
            }
        }

        // Handle results from worker
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                MoveCopyResult::Connected => {
                    self.state = MoveCopyState::Active;
                    work_units += 1;
                }
                MoveCopyResult::MovedCopied => {
                    self.inflight_operations = self.inflight_operations.saturating_sub(1);
                    work_units += 1;
                }
                MoveCopyResult::Error(err) => {
                    error!("IMAP movecopy worker failed: {}", err);
                    self.state = MoveCopyState::Finished;
                    return ProcessResult::Finished;
                }
                MoveCopyResult::Stopped => {
                    self.state = MoveCopyState::Finished;
                    return ProcessResult::Finished;
                }
            }
        }

        match self.state {
            MoveCopyState::WaitingForConfig => {
                if let Ok(conf_msg) = self.conf.pop() {
                    let conf_text = conf_msg.as_text().expect("config must be text");
                    debug!("received config: {}", conf_text);

                    // Parse config to extract copy mode
                    // Format: imaps://user:pass@host:993/mailbox?copy=true
                    let url = url::Url::parse(conf_text).unwrap_or_else(|_| url::Url::parse("imaps://localhost").unwrap());
                    self.copy_mode = url.query_pairs()
                        .find(|(k, _)| k == "copy")
                        .and_then(|(_, v)| v.parse().ok())
                        .unwrap_or(false);

                    if self.cmd_tx.send(MoveCopyCommand::Connect(conf_text.to_string())).is_err() {
                        self.state = MoveCopyState::Finished;
                        return ProcessResult::Finished;
                    }
                    self.state = MoveCopyState::Connecting;
                    work_units += 1;
                }
            }
            MoveCopyState::Connecting => {}
            MoveCopyState::Active => {
                if let Ok(chunk) = self.inn.read_chunk(self.inn.slots()) {
                    for ip in chunk {
                        self.pending_input.push_back(ip);
                    }
                }

                while context.remaining_budget > 0 {
                    let Some(packet) = self.pending_input.front().cloned() else {
                        break;
                    };

                    // Parse input JSON: {"uid": "123", "mailbox": "INBOX.Work"}
                    if let Some(json_str) = packet.as_text() {
                        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(json_str) {
                            if let (Some(uid), Some(mailbox)) = (
                                json_val.get("uid").and_then(|v| v.as_str()),
                                json_val.get("mailbox").and_then(|v| v.as_str())
                            ) {
                                if self.cmd_tx.send(MoveCopyCommand::MoveCopy {
                                    uid: uid.to_string(),
                                    mailbox: mailbox.to_string(),
                                    copy: self.copy_mode,
                                }).is_err() {
                                    self.state = MoveCopyState::Finished;
                                    return ProcessResult::Finished;
                                }
                                self.pending_input.pop_front();
                                self.inflight_operations += 1;
                                context.remaining_budget -= 1;
                                work_units += 1;
                            } else {
                                warn!("Invalid input JSON format, expected {{uid, mailbox}}");
                                self.pending_input.pop_front();
                            }
                        } else {
                            warn!("Failed to parse input as JSON");
                            self.pending_input.pop_front();
                        }
                    } else {
                        warn!("Input is not text");
                        self.pending_input.pop_front();
                    }
                }

                if self.pending_input.is_empty()
                    && self.inn.is_abandoned()
                    && self.inflight_operations == 0
                {
                    info!("EOF on inport, shutting down");
                    let _ = self.cmd_tx.send(MoveCopyCommand::Shutdown);
                    self.state = MoveCopyState::Finished;
                    return ProcessResult::Finished;
                }

                if !self.pending_input.is_empty() {
                    context.signal_ready();
                }
            }
            MoveCopyState::Finished => return ProcessResult::Finished,
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
            name: String::from("IMAPMoveCopy"),
            description: String::from("Moves or copies messages between IMAP mailboxes based on UID and target mailbox."),
            icon: String::from("arrows-alt"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL with optional copy=true query parameter"),
                    values_allowed: vec![],
                    value_default: String::from("imaps://username:password@example.com:993/INBOX?copy=true"),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("JSON with uid and mailbox fields"),
                    values_allowed: vec![],
                    value_default: String::from(r#"{"uid": "123", "mailbox": "INBOX.Work"}"#),
                },
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

impl Drop for IMAPMoveCopyComponent {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(MoveCopyCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

// ===== IMAP DELETE COMPONENT =====

#[derive(Debug)]
enum DeleteState {
    WaitingForConfig,
    Connecting,
    Active,
    Finished,
}

#[derive(Debug)]
enum DeleteCommand {
    Connect(String),
    Delete(String), // UID to delete
    Shutdown,
}

#[derive(Debug)]
enum DeleteResult {
    Connected,
    Deleted,
    Error(String),
    Stopped,
}

pub struct IMAPDeleteComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: DeleteState,
    cmd_tx: Sender<DeleteCommand>,
    result_rx: Receiver<DeleteResult>,
    worker_handle: Option<JoinHandle<()>>,
    pending_input: VecDeque<FbpMessage>,
    inflight_operations: usize,
}

impl Component for IMAPDeleteComponent {
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
            delete_worker_loop(cmd_rx, result_tx, scheduler_waker)
        }));

        IMAPDeleteComponent {
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
            state: DeleteState::WaitingForConfig,
            cmd_tx,
            result_rx,
            worker_handle,
            pending_input: VecDeque::new(),
            inflight_operations: 0,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("IMAPDelete process() called, state: {:?}", self.state);
        let mut work_units = 0u32;

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                let _ = self.cmd_tx.send(DeleteCommand::Shutdown);
                self.state = DeleteState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text);
            }
        }

        // Handle results from worker
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                DeleteResult::Connected => {
                    self.state = DeleteState::Active;
                    work_units += 1;
                }
                DeleteResult::Deleted => {
                    self.inflight_operations = self.inflight_operations.saturating_sub(1);
                    work_units += 1;
                }
                DeleteResult::Error(err) => {
                    error!("IMAP delete worker failed: {}", err);
                    self.state = DeleteState::Finished;
                    return ProcessResult::Finished;
                }
                DeleteResult::Stopped => {
                    self.state = DeleteState::Finished;
                    return ProcessResult::Finished;
                }
            }
        }

        match self.state {
            DeleteState::WaitingForConfig => {
                if let Ok(conf_msg) = self.conf.pop() {
                    let conf_text = conf_msg.as_text().expect("config must be text");
                    debug!("received config: {}", conf_text);
                    if self.cmd_tx.send(DeleteCommand::Connect(conf_text.to_string())).is_err() {
                        self.state = DeleteState::Finished;
                        return ProcessResult::Finished;
                    }
                    self.state = DeleteState::Connecting;
                    work_units += 1;
                }
            }
            DeleteState::Connecting => {}
            DeleteState::Active => {
                if let Ok(chunk) = self.inn.read_chunk(self.inn.slots()) {
                    for ip in chunk {
                        self.pending_input.push_back(ip);
                    }
                }

                while context.remaining_budget > 0 {
                    let Some(packet) = self.pending_input.front().cloned() else {
                        break;
                    };

                    // Parse input: either UID string or JSON with uid field
                    let uid = if let Some(json_str) = packet.as_text() {
                        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(json_str) {
                            json_val.get("uid")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| json_str.to_string())
                        } else {
                            json_str.to_string()
                        }
                    } else {
                        warn!("Input is not text");
                        self.pending_input.pop_front();
                        continue;
                    };

                    if self.cmd_tx.send(DeleteCommand::Delete(uid.to_string())).is_err() {
                        self.state = DeleteState::Finished;
                        return ProcessResult::Finished;
                    }
                    self.pending_input.pop_front();
                    self.inflight_operations += 1;
                    context.remaining_budget -= 1;
                    work_units += 1;
                }

                if self.pending_input.is_empty()
                    && self.inn.is_abandoned()
                    && self.inflight_operations == 0
                {
                    info!("EOF on inport, shutting down");
                    let _ = self.cmd_tx.send(DeleteCommand::Shutdown);
                    self.state = DeleteState::Finished;
                    return ProcessResult::Finished;
                }

                if !self.pending_input.is_empty() {
                    context.signal_ready();
                }
            }
            DeleteState::Finished => return ProcessResult::Finished,
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
            name: String::from("IMAPDelete"),
            description: String::from("Permanently deletes messages from IMAP mailbox by UID."),
            icon: String::from("trash"),
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
                    description: String::from("Message UID to delete (string or JSON with uid field)"),
                    values_allowed: vec![],
                    value_default: String::from(r#""123""#),
                },
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

impl Drop for IMAPDeleteComponent {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(DeleteCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

fn fetch_unseen_messages_configurable(
    imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
    delete: bool,
) -> Result<Vec<(String, Vec<u8>)>, String> {
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
        if let (Some(uid), Some(body)) = (message.uid, message.body()) {
            out.push((uid.to_string(), body.to_vec()));
        }
    }

    if delete {
        imap_session
            .uid_store(&uid_set, "+FLAGS (\\Deleted)")
            .map_err(|e| e.to_string())?;
    }

    Ok(out)
}

// ===== WORKER LOOPS FOR NEW COMPONENTS =====

fn fetch_worker_loop_new(
    cmd_rx: Receiver<NewFetchCommand>,
    result_tx: Sender<NewFetchResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let mut session: Option<imap::Session<native_tls::TlsStream<std::net::TcpStream>>> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        let result = match cmd {
            NewFetchCommand::Connect(url) => match login_and_connect(&url) {
                Ok((mut sess, _mailbox)) => {
                    // fetch unseen once immediately after connect (but don't delete!)
                    let initial = fetch_unseen_messages_no_delete(&mut sess).unwrap_or_default();
                    session = Some(sess);
                    if !initial.is_empty() {
                        let messages: Vec<serde_json::Value> = initial.into_iter()
                            .map(|(uid, body)| serde_json::json!({"uid": uid, "body": body}))
                            .collect();
                        let _ = result_tx.send(NewFetchResult::Messages(messages));
                    }
                    NewFetchResult::Connected
                }
                Err(e) => NewFetchResult::Error(e),
            },
            NewFetchCommand::Poll => {
                if let Some(sess) = session.as_mut() {
                    match sess.idle().and_then(|idle| idle.wait_with_timeout(IDLE_POLL_TIMEOUT)) {
                        Ok(WaitOutcome::MailboxChanged) => match fetch_unseen_messages_no_delete(sess) {
                            Ok(messages_with_uids) => {
                                let messages: Vec<serde_json::Value> = messages_with_uids.into_iter()
                                    .map(|(uid, body)| serde_json::json!({"uid": uid, "body": body}))
                                    .collect();
                                NewFetchResult::Messages(messages)
                            }
                            Err(e) => NewFetchResult::Error(e),
                        },
                        Ok(WaitOutcome::TimedOut) => NewFetchResult::IdleTimeout,
                        Err(e) => NewFetchResult::Error(e.to_string()),
                    }
                } else {
                    NewFetchResult::Error("poll requested before connection".to_string())
                }
            }
            NewFetchCommand::Shutdown => {
                if let Some(mut sess) = session.take() {
                    let _ = close(&mut sess);
                }
                let _ = result_tx.send(NewFetchResult::Stopped);
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

fn fetch_unseen_messages_no_delete(
    imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
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
        if let (Some(uid), Some(body)) = (message.uid, message.body()) {
            out.push((uid.to_string(), body.to_vec()));
        }
    }

    // NOTE: We do NOT mark messages as deleted here!
    // That's the key difference from the old fetch_unseen_messages

    Ok(out)
}

fn movecopy_worker_loop(
    cmd_rx: Receiver<MoveCopyCommand>,
    result_tx: Sender<MoveCopyResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let mut session: Option<imap::Session<native_tls::TlsStream<std::net::TcpStream>>> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        let result = match cmd {
            MoveCopyCommand::Connect(url) => match login_and_connect(&url) {
                Ok((sess, _mailbox)) => {
                    session = Some(sess);
                    MoveCopyResult::Connected
                }
                Err(e) => MoveCopyResult::Error(e),
            },
            MoveCopyCommand::MoveCopy { uid, mailbox, copy } => {
                if let Some(sess) = session.as_mut() {
                    let operation_result = if copy {
                        // Copy message to target mailbox, then delete from source
                        match sess.uid_copy(&uid, &mailbox) {
                            Ok(_) => {
                                // After successful copy, mark source as deleted
                                match sess.uid_store(&uid, "+FLAGS (\\Deleted)") {
                                    Ok(_) => Ok(()),
                                    Err(e) => Err(e.to_string()),
                                }
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    } else {
                        // For move mode, copy then delete (since MOVE may not be supported)
                        match sess.uid_copy(&uid, &mailbox) {
                            Ok(_) => {
                                match sess.uid_store(&uid, "+FLAGS (\\Deleted)") {
                                    Ok(_) => Ok(()),
                                    Err(e) => Err(e.to_string()),
                                }
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    };

                    match operation_result {
                        Ok(_) => MoveCopyResult::MovedCopied,
                        Err(e) => MoveCopyResult::Error(e),
                    }
                } else {
                    MoveCopyResult::Error("movecopy requested before connection".to_string())
                }
            }
            MoveCopyCommand::Shutdown => {
                if let Some(mut sess) = session.take() {
                    let _ = close(&mut sess);
                }
                let _ = result_tx.send(MoveCopyResult::Stopped);
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

fn delete_worker_loop(
    cmd_rx: Receiver<DeleteCommand>,
    result_tx: Sender<DeleteResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let mut session: Option<imap::Session<native_tls::TlsStream<std::net::TcpStream>>> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        let result = match cmd {
            DeleteCommand::Connect(url) => match login_and_connect(&url) {
                Ok((sess, _mailbox)) => {
                    session = Some(sess);
                    DeleteResult::Connected
                }
                Err(e) => DeleteResult::Error(e),
            },
            DeleteCommand::Delete(uid) => {
                if let Some(sess) = session.as_mut() {
                    match sess.uid_store(&uid, "+FLAGS (\\Deleted)") {
                        Ok(_) => {
                            // Expunge to permanently remove
                            match sess.expunge() {
                                Ok(_) => DeleteResult::Deleted,
                                Err(e) => DeleteResult::Error(e.to_string()),
                            }
                        }
                        Err(e) => DeleteResult::Error(e.to_string()),
                    }
                } else {
                    DeleteResult::Error("delete requested before connection".to_string())
                }
            }
            DeleteCommand::Shutdown => {
                if let Some(mut sess) = session.take() {
                    let _ = close(&mut sess);
                }
                let _ = result_tx.send(DeleteResult::Stopped);
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
