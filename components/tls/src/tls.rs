use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

// component-specific
use std::sync::Arc;
//use std::net::SocketAddr;
use rustls::RootCertStore;
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct TLSClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    conn: Option<rustls::ClientConnection>,
    sock: Option<TcpStream>,
    pending_data: std::collections::VecDeque<Vec<u8>>, // data to send, buffered for backpressure
    pending_received: std::collections::VecDeque<Vec<u8>>,
    connect_result: Option<mpsc::Receiver<Result<(rustls::ClientConnection, TcpStream), String>>>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(7);
const READ_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const READ_BUFFER: usize = 65536; // is allocated once and re-used for each read() call

// Connection state machine for cooperative TLS server
#[derive(Debug)]
#[allow(dead_code)]
enum ConnectionState {
    Handshaking {
        conn: rustls::ServerConnection,
        sock: TcpStream,
    },
    Active {
        conn: rustls::ServerConnection,
        sock: TcpStream,
        client_id: u32,
    },
    #[allow(dead_code)]
    Writing {
        conn: rustls::ServerConnection,
        sock: TcpStream,
        client_id: u32,
        data: Vec<u8>,
    },
    #[allow(dead_code)]
    Closed,
}

#[derive(Debug)]
struct Connection {
    state: ConnectionState,
    last_active: Instant,
}

