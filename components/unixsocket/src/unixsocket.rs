use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError, SchedulerWaker, create_io_channels,
    wake_scheduler,
};
use log::{debug, error, info, trace, warn};

// component-specific
use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use uds::UnixSocketAddr;

#[derive(Debug)]
enum UnixSocketClientState {
    WaitingForConfig,
    Connecting,
    Connected {
        client: uds::UnixSeqpacketConn,
        pending_messages: Vec<Vec<u8>>,
    },
    Finished,
}

pub struct UnixSocketClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: UnixSocketClientState,
    scheduler_waker: Option<SchedulerWaker>,
    read_timeout: Duration,
    pending_outbound: std::collections::VecDeque<Vec<u8>>,
    // ADR-017: Bounded IO channels
    cmd_tx: std::sync::mpsc::SyncSender<(UnixSocketAddr, SocketType)>,
    result_rx: std::sync::mpsc::Receiver<Result<uds::UnixSeqpacketConn, std::io::Error>>,
    // Background async worker
    _async_worker: Option<std::thread::JoinHandle<()>>,
}

/*
Situation 2024-04:
* socket types:  https://www.man7.org/linux/man-pages/man7/unix.7.html
* the best type is seqpacket
* https://internals.rust-lang.org/t/pre-rfc-adding-sock-seqpacket-unix-sockets-to-std/7323
* https://github.com/rust-lang-nursery/unix-socket/pull/25
* https://github.com/rust-lang/rust/pull/50348
* they want to have it on crates.io first and - if popular enough - put it into std, pfff
* that crate is:  https://crates.io/crates/uds
* problem is that for example credential-passing and ancillary data has been added to std, but not do uds crate, but seqpacket is in uds but not in std
* best place to add SeqPacket socket support would be in std::os::linux::net::UnixSocketExt
* wtf
* if flowd ever moves from threading to tokio, then there is already https://crates.io/crates/tokio-seqpacket
*/
//TODO optimize - u8?
enum SocketType {
    Datagram,
    Stream,
    SeqPacket, // NOTE: what is seqpacket, it is the way to go unless really streaming is employed and many many messages are received since 1 read syscall will yield only 1 seqpacket=message.
               // http://www.ccplusplus.com/2011/08/understanding-sockseqpacket-socket-type.html
}

const DEFAULT_READ_BUFFER_SIZE: usize = 65536;
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(500);

