#![feature(addr_parse_ascii)] // for TCPClientComponent -> SocketAddr::parse_ascii()
use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError, SchedulerWaker, create_io_channels,
    wake_scheduler,
    ErrorType, RetryConfig, RetryState,
};
use log::{debug, error, info, trace, warn};

// component-specific
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};


// for non-blocking I/O
use mio::Token;



#[derive(Debug)]
enum ClientState {
    WaitingForConfig,
    Connecting {
        addr: SocketAddr,
    },
    // ADR-017: Retry states for failed connections
    RetryingConnection {
        addr: SocketAddr,
        retry_state: RetryState,
    },
    Connected {
        pending_messages: Vec<FbpMessage>,
    },
    Finished,
}

pub struct TCPClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: ClientState,
    scheduler_waker: Option<SchedulerWaker>,
    pending_received: std::collections::VecDeque<FbpMessage>,
    // ADR-017: Bounded IO channels
    cmd_tx: std::sync::mpsc::SyncSender<SocketAddr>,
    result_rx: std::sync::mpsc::Receiver<Result<TcpStream, std::io::Error>>,
    // Background async worker
    _async_worker: Option<std::thread::JoinHandle<()>>,
    // ADR-017: Retry configuration and state
    retry_config: RetryConfig,
    retry_state: RetryState,
}

const CONNECT_TIMEOUT: Duration = Duration::from_millis(10000);
const READ_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const READ_BUFFER: usize = 4096;

// ADR-017: Error classification for TCP connections
fn classify_tcp_error(error: &std::io::Error) -> ErrorType {
    match error.kind() {
        // Transient errors that should be retried
        ErrorKind::TimedOut | ErrorKind::Interrupted | ErrorKind::WouldBlock => ErrorType::Transient,

        // Connection-related errors that are often transient
        ErrorKind::ConnectionRefused | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => {
            ErrorType::Transient
        }

        // Network unreachable, host unreachable - could be transient
        ErrorKind::NetworkUnreachable | ErrorKind::HostUnreachable => ErrorType::Transient,

        // Permanent errors that should fail immediately
        ErrorKind::NotFound | ErrorKind::PermissionDenied | ErrorKind::InvalidInput => ErrorType::Permanent,

        // Default to transient for unknown errors (network issues, etc.)
        _ => ErrorType::Transient,
    }
}

// ADR-017: Async TCP worker that runs in background thread with Tokio runtime
async fn async_tcp_worker(
    cmd_rx: std::sync::mpsc::Receiver<SocketAddr>,
    result_tx: std::sync::mpsc::SyncSender<Result<TcpStream, std::io::Error>>,
    waker: Option<SchedulerWaker>,
) {
    debug!("TCP async worker started");

    while let Ok(addr) = cmd_rx.recv() {
        debug!("TCP worker connecting to: {}", addr);

        // Perform TCP connection (this is synchronous but we're in an async context)
        let result = std::panic::catch_unwind(|| {
            TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        });

        let connection_result = match result {
            Ok(stream_result) => stream_result,
            Err(_) => Err(std::io::Error::new(std::io::ErrorKind::Other, "Connection panicked")),
        };

        // Send result back (ignore send errors - component may have been destroyed)
        let _ = result_tx.try_send(connection_result);

        // ADR-002: Signal scheduler that work is ready
        wake_scheduler(&waker);
    }

    debug!("TCP async worker exiting");
}

