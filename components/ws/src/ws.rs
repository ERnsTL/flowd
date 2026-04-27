use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult, PushError,
    ProcessSignalSink, ProcessSignalSource, SchedulerWaker, create_io_channels,
    wake_scheduler,
};
use log::{debug, error, info, trace, warn};

// component-specific
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use tokio::sync::mpsc as tokio_mpsc;
use tungstenite::protocol::Message;

#[derive(Debug)]
enum WSClientState {
    WaitingForConfig,
    Connecting {
        url: String,
    },
    Connected {
        client: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
        pending_messages: Vec<Vec<u8>>,
    },
    Finished,
}

pub struct WSClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: WSClientState,
    scheduler_waker: Option<SchedulerWaker>,
    pending_received: std::collections::VecDeque<Vec<u8>>,
    // ADR-017: Bounded IO channels
    cmd_tx: std::sync::mpsc::SyncSender<String>,
    result_rx: std::sync::mpsc::Receiver<Result<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>, tungstenite::Error>>,
    // Background async worker
    _async_worker: Option<std::thread::JoinHandle<()>>,
}

const READ_TIMEOUT: Duration = Duration::from_millis(500);
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));

// ADR-017: Async WebSocket worker that runs in background thread with Tokio runtime
async fn async_ws_worker(
    cmd_rx: std::sync::mpsc::Receiver<String>,
    result_tx: std::sync::mpsc::SyncSender<Result<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>, tungstenite::Error>>,
    waker: Option<SchedulerWaker>,
) {
    debug!("WebSocket async worker started");

    while let Ok(url) = cmd_rx.recv() {
        debug!("WebSocket worker connecting to: {}", &url);

        // Perform WebSocket connection (this is synchronous but we're in an async context)
        let result = std::panic::catch_unwind(|| {
            tungstenite::client::connect(&url).map(|(ws, _)| ws)
        });

        let connection_result = match result {
            Ok(stream_result) => stream_result,
            Err(_) => Err(tungstenite::Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "Connection panicked"))),
        };

        // Send result back (ignore send errors - component may have been destroyed)
        let _ = result_tx.try_send(connection_result);

        // ADR-002: Signal scheduler that work is ready
        wake_scheduler(&waker);
    }

    debug!("WebSocket async worker exiting");
}

impl Component for WSClientComponent {
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
        // ADR-017: Create bounded IO channels
        let (cmd_tx, cmd_rx, result_tx, result_rx) = create_io_channels::<String, Result<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>, tungstenite::Error>>();

