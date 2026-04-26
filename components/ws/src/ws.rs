use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

// component-specific
use std::net::{TcpListener, TcpStream};
use std::time::Duration;
use tungstenite::protocol::Message;

#[derive(Debug)]
enum WSClientState {
    WaitingForConfig,
    Connecting { url: String, connect_start: std::time::Instant },
    Connected { client: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>> },
    Finished,
}

pub struct WSClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: WSClientState,
    //graph_inout: GraphInportOutportHandle,
}

const READ_TIMEOUT: Duration = Duration::from_millis(500);
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(7);



impl Component for WSClientComponent {
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
            signals_in: signals_in,
            signals_out: signals_out,
            state: WSClientState::WaitingForConfig,
            //graph_inout: graph_inout,
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

        match &mut self.state {
            WSClientState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url_str);
                    self.state = WSClientState::Connecting {
                        url: url_str.to_owned(),
                        connect_start: std::time::Instant::now(),
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

            WSClientState::Connecting { url, connect_start } => {
                // Try to connect (this is blocking, but we limit attempts)
                match tungstenite::client::connect(url.as_str()) {
                    Ok((mut client, _)) => {
                        debug!("WebSocket connection established");
                        if let Err(err) = set_client_stream_timeouts(client.get_mut()) {
                            warn!("failed to set client socket timeouts: {}", err);
                        }
                        self.state = WSClientState::Connected { client };
                        return ProcessResult::DidWork(1);
                    }
                    Err(tungstenite::Error::Io(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        // Connection in progress, check timeout
                        if connect_start.elapsed() > CONNECT_TIMEOUT {
                            error!("timed out connecting to WebSocket server after {:?}", CONNECT_TIMEOUT);
                            self.state = WSClientState::Finished;
                            return ProcessResult::Finished;
                        }
                        // Yield if no budget left
                        if context.remaining_budget == 0 {
                            return ProcessResult::NoWork;
                        }
                        context.remaining_budget -= 1;
                        return ProcessResult::NoWork;
                    }
                    Err(err) => {
                        error!("failed to connect to WebSocket server: {}", err);
                        self.state = WSClientState::Finished;
                        return ProcessResult::Finished;
                    }
                }
            }

            WSClientState::Connected { ref mut client } => {
                let mut work_units = 0;

                // Send data
                while !self.inn.is_empty() && context.remaining_budget > 0 {
                    debug!("got {} packets, sending into WebSocket...", self.inn.slots());
                    let chunk = self.inn
                        .read_chunk(self.inn.slots())
                        .expect("receive as chunk failed");

                    for ip in chunk.into_iter() {
                        //TODO add support for Text messages
                        if let Err(err) = client.write(Message::Binary(ip)) {
                            error!("failed to write into WebSocket client: {}", err);
                            self.state = WSClientState::Finished;
                            return ProcessResult::Finished;
                        }
                    }

                    if let Err(err) = client.flush() {
                        error!("failed to flush WebSocket client: {}", err);
                        self.state = WSClientState::Finished;
                        return ProcessResult::Finished;
                    }

                    work_units += 1;
                    context.remaining_budget -= 1;
                }

                // Receive data
                while client.can_read() && context.remaining_budget > 0 {
                    debug!("got message from WebSocket, repeating...");
                    match client.read() {
                        Ok(msg) => {
                            self.out.push(msg.into_data()).expect("could not push into OUT");
                            debug!("done");
                            work_units += 1;
                            context.remaining_budget -= 1;
                        }
                        Err(tungstenite::Error::Io(err))
                            if err.kind() == std::io::ErrorKind::WouldBlock
                                || err.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            break; // No more data available
                        }
                        Err(tungstenite::Error::ConnectionClosed)
                        | Err(tungstenite::Error::AlreadyClosed) => {
                            info!("WebSocket connection closed");
                            self.state = WSClientState::Finished;
                            return ProcessResult::Finished;
                        }
                        Err(err) => {
                            error!("failed to read from WebSocket: {}", err);
                            self.state = WSClientState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                }

                // Check if input is abandoned
                if self.inn.is_abandoned() {
                    info!("EOF on inport, shutting down");
                    self.state = WSClientState::Finished;
                    return ProcessResult::Finished;
                }

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
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
    //graph_inout: GraphInportOutportHandle,
}

impl Component for WSServerComponent {
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

            WSServerState::Listening { listener, connections, next_connection_id } => {
                let mut work_units = 0;

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
                                    error!("failed to accept/upgrade to WebSocket connection: {}", e);
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
                            self.out.push(message.into_data()).expect("could not push IP into OUT");
                            work_units += 1;
                            context.remaining_budget -= 1;
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