impl Component for TLSClientComponent {
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
        TLSClientComponent {
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
            conn: None,
            sock: None,
            pending_data: std::collections::VecDeque::new(),
            pending_received: std::collections::VecDeque::new(),
            connect_result: None,
            scheduler_waker,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("TLSClient is now process()ing!");
        let mut work_units = 0u32;

        while context.remaining_budget > 0 && !self.pending_received.is_empty() {
            if let Some(data) = self.pending_received.front().cloned() {
                match self.out.push(data) {
                    Ok(()) => {
                        self.pending_received.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(PushError::Full(_)) => break,
                }
            }
        }

        // Read configuration and establish connection asynchronously.
        if self.conn.is_none() && self.connect_result.is_none() {
            if let Ok(url_vec) = self.conf.pop() {
                let url_str = std::str::from_utf8(&url_vec)
                    .expect("invalid utf-8")
                    .to_owned();
                let (tx, rx) = mpsc::channel();
                let scheduler_waker = self.scheduler_waker.clone();
                self.connect_result = Some(rx);
                std::thread::spawn(move || {
                    let result = (|| -> Result<(rustls::ClientConnection, TcpStream), String> {
                        let url = url::Url::parse(&url_str).map_err(|e| e.to_string())?;
                        let host = url
                            .host_str()
                            .ok_or_else(|| "failed to parse host from URL".to_string())?
                            .to_owned();
                        let port = url
                            .port()
                            .ok_or_else(|| "failed to parse port from URL".to_string())?;
                        let addr_str = format!("{}:{}", host, port);
                        let socket_addr = addr_str
                            .to_socket_addrs()
                            .map_err(|e| e.to_string())?
                            .next()
                            .ok_or_else(|| "TLS server host resolved to no address".to_string())?;
                        let sock = TcpStream::connect_timeout(&socket_addr, CONNECT_TIMEOUT)
                            .map_err(|e| e.to_string())?;
                        sock.set_nonblocking(true).map_err(|e| e.to_string())?;

                        let root_store = RootCertStore {
                            roots: webpki_roots::TLS_SERVER_ROOTS.into(),
                        };
                        let mut config = rustls::ClientConfig::builder()
                            .with_root_certificates(root_store)
                            .with_no_client_auth();
                        config.key_log = Arc::new(rustls::KeyLogFile::new());
                        let server_name = host.try_into().map_err(|_| "invalid server name".to_string())?;
                        let conn = rustls::ClientConnection::new(Arc::new(config), server_name)
                            .map_err(|e| e.to_string())?;
                        Ok((conn, sock))
                    })();
                    let _ = tx.send(result);
                    if let Some(waker) = scheduler_waker {
                        waker();
                    }
                });
            } else {
                trace!("no config IP available yet");
                return ProcessResult::NoWork;
            }
        }

        if self.conn.is_none() {
            if let Some(rx) = &self.connect_result {
                match rx.try_recv() {
                    Ok(Ok((conn, sock))) => {
                        self.conn = Some(conn);
                        self.sock = Some(sock);
                        self.connect_result = None;
                        debug!("connected to TLS server");
                    }
                    Ok(Err(err)) => {
                        warn!("failed to connect TLS client: {}", err);
                        self.connect_result = None;
                        return ProcessResult::NoWork;
                    }
                    Err(mpsc::TryRecvError::Empty) => return ProcessResult::NoWork,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.connect_result = None;
                        return ProcessResult::NoWork;
                    }
                }
            } else {
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
            if let (Some(conn), Some(sock)) = (&mut self.conn, &mut self.sock) {
                if let Some(pending_data) = self.pending_data.front() {
                    // Write to TLS connection
                    match conn.writer().write(pending_data) {
                        Ok(_) => {
                            // Complete the TLS handshake/write
                            match conn.complete_io(sock) {
                                Ok(_) => {
                                    self.pending_data.pop_front();
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                    debug!("sent pending data to TLS server");
                                }
                                Err(e) => {
                                    if e.kind() == std::io::ErrorKind::WouldBlock {
                                        // Would block, try again later
                                        break;
                                    } else {
                                        warn!("failed to complete TLS IO: {}", e);
                                        // Connection error, clear connection to force reconnection
                                        self.conn = None;
                                        self.sock = None;
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("failed to write to TLS connection: {}", e);
                            self.conn = None;
                            self.sock = None;
                            break;
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
                    if let (Some(conn), Some(sock)) = (&mut self.conn, &mut self.sock) {
                        match conn.writer().write(&ip) {
                            Ok(_) => {
                                match conn.complete_io(sock) {
                                    Ok(_) => {
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                        debug!("sent data to TLS server");
                                    }
                                    Err(e) => {
                                        if e.kind() == std::io::ErrorKind::WouldBlock {
                                            // Buffer for later
                                            self.pending_data.push_back(ip);
                                            work_units += 1;
                                            context.remaining_budget -= 1;
                                            debug!("buffered data for later send");
                                        } else {
                                            warn!("failed to complete TLS IO: {}", e);
                                            self.conn = None;
                                            self.sock = None;
                                            self.pending_data.push_back(ip); // Keep for retry
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("failed to write to TLS connection: {}", e);
                                self.conn = None;
                                self.sock = None;
                                self.pending_data.push_back(ip); // Keep for retry
                                break;
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
            if let (Some(conn), Some(sock)) = (&mut self.conn, &mut self.sock) {
                // First, read any available data from the socket into the TLS connection
                match conn.read_tls(sock) {
                    Ok(0) => {
                        // Connection closed by server
                        debug!("TLS connection closed by server");
                        self.conn = None;
                        self.sock = None;
                        break;
                    }
                    Ok(_) => {
                        // Process any new packets
                        if let Err(e) = conn.process_new_packets() {
                            warn!("failed to process TLS packets: {}", e);
                            self.conn = None;
                            self.sock = None;
                            break;
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            // No data available
                            break;
                        } else {
                            warn!("failed to read TLS data: {}", e);
                            self.conn = None;
                            self.sock = None;
                            break;
                        }
                    }
                }

                // Now read decrypted data from the TLS connection
                let mut buf = [0; READ_BUFFER];
                match conn.reader().read(&mut buf) {
                    Ok(bytes_in) => {
                        if bytes_in > 0 {
                            debug!("got {} bytes from TLS server", bytes_in);
                            let data = Vec::from(&buf[0..bytes_in]);
                            match self.out.push(data) {
                                Ok(()) => {
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                    debug!("sent received data to output");
                                }
                                Err(PushError::Full(_returned_data)) => {
                                    debug!("output buffer full, buffering received data");
                                    self.pending_received.push_back(Vec::from(&buf[0..bytes_in]));
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                }
                            }
                        } else {
                            // No decrypted data available
                            break;
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            // No decrypted data available
                            break;
                        } else {
                            warn!("failed to read decrypted TLS data: {}", e);
                            self.conn = None;
                            self.sock = None;
                            break;
                        }
                    }
                }
            } else {
                // No connection, can't receive
                break;
            }
        }

        // are we done?
        if self.inn.is_abandoned()
            && self.pending_data.is_empty()
            && self.pending_received.is_empty()
        {
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
            name: String::from("TLSClient"),
            description: String::from("Sends and receives bytes, transformed into IPs, via a TLS+TCP connection."),
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
                    value_default: String::from("tls://example.com:8080")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("incoming decrypted bytes from TLS, transformed to IPs"),
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
                    description: String::from("IPs to be sent as encrypted bytes via TLS"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

pub struct TLSServerComponent {
    // Configuration inputs
    conf: ProcessEdgeSource,
    cert: ProcessEdgeSource,
    key: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,

    // Runtime state for cooperative TLS server
    listen_addr: Option<String>,
    cert_data: Option<Vec<u8>>,
    key_data: Option<Vec<u8>>,
    server_config: Option<rustls::ServerConfig>,
    listener: Option<std::net::TcpListener>,
    connections: std::collections::HashMap<u32, Connection>,
    next_client_id: u32,
    pending_outbound: std::collections::VecDeque<Vec<u8>>,
    pending_responses: std::collections::VecDeque<Vec<u8>>,
}

impl Component for TLSServerComponent {
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
        TLSServerComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            cert: inports
                .remove("CERT")
                .expect("found no CERT inport")
                .pop()
                .unwrap(),
            key: inports
                .remove("KEY")
                .expect("found no KEY inport")
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
            cert_data: None,
            key_data: None,
            server_config: None,
            listener: None,
            connections: std::collections::HashMap::new(),
            next_client_id: 0,
            pending_outbound: std::collections::VecDeque::new(),
            pending_responses: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("TLSServer is now process()ing!");
        let mut work_units = 0u32;

        while context.remaining_budget > 0 && !self.pending_outbound.is_empty() {
            if let Some(data) = self.pending_outbound.front().cloned() {
                match self.out.push(data) {
                    Ok(()) => {
                        self.pending_outbound.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(PushError::Full(_)) => break,
                }
            }
        }

        // Read configuration if not yet configured
        if self.server_config.is_none() {
            // Try to read configuration
            if let Ok(listen_addr_bytes) = self.conf.pop() {
                self.listen_addr = Some(
                    std::str::from_utf8(&listen_addr_bytes)
                        .expect("invalid utf-8 listen address")
                        .to_owned(),
                );
                trace!("got listen address: {}", self.listen_addr.as_ref().unwrap());
            }

            if let Ok(cert_bytes) = self.cert.pop() {
                self.cert_data = Some(cert_bytes);
                trace!("got certificate data");
            }

            if let Ok(key_bytes) = self.key.pop() {
                self.key_data = Some(key_bytes);
                trace!("got private key data");
            }

            // If we have all config, set up the server
            if let (Some(listen_addr), Some(cert_data), Some(key_data)) =
                (&self.listen_addr, &self.cert_data, &self.key_data)
            {
                // Parse certificates and key
                let certs = rustls_pemfile::certs(&mut BufReader::new(&cert_data[..]))
                    .collect::<Result<Vec<_>, _>>()
                    .expect("failed to parse certificate");
                let private_key = rustls_pemfile::private_key(&mut BufReader::new(&key_data[..]))
                    .unwrap()
                    .expect("failed to parse private key");

                // Create server config
                let server_config = rustls::ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(certs, private_key)
                    .expect("failed to build server config");

                self.server_config = Some(server_config);

                // Create listener
                let listener = std::net::TcpListener::bind(listen_addr)
                    .expect("failed to bind TLS listener socket");
                listener
                    .set_nonblocking(true)
                    .expect("failed to set non-blocking on TLS listener");
                self.listener = Some(listener);

                debug!("TLS server configured and listening on {}", listen_addr);
            } else {
                trace!("waiting for complete configuration");
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

        // Accept new connections (within budget)
        if context.remaining_budget > 0 {
            if let Some(listener) = &self.listener {
                match listener.accept() {
                    Ok((sock, addr)) => {
                        debug!("accepted new TLS connection from {}", addr);

                        // Set up timeouts
                        let sock = sock;
                        sock.set_read_timeout(READ_TIMEOUT)
                            .expect("failed to set read timeout on client socket");
                        sock.set_write_timeout(WRITE_TIMEOUT)
                            .expect("failed to set write timeout on client socket");

                        // Create TLS connection
                        let conn = rustls::ServerConnection::new(Arc::new(
                            self.server_config.as_ref().unwrap().clone(),
                        ))
                        .expect("failed to create TLS server connection");

                        let client_id = self.next_client_id;
                        self.next_client_id += 1;

                        let connection = Connection {
                            state: ConnectionState::Handshaking { conn, sock },
                            last_active: Instant::now(),
                        };

                        self.connections.insert(client_id, connection);
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("new TLS connection {} in handshaking state", client_id);
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            warn!("failed to accept TLS connection: {}", e);
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

            match &mut connection.state {
                ConnectionState::Handshaking { conn, sock } => {
                    // Try to complete TLS handshake
                    match conn.complete_io(sock) {
                        Ok(_) => {
                            // Handshake complete
                            let conn = std::mem::replace(
                                conn,
                                rustls::ServerConnection::new(Arc::new(
                                    self.server_config.as_ref().unwrap().clone(),
                                ))
                                .unwrap(),
                            );
                            let sock =
                                std::mem::replace(sock, TcpStream::connect("127.0.0.1:0").unwrap());

                            connection.state = ConnectionState::Active {
                                conn,
                                sock,
                                client_id: *client_id,
                            };
                            connection.last_active = Instant::now();
                            work_units += 1;
                            context.remaining_budget -= 1;
                            debug!("TLS handshake completed for client {}", client_id);
                        }
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::WouldBlock {
                                // Still handshaking, check timeout
                                if connection.last_active.elapsed() > Duration::from_secs(30) {
                                    warn!("TLS handshake timeout for client {}", client_id);
                                    connections_to_remove.push(*client_id);
                                }
                            } else {
                                warn!("TLS handshake failed for client {}: {}", client_id, e);
                                connections_to_remove.push(*client_id);
                            }
                        }
                    }
                }

                ConnectionState::Active {
                    conn,
                    sock,
                    client_id,
                } => {
                    // Try to read data from client
                    match conn.read_tls(sock) {
                        Ok(0) => {
                            // Connection closed by client
                            debug!("TLS connection closed by client {}", client_id);
                            connections_to_remove.push(*client_id);
                            continue;
                        }
                        Ok(_) => {
                            // Process any new packets
                            if let Err(e) = conn.process_new_packets() {
                                warn!(
                                    "failed to process TLS packets for client {}: {}",
                                    client_id, e
                                );
                                connections_to_remove.push(*client_id);
                                continue;
                            }
                        }
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::WouldBlock {
                                // No data available, continue to next connection
                                continue;
                            } else {
                                warn!("failed to read TLS data from client {}: {}", client_id, e);
                                connections_to_remove.push(*client_id);
                                continue;
                            }
                        }
                    }

                    // Read decrypted data
                    let mut buf = [0; READ_BUFFER];
                    match conn.reader().read(&mut buf) {
                        Ok(bytes_in) => {
                            if bytes_in > 0 {
                                debug!("got {} bytes from TLS client {}", bytes_in, client_id);
                                let data = Vec::from(&buf[0..bytes_in]);
                                match self.out.push(data) {
                                    Ok(()) => {
                                        connection.last_active = Instant::now();
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                        debug!("sent received data to output");
                                    }
                                    Err(PushError::Full(_returned_data)) => {
                                        debug!("output buffer full, buffering received data from client {}", client_id);
                                        self.pending_outbound.push_back(Vec::from(&buf[0..bytes_in]));
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            if e.kind() != std::io::ErrorKind::WouldBlock {
                                warn!(
                                    "failed to read decrypted TLS data from client {}: {}",
                                    client_id, e
                                );
                                connections_to_remove.push(*client_id);
                            }
                        }
                    }
                }

                ConnectionState::Writing {
                    conn: _,
                    sock: _,
                    client_id: _,
                    data: _,
                } => {
                    // TODO: Implement response writing
                    // For now, just mark as needing implementation
                    debug!(
                        "response writing not yet implemented for client {}",
                        client_id
                    );
                }

                ConnectionState::Closed => {
                    connections_to_remove.push(*client_id);
                }
            }
        }

        // Remove closed connections
        for client_id in connections_to_remove {
            self.connections.remove(&client_id);
            work_units += 1; // Count cleanup as work
            debug!("cleaned up TLS connection {}", client_id);
        }

        if let Ok(response_data) = self.resp.pop() {
            self.pending_responses.push_back(response_data);
        }

        // Check for responses to send (simplified - send to first active connection)
        if context.remaining_budget > 0 {
            if let Some(response_data) = self.pending_responses.front().cloned() {
                let mut sent = false;
                // Find first active connection to send response to
                for connection in self.connections.values_mut() {
                    if let ConnectionState::Active {
                        conn,
                        sock,
                        client_id,
                    } = &mut connection.state
                    {
                        match conn.writer().write(&response_data) {
                            Ok(_) => {
                                match conn.complete_io(sock) {
                                    Ok(_) => {
                                        connection.last_active = Instant::now();
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                        self.pending_responses.pop_front();
                                        debug!("sent response to TLS client {}", client_id);
                                        sent = true;
                                        break; // Sent to first connection
                                    }
                                    Err(e) => {
                                        if e.kind() != std::io::ErrorKind::WouldBlock {
                                            warn!(
                                                "failed to send response to TLS client {}: {}",
                                                client_id, e
                                            );
                                            // Could mark connection for removal
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "failed to write response to TLS client {}: {}",
                                    client_id, e
                                );
                            }
                        }
                        break; // Only try first connection for now
                    }
                }
                if !sent && self.connections.is_empty() {
                    // No active connections available yet; keep data queued.
                }
            }
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
            name: String::from("TLSServer"),
            description: String::from("TLS over TCP server"),
            icon: String::from("server"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("configuration value, currently the IP and port to listen on"),
                    values_allowed: vec![],
                    value_default: String::from("localhost:1234")
                },
                ComponentPort {
                    name: String::from("CERT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("the TLS certificate with the public key; can be multiple chained certificates; generate selfsigned with openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -sha256 -days 365 -nodes -subj '/CN=localhost'"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("KEY"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("private key fitting the public key in the TLS certificate"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("RESP"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("response data from downstream process for each connection"),
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
                    description: String::from("signal and content data from the client connections"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
