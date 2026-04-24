use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

// component-specific
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// Hyper imports for the old run() method
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body as HyperBody, Request as HyperRequest, Response as HyperResponse, Server as HyperServer, StatusCode};
use tokio::sync::oneshot;

const SERVER_JOIN_GRACE: Duration = Duration::from_secs(5);
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

fn handoff_join(handle: thread::JoinHandle<()>, label: &'static str) {
    thread::Builder::new()
        .name(format!("http-join-{}", label))
        .spawn(move || {
            if let Err(err) = handle.join() {
                warn!("{}: deferred thread join returned error: {:?}", label, err);
            }
        })
        .expect("failed to spawn http deferred join thread");
}

// HTTP parsing and response structures for cooperative server
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
enum HttpParseState {
    RequestLine,
    Headers { headers: HashMap<String, String> },
    Body { content_length: Option<usize>, received: usize, buffer: Vec<u8> },
    ChunkedBody { chunks: Vec<Vec<u8>>, chunk_size: Option<usize>, buffer: Vec<u8> },
    Complete { request: HttpRequest },
}

#[derive(Debug)]
enum HttpResponseState {
    Pending,
    Writing { response: HttpResponse, sent: usize },
    Complete,
}

#[derive(Debug)]
enum ConnectionState {
    ReadingRequest { parse_state: HttpParseState, buffer: Vec<u8> },
    ProcessingRequest { request: HttpRequest, client_id: u32 },
    WritingResponse { response_state: HttpResponseState, client_id: u32 },
    KeepAlive,
    Closed,
}

// Helper function to find CRLF in buffer
fn find_crlf(buffer: &[u8]) -> Option<usize> {
    buffer.windows(2).position(|w| w == b"\r\n")
}

// Helper function to format HTTP response
fn format_response(response: &HttpResponse) -> Vec<u8> {
    let mut result = Vec::new();

    // Status line
    result.extend_from_slice(b"HTTP/1.1 ");
    result.extend_from_slice(response.status_code.to_string().as_bytes());
    result.push(b' ');
    result.extend_from_slice(response.status_text.as_bytes());
    result.extend_from_slice(b"\r\n");

    // Headers
    for (key, value) in &response.headers {
        result.extend_from_slice(key.as_bytes());
        result.extend_from_slice(b": ");
        result.extend_from_slice(value.as_bytes());
        result.extend_from_slice(b"\r\n");
    }

    // Content-Length header
    result.extend_from_slice(b"Content-Length: ");
    result.extend_from_slice(response.body.len().to_string().as_bytes());
    result.extend_from_slice(b"\r\n");

    // End of headers
    result.extend_from_slice(b"\r\n");

    // Body
    result.extend_from_slice(&response.body);

    result
}

#[derive(Debug)]
struct Connection {
    state: ConnectionState,
    last_active: Instant,
    sock: TcpStream,
}

pub struct HTTPClientComponent {
    req: ProcessEdgeSource,
    out_resp: ProcessEdgeSink,
    out_err: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    client: reqwest::blocking::Client,
    pending_responses: std::collections::VecDeque<Vec<u8>>, // responses to send, buffered for backpressure
    pending_errors: std::collections::VecDeque<Vec<u8>>,    // errors to send, buffered for backpressure
}

impl Component for HTTPClientComponent {
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
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .timeout(HTTP_REQUEST_TIMEOUT)
            .build()
            .expect("failed to build HTTP client");

        HTTPClientComponent {
            req: inports
                .remove("REQ")
                .expect("found no REQ inport")
                .pop()
                .unwrap(),
            out_resp: outports
                .remove("RESP")
                .expect("found no RESP outport")
                .pop()
                .unwrap(),
            out_err: outports
                .remove("ERR")
                .expect("found no ERR outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
            client,
            pending_responses: std::collections::VecDeque::new(),
            pending_errors: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("HTTPClient is now process()ing!");
        let mut work_units = 0u32;

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

        // First, try to send any pending responses that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_responses.is_empty() {
            if let Some(pending_response) = self.pending_responses.front() {
                match self.out_resp.push(pending_response.clone()) {
                    Ok(()) => {
                        self.pending_responses.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("sent pending HTTP response");
                    }
                    Err(PushError::Full(_)) => {
                        // Still can't send, stop trying for now
                        break;
                    }
                }
            }
        }

        // Then, try to send any pending errors that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_errors.is_empty() {
            if let Some(pending_error) = self.pending_errors.front() {
                match self.out_err.push(pending_error.clone()) {
                    Ok(()) => {
                        self.pending_errors.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("sent pending HTTP error");
                    }
                    Err(PushError::Full(_)) => {
                        // Still can't send, stop trying for now
                        break;
                    }
                }
            }
        }

