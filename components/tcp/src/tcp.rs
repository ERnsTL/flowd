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
use std::time::{Duration, Instant};

pub struct TCPClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    socket_addr: Option<SocketAddr>,
    stream: Option<TcpStream>,
    pending_data: std::collections::VecDeque<Vec<u8>>, // data to send, buffered for backpressure
}

const CONNECT_TIMEOUT: Duration = Duration::from_millis(10000);
const READ_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const READ_BUFFER: usize = 4096;

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
            socket_addr: None,
            stream: None,
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

                // Try to connect
                match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                    Ok(stream) => {
                        stream
                            .set_read_timeout(READ_TIMEOUT)
                            .expect("failed to set read timeout on TCP client");
                        stream
                            .set_write_timeout(WRITE_TIMEOUT)
                            .expect("failed to set write timeout on TCP client");
                        self.stream = Some(stream);
                        debug!("connected to {}", addr);
                    }
                    Err(e) => {
                        warn!("failed to connect to {}: {}", addr, e);
                        return ProcessResult::NoWork; // Can't proceed without connection
                    }
                }
            } else {
                trace!("no config IP available yet");
                return ProcessResult::NoWork;
            }
        }

        // check signals
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

        // Send pending data first
        while context.remaining_budget > 0 && !self.pending_data.is_empty() {
            if let Some(stream) = &mut self.stream {
                if let Some(pending_data) = self.pending_data.front() {
                    match stream.write(pending_data) {
                        Ok(_) => {
                            stream.flush().expect("failed to flush TCP stream");
                            self.pending_data.pop_front();
                            work_units += 1;
                            context.remaining_budget -= 1;
                            debug!("sent pending data to TCP server");
                        }
                        Err(e) => {
                            if e.kind() == ErrorKind::WouldBlock {
                                // Would block, try again later
                                break;
                            } else {
                                warn!("failed to write to TCP stream: {}", e);
                                // Connection error, clear stream to force reconnection
                                self.stream = None;
                                break;
                            }
                        }
                    }
                }
            } else {
                // No connection, can't send
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
                    if let Some(stream) = &mut self.stream {
                        match stream.write(&ip) {
                            Ok(_) => {
                                stream.flush().expect("failed to flush TCP stream");
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("sent data to TCP server");
                            }
                            Err(e) => {
                                if e.kind() == ErrorKind::WouldBlock {
                                    // Buffer for later
                                    self.pending_data.push_back(ip);
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                    debug!("buffered data for later send");
                                } else {
                                    warn!("failed to write to TCP stream: {}", e);
                                    // Connection error, clear stream to force reconnection
                                    self.stream = None;
                                    self.pending_data.push_back(ip); // Keep for retry
                                    break;
                                }
                            }
                        }
                    } else {
                        // No connection, buffer data
                        self.pending_data.push_back(ip);
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("buffered data (no connection)");
                    }
                }
            } else {
                break; // No more data
            }
        }

        // Receive data within remaining budget
        while context.remaining_budget > 0 {
            if let Some(stream) = &mut self.stream {
                let mut buf = [0; READ_BUFFER];
                match stream.read(&mut buf) {
                    Ok(bytes_in) => {
                        if bytes_in > 0 {
                            debug!("got {} bytes from TCP server", bytes_in);
                            let data = Vec::from(&buf[0..bytes_in]);
                            match self.out.push(data) {
                                Ok(()) => {
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                    debug!("sent received data to output");
                                }
                                Err(PushError::Full(_returned_data)) => {
                                    // Output buffer full - for TCP, we might want to buffer or drop
                                    // For now, we'll drop since TCP is streaming
                                    debug!("output buffer full, dropping received data");
                                    work_units += 1; // Count as work even if dropped
                                    context.remaining_budget -= 1;
                                }
                            }
                        } else {
                            // Connection closed by server
                            debug!("TCP connection closed by server");
                            self.stream = None;
                            break;
                        }
                    }
                    Err(e) => {
                        match e.kind() {
                            ErrorKind::WouldBlock => {
                                // No data available, normal
                                break;
                            }
                            _ => {
                                warn!("failed to read from TCP stream: {}", e);
                                self.stream = None;
                                break;
                            }
                        }
                    }
                }
            } else {
                // No connection, can't receive
                break;
            }
        }

        // are we done?
        if self.inn.is_abandoned() && self.pending_data.is_empty() {
            info!("EOF on inport IN and no pending data, finishing");
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

                        match self.out.push(data) {
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

        // Send responses to connections (within remaining budget)
        while context.remaining_budget > 0 {
            if let Ok(response_data) = self.resp.pop() {
                // For now, send to the first available connection
                // TODO: Implement proper client targeting based on framing/metadata
                if let Some((client_id, connection)) = self.connections.iter_mut().find(|(_, conn)| {
                    let ConnectionState::Active { .. } = &conn.state;
                    true
                }) {
                    match connection.sock.write_all(&response_data) {
                        Ok(()) => {
                            debug!("sent response to TCP client {}", client_id);
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
                                    self.pending_responses.insert(*client_id, response_data);
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                    debug!("buffered response for client {} (would block)", client_id);
                                }
                                ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::NotConnected => {
                                    warn!("connection to client {} broken, removing", client_id);
                                    connections_to_remove.push(*client_id);
                                }
                                _ => {
                                    warn!("failed to write to TCP client {}: {}", client_id, e);
                                    connections_to_remove.push(*client_id);
                                }
                            }
                        }
                    }
                } else {
                    debug!("no active connections, dropping response packet");
                    // Could buffer responses, but for now we'll drop them
                }
            } else {
                break; // No more responses
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