// ADR-017: Async UnixSocket worker that runs in background thread with Tokio runtime
async fn async_unixsocket_worker(
    cmd_rx: std::sync::mpsc::Receiver<(UnixSocketAddr, SocketType)>,
    result_tx: std::sync::mpsc::SyncSender<Result<uds::UnixSeqpacketConn, std::io::Error>>,
    waker: Option<SchedulerWaker>,
) {
    debug!("UnixSocket async worker started");

    while let Ok((socket_addr, socket_type)) = cmd_rx.recv() {
        debug!("UnixSocket worker connecting to: {:?}", socket_addr);

        // Perform Unix socket connection (this is synchronous but we're in an async context)
        let result = std::panic::catch_unwind(|| {
            match socket_type {
                SocketType::SeqPacket => {
                    uds::UnixSeqpacketConn::connect_unix_addr(&socket_addr)
                }
                SocketType::Stream => {
                    error!("stream sockets not yet implemented");
                    Err(std::io::Error::new(std::io::ErrorKind::Other, "Not implemented"))
                }
                SocketType::Datagram => {
                    error!("datagram sockets not yet implemented");
                    Err(std::io::Error::new(std::io::ErrorKind::Other, "Not implemented"))
                }
            }
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

    debug!("UnixSocket async worker exiting");
}

impl Component for UnixSocketClientComponent {
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
        let (cmd_tx, cmd_rx, result_tx, result_rx) = create_io_channels::<(UnixSocketAddr, SocketType), Result<uds::UnixSeqpacketConn, std::io::Error>>();

        // Start background async worker thread
        let waker = scheduler_waker.clone();
        let async_worker = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async_unixsocket_worker(cmd_rx, result_tx, waker));
        }));

        UnixSocketClientComponent {
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
            state: UnixSocketClientState::WaitingForConfig,
            scheduler_waker,
            read_timeout: DEFAULT_READ_TIMEOUT,
            pending_outbound: std::collections::VecDeque::new(),
            // ADR-017: Bounded IO channels
            cmd_tx,
            result_rx,
            // Background async worker
            _async_worker: async_worker,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("UnixSocketClient is now process()ing!");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = UnixSocketClientState::Finished;
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
        if let UnixSocketClientState::Connecting = &self.state {
            // Check if connection completed
            match self.result_rx.try_recv() {
                Ok(Ok(client)) => {
                    // Connection successful - transition to connected state
                    if let Err(err) = client.set_read_timeout(Some(self.read_timeout)) {
                        warn!("failed to set read timeout: {}", err);
                    }
                    self.state = UnixSocketClientState::Connected {
                        client,
                        pending_messages: Vec::new(),
                    };
                    debug!("Unix socket connection established");
                    return ProcessResult::DidWork(1);
                }
                Ok(Err(e)) => {
                    error!("Unix socket connection failed: {}", e);
                    self.state = UnixSocketClientState::Finished;
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

        let current_state = std::mem::replace(&mut self.state, UnixSocketClientState::Finished);
        match current_state {
            UnixSocketClientState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url_str = String::from_utf8(url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url_str);

                    // Parse configuration URL
                    let url = url::Url::parse(&url_str).expect("failed to parse URL");

                    // Determine if address is abstract or path-based
                    let address_is_abstract = url.has_host();
                    let address_str = if address_is_abstract {
                        url.host_str()
                            .expect("failed to get abstract socket address from URL host")
                    } else {
                        let path = url.path();
                        if path.is_empty() || path == "/" {
                            error!("no socket address given in config URL path");
                            return ProcessResult::NoWork;
                        }
                        path
                    };

                    // Parse socket type from URL
                    let socket_type = if let Some((_key, value)) =
                        url.query_pairs().find(|(key, _)| key == "socket_type")
                    {
                        match value.to_string().as_str() {
                            "seqpacket" => SocketType::SeqPacket,
                            "stream" => SocketType::Stream,
                            "dgram" | "datagram" => SocketType::Datagram,
                            _ => {
                                error!("invalid socket type in config URL");
                                return ProcessResult::NoWork;
                            }
                        }
                    } else {
                        error!("missing socket_type in config URL");
                        return ProcessResult::NoWork;
                    };

                    // Create socket address
                    let socket_address = if address_is_abstract {
                        UnixSocketAddr::from_abstract(address_str)
                            .expect("failed to parse abstract socket address")
                    } else {
                        UnixSocketAddr::from_path(address_str)
                            .expect("failed to parse path socket address")
                    };

                    // Parse read timeout
                    let mut read_timeout = DEFAULT_READ_TIMEOUT;
                    if let Some((_key, value)) = url.query_pairs().find(|(key, _)| key == "rtimeout") {
                        if let Ok(timeout_ms) = value.to_string().parse::<u64>() {
                            read_timeout = Duration::from_millis(timeout_ms);
                        }
                    }
                    self.read_timeout = read_timeout;

                    // ADR-017: Send to background async worker via bounded channel
                    match self.cmd_tx.try_send((socket_address, socket_type)) {
                        Ok(()) => {
                            debug!("UnixSocket connection request enqueued to async worker");
                            self.state = UnixSocketClientState::Connecting;
                            return ProcessResult::DidWork(1);
                        }
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            // ADR-017: Channel full, apply backpressure
                            debug!("UnixSocket command channel full, applying backpressure");
                            warn!("UnixSocket connection request dropped due to full command channel");
                            self.state = UnixSocketClientState::WaitingForConfig;
                            return ProcessResult::NoWork;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            // Worker thread died, component should finish
                            warn!("UnixSocket async worker disconnected");
                            self.state = UnixSocketClientState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                }
                // No config yet, but check if we should yield budget
                if context.remaining_budget == 0 {
                    return ProcessResult::NoWork;
                }
                context.remaining_budget -= 1;
                return ProcessResult::NoWork;
            }

            UnixSocketClientState::Connecting { .. } => {
                // This should have been handled above
                self.state = UnixSocketClientState::Finished;
                ProcessResult::Finished
            }

            UnixSocketClientState::Connected {
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
                                // we'd need proper async Unix socket operations
                                debug!("Would send Unix socket message: {:?}", message);
                            }) {
                                warn!("Failed to send Unix socket message");
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

                // Handle received messages from pending_outbound queue
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

                // Check if input is abandoned
                if self.inn.is_abandoned() && self.pending_outbound.is_empty() {
                    info!("EOF on inport, shutting down");
                    self.state = UnixSocketClientState::Finished;
                    return ProcessResult::Finished;
                }

                // Put the state back with updated pending_messages
                self.state = UnixSocketClientState::Connected {
                    client,
                    pending_messages,
                };

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    context.wake_at(
                        Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                    );
                    ProcessResult::NoWork
                }
            }

            UnixSocketClientState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("UnixSocketClient"),
            description: String::from("Drops all packets received on IN port."), //###
            icon: String::from("trash-o"),                                       //###
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be dropped"), //###
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be dropped"),
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
                description: String::from("data to be dropped"), //###
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}

pub struct UnixSocketServerComponent {
    conf: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,

    // Runtime state for cooperative unix socket server
    listen_path: Option<String>,
    listener: Option<std::os::unix::net::UnixListener>,
    connections: std::collections::HashMap<u32, std::os::unix::net::UnixStream>,
    next_client_id: u32,
    read_buffer: [u8; DEFAULT_READ_BUFFER_SIZE],
    pending_outbound: std::collections::VecDeque<Vec<u8>>,
    pending_responses: std::collections::HashMap<u32, Vec<u8>>,
}

impl Component for UnixSocketServerComponent {
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
        UnixSocketServerComponent {
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
            listen_path: None,
            listener: None,
            connections: std::collections::HashMap::new(),
            next_client_id: 0,
            read_buffer: [0u8; DEFAULT_READ_BUFFER_SIZE],
            pending_outbound: std::collections::VecDeque::new(),
            pending_responses: std::collections::HashMap::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("UnixSocketServer is now process()ing!");
        let mut work_units = 0u32;

        // Initialize listener if not already done
        if self.listener.is_none() {
            // Try to read configuration
            if let Ok(config_bytes) = self.conf.pop() {
                let listen_path = std::str::from_utf8(&config_bytes)
                    .expect("could not parse listen path as utf8")
                    .to_owned();

                // Remove existing socket file if it exists
                std::fs::remove_file(&listen_path).ok();

                // Create listener
                match std::os::unix::net::UnixListener::bind(std::path::Path::new(&listen_path)) {
                    Ok(listener) => {
                        if let Err(e) = listener.set_nonblocking(true) {
                            error!("failed to set listener to non-blocking: {}", e);
                            return ProcessResult::NoWork;
                        }
                        self.listener = Some(listener);
                        self.listen_path = Some(listen_path.clone());
                        debug!("unix socket server listening on {}", listen_path);
                        work_units += 1;
                    }
                    Err(e) => {
                        error!("failed to bind unix listener socket: {}", e);
                        return ProcessResult::NoWork;
                    }
                }
            } else {
                trace!("waiting for configuration");
                return ProcessResult::NoWork;
            }
        }

        // Check signals
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            // stop signal
            if ip == b"stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!(
                    "received unknown signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                );
            }
        }

        // Accept new connections within budget
        while context.remaining_budget > 0 && !self.pending_outbound.is_empty() {
            if let Some(data) = self.pending_outbound.front() {
                match self.out.push(data.clone()) {
                    Ok(()) => {
                        let _ = self.pending_outbound.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(_) => break,
                }
            }
        }

        if context.remaining_budget > 0 {
            if let Some(listener) = &self.listener {
                match listener.accept() {
                    Ok((socket, addr)) => {
                        debug!("accepted new unix socket connection from {:?}", addr);

                        // Set socket to non-blocking mode
                        if let Err(e) = socket.set_nonblocking(true) {
                            error!("failed to set non-blocking on client socket: {}", e);
                            return ProcessResult::NoWork;
                        }

                        let client_id = self.next_client_id;
                        self.next_client_id += 1;
                        self.connections.insert(client_id, socket);
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("new unix socket connection {} established", client_id);
                    }
                    Err(e) => {
                        if e.kind() != ErrorKind::WouldBlock {
                            error!("failed to accept unix socket connection: {}", e);
                        }
                        // WouldBlock is normal, no work done
                    }
                }
            }
        }

        // Process existing connections within remaining budget
        let mut connections_to_remove = Vec::new();

        for (client_id, socket) in self.connections.iter_mut() {
            if context.remaining_budget == 0 {
                break; // No more budget
            }

            // Read data from client
            match socket.read(&mut self.read_buffer) {
                Ok(bytes_in) => {
                    if bytes_in > 0 {
                        debug!(
                            "got {} bytes from unix socket client {}",
                            bytes_in, client_id
                        );
                        let data = Vec::from(&self.read_buffer[0..bytes_in]);
                        // Frame data with client ID for multi-client support
                        let framed_data =
                            format!("{}:{}", client_id, String::from_utf8_lossy(&data))
                                .into_bytes();
                        match self.out.push(framed_data) {
                            Ok(()) => {
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("sent received data from client {} to output", client_id);
                            }
                            Err(flowd_component_api::PushError::Full(returned)) => {
                                debug!(
                                    "output buffer full, buffering received data from client {}",
                                    client_id
                                );
                                self.pending_outbound.push_back(returned);
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                        }
                    } else {
                        // Connection closed by client
                        debug!("unix socket connection closed by client {}", client_id);
                        connections_to_remove.push(*client_id);
                    }
                }
                Err(err) => {
                    if err.kind() == ErrorKind::WouldBlock {
                        // No data available, continue to next connection
                        continue;
                    } else {
                        error!(
                            "failed to read from unix socket client {}: {}",
                            client_id, err
                        );
                        connections_to_remove.push(*client_id);
                    }
                }
            }
        }

        // Send responses within remaining budget
        if context.remaining_budget > 0 {
            if let Ok(response_data) = self.resp.pop() {
                debug!("got response packet, parsing client ID and routing...");

                // Parse client ID from framed response data: "CLIENT_ID:response_data"
                let response_str = String::from_utf8_lossy(&response_data);
                if let Some(colon_pos) = response_str.find(':') {
                    let client_id_str = &response_str[..colon_pos];
                    if let Ok(target_client_id) = client_id_str.parse::<u32>() {
                        let actual_response = &response_data[colon_pos + 1..];

                        // Find and send to the specific client
                        if let Some(socket) = self.connections.get_mut(&target_client_id) {
                            match socket.write_all(actual_response) {
                                Ok(_) => {
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                    debug!(
                                        "sent response to unix socket client {}",
                                        target_client_id
                                    );
                                }
                                Err(err) => {
                                    if err.kind() == ErrorKind::WouldBlock {
                                        self.pending_responses
                                            .insert(target_client_id, actual_response.to_vec());
                                    } else {
                                        warn!("failed to write response to client {}: {}, dropping client", target_client_id, err);
                                        connections_to_remove.push(target_client_id);
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

        let mut responses_to_remove = Vec::new();
        for (client_id, response_data) in &self.pending_responses {
            if context.remaining_budget == 0 {
                break;
            }
            if let Some(socket) = self.connections.get_mut(client_id) {
                match socket.write_all(response_data) {
                    Ok(_) => {
                        responses_to_remove.push(*client_id);
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(err) => {
                        if err.kind() != ErrorKind::WouldBlock {
                            responses_to_remove.push(*client_id);
                            connections_to_remove.push(*client_id);
                        }
                    }
                }
            } else {
                responses_to_remove.push(*client_id);
            }
        }
        for client_id in responses_to_remove {
            self.pending_responses.remove(&client_id);
        }

        // Clean up closed connections
        for client_id in connections_to_remove {
            self.connections.remove(&client_id);
            self.pending_responses.remove(&client_id);
            work_units += 1; // Count cleanup as work
            debug!("cleaned up unix socket connection {}", client_id);
        }

        // Return appropriate result based on work done
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
            name: String::from("UnixSocketServer"),
            description: String::from("Unix socket server"),
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
                        "configuration value, currently the path to listen on",
                    ),
                    values_allowed: vec![],
                    value_default: String::from("/tmp/server.sock"),
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