        // Then, process new URLs within remaining budget
        while context.remaining_budget > 0 {
            // stay responsive to stop/ping even while making HTTP requests
            if let Ok(sig) = self.signals_in.try_recv() {
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&sig).expect("invalid utf-8")
                );
                if sig == b"stop" {
                    info!("got stop signal while processing, finishing");
                    return ProcessResult::Finished;
                } else if sig == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                }
            }

            if let Ok(ip) = self.req.pop() {
                let url = std::str::from_utf8(&ip).expect("non utf-8 data");
                debug!("got a request: {}", &url);

                // make HTTP request
                //TODO may be big file - add chunking
                //TODO enclose files in brackets to know where its stream of chunks start and end
                //TODO enable use of async and/or timeout
                debug!("making HTTP request...");
                match self.client.get(url).send() {
                    Ok(resp) => {
                        debug!("forwarding HTTP results...");
                        match resp.bytes() {
                            Ok(body_bytes) => {
                                let body = body_bytes.to_vec();
                                // Try to send response
                                match self.out_resp.push(body) {
                                    Ok(()) => {
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                        debug!("sent HTTP response successfully");
                                    }
                                    Err(PushError::Full(returned_body)) => {
                                        // Output buffer full, buffer internally for later retry
                                        debug!("response output buffer full, buffering internally");
                                        self.pending_responses.push_back(returned_body);
                                        work_units += 1; // We processed the request, just couldn't send
                                        context.remaining_budget -= 1;
                                    }
                                }
                            }
                            Err(e) => {
                                let error_msg = format!("Failed to read response body: {}", e);
                                warn!("{}", error_msg);
                                match self.out_err.push(error_msg.into_bytes()) {
                                    Ok(()) => {
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                        debug!("sent HTTP error successfully");
                                    }
                                    Err(PushError::Full(returned_error)) => {
                                        debug!("error output buffer full, buffering internally");
                                        self.pending_errors.push_back(returned_error);
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        // send error
                        debug!("got HTTP error, sending...");
                        let error_msg = err.to_string();
                        match self.out_err.push(error_msg.into_bytes()) {
                            Ok(()) => {
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("sent HTTP error successfully");
                            }
                            Err(PushError::Full(returned_error)) => {
                                // Output buffer full, buffer internally for later retry
                                debug!("error output buffer full, buffering internally");
                                self.pending_errors.push_back(returned_error);
                                work_units += 1; // We processed the request, just couldn't send
                                context.remaining_budget -= 1;
                            }
                        }
                    }
                };
                debug!("done processing request");
            } else {
                break;
            }
        }

        // are we done?
        if self.req.is_abandoned() && self.pending_responses.is_empty() && self.pending_errors.is_empty() {
            info!("EOF on inport REQ and all requests processed, finishing");
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
            name: String::from("HTTPClient"),
            description: String::from("Reads URLs and sends the response body out via RESP or ERR"), //TODO change according to new features
            icon: String::from("web"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("REQ"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("URLs, one per IP"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![
                ComponentPort {
                    name: String::from("RESP"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("response body if response is non-error"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
                ComponentPort {
                    name: String::from("ERR"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("error responses in human-readable error format"), //TODO some machine-readable format better?
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
            ],
            ..Default::default()
        }
    }
}

pub struct HTTPServerComponent {
    // Configuration inputs
    conf: ProcessEdgeSource,
    routes: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    req: Vec<ProcessEdgeSink>,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,

    // Runtime state for cooperative HTTP server
    listen_addr: Option<String>,
    routes_config: Option<Vec<String>>,
    listener: Option<TcpListener>,
    connections: HashMap<u32, Connection>,
    next_client_id: u32,
    pending_requests: std::collections::VecDeque<(u32, HttpRequest)>, // (client_id, request) waiting to be sent
    pending_responses: HashMap<u32, HttpResponse>, // responses waiting to be sent to clients
}

impl Component for HTTPServerComponent {
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
        HTTPServerComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            routes: inports
                .remove("ROUTES")
                .expect("found no ROUTES inport")
                .pop()
                .unwrap(),
            resp: inports
                .remove("RESP")
                .expect("found no RESP inport")
                .pop()
                .unwrap(),
            req: outports.remove("REQ").expect("found no OUT outport"),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
            listen_addr: None,
            routes_config: None,
            listener: None,
            connections: HashMap::new(),
            next_client_id: 0,
            pending_requests: std::collections::VecDeque::new(),
            pending_responses: HashMap::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("HTTPServer is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration if not yet configured
        if self.listener.is_none() {
            // Try to read configuration
            if let Ok(listen_addr_bytes) = self.conf.pop() {
                self.listen_addr = Some(std::str::from_utf8(&listen_addr_bytes)
                    .expect("invalid utf-8 listen address").to_owned());
                trace!("got listen address: {}", self.listen_addr.as_ref().unwrap());
            }

            if let Ok(routes_bytes) = self.routes.pop() {
                let routes_str = std::str::from_utf8(&routes_bytes)
                    .expect("invalid utf-8 routes").split(",")
                    .map(|s| s.to_string()).collect::<Vec<_>>();
                self.routes_config = Some(routes_str);
                trace!("got routes configuration");
            }

            // If we have all config, set up the server
            if let (Some(listen_addr), Some(_routes)) = (&self.listen_addr, &self.routes_config) {
                let listener = std::net::TcpListener::bind(listen_addr)
                    .expect("failed to bind HTTP listener socket");
                listener.set_nonblocking(true)
                    .expect("failed to set non-blocking on HTTP listener");
                self.listener = Some(listener);
                debug!("HTTP server configured and listening on {}", listen_addr);
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
                        debug!("accepted new HTTP connection from {}", addr);

                        // Set up timeouts
                        let sock = sock;
                        sock.set_read_timeout(Some(Duration::from_millis(500)))
                            .expect("failed to set read timeout on client socket");
                        sock.set_write_timeout(Some(Duration::from_millis(500)))
                            .expect("failed to set write timeout on client socket");

                        let client_id = self.next_client_id;
                        self.next_client_id += 1;

                        let connection = Connection {
                            state: ConnectionState::ReadingRequest {
                                parse_state: HttpParseState::RequestLine,
                                buffer: Vec::new(),
                            },
                            last_active: Instant::now(),
                            sock,
                        };

                        self.connections.insert(client_id, connection);
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("new HTTP connection {} in reading state", client_id);
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            warn!("failed to accept HTTP connection: {}", e);
                        }
                        // WouldBlock is normal, no work done
                    }
                }
            }
        }

        // Process existing connections (within remaining budget)
        let mut connections_to_remove = Vec::new();
        let mut state_changes = Vec::new(); // (client_id, new_state)

        for (client_id, connection) in self.connections.iter_mut() {
            if context.remaining_budget == 0 {
                break; // No more budget
            }

            match &mut connection.state {
                ConnectionState::ReadingRequest { parse_state, buffer } => {
                    // Try to read more data from the socket
                    let mut temp_buf = [0; 1024];
                    match connection.sock.read(&mut temp_buf) {
                        Ok(bytes_read) => {
                            if bytes_read > 0 {
                                buffer.extend_from_slice(&temp_buf[0..bytes_read]);
                                connection.last_active = Instant::now();

                                // Try to parse the HTTP request incrementally
                                if let HttpParseState::RequestLine = parse_state {
                                    if let Some(line_end) = find_crlf(buffer) {
                                        let line_str = std::str::from_utf8(&buffer[0..line_end])
                                            .unwrap_or("").to_string();
                                        if let Some((method, rest)) = line_str.split_once(' ') {
                                            if let Some((path, version)) = rest.rsplit_once(' ') {
                                                // Move to headers state
                                                *parse_state = HttpParseState::Headers {
                                                    headers: HashMap::new(),
                                                };
                                                // Remove the request line from buffer
                                                buffer.drain(0..line_end + 2);

                                                // For simplicity, assume no body for GET requests
                                                let request = HttpRequest {
                                                    method: method.to_string(),
                                                    path: path.to_string(),
                                                    version: version.to_string(),
                                                    headers: HashMap::new(), // Simplified
                                                    body: Vec::new(),
                                                };

                                                let new_state = ConnectionState::ProcessingRequest {
                                                    request: request.clone(),
                                                    client_id: *client_id,
                                                };
                                                state_changes.push((*client_id, new_state));

                                                // Add to pending requests
                                                self.pending_requests.push_back((*client_id, request));
                                                work_units += 1;
                                                context.remaining_budget -= 1;
                                                debug!("parsed HTTP request from client {}", client_id);
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Connection closed by client
                                debug!("HTTP connection closed by client {}", client_id);
                                connections_to_remove.push(*client_id);
                            }
                        }
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::WouldBlock {
                                // No data available, continue to next connection
                                continue;
                            } else {
                                warn!("failed to read from HTTP client {}: {}", client_id, e);
                                connections_to_remove.push(*client_id);
                            }
                        }
                    }
                }

                ConnectionState::ProcessingRequest { .. } => {
                    // Request is already in pending_requests, waiting for processing
                    // This state exists to track that we're waiting
                }

                ConnectionState::WritingResponse { response_state, client_id } => {
                    match response_state {
                        HttpResponseState::Pending => {
                            // Check if we have a response for this client
                            if let Some(response) = self.pending_responses.remove(client_id) {
                                *response_state = HttpResponseState::Writing {
                                    response,
                                    sent: 0,
                                };
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("starting to write HTTP response to client {}", client_id);
                            }
                        }
                        HttpResponseState::Writing { response, sent } => {
                            // Write response data
                            let response_bytes = format_response(response);
                            let remaining = &response_bytes[*sent..];

                            match connection.sock.write(remaining) {
                                Ok(bytes_written) => {
                                    *sent += bytes_written;
                                    if *sent >= response_bytes.len() {
                                        // Response fully sent
                                        *response_state = HttpResponseState::Complete;
                                        // Schedule state change to avoid borrowing issues
                                        let new_state = ConnectionState::KeepAlive;
                                        state_changes.push((*client_id, new_state));
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                        debug!("completed HTTP response to client {}", client_id);
                                    } else {
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                    }
                                }
                                Err(e) => {
                                    if e.kind() == std::io::ErrorKind::WouldBlock {
                                        // Would block, try again later
                                    } else {
                                        warn!("failed to write HTTP response to client {}: {}", client_id, e);
                                        connections_to_remove.push(*client_id);
                                    }
                                }
                            }
                        }
                        HttpResponseState::Complete => {
                            // Should not reach here in WritingResponse state
                        }
                    }
                }

                ConnectionState::KeepAlive => {
                    // Connection is keep-alive, could read another request
                    // For simplicity, we'll close after one request
                    let new_state = ConnectionState::Closed;
                    state_changes.push((*client_id, new_state));
                    connections_to_remove.push(*client_id);
                }

                ConnectionState::Closed => {
                    connections_to_remove.push(*client_id);
                }
            }
        }

        // Apply state changes
        for (client_id, new_state) in state_changes {
            if let Some(connection) = self.connections.get_mut(&client_id) {
                connection.state = new_state;
            }
        }

        // Remove closed connections
        for client_id in connections_to_remove {
            self.connections.remove(&client_id);
            work_units += 1; // Count cleanup as work
            debug!("cleaned up HTTP connection {}", client_id);
        }

        // Send pending requests to FBP network
        while context.remaining_budget > 0 && !self.pending_requests.is_empty() {
            // Get the front item without borrowing
            let (client_id, request) = match self.pending_requests.front() {
                Some(item) => item.clone(),
                None => break,
            };

            // Route the request based on path
            if let Some(routes) = &self.routes_config {
                let mut route_index = None;
                for (i, route) in routes.iter().enumerate() {
                    if request.path.starts_with(route) {
                        route_index = Some(i);
                        break;
                    }
                }

                if let Some(index) = route_index {
                    if index < self.req.len() {
                        // Send the request body (simplified - just send path for now)
                        match self.req[index].push(request.path.clone().into_bytes()) {
                            Ok(()) => {
                                self.pending_requests.pop_front();
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("sent HTTP request to FBP network for route {}", index);

                                // Mark connection as waiting for response
                                if let Some(conn) = self.connections.get_mut(&client_id) {
                                    conn.state = ConnectionState::WritingResponse {
                                        response_state: HttpResponseState::Pending,
                                        client_id,
                                    };
                                }
                            }
                            Err(PushError::Full(_)) => {
                                // Output buffer full, try again later
                                break;
                            }
                        }
                    } else {
                        // No route sink available
                        self.pending_requests.pop_front();
                        // Send 404 response
                        let response = HttpResponse {
                            status_code: 404,
                            status_text: "Not Found".to_string(),
                            headers: HashMap::new(),
                            body: b"Route not configured".to_vec(),
                        };
                        self.pending_responses.insert(client_id, response);
                    }
                } else {
                    // No route matched
                    self.pending_requests.pop_front();
                    let response = HttpResponse {
                        status_code: 404,
                        status_text: "Not Found".to_string(),
                        headers: HashMap::new(),
                        body: b"Route not found".to_vec(),
                    };
                    self.pending_responses.insert(client_id, response);
                }
            }
        }

        // Read responses from FBP network
        if context.remaining_budget > 0 {
            if let Ok(response_body) = self.resp.pop() {
                // For simplicity, assume the response is for the first waiting client
                // In a real implementation, we'd need a way to correlate requests/responses
                for (client_id, connection) in &self.connections {
                    if let ConnectionState::WritingResponse { response_state: HttpResponseState::Pending, client_id: _ } = &connection.state {
                        let response = HttpResponse {
                            status_code: 200,
                            status_text: "OK".to_string(),
                            headers: HashMap::new(),
                            body: response_body,
                        };
                        self.pending_responses.insert(*client_id, response);
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("received response from FBP network for client {}", client_id);
                        break;
                    }
                }
            }
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    fn run(mut self) {
        debug!("HTTPServer is now run()ning!");

        // read configuration
        trace!("spinning for listen IP address on CONF...");
        loop {
            if !self.conf.is_empty() {
                break;
            }
            if let Ok(ip) = self.signals_in.try_recv() {
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                );
                if ip == b"stop" {
                    info!("got stop signal while waiting for CONF, exiting");
                    return;
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
            thread::yield_now();
        }
        trace!("spinning for routes on ROUTES...");
        loop {
            if !self.routes.is_empty() {
                break;
            }
            if let Ok(ip) = self.signals_in.try_recv() {
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                );
                if ip == b"stop" {
                    info!("got stop signal while waiting for ROUTES, exiting");
                    return;
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
            thread::yield_now();
        }
        //TODO optimize string conversions to listen on a path
        let config = self
            .conf
            .pop()
            .expect("not empty but still got an error on pop");
        let listenaddress = std::str::from_utf8(&config)
            .expect("could not parse listenpath as utf8")
            .to_owned(); //TODO optimize
        trace!("got listen address {}", listenaddress);
        let routes_bytes = self
            .routes
            .pop()
            .expect("not empty but still got an error on pop");
        let routes_str = std::str::from_utf8(&routes_bytes)
            .expect("could not parse routes as utf8")
            .split(",")
            .collect::<Vec<_>>(); //TODO optimize
        let routes = routes_str.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        // set up work inports and outports
        let resp = Arc::new(Mutex::new(self.resp));
        let out_req = Arc::new(Mutex::new(self.req));
        let server_shutdown = Arc::new(AtomicBool::new(false));
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let mut shutdown_tx = Some(shutdown_tx);

        // start HTTP server in separate thread so we can also handle stop/ping in the component thread
        let resp_for_server = resp.clone();
        let out_req_for_server = out_req.clone();
        let routes_for_server = routes.clone();
        let shutdown_for_server = server_shutdown.clone();
        let server_thread = thread::Builder::new()
            .name(format!(
                "{}_handler",
                thread::current()
                    .name()
                    .expect("could not get component thread name")
            ))
            .spawn(move || {
                let bind_addr = listenaddress
                    .to_socket_addrs()
                    .expect("failed to resolve HTTP listen address")
                    .next()
                    .expect("no resolved address for HTTP listen address");
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for HTTPServer");
                rt.block_on(async move {
                    let make_svc = make_service_fn(move |_| {
                        let routes = routes_for_server.clone();
                        let out_req = out_req_for_server.clone();
                        let resp = resp_for_server.clone();
                        let shutdown = shutdown_for_server.clone();
                        async move {
                            Ok::<_, Infallible>(service_fn(
                                move |mut req: HyperRequest<HyperBody>| {
                                    let routes = routes.clone();
                                    let out_req = out_req.clone();
                                    let resp = resp.clone();
                                    let shutdown = shutdown.clone();
                                    async move {
                                        if shutdown.load(Ordering::Relaxed) {
                                            let mut response = HyperResponse::new(HyperBody::from(
                                                "server shutting down",
                                            ));
                                            *response.status_mut() =
                                                StatusCode::SERVICE_UNAVAILABLE;
                                            return Ok::<_, Infallible>(response);
                                        }

                                        let route_req = req.uri().path().to_owned();
                                        let mut route_index = usize::MAX;
                                        for (i, route) in routes.iter().enumerate() {
                                            if route_req.starts_with(route) {
                                                route_index = i;
                                                break;
                                            }
                                        }
                                        if route_index == usize::MAX {
                                            let mut response = HyperResponse::new(HyperBody::from(
                                                "no route matched",
                                            ));
                                            *response.status_mut() = StatusCode::NOT_FOUND;
                                            return Ok::<_, Infallible>(response);
                                        }

                                        let body = match hyper::body::to_bytes(req.body_mut()).await
                                        {
                                            Ok(bytes) => bytes,
                                            Err(err) => {
                                                warn!(
                                                    "HTTPServer: failed to read request body: {}",
                                                    err
                                                );
                                                let mut response = HyperResponse::new(
                                                    HyperBody::from("failed to read request body"),
                                                );
                                                *response.status_mut() = StatusCode::BAD_REQUEST;
                                                return Ok::<_, Infallible>(response);
                                            }
                                        };

                                        let pushed_ok = {
                                            let mut out_req_locked = out_req
                                                .lock()
                                                .expect("poisoned lock on REQ outport");
                                            if let Some(out_req_one) =
                                                out_req_locked.get_mut(route_index)
                                            {
                                                out_req_one
                                                    .sink
                                                    .push(body.to_vec())
                                                    .expect("could not push IP into FBP network");
                                                out_req_one
                                                    .wakeup
                                                    .as_ref()
                                                    .expect("got no wakeup handle on REQ outport")
                                                    .unpark();
                                                true
                                            } else {
                                                false
                                            }
                                        };
                                        if !pushed_ok {
                                            let mut response = HyperResponse::new(HyperBody::from(
                                                "internal route mismatch",
                                            ));
                                            *response.status_mut() =
                                                StatusCode::INTERNAL_SERVER_ERROR;
                                            return Ok::<_, Infallible>(response);
                                        }

                                        // wait for response from FBP network
                                        loop {
                                            if shutdown.load(Ordering::Relaxed) {
                                                let mut response = HyperResponse::new(
                                                    HyperBody::from("server shutting down"),
                                                );
                                                *response.status_mut() =
                                                    StatusCode::SERVICE_UNAVAILABLE;
                                                return Ok::<_, Infallible>(response);
                                            }
                                            if let Ok(ip) = resp
                                                .lock()
                                                .expect("poisoned lock on RESP inport")
                                                .pop()
                                            {
                                                return Ok::<_, Infallible>(HyperResponse::new(
                                                    HyperBody::from(ip),
                                                ));
                                            }
                                            tokio::task::yield_now().await;
                                        }
                                    }
                                },
                            ))
                        }
                    });

                    let server = HyperServer::bind(&bind_addr).serve(make_svc);
                    let graceful = server.with_graceful_shutdown(async {
                        let _ = shutdown_rx.await;
                    });
                    if let Err(err) = graceful.await {
                        warn!("HTTPServer: server thread exited with error: {}", err);
                    }
                });
            })
            .expect("failed to spawn HTTP server thread");

        debug!("entering main loop");
        loop {
            trace!("begin of iteration");

            // check signals
            //TODO optimize, there is also try_recv() and recv_timeout()
            if let Ok(ip) = self.signals_in.try_recv() {
                //TODO optimize string conversions
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                );
                // stop signal
                if ip == b"stop" {
                    info!("got stop signal, exiting");
                    break;
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
            trace!("end of iteration");
            std::thread::park();
        }
        debug!("cleaning up");
        server_shutdown.store(true, Ordering::Relaxed);
        if let Some(tx) = shutdown_tx.take() {
            let _ = tx.send(());
        }
        let join_started = Instant::now();
        while !server_thread.is_finished() && join_started.elapsed() < SERVER_JOIN_GRACE {
            thread::sleep(Duration::from_millis(10));
        }
        if server_thread.is_finished() {
            if let Err(err) = server_thread.join() {
                warn!("HTTPServer: failed to join server thread: {:?}", err);
            }
        } else {
            warn!(
                "HTTPServer: server thread did not exit within {:?}, handing off to join reaper",
                SERVER_JOIN_GRACE
            );
            handoff_join(server_thread, "HTTPServer");
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("HTTPServer"),
            description: String::from("HTTP server"),
            icon: String::from("server"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("configuration value, currently the IP address to listen on"),
                    values_allowed: vec![],
                    value_default: String::from("localhost:8080")
                },
                ComponentPort {
                    name: String::from("ROUTES"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("configuration value, currently a comma-separated list of routes; matching route index = outport index"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("RESP"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,    //TODO currently this needs a Muxer before it and the RESP port is thus intentionally not marked as an arrayport
                    description: String::from("response data from HTTP route handlers"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("REQ"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: true,
                    description: String::from("incoming requests from HTTP clients"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
