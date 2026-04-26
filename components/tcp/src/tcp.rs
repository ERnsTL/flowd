#![feature(addr_parse_ascii)] // for TCPClientComponent -> SocketAddr::parse_ascii()
use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

// component-specific
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug)]
enum ClientCommand {
    Connect(SocketAddr),
    SendData(Vec<u8>),
    Disconnect,
}

#[derive(Debug)]
enum ClientResult {
    Connected,
    ConnectionFailed(std::io::Error),
    DataSent,
    DataReceived(Vec<u8>),
    ConnectionClosed,
    Error(std::io::Error),
}

#[derive(Debug, Clone)]
enum ClientState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

pub struct TCPClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,

    // Worker thread communication
    command_tx: Sender<ClientCommand>,
    result_rx: Receiver<ClientResult>,
    worker_handle: Option<JoinHandle<()>>,

    // Runtime state
    state: ClientState,
    socket_addr: Option<SocketAddr>,
    pending_data: std::collections::VecDeque<Vec<u8>>, // data to send, buffered for backpressure
}

const CONNECT_TIMEOUT: Duration = Duration::from_millis(10000);
const READ_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const READ_BUFFER: usize = 4096;

impl TCPClientComponent {
    fn worker_thread(command_rx: Receiver<ClientCommand>, result_tx: Sender<ClientResult>) {
        let mut stream: Option<TcpStream> = None;

        loop {
            // Try to receive a command
            match command_rx.try_recv() {
                Ok(command) => {
                    match command {
                        ClientCommand::Connect(addr) => {
                            debug!("Worker: attempting to connect to {}", addr);
                            match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                                Ok(s) => {
                                    if let Err(e) = s.set_read_timeout(READ_TIMEOUT) {
                                        let _ = result_tx.send(ClientResult::Error(e));
                                        continue;
                                    }
                                    if let Err(e) = s.set_write_timeout(WRITE_TIMEOUT) {
                                        let _ = result_tx.send(ClientResult::Error(e));
                                        continue;
                                    }
                                    stream = Some(s);
                                    let _ = result_tx.send(ClientResult::Connected);
                                    debug!("Worker: connected to {}", addr);
                                }
                                Err(e) => {
                                    debug!("Worker: failed to connect to {}: {}", addr, e);
                                    let _ = result_tx.send(ClientResult::ConnectionFailed(e));
                                }
                            }
                        }
                        ClientCommand::SendData(data) => {
                            if let Some(ref mut s) = stream {
                                match s.write_all(&data) {
                                    Ok(()) => {
                                        let _ = s.flush();
                                        let _ = result_tx.send(ClientResult::DataSent);
                                        debug!("Worker: sent {} bytes", data.len());
                                    }
                                    Err(e) => {
                                        debug!("Worker: failed to send data: {}", e);
                                        let _ = result_tx.send(ClientResult::Error(e));
                                    }
                                }
                            } else {
                                let _ = result_tx.send(ClientResult::Error(std::io::Error::new(
                                    std::io::ErrorKind::NotConnected,
                                    "No active connection"
                                )));
                            }
                        }
                        ClientCommand::Disconnect => {
                            stream = None;
                            let _ = result_tx.send(ClientResult::ConnectionClosed);
                            debug!("Worker: disconnected");
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // No command, try to read data if connected
                    if let Some(ref mut s) = stream {
                        let mut buf = [0; READ_BUFFER];
                        match s.read(&mut buf) {
                            Ok(bytes_read) => {
                                if bytes_read > 0 {
                                    let data = Vec::from(&buf[0..bytes_read]);
                                    let _ = result_tx.send(ClientResult::DataReceived(data));
                                    debug!("Worker: received {} bytes", bytes_read);
                                } else {
                                    // Connection closed by peer
                                    stream = None;
                                    let _ = result_tx.send(ClientResult::ConnectionClosed);
                                    debug!("Worker: connection closed by peer");
                                }
                            }
                            Err(e) => {
                                match e.kind() {
                                    ErrorKind::WouldBlock => {
                                        // No data available, continue
                                    }
                                    _ => {
                                        // Read error
                                        debug!("Worker: read error: {}", e);
                                        stream = None;
                                        let _ = result_tx.send(ClientResult::Error(e));
                                    }
                                }
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Main thread disconnected, exit
                    debug!("Worker: command channel disconnected, exiting");
                    break;
                }
            }

            // Small sleep to prevent busy looping
            thread::sleep(Duration::from_millis(10));
        }

        debug!("Worker thread exiting");
    }
}

impl Drop for TCPClientComponent {
    fn drop(&mut self) {
        // Send disconnect command and wait for worker to finish
        let _ = self.command_tx.send(ClientCommand::Disconnect);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Component for TCPClientComponent {
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
        // Create communication channels for worker thread
        let (command_tx, command_rx) = mpsc::channel::<ClientCommand>();
        let (result_tx, result_rx) = mpsc::channel::<ClientResult>();

        // Spawn worker thread
        let worker_handle = Some(thread::spawn(move || {
            Self::worker_thread(command_rx, result_tx);
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
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
            command_tx,
            result_rx,
            worker_handle,
            state: ClientState::Disconnected,
            socket_addr: None,
            pending_data: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("TCPClient is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration if not yet configured
        if self.socket_addr.is_none() {
            if let Ok(url_vec) = self.conf.pop() {
                let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");
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

                self.socket_addr = Some(addr);
                trace!("got config IP, parsed address: {}", addr);

                // Initiate connection via worker thread
                if let Err(e) = self.command_tx.send(ClientCommand::Connect(addr)) {
                    warn!("failed to send connect command to worker: {}", e);
                    self.state = ClientState::Error("Failed to communicate with worker".to_string());
                } else {
                    self.state = ClientState::Connecting;
                    work_units += 1;
                    context.remaining_budget -= 1;
                    debug!("initiated connection to {}", addr);
                }
            } else {
                trace!("no config IP available yet");
                return ProcessResult::NoWork;
            }
        }

        // Check for results from worker thread
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                ClientResult::Connected => {
                    self.state = ClientState::Connected;
                    work_units += 1;
                    debug!("connection established");
                }
                ClientResult::ConnectionFailed(e) => {
                    self.state = ClientState::Error(format!("Connection failed: {}", e));
                    work_units += 1;
                    warn!("connection failed: {}", e);
                }
                ClientResult::DataReceived(data) => {
                    match self.out.push(data) {
                        Ok(()) => {
                            work_units += 1;
                            debug!("forwarded received data to output");
                        }
                        Err(PushError::Full(_)) => {
                            // Output buffer full, drop data
                            debug!("output buffer full, dropping received data");
                            work_units += 1;
                        }
                    }
                }
                ClientResult::DataSent => {
                    // Data was sent successfully
                    work_units += 1;
                    debug!("data sent confirmation received");
                }
                ClientResult::ConnectionClosed => {
                    self.state = ClientState::Disconnected;
                    work_units += 1;
                    debug!("connection closed by peer");
                }
                ClientResult::Error(e) => {
                    self.state = ClientState::Error(format!("Worker error: {}", e));
                    work_units += 1;
                    warn!("worker error: {}", e);
                }
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
                let _ = self.command_tx.send(ClientCommand::Disconnect);
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

        // Send pending data within remaining budget
        while context.remaining_budget > 0 && !self.pending_data.is_empty() {
            if let ClientState::Connected = self.state {
                if let Some(data) = self.pending_data.front().cloned() {
                    if let Err(e) = self.command_tx.send(ClientCommand::SendData(data)) {
                        warn!("failed to send data command to worker: {}", e);
                        break;
                    }
                    self.pending_data.pop_front();
                    work_units += 1;
                    context.remaining_budget -= 1;
                    debug!("sent pending data to worker thread");
                }
            } else {
                // Not connected, can't send
                break;
            }
        }

        // Send new data within remaining budget
        while context.remaining_budget > 0 {
            if let Ok(chunk) = self.inn.read_chunk(self.inn.slots()) {
                if chunk.is_empty() {
                    break; // No more data
                }

                debug!("got {} packets to send", chunk.len());

                for ip in chunk.into_iter() {
                    if let ClientState::Connected = self.state {
                        let data_to_send = ip.clone();
                        if let Err(e) = self.command_tx.send(ClientCommand::SendData(ip)) {
                            warn!("failed to send data command to worker: {}", e);
                            self.pending_data.push_back(data_to_send); // Buffer for retry
                            break;
                        }
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("sent data to worker thread");
                    } else {
                        // Not connected, buffer data
                        self.pending_data.push_back(ip);
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("buffered data (not connected)");
                    }
                }
            } else {
                break; // No more data
            }
        }

        // are we done?
        if self.inn.is_abandoned() && self.pending_data.is_empty() {
            info!("EOF on inport IN and no pending data, finishing");
            let _ = self.command_tx.send(ClientCommand::Disconnect);
            return ProcessResult::Finished;
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
    pending_responses: HashMap<u32, Vec<u8>>, // client_id -> response data
}

impl Component for TCPServerComponent {
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
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("TCPServer is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration if not yet configured
        if self.listener.is_none() {
            if let Ok(listen_addr_bytes) = self.conf.pop() {
                let listen_addr = std::str::from_utf8(&listen_addr_bytes)
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
                )
            }
        }

        // Accept new connections (within budget)
        if context.remaining_budget > 0 {
            if let Some(listener) = &self.listener {
                match listener.accept() {
                    Ok((sock, addr)) => {
                        debug!("accepted new TCP connection from {}", addr);

                        // Set up timeouts
                        let sock = sock;
                        sock.set_read_timeout(READ_TIMEOUT)
                            .expect("failed to set read timeout on client socket");
                        sock.set_write_timeout(WRITE_TIMEOUT)
                            .expect("failed to set write timeout on client socket");

                        let client_id = self.next_client_id;
                        self.next_client_id += 1;

                        let connection = Connection {
                            state: ConnectionState::Active {
                                last_active: Instant::now(),
                            },
                            sock,
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
                        let ConnectionState::Active { ref mut last_active } = connection.state;
                        *last_active = Instant::now();

                        // Frame data with client ID for multi-client support
                        let framed_data = format!("{}:{}", client_id, String::from_utf8_lossy(&data)).into_bytes();
                        match self.out.push(framed_data) {
                            Ok(()) => {
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("sent received data from client {} to output", client_id);
                            }
                            Err(PushError::Full(_)) => {
                                // Output buffer full, drop the data for now
                                debug!("output buffer full, dropping received data from client {}", client_id);
                                work_units += 1; // Count as work even if dropped
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
                let response_str = String::from_utf8_lossy(&response_data);
                if let Some(colon_pos) = response_str.find(':') {
                    let client_id_str = &response_str[..colon_pos];
                    if let Ok(target_client_id) = client_id_str.parse::<u32>() {
                        let actual_response = &response_data[colon_pos + 1..];

                        // Find and send to the specific client
                        if let Some(connection) = self.connections.get_mut(&target_client_id) {
                            match connection.sock.write_all(actual_response) {
                                Ok(()) => {
                                    debug!("sent response to TCP client {}", target_client_id);
                                    work_units += 1;
                                    context.remaining_budget -= 1;

                                    // Update last active time
                                    let ConnectionState::Active { ref mut last_active } = connection.state;
                                    *last_active = Instant::now();
                                }
                                Err(e) => {
                                    match e.kind() {
                                        ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                                            // Would block, buffer for later retry
                                            self.pending_responses.insert(target_client_id, actual_response.to_vec());
                                            work_units += 1;
                                            context.remaining_budget -= 1;
                                            debug!("buffered response for client {} (would block)", target_client_id);
                                        }
                                        ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::NotConnected => {
                                            warn!("connection to client {} broken, removing", target_client_id);
                                            connections_to_remove.push(target_client_id);
                                        }
                                        _ => {
                                            warn!("failed to write to TCP client {}: {}", target_client_id, e);
                                            connections_to_remove.push(target_client_id);
                                        }
                                    }
                                }
                            }
                        } else {
                            debug!("target client {} not connected, dropping response packet", target_client_id);
                        }
                    } else {
                        warn!("invalid client ID '{}' in response, dropping packet", client_id_str);
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
                match connection.sock.write_all(response_data) {
                    Ok(()) => {
                        debug!("sent buffered response to TCP client {}", client_id);
                        work_units += 1;
                        context.remaining_budget -= 1;
                        responses_to_remove.push(*client_id);

                        // Update last active time
                        let ConnectionState::Active { ref mut last_active } = connection.state;
                        *last_active = Instant::now();
                    }
                    Err(e) => {
                        match e.kind() {
                            ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                                // Still would block, leave in buffer
                            }
                            _ => {
                                warn!("failed to write buffered response to TCP client {}: {}", client_id, e);
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
        if self.resp.is_abandoned() && self.pending_responses.is_empty() {
            info!("RESP input abandoned and no pending responses, finishing");
            return ProcessResult::Finished;
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
