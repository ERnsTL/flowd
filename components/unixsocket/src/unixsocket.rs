use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

// component-specific
use std::io::{ErrorKind, Read, Write};
use std::time::Duration;
use uds::UnixSocketAddr;

pub struct UnixSocketClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,

    // Runtime state for cooperative unix socket client
    socket_address: Option<UnixSocketAddr>,
    socket_type: Option<SocketType>,
    client_seqpacket: Option<uds::UnixSeqpacketConn>,
    read_timeout: Duration,
    read_buffer: [u8; DEFAULT_READ_BUFFER_SIZE],
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
const DEFAULT_WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));


impl Component for UnixSocketClientComponent {
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
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
            socket_address: None,
            socket_type: None,
            client_seqpacket: None,
            read_timeout: DEFAULT_READ_TIMEOUT,
            read_buffer: [0u8; DEFAULT_READ_BUFFER_SIZE],
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("UnixSocketClient is now process()ing!");
        let mut work_units = 0u32;

        // Initialize connection if not already done
        if self.socket_address.is_none() {
            // Try to read configuration
            if let Ok(url_vec) = self.conf.pop() {
                // Parse configuration URL
                let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");
                let url = url::Url::parse(&url_str).expect("failed to parse URL");

                // Determine if address is abstract or path-based
                let address_is_abstract = url.has_host();
                let address_str = if address_is_abstract {
                    url.host_str().expect("failed to get abstract socket address from URL host")
                } else {
                    let path = url.path();
                    if path.is_empty() || path == "/" {
                        error!("no socket address given in config URL path");
                        return ProcessResult::NoWork;
                    }
                    path
                };

                // Parse socket type from URL
                let socket_type = if let Some((_key, value)) = url.query_pairs().find(|(key, _)| key == "socket_type") {
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
                    UnixSocketAddr::from_abstract(address_str).expect("failed to parse abstract socket address")
                } else {
                    UnixSocketAddr::from_path(address_str).expect("failed to parse path socket address")
                };

                // Store configuration
                self.socket_address = Some(socket_address);
                self.socket_type = Some(socket_type);

                // Parse read timeout
                if let Some((_key, value)) = url.query_pairs().find(|(key, _)| key == "rtimeout") {
                    if let Ok(timeout_ms) = value.to_string().parse::<u64>() {
                        self.read_timeout = Duration::from_millis(timeout_ms);
                    }
                }

                debug!("configured unix socket client for address: {}", address_str);
                work_units += 1;
            } else {
                trace!("waiting for configuration");
                return ProcessResult::NoWork;
            }
        }

        // Connect if not already connected
        if self.client_seqpacket.is_none() && self.socket_address.is_some() {
            if let (Some(socket_address), Some(socket_type)) = (&self.socket_address, &self.socket_type) {
                match socket_type {
                    SocketType::SeqPacket => {
                        match uds::UnixSeqpacketConn::connect_unix_addr(socket_address) {
                            Ok(conn) => {
                                if let Err(e) = conn.set_read_timeout(Some(self.read_timeout)) {
                                    error!("failed to set read timeout: {}", e);
                                    return ProcessResult::NoWork;
                                }
                                self.client_seqpacket = Some(conn);
                                debug!("connected to unix socket server");
                                work_units += 1;
                            }
                            Err(e) => {
                                error!("failed to connect to unix socket: {}", e);
                                return ProcessResult::NoWork;
                            }
                        }
                    }
                    SocketType::Stream => {
                        error!("stream sockets not yet implemented");
                        return ProcessResult::NoWork;
                    }
                    SocketType::Datagram => {
                        error!("datagram sockets not yet implemented");
                        return ProcessResult::NoWork;
                    }
                }
            }
        }