impl Component for TCPClientComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        // ADR-017: Create bounded IO channels
        let (cmd_tx, cmd_rx, result_tx, result_rx) = create_io_channels::<SocketAddr, Result<TcpStream, std::io::Error>>();

        // Start background async worker thread
        let waker = scheduler_waker.clone();
        let async_worker = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async_tcp_worker(cmd_rx, result_tx, waker));
        }));

        TCPClientComponent {
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
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in,
            signals_out,
            state: ClientState::WaitingForConfig,
            scheduler_waker,
            pending_received: std::collections::VecDeque::new(),
            // ADR-017: Bounded IO channels
            cmd_tx,
            result_rx,
            // Background async worker
            _async_worker: async_worker,
            // ADR-017: Retry configuration
            retry_config: RetryConfig::default(),
            retry_state: RetryState::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("TCPClient is now process()ing!");

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                self.state = ClientState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        // Handle state transitions that require borrowing
        if let ClientState::Connecting { addr } = &self.state {
            // Check if connection completed
            match self.result_rx.try_recv() {
                Ok(Ok(stream)) => {
                    // Connection successful - transition to connected state
                    if let Err(err) = stream.set_read_timeout(READ_TIMEOUT) {
                        warn!("failed to set read timeout: {}", err);
                    }
                    if let Err(err) = stream.set_write_timeout(WRITE_TIMEOUT) {
                        warn!("failed to set write timeout: {}", err);
                    }
                    self.state = ClientState::Connected {
                        pending_messages: Vec::new(),
                    };
                    debug!("TCP connection established");
                    return ProcessResult::DidWork(1);
                }
                Ok(Err(e)) => {
                    // ADR-017: Classify error and handle retry logic
                    let error_type = classify_tcp_error(&e);
                    match error_type {
                        ErrorType::Transient => {
                            if self.retry_state.should_retry(error_type, &self.retry_config) {
                                let mut retry_state = std::mem::take(&mut self.retry_state);
                                retry_state.calculate_next_retry(&self.retry_config);
                                retry_state.last_error_type = error_type;
                                warn!("TCP connection failed (transient error), will retry in {:?} (attempt {}): {}",
                                      retry_state.next_retry_at - Instant::now(), retry_state.attempt, e);
                                self.state = ClientState::RetryingConnection {
                                    addr: *addr,
                                    retry_state,
                                };
                                return ProcessResult::DidWork(1);
                            } else {
                                error!("TCP connection failed after {} retries, giving up: {}", self.retry_config.max_retries, e);
                                self.state = ClientState::Finished;
                                return ProcessResult::Finished;
                            }
                        }
                        ErrorType::Permanent => {
                            error!("TCP connection failed (permanent error), not retrying: {}", e);
                            self.state = ClientState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                }
                Err(_) => {
                    // Connection still in progress, wait
                    context.wake_at(
                        Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                    );
                    return ProcessResult::NoWork;
                }
            }
        }

        let current_state = std::mem::replace(&mut self.state, ClientState::Finished);
        match current_state {
            ClientState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_msg) = self.conf.pop() {
                    let url_str = url_msg.as_text()
                        .or_else(|| url_msg.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                        .expect("invalid utf-8");
                    debug!("got config URL: {}", url_str);

                    // Parse address from URL
                    let url = url::Url::parse(&url_str).expect("failed to parse URL");
                    let addr = SocketAddr::parse_ascii(
                        format!(
                            "{}:{}",
                            url.host_str().expect("failed to parse host from URL"),
                            url.port().expect("failed to parse port from URL")
                        )
                        .as_bytes(),
                    )
                    .expect("failed to parse socket address from URL");

                    // ADR-017: Send to background async worker via bounded channel
                    match self.cmd_tx.try_send(addr) {
                        Ok(()) => {
                            debug!("TCP connection request enqueued to async worker");
                            self.state = ClientState::Connecting {
                                addr,
                            };
                            return ProcessResult::DidWork(1);
                        }
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            // ADR-017: Channel full, apply backpressure
                            debug!("TCP command channel full, applying backpressure");
                            warn!("TCP connection request dropped due to full command channel");
                            self.state = ClientState::WaitingForConfig;
                            return ProcessResult::NoWork;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            // Worker thread died, component should finish
                            warn!("TCP async worker disconnected");
                            self.state = ClientState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                }

                // Check for completed connection results from async worker
                match self.result_rx.try_recv() {
                    Ok(result) => {
                        // Process the connection result
                        match result {
                            Ok(stream) => {
                                // Connection successful - transition to connected state
                                if let Err(err) = stream.set_read_timeout(READ_TIMEOUT) {
                                    warn!("failed to set read timeout: {}", err);
                                }
                                if let Err(err) = stream.set_write_timeout(WRITE_TIMEOUT) {
                                    warn!("failed to set write timeout: {}", err);
                                }
                                self.state = ClientState::Connected {
                                    pending_messages: Vec::new(),
                                };
                                debug!("TCP connection established");
                                return ProcessResult::DidWork(1);
                            }
                            Err(e) => {
                                // Connection failed
                                error!("TCP connection failed: {}", e);
                                self.state = ClientState::Finished;
                                return ProcessResult::Finished;
                            }
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // No results ready, continue waiting
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // Worker thread died
                        warn!("TCP async worker result channel disconnected");
                        self.state = ClientState::Finished;
                        return ProcessResult::Finished;
                    }
                }

                // No config yet, but check if we should yield budget
                if context.remaining_budget == 0 {
                    return ProcessResult::NoWork;
                }
                context.remaining_budget -= 1;
                return ProcessResult::NoWork;
            }

            ClientState::Connecting { addr: _ } => {
                // This should have been handled above
                self.state = ClientState::Finished;
                ProcessResult::Finished
            }

            // ADR-017: Handle retry state
            ClientState::RetryingConnection { addr, retry_state } => {
                // Check if it's time to retry
                if retry_state.is_ready_to_retry() {
                    // Try to send connection request again
                    match self.cmd_tx.try_send(addr) {
                        Ok(()) => {
                            debug!("TCP retry connection request enqueued to async worker (attempt {})", retry_state.attempt);
                            self.state = ClientState::Connecting { addr };
                            return ProcessResult::DidWork(1);
                        }
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            // Channel full, wait and try again later
                            context.wake_at(
                                Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                            );
                            self.state = ClientState::RetryingConnection { addr, retry_state };
                            return ProcessResult::NoWork;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            // Worker thread died
                            warn!("TCP async worker disconnected during retry");
                            self.state = ClientState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                } else {
                    // Not ready to retry yet, schedule wakeup
                    context.wake_at(retry_state.next_retry_at);
                    self.state = ClientState::RetryingConnection { addr, retry_state };
                    return ProcessResult::NoWork;
                }
            }

            ClientState::Connected { mut pending_messages } => {
                let mut work_units = 0;

                // Collect available messages
                while context.remaining_budget > 0 && !self.inn.is_empty() {
                    let chunk = self.inn.read_chunk(1).expect("receive as chunk failed");
                    for ip in chunk {
                        pending_messages.push(ip);
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                }

                // Send pending messages asynchronously
                if !pending_messages.is_empty() && context.remaining_budget > 0 {
                    let messages = std::mem::take(&mut pending_messages);
                    let waker = self.scheduler_waker.clone();

                    tokio::spawn(async move {
                        // Send messages (in async task to avoid blocking)
                        for message in messages {
                            if let Err(_e) = std::panic::catch_unwind(|| {
                                // This is a simplified approach - in a real implementation
                                // we'd need proper async TCP operations
                                debug!("Would send TCP message: {:?}", message);
                            }) {
                                warn!("Failed to send TCP message");
                            }
                        }
                        // Signal scheduler that async work completed
                        if let Some(waker) = waker {
                            waker();
                        }
                    });

                    work_units += 1;
                    context.remaining_budget -= 1;
                }

                // Handle received messages from pending_received queue
                while context.remaining_budget > 0 && !self.pending_received.is_empty() {
                    if let Some(ip) = self.pending_received.front().cloned() {
                        match self.out.push(ip) {
                            Ok(()) => {
                                self.pending_received.pop_front();
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            Err(PushError::Full(_)) => break,
                        }
                    }
                }

                // Check if input is abandoned
                if self.inn.is_abandoned() {
                    info!("EOF on inport, shutting down");
                    self.state = ClientState::Finished;
                    return ProcessResult::Finished;
                }

                // Put the state back with updated pending_messages
                self.state = ClientState::Connected { pending_messages };

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    context.wake_at(
                        Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                    );
                    ProcessResult::NoWork
                }
            }

            ClientState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("TCPClient"),
            description: String::from("Sends and receives bytes, transformed into IPs, via a TCP connection."),
            icon: String::from("plug"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL with optional configuration parameters in query part - currently none defined"),
                    values_allowed: vec![],
                    value_default: String::from("tcp://example.com:8080")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("incoming TCP bytes, transformed to IPs"),
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
                    description: String::from("IPs to be sent as TCP bytes"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

#[derive(Debug)]
enum ConnectionState {
    Active { last_active: Instant },
}

#[derive(Debug)]
struct Connection {
    state: ConnectionState,
    sock: TcpStream,
    #[allow(dead_code)]
    mio_token: Token,
}

pub struct TCPServerComponent {
    conf: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,

    // Cooperative server state
    listen_addr: Option<String>,
    listener: Option<TcpListener>,
    connections: HashMap<u32, Connection>,
    next_client_id: u32,
    pending_responses: HashMap<u32, FbpMessage>, // client_id -> response data
    pending_outbound: std::collections::VecDeque<FbpMessage>,
}

impl Component for TCPServerComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        TCPServerComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            resp: inports
                .remove("RESP")
                .expect("found no RESP inport")
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
            listen_addr: None,
            listener: None,
            connections: HashMap::new(),
            next_client_id: 0,
            pending_responses: HashMap::new(),
            pending_outbound: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("TCPServer is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration if not yet configured
        if self.listener.is_none() {
            if let Ok(listen_addr_msg) = self.conf.pop() {
                let listen_addr = listen_addr_msg.as_text()
                    .or_else(|| listen_addr_msg.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                    .expect("invalid utf-8 listen address")
                    .to_owned();
                self.listen_addr = Some(listen_addr.clone());
                trace!("got listen address: {}", listen_addr);

                // Set up the listener
                match TcpListener::bind(&listen_addr) {
                    Ok(listener) => {
                        listener
                            .set_nonblocking(true)
                            .expect("failed to set non-blocking on TCP listener");
                        self.listener = Some(listener);
                        debug!("TCP server listening on {}", listen_addr);
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(e) => {
                        warn!("failed to bind TCP listener on {}: {}", listen_addr, e);
                        return ProcessResult::NoWork;
                    }
                }
            } else {
                trace!("waiting for listen address configuration");
                return ProcessResult::NoWork;
            }
        }

        // Check signals
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            // stop signal
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        // Drain buffered outbound packets before reading new client data.
        while context.remaining_budget > 0 && !self.pending_outbound.is_empty() {
            if let Some(data) = self.pending_outbound.front() {
                match self.out.push(data.clone()) {
                    Ok(()) => {
                        let _ = self.pending_outbound.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(PushError::Full(_)) => break,
                }
            }
        }

        // Accept new connections (within budget)
        if context.remaining_budget > 0 {
            if let Some(listener) = &self.listener {
                match listener.accept() {
                    Ok((sock, addr)) => {
                        debug!("accepted new TCP connection from {}", addr);

                        // Set socket to non-blocking mode
                        sock.set_nonblocking(true)
                            .expect("failed to set non-blocking on client socket");

                        let client_id = self.next_client_id;
                        self.next_client_id += 1;

                        let connection = Connection {
                            state: ConnectionState::Active {
                                last_active: Instant::now(),
                            },
                            sock,
                            mio_token: Token(client_id as usize),
                        };

                        self.connections.insert(client_id, connection);
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("new TCP connection {} in active state", client_id);
                    }
                    Err(e) => {
                        if e.kind() != ErrorKind::WouldBlock {
                            warn!("failed to accept TCP connection: {}", e);
                        }
                        // WouldBlock is normal, no work done
                    }
                }
            }
        }

        // Process existing connections (within remaining budget)
        let mut connections_to_remove = Vec::new();

        for (client_id, connection) in self.connections.iter_mut() {
            if context.remaining_budget == 0 {
                break; // No more budget
            }

            // All connections are active, try to read data from this connection
            let mut buf = [0; READ_BUFFER];
            match connection.sock.read(&mut buf) {
                Ok(bytes_read) => {
                    if bytes_read > 0 {
                        debug!("got {} bytes from TCP client {}", bytes_read, client_id);
                        let data = Vec::from(&buf[0..bytes_read]);

                        // Update last active time
                        let ConnectionState::Active {
                            ref mut last_active,
                        } = connection.state;
                        *last_active = Instant::now();

                        // Frame data with client ID for multi-client support
                        let framed_data_str = format!("{}:{}", client_id, String::from_utf8_lossy(&data));
                        let framed_msg = FbpMessage::from_text(framed_data_str);
                        match self.out.push(framed_msg) {
                            Ok(()) => {
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("sent received data from client {} to output", client_id);
                            }
                            Err(PushError::Full(returned_msg)) => {
                                debug!("output buffer full, buffering received data from client {}", client_id);
                                self.pending_outbound.push_back(returned_msg);
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                        }
                    } else {
                        // Connection closed by client
                        debug!("TCP connection {} closed by client", client_id);
                        connections_to_remove.push(*client_id);
                    }
                }
                Err(e) => {
                    match e.kind() {
                        ErrorKind::WouldBlock => {
                            // No data available, continue to next connection
                        }
                        _ => {
                            warn!("failed to read from TCP client {}: {}", client_id, e);
                            connections_to_remove.push(*client_id);
                        }
                    }
                }
            }
        }

        // Send responses within remaining budget
        if context.remaining_budget > 0 {
            if let Ok(response_data) = self.resp.pop() {
                debug!("got response packet, parsing client ID and routing...");

                // Parse client ID from framed response data: "CLIENT_ID:response_data"
                let response_bytes = response_data.as_bytes().unwrap_or(&[]);
                let response_str = String::from_utf8_lossy(response_bytes);
                if let Some(colon_pos) = response_str.find(':') {
                    let client_id_str = &response_str[..colon_pos];
                    if let Ok(target_client_id) = client_id_str.parse::<u32>() {
                        let actual_response = &response_bytes[colon_pos + 1..];

                        // Find and send to the specific client
                        if let Some(connection) = self.connections.get_mut(&target_client_id) {
                            match connection.sock.write_all(actual_response) {
                                Ok(()) => {
                                    debug!("sent response to TCP client {}", target_client_id);
                                    work_units += 1;
                                    context.remaining_budget -= 1;

                                    // Update last active time
                                    let ConnectionState::Active {
                                        ref mut last_active,
                                    } = connection.state;
                                    *last_active = Instant::now();
                                }
                                Err(e) => {
                                    match e.kind() {
                                        ErrorKind::WouldBlock => {
                                            // Would block, buffer for later retry
                                            let response_msg = FbpMessage::from_bytes(actual_response.to_vec());
                                            self.pending_responses
                                                .insert(target_client_id, response_msg);
                                            work_units += 1;
                                            context.remaining_budget -= 1;
                                            debug!(
                                                "buffered response for client {} (would block)",
                                                target_client_id
                                            );
                                        }
                                        ErrorKind::BrokenPipe
                                        | ErrorKind::ConnectionReset
                                        | ErrorKind::NotConnected => {
                                            warn!(
                                                "connection to client {} broken, removing",
                                                target_client_id
                                            );
                                            connections_to_remove.push(target_client_id);
                                        }
                                        _ => {
                                            warn!(
                                                "failed to write to TCP client {}: {}",
                                                target_client_id, e
                                            );
                                            connections_to_remove.push(target_client_id);
                                        }
                                    }
                                }
                            }
                        } else {
                            debug!(
                                "target client {} not connected, dropping response packet",
                                target_client_id
                            );
                        }
                    } else {
                        warn!(
                            "invalid client ID '{}' in response, dropping packet",
                            client_id_str
                        );
                    }
                } else {
                    warn!("response packet missing client ID framing (expected 'CLIENT_ID:data'), dropping packet");
                }
            }
        }

        // Send any buffered responses (within remaining budget)
        let mut responses_to_remove = Vec::new();
        for (client_id, response_data) in &self.pending_responses {
            if context.remaining_budget == 0 {
                break;
            }

            if let Some(connection) = self.connections.get_mut(client_id) {
                let ConnectionState::Active { .. } = &connection.state;
                let response_bytes = response_data.as_bytes().unwrap_or(&[]);
                match connection.sock.write_all(response_bytes) {
                    Ok(()) => {
                        debug!("sent buffered response to TCP client {}", client_id);
                        work_units += 1;
                        context.remaining_budget -= 1;
                        responses_to_remove.push(*client_id);

                        // Update last active time
                        let ConnectionState::Active {
                            ref mut last_active,
                        } = connection.state;
                        *last_active = Instant::now();
                    }
                    Err(e) => {
                        match e.kind() {
                            ErrorKind::WouldBlock => {
                                // Still would block, leave in buffer
                            }
                            _ => {
                                warn!(
                                    "failed to write buffered response to TCP client {}: {}",
                                    client_id, e
                                );
                                connections_to_remove.push(*client_id);
                                responses_to_remove.push(*client_id);
                            }
                        }
                    }
                }
            } else {
                // Connection no longer exists, remove buffered response
                responses_to_remove.push(*client_id);
            }
        }

        // Clean up sent responses
        for client_id in responses_to_remove {
            self.pending_responses.remove(&client_id);
        }

        // Clean up closed connections
        for client_id in connections_to_remove {
            self.connections.remove(&client_id);
            self.pending_responses.remove(&client_id);
            work_units += 1; // Count cleanup as work
            debug!("cleaned up TCP connection {}", client_id);
        }

        // Check for abandoned input ports
        if self.resp.is_abandoned()
            && self.pending_responses.is_empty()
            && self.pending_outbound.is_empty()
        {
            info!("RESP input abandoned and no pending responses, finishing");
            return ProcessResult::Finished;
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            if self.listener.is_some() {
                context.wake_at(
                    std::time::Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                );
            }
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("TCPServer"),
            description: String::from("TCP server"),
            icon: String::from("server"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from(
                        "configuration value, currently the IP and port to listen on",
                    ),
                    values_allowed: vec![],
                    value_default: String::from("localhost:1234"),
                },
                ComponentPort {
                    name: String::from("RESP"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from(
                        "framed response data in format 'CLIENT_ID:response_data' to route to specific client",
                    ),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
            ],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("framed data from client connections in format 'CLIENT_ID:data'"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}