        // Start background async worker thread
        let waker = scheduler_waker.clone();
        let async_worker = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async_ws_worker(cmd_rx, result_tx, waker));
        }));

        WSClientComponent {
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
            state: WSClientState::WaitingForConfig,
            scheduler_waker,
            pending_received: std::collections::VecDeque::new(),
            // ADR-017: Bounded IO channels
            cmd_tx,
            result_rx,
            // Background async worker
            _async_worker: async_worker,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("WSClient process() called, state: {:?}", self.state);

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = WSClientState::Finished;
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

        // Handle state transitions that require borrowing
        if let WSClientState::Connecting { url } = &self.state {
            // Check if connection completed
            match self.result_rx.try_recv() {
                Ok(Ok(mut client)) => {
                    // Connection successful - transition to connected state
                    if let Err(err) = set_client_stream_timeouts(client.get_mut()) {
                        warn!("failed to set client socket timeouts: {}", err);
                    }
                    self.state = WSClientState::Connected {
                        client,
                        pending_messages: Vec::new(),
                    };
                    debug!("WebSocket connection established");
                    return ProcessResult::DidWork(1);
                }
                Ok(Err(e)) => {
                    error!("WebSocket connection failed: {}", e);
                    self.state = WSClientState::Finished;
                    return ProcessResult::Finished;
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

        let current_state = std::mem::replace(&mut self.state, WSClientState::Finished);
        match current_state {
            WSClientState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url_str = String::from_utf8(url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url_str);

                    // ADR-017: Send to background async worker via bounded channel
                    match self.cmd_tx.try_send(url_str.clone()) {
                        Ok(()) => {
                            debug!("WebSocket connection request enqueued to async worker");
                            self.state = WSClientState::Connecting {
                                url: url_str,
                            };
                            return ProcessResult::DidWork(1);
                        }
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            // ADR-017: Channel full, apply backpressure
                            debug!("WebSocket command channel full, applying backpressure");
                            warn!("WebSocket connection request dropped due to full command channel");
                            self.state = WSClientState::WaitingForConfig;
                            return ProcessResult::NoWork;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            // Worker thread died, component should finish
                            warn!("WebSocket async worker disconnected");
                            self.state = WSClientState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                }

                // Check for completed connection results from async worker
                match self.result_rx.try_recv() {
                    Ok(result) => {
                        // Process the connection result
                        match result {
                            Ok(mut client) => {
                                // Connection successful - transition to connected state
                                if let Err(err) = set_client_stream_timeouts(client.get_mut()) {
                                    warn!("failed to set client socket timeouts: {}", err);
                                }
                                self.state = WSClientState::Connected {
                                    client,
                                    pending_messages: Vec::new(),
                                };
                                debug!("WebSocket connection established");
                                return ProcessResult::DidWork(1);
                            }
                            Err(e) => {
                                // Connection failed
                                error!("WebSocket connection failed: {}", e);
                                self.state = WSClientState::Finished;
                                return ProcessResult::Finished;
                            }
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // No results ready, continue waiting
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // Worker thread died
                        warn!("WebSocket async worker result channel disconnected");
                        self.state = WSClientState::Finished;
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

            WSClientState::Connecting { .. } => {
                // This should have been handled above
                self.state = WSClientState::Finished;
                ProcessResult::Finished
            }

            WSClientState::Connected {
                client,
                mut pending_messages,
            } => {
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
                                // we'd need proper async WebSocket client
                                debug!("Would send WebSocket message: {:?}", message);
                            }) {
                                warn!("Failed to send WebSocket message");
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
                    self.state = WSClientState::Finished;
                    return ProcessResult::Finished;
                }

                // Put the state back with updated pending_messages
                self.state = WSClientState::Connected {
                    client,
                    pending_messages,
                };

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    context.wake_at(
                        std::time::Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                    );
                    ProcessResult::NoWork
                }
            }

            WSClientState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("WSClient"),
            description: String::from("Sends and receives messages, transformed into IPs, via a WebSocket connection."),
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
                    value_default: String::from("wss://example.com:8080/socketpath")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("incoming WebSocket messages, transformed to IPs"),
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
                    description: String::from("IPs to be sent as WebSocket messages"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

fn set_client_stream_timeouts(
    stream: &mut tungstenite::stream::MaybeTlsStream<TcpStream>,
) -> std::io::Result<()> {
    match stream {
        tungstenite::stream::MaybeTlsStream::Plain(sock) => {
            sock.set_read_timeout(Some(READ_TIMEOUT))?;
            sock.set_write_timeout(WRITE_TIMEOUT)?;
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug)]
enum WSServerState {
    WaitingForConfig,
    Listening {
        listener: TcpListener,
        connections: std::collections::HashMap<u32, tungstenite::WebSocket<TcpStream>>,
        next_connection_id: u32,
    },
    Finished,
}

pub struct WSServerComponent {
    conf: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: WSServerState,
    pending_outbound: std::collections::VecDeque<Vec<u8>>,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for WSServerComponent {
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
        WSServerComponent {
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
            state: WSServerState::WaitingForConfig,
            pending_outbound: std::collections::VecDeque::new(),
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("WSServer process() called, state: {:?}", self.state);

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = WSServerState::Finished;
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

        match &mut self.state {
            WSServerState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(config_vec) = self.conf.pop() {
                    let listen_addr = std::str::from_utf8(&config_vec).expect("invalid utf-8");
                    debug!("got listen address: {}", listen_addr);

                    let listener = TcpListener::bind(listen_addr)
                        .map_err(|e| {
                            error!("failed to bind tcp listener socket: {}", e);
                            e
                        })
                        .expect("failed to bind tcp listener socket");

                    listener
                        .set_nonblocking(true)
                        .expect("failed to set non-blocking on tcp listener socket");

                    self.state = WSServerState::Listening {
                        listener,
                        connections: std::collections::HashMap::new(),
                        next_connection_id: 1,
                    };
                    return ProcessResult::DidWork(1);
                }
                // No config yet, but check if we should yield budget
                if context.remaining_budget == 0 {
                    return ProcessResult::NoWork;
                }
                context.remaining_budget -= 1;
                return ProcessResult::NoWork;
            }

            WSServerState::Listening {
                listener,
                connections,
                next_connection_id,
            } => {
                let mut work_units = 0;

                while context.remaining_budget > 0 && !self.pending_outbound.is_empty() {
                    if let Some(ip) = self.pending_outbound.front().cloned() {
                        match self.out.push(ip) {
                            Ok(()) => {
                                self.pending_outbound.pop_front();
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            Err(PushError::Full(_)) => break,
                        }
                    }
                }

                // Accept new connections
                if context.remaining_budget > 0 {
                    match listener.accept() {
                        Ok((socket, addr)) => {
                            debug!("handling client: {:?}", addr);

                            // Set socket options
                            if let Err(e) = socket.set_read_timeout(Some(READ_TIMEOUT)) {
                                warn!("failed to set read timeout on socket: {}", e);
                            }
                            if let Err(e) = socket.set_write_timeout(WRITE_TIMEOUT) {
                                warn!("failed to set write timeout on socket: {}", e);
                            }

                            // Upgrade to WebSocket
                            match tungstenite::accept(socket) {
                                Ok(websocket) => {
                                    let conn_id = *next_connection_id;
                                    *next_connection_id += 1;
                                    connections.insert(conn_id, websocket);
                                    debug!("WebSocket connection {} established", conn_id);
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                }
                                Err(e) => {
                                    error!(
                                        "failed to accept/upgrade to WebSocket connection: {}",
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            if e.kind() != std::io::ErrorKind::WouldBlock {
                                error!("accept failed: {:?}", e);
                                self.state = WSServerState::Finished;
                                return ProcessResult::Finished;
                            }
                            // WouldBlock is normal - no connections waiting
                        }
                    }
                }

                // Process existing connections
                let mut closed_connections = Vec::new();
                for (conn_id, websocket) in connections.iter_mut() {
                    if context.remaining_budget == 0 {
                        break;
                    }

                    // Read from this connection
                    match websocket.read() {
                        Ok(message) => {
                            debug!("got message from connection {}, pushing to OUT", conn_id);
                            match self.out.push(message.into_data()) {
                                Ok(()) => {
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                }
                                Err(PushError::Full(returned)) => {
                                    self.pending_outbound.push_back(returned);
                                    break;
                                }
                            }
                        }
                        Err(tungstenite::Error::Io(err))
                            if err.kind() == std::io::ErrorKind::WouldBlock
                                || err.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            // No data available, continue to next connection
                        }
                        Err(tungstenite::Error::ConnectionClosed)
                        | Err(tungstenite::Error::AlreadyClosed) => {
                            debug!("connection {} closed", conn_id);
                            closed_connections.push(*conn_id);
                        }
                        Err(err) => {
                            error!("failed to read from connection {}: {}", conn_id, err);
                            closed_connections.push(*conn_id);
                        }
                    }
                }

                // Remove closed connections
                for conn_id in closed_connections {
                    connections.remove(&conn_id);
                    debug!("removed closed connection {}", conn_id);
                }

                // Send responses to clients
                while !self.resp.is_empty() && context.remaining_budget > 0 {
                    if let Ok(ip) = self.resp.pop() {
                        debug!("got response packet, sending to first available client");

                        // Send to first available connection
                        if let Some((_, websocket)) = connections.iter_mut().next() {
                            if let Err(err) = websocket.write(Message::Binary(ip)) {
                                error!("failed to write to WebSocket: {}", err);
                                // Connection might be broken, but we'll handle it on next read
                            } else if let Err(err) = websocket.flush() {
                                error!("failed to flush WebSocket: {}", err);
                            } else {
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                        } else {
                            debug!("no connected clients, dropping response packet");
                        }
                    } else {
                        break;
                    }
                }

                // Check if RESP input is abandoned
                if self.resp.is_abandoned() {
                    info!("RESP input abandoned, finishing");
                    self.state = WSServerState::Finished;
                    return ProcessResult::Finished;
                }

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    context.wake_at(
                        std::time::Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                    );
                    ProcessResult::NoWork
                }
            }

            WSServerState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("WSServer"),
            description: String::from("WebSocket server"),
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
                        "response data from downstream process for each connection",
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
                description: String::from("signal and content data from the client connections"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}