        // Check signals
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
            // stop signal
            if ip == b"stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
            }
        }

        // Send data within budget
        while context.remaining_budget > 0 {
            if let Ok(chunk) = self.inn.read_chunk(self.inn.slots()) {
                if chunk.is_empty() {
                    break; // No more data
                }

                debug!("got {} packets, sending into socket...", chunk.len());

                for ip in chunk.into_iter() {
                    if let Some(conn) = &mut self.client_seqpacket {
                        match conn.send(ip.as_ref()) {
                            Ok(_) => {
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("sent data to unix socket server");
                            }
                            Err(err) => {
                                error!("failed to send to unix socket: {:?}", err);
                                // Connection error, clear connection to force reconnection
                                self.client_seqpacket = None;
                                break;
                            }
                        }
                    } else {
                        // No connection, can't send
                        break;
                    }
                }
            } else {
                break; // No more data
            }
        }

        // Receive data within remaining budget
        while context.remaining_budget > 0 {
            if let Some(conn) = &mut self.client_seqpacket {
                match conn.recv(&mut self.read_buffer) {
                    Ok(bytes_in) => {
                        if bytes_in > 0 {
                            debug!("got packet with {} bytes from unix socket server", bytes_in);
                            let data = Vec::from(&self.read_buffer[0..bytes_in]);
                            match self.out.push(data) {
                                Ok(()) => {
                                    work_units += 1;
                                    context.remaining_budget -= 1;
                                    debug!("sent received data to output");
                                }
                                Err(_) => {
                                    // Output buffer full, drop data
                                    debug!("output buffer full, dropping received data");
                                    work_units += 1; // Count as work even if dropped
                                    context.remaining_budget -= 1;
                                }
                            }
                        } else {
                            // Connection closed by server
                            debug!("unix socket connection closed by server");
                            self.client_seqpacket = None;
                            break;
                        }
                    }
                    Err(err) => {
                        if err.kind() == ErrorKind::WouldBlock {
                            // No data available
                            break;
                        } else {
                            error!("failed to read from unix socket: {:?}", err);
                            self.client_seqpacket = None;
                            break;
                        }
                    }
                }
            } else {
                // No connection, can't receive
                break;
            }
        }

        // Check if input is abandoned (EOF)
        if self.inn.is_abandoned() {
            info!("EOF on inport IN, finishing");
            return ProcessResult::Finished;
        }

        // Return appropriate result based on work done
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
}

impl Component for UnixSocketServerComponent {
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
            trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
            // stop signal
            if ip == b"stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
            }
        }

        // Accept new connections within budget
        if context.remaining_budget > 0 {
            if let Some(listener) = &self.listener {
                match listener.accept() {
                    Ok((socket, addr)) => {
                        debug!("accepted new unix socket connection from {:?}", addr);

                        // Set timeouts on the socket
                        if let Err(e) = socket.set_read_timeout(Some(DEFAULT_READ_TIMEOUT)) {
                            error!("failed to set read timeout on client socket: {}", e);
                            return ProcessResult::NoWork;
                        }
                        if let Err(e) = socket.set_write_timeout(DEFAULT_WRITE_TIMEOUT) {
                            error!("failed to set write timeout on client socket: {}", e);
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
                        debug!("got {} bytes from unix socket client {}", bytes_in, client_id);
                        let data = Vec::from(&self.read_buffer[0..bytes_in]);
                        // Frame data with client ID for multi-client support
                        let framed_data = format!("{}:{}", client_id, String::from_utf8_lossy(&data)).into_bytes();
                        match self.out.push(framed_data) {
                            Ok(()) => {
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("sent received data from client {} to output", client_id);
                            }
                            Err(_) => {
                                // Output buffer full, drop data
                                debug!("output buffer full, dropping received data from client {}", client_id);
                                work_units += 1; // Count as work even if dropped
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
                        error!("failed to read from unix socket client {}: {}", client_id, err);
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
                                    debug!("sent response to unix socket client {}", target_client_id);
                                }
                                Err(err) => {
                                    if err.kind() == ErrorKind::WouldBlock || err.kind() == ErrorKind::TimedOut {
                                        warn!("timed out writing response to client {}, dropping packet", target_client_id);
                                    } else {
                                        warn!("failed to write response to client {}: {}, dropping client", target_client_id, err);
                                        connections_to_remove.push(target_client_id);
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

        // Clean up closed connections
        for client_id in connections_to_remove {
            self.connections.remove(&client_id);
            work_units += 1; // Count cleanup as work
            debug!("cleaned up unix socket connection {}", client_id);
        }

        // Return appropriate result based on work done
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
