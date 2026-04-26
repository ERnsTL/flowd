use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

// component-specific
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// Hyper imports for the old run() method
use hyper::service::{make_service_fn, service_fn};
use hyper::{
    Body as HyperBody, Request as HyperRequest, Response as HyperResponse, Server as HyperServer,
    StatusCode,
};
use tokio::sync::oneshot;

// Time formatting is handled by SystemTime

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
    pub query_params: HashMap<String, String>,
    pub path_params: HashMap<String, String>,
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
#[allow(unused)]
enum HttpParseState {
    RequestLine,
    Headers {
        headers: HashMap<String, String>,
    },
    Body {
        content_length: usize,
        received: usize,
        buffer: Vec<u8>,
    },
    ChunkedBody {
        chunks: Vec<Vec<u8>>,
        chunk_size: Option<usize>,
        buffer: Vec<u8>,
    },
    Trailers {
        headers: HashMap<String, String>,
        chunks: Vec<Vec<u8>>,
    },
    Complete {
        request: HttpRequest,
    },
}

#[derive(Debug)]
enum HttpResponseState {
    Pending,
    Writing { response: HttpResponse, sent: usize },
    Complete,
}

#[derive(Debug)]
#[allow(unused)]
enum ConnectionState {
    ReadingRequest {
        parse_state: HttpParseState,
        buffer: Vec<u8>,
        request: Option<HttpRequest>,
    },
    ProcessingRequest {
        request: HttpRequest,
        client_id: u32,
    },
    WritingResponse {
        request_id: u64,
        response_state: HttpResponseState,
        client_id: u32,
    },
    KeepAlive,
    Closed,
}

// Helper function to find CRLF in buffer
fn find_crlf(buffer: &[u8]) -> Option<usize> {
    buffer.windows(2).position(|w| w == b"\r\n")
}

// Helper function to parse HTTP headers from a line
fn parse_header_line(line: &str) -> Option<(String, String)> {
    if let Some(colon_pos) = line.find(':') {
        let key = line[..colon_pos].trim().to_lowercase(); // HTTP headers are case-insensitive
        let value = line[colon_pos + 1..].trim().to_string();
        Some((key, value))
    } else {
        None
    }
}

// Helper function to parse chunk size from hex string
fn parse_chunk_size(hex_str: &str) -> Option<usize> {
    usize::from_str_radix(hex_str.trim(), 16).ok()
}

// Helper function to match routes with path parameters (e.g., /users/{id})
fn match_route_with_params(
    request_path: &str,
    route_pattern: &str,
) -> Option<HashMap<String, String>> {
    let request_parts: Vec<&str> = request_path.split('/').collect();
    let route_parts: Vec<&str> = route_pattern.split('/').collect();

    if request_parts.len() != route_parts.len() {
        return None;
    }

    let mut params = HashMap::new();

    for (req_part, route_part) in request_parts.iter().zip(route_parts.iter()) {
        if route_part.starts_with('{') && route_part.ends_with('}') {
            // This is a parameter
            let param_name = &route_part[1..route_part.len() - 1];
            params.insert(param_name.to_string(), req_part.to_string());
        } else if req_part != route_part {
            // Static parts must match exactly
            return None;
        }
    }

    Some(params)
}

fn encode_map_line(map: &HashMap<String, String>) -> String {
    let mut pairs: Vec<String> = map.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
    pairs.sort();
    pairs.join("&")
}

fn encode_request_for_req_port(request: &HttpRequest) -> Vec<u8> {
    let body = String::from_utf8_lossy(&request.body);
    format!(
        "METHOD={}\nPATH={}\nVERSION={}\nQUERY={}\nPATH_PARAMS={}\nBODY={}\n",
        request.method,
        request.path,
        request.version,
        encode_map_line(&request.query_params),
        encode_map_line(&request.path_params),
        body
    )
    .into_bytes()
}

fn parse_http_server_conf(conf: &str) -> (String, Option<Duration>) {
    let (listen_addr, params) = match conf.split_once('?') {
        Some((addr, query)) => (addr.trim().to_string(), Some(query)),
        None => (conf.trim().to_string(), None),
    };

    let mut keep_alive_timeout = None;
    if let Some(query) = params {
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            if key.trim().eq_ignore_ascii_case("keep_alive_timeout_ms") {
                if let Ok(ms) = value.trim().parse::<u64>() {
                    if ms > 0 {
                        keep_alive_timeout = Some(Duration::from_millis(ms));
                    }
                }
            }
        }
    }

    (listen_addr, keep_alive_timeout)
}

// Route configuration for HTTP server
#[derive(Debug, Clone)]
struct RouteConfig {
    method: String,
    path: String,
}

// Helper function to get standard HTTP status text
#[allow(unused)]
fn get_status_text(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "Unknown Status",
    }
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

    // Add Date header if not present
    let mut has_date = false;
    let mut has_content_length = false;
    let mut has_connection = false;

    for (key, _) in &response.headers {
        if key.eq_ignore_ascii_case("date") {
            has_date = true;
        }
        if key.eq_ignore_ascii_case("content-length") {
            has_content_length = true;
        }
        if key.eq_ignore_ascii_case("connection") {
            has_connection = true;
        }
    }

    // Add Date header (RFC 7231 format)
    if !has_date {
        use std::time::{SystemTime, UNIX_EPOCH};
        if let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let datetime = time::OffsetDateTime::from_unix_timestamp(duration.as_secs() as i64)
                .unwrap_or_else(|_| time::OffsetDateTime::UNIX_EPOCH);
            let formatted = datetime
                .format(&time::format_description::well_known::Rfc2822)
                .unwrap_or_else(|_| "Mon, 01 Jan 2024 00:00:00 GMT".to_string());
            result.extend_from_slice(b"Date: ");
            result.extend_from_slice(formatted.as_bytes());
            result.extend_from_slice(b"\r\n");
        }
    }

    // Add Connection header for keep-alive/close
    if !has_connection {
        // Default to close for HTTP/1.0 compatibility, but could be keep-alive for HTTP/1.1
        result.extend_from_slice(b"Connection: close\r\n");
    }

    // Check if response should be chunked
    let use_chunked = response.body.len() > 8192
        || response
            .headers
            .get("transfer-encoding")
            .map(|v| v.to_lowercase())
            == Some("chunked".to_string());

    if use_chunked {
        // Use chunked encoding
        if !response.headers.contains_key("transfer-encoding") {
            result.extend_from_slice(b"Transfer-Encoding: chunked\r\n");
        }
        // Don't add Content-Length for chunked encoding
    } else {
        // Add Content-Length header
        if !has_content_length {
            result.extend_from_slice(b"Content-Length: ");
            result.extend_from_slice(response.body.len().to_string().as_bytes());
            result.extend_from_slice(b"\r\n");
        }
    }

    // Server header
    result.extend_from_slice(b"Server: flowd/0.5\r\n");

    // Custom headers
    for (key, value) in &response.headers {
        result.extend_from_slice(key.as_bytes());
        result.extend_from_slice(b": ");
        result.extend_from_slice(value.as_bytes());
        result.extend_from_slice(b"\r\n");
    }

    // End of headers
    result.extend_from_slice(b"\r\n");

    // Body
    if use_chunked {
        // Send body in chunks
        let chunk_size = 4096; // 4KB chunks
        for chunk in response.body.chunks(chunk_size) {
            // Chunk size in hex
            let size_hex = format!("{:x}\r\n", chunk.len());
            result.extend_from_slice(size_hex.as_bytes());
            // Chunk data
            result.extend_from_slice(chunk);
            result.extend_from_slice(b"\r\n");
        }
        // Final chunk
        result.extend_from_slice(b"0\r\n\r\n");
    } else {
        result.extend_from_slice(&response.body);
    }

    result
}

#[derive(Debug)]
#[allow(unused)]
struct Connection {
    state: ConnectionState,
    last_active: Instant,
    sock: TcpStream,
    keep_alive_requested: bool, // Whether client requested keep-alive
}

#[derive(Debug)]
enum HttpClientCommand {
    MakeRequest(String), // URL to request
    Shutdown,
}

#[derive(Debug)]
enum HttpClientResult {
    Response(Vec<u8>), // Response body
    Error(String),     // Error message
}

#[derive(Debug, Clone)]
enum HttpClientState {
    Idle,
    Processing,
    Error(()),
}

pub struct HTTPClientComponent {
    req: ProcessEdgeSource,
    out_resp: ProcessEdgeSink,
    out_err: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,

    // Worker thread communication
    command_tx: Sender<HttpClientCommand>,
    result_rx: Receiver<HttpClientResult>,
    worker_handle: Option<JoinHandle<()>>,

    // Runtime state
    state: HttpClientState,
    pending_responses: std::collections::VecDeque<Vec<u8>>, // responses to send, buffered for backpressure
    pending_errors: std::collections::VecDeque<Vec<u8>>, // errors to send, buffered for backpressure
}

impl HTTPClientComponent {
    fn worker_thread(
        command_rx: Receiver<HttpClientCommand>,
        result_tx: Sender<HttpClientResult>,
        client: reqwest::blocking::Client,
        scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) {
        loop {
            // Try to receive a command
            match command_rx.try_recv() {
                Ok(command) => match command {
                    HttpClientCommand::MakeRequest(url) => {
                        debug!("Worker: making HTTP request to {}", url);
                        match client.get(&url).send() {
                            Ok(resp) => {
                                debug!("Worker: got response, reading body...");
                                let status = resp.status();
                                match resp.bytes() {
                                    Ok(body_bytes) => {
                                        let body = body_bytes.to_vec();
                                        if status.is_success() {
                                            debug!(
                                                "Worker: sent response body ({} bytes)",
                                                body.len()
                                            );
                                            let _ =
                                                result_tx.send(HttpClientResult::Response(body));
                                        } else {
                                            let message = format!(
                                                "HTTP {}: {}",
                                                status,
                                                String::from_utf8_lossy(&body)
                                            );
                                            let _ =
                                                result_tx.send(HttpClientResult::Error(message));
                                        }
                                        if let Some(ref waker) = scheduler_waker {
                                            waker();
                                        }
                                    }
                                    Err(e) => {
                                        let error_msg = format!(
                                            "HTTP {}: Failed to read response body: {}",
                                            status, e
                                        );
                                        let _ = result_tx.send(HttpClientResult::Error(error_msg));
                                        if let Some(ref waker) = scheduler_waker {
                                            waker();
                                        }
                                        debug!("Worker: failed to read response body: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                let error_msg = format!("HTTP request failed: {}", e);
                                let _ = result_tx.send(HttpClientResult::Error(error_msg));
                                if let Some(ref waker) = scheduler_waker {
                                    waker();
                                }
                                debug!("Worker: HTTP request failed: {}", e);
                            }
                        }
                    }
                    HttpClientCommand::Shutdown => {
                        debug!("Worker: received shutdown command, exiting");
                        break;
                    }
                },
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // No command, sleep briefly to avoid busy looping
                    thread::sleep(Duration::from_millis(10));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Main thread disconnected, exit
                    debug!("Worker: command channel disconnected, exiting");
                    break;
                }
            }
        }

        debug!("HTTP worker thread exiting");
    }
}

impl Drop for HTTPClientComponent {
    fn drop(&mut self) {
        // Tell worker to exit and wait for a clean shutdown.
        let _ = self.command_tx.send(HttpClientCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Component for HTTPClientComponent {
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
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(HTTP_CONNECT_TIMEOUT)
            .timeout(HTTP_REQUEST_TIMEOUT)
            .build()
            .expect("failed to build HTTP client");

        // Create communication channels for worker thread
        let (command_tx, command_rx) = mpsc::channel::<HttpClientCommand>();
        let (result_tx, result_rx) = mpsc::channel::<HttpClientResult>();

        // Spawn worker thread
        let worker_handle = Some(thread::spawn(move || {
            Self::worker_thread(command_rx, result_tx, client, scheduler_waker);
        }));

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
            command_tx,
            result_rx,
            worker_handle,
            state: HttpClientState::Idle,
            pending_responses: std::collections::VecDeque::new(),
            pending_errors: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("HTTPClient is now process()ing!");
        let mut work_units = 0u32;

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

        // Check for results from worker thread
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                HttpClientResult::Response(body) => {
                    match self.out_resp.push(body) {
                        Ok(()) => {
                            work_units += 1;
                            debug!("forwarded HTTP response to output");
                        }
                        Err(PushError::Full(body)) => {
                            // Output buffer full, buffer internally for later retry
                            debug!("response output buffer full, buffering internally");
                            self.pending_responses.push_back(body);
                            work_units += 1;
                        }
                    }
                }
                HttpClientResult::Error(error_msg) => {
                    match self.out_err.push(error_msg.into_bytes()) {
                        Ok(()) => {
                            work_units += 1;
                            debug!("forwarded HTTP error to output");
                        }
                        Err(PushError::Full(error_bytes)) => {
                            // Output buffer full, buffer internally for later retry
                            debug!("error output buffer full, buffering internally");
                            self.pending_errors.push_back(error_bytes);
                            work_units += 1;
                        }
                    }
                }
            }
        }

        // Send any pending responses that were buffered due to backpressure
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

        // Send any pending errors that were buffered due to backpressure
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

        // Queue new requests to worker thread within remaining budget
        while context.remaining_budget > 0 {
            if let Ok(ip) = self.req.pop() {
                let url = std::str::from_utf8(&ip).expect("non utf-8 data");
                debug!("got a request: {}", &url);

                // Send request to worker thread
                if let Err(e) = self
                    .command_tx
                    .send(HttpClientCommand::MakeRequest(url.to_string()))
                {
                    warn!("failed to send HTTP request to worker: {}", e);
                    self.state = HttpClientState::Error(());
                } else {
                    self.state = HttpClientState::Processing;
                    work_units += 1;
                    context.remaining_budget -= 1;
                    debug!("queued HTTP request to worker thread");
                }
            } else {
                break;
            }
        }

        // are we done?
        if self.req.is_abandoned()
            && self.pending_responses.is_empty()
            && self.pending_errors.is_empty()
        {
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
    routes_config: Option<Vec<RouteConfig>>,
    listener: Option<TcpListener>,
    connections: HashMap<u32, Connection>,
    next_client_id: u32,
    pending_requests: std::collections::VecDeque<(u64, u32, HttpRequest)>, // (request_id, client_id, request) waiting to be sent
    pending_responses: HashMap<u64, HttpResponse>, // responses waiting to be sent to clients, keyed by request_id
    response_wait_queue: VecDeque<u64>,            // request IDs waiting for RESP in FIFO order
    next_request_id: u64,                          // for correlating requests and responses
    keep_alive_timeout: Duration,
}

impl Component for HTTPServerComponent {
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
            response_wait_queue: VecDeque::new(),
            next_request_id: 0,
            keep_alive_timeout: Duration::from_secs(30),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("HTTPServer is now process()ing!");
        let mut work_units = 0u32;

        // Always try to read configuration (in case it arrives late)
        let mut config_changed = false;
        if let Ok(listen_addr_bytes) = self.conf.pop() {
            let conf_str = std::str::from_utf8(&listen_addr_bytes)
                .expect("invalid utf-8 listen address")
                .to_owned();
            let (listen_addr, keep_alive_timeout) = parse_http_server_conf(&conf_str);
            self.listen_addr = Some(listen_addr);
            if let Some(timeout) = keep_alive_timeout {
                self.keep_alive_timeout = timeout;
            }
            trace!(
                "got listen config: addr={}, keep_alive_timeout={:?}",
                self.listen_addr.as_ref().unwrap(),
                self.keep_alive_timeout
            );
            config_changed = true;
        }

        if let Ok(routes_bytes) = self.routes.pop() {
            let routes_str = std::str::from_utf8(&routes_bytes).expect("invalid utf-8 routes");
            let routes_config: Vec<RouteConfig> = routes_str
                .split(",")
                .filter_map(|route_spec| {
                    let parts: Vec<&str> = route_spec.trim().split_whitespace().collect();
                    if parts.len() == 2 {
                        Some(RouteConfig {
                            method: parts[0].to_uppercase(),
                            path: parts[1].to_string(),
                        })
                    } else {
                        // Default to GET if no method specified
                        Some(RouteConfig {
                            method: "GET".to_string(),
                            path: route_spec.trim().to_string(),
                        })
                    }
                })
                .collect();
            self.routes_config = Some(routes_config);
            trace!("got routes configuration");
            config_changed = true;
        }

        // If we have all config and listener not set up, set up the server
        if self.listener.is_none() {
            if let (Some(listen_addr), Some(_routes)) = (&self.listen_addr, &self.routes_config) {
                let listener = std::net::TcpListener::bind(listen_addr)
                    .expect("failed to bind HTTP listener socket");
                listener
                    .set_nonblocking(true)
                    .expect("failed to set non-blocking on HTTP listener");
                self.listener = Some(listener);
                debug!("HTTP server configured and listening on {}", listen_addr);
                work_units += 1; // Count configuration as work
                context.remaining_budget -= 1;
            } else {
                trace!("waiting for complete configuration");
                return ProcessResult::NoWork;
            }
        } else if config_changed {
            // Configuration updated while already running
            work_units += 1;
            context.remaining_budget -= 1;
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

        // Accept all currently pending connections (within budget)
        if let Some(listener) = &self.listener {
            while context.remaining_budget > 0 {
                match listener.accept() {
                    Ok((sock, addr)) => {
                        debug!("accepted new HTTP connection from {}", addr);

                        // Keep client sockets non-blocking so one slow client does not stall others.
                        sock.set_nonblocking(true)
                            .expect("failed to set non-blocking on client socket");

                        let client_id = self.next_client_id;
                        self.next_client_id += 1;

                        let connection = Connection {
                            state: ConnectionState::ReadingRequest {
                                parse_state: HttpParseState::RequestLine,
                                buffer: Vec::new(),
                                request: None,
                            },
                            last_active: Instant::now(),
                            sock,
                            keep_alive_requested: false, // Will be set during header parsing
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
                        // No more pending accepts at the moment.
                        break;
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
                ConnectionState::ReadingRequest {
                    parse_state,
                    buffer: conn_buffer,
                    request: current_request,
                } => {
                    // Try to read more data from the socket
                    let mut temp_buf = [0; 1024];
                    match connection.sock.read(&mut temp_buf) {
                        Ok(bytes_read) => {
                            if bytes_read > 0 {
                                conn_buffer.extend_from_slice(&temp_buf[0..bytes_read]);
                                connection.last_active = Instant::now();

                                // Try to parse the HTTP request incrementally
                                let mut parsing_complete = false;

                                while !parsing_complete && context.remaining_budget > 0 {
                                    match parse_state {
                                        HttpParseState::RequestLine => {
                                            if let Some(line_end) = find_crlf(conn_buffer) {
                                                let line_str = match std::str::from_utf8(
                                                    &conn_buffer[0..line_end],
                                                ) {
                                                    Ok(s) => s,
                                                    Err(_) => {
                                                        // Invalid UTF-8 in request line
                                                        let response = HttpResponse {
                                                            status_code: 400,
                                                            status_text: "Bad Request".to_string(),
                                                            headers: HashMap::new(),
                                                            body: b"Invalid UTF-8 in request line"
                                                                .to_vec(),
                                                        };
                                                        let request_id = self.next_request_id;
                                                        self.next_request_id += 1;
                                                        self.pending_responses
                                                            .insert(request_id, response);
                                                        let new_state =
                                                            ConnectionState::WritingResponse {
                                                                request_id,
                                                                response_state:
                                                                    HttpResponseState::Pending,
                                                                client_id: *client_id,
                                                            };
                                                        state_changes.push((*client_id, new_state));
                                                        parsing_complete = true;
                                                        break;
                                                    }
                                                };

                                                let parts: Vec<&str> =
                                                    line_str.split_whitespace().collect();
                                                if parts.len() != 3 {
                                                    let response = HttpResponse {
                                                        status_code: 400,
                                                        status_text: "Bad Request".to_string(),
                                                        headers: HashMap::new(),
                                                        body: b"Invalid request line format"
                                                            .to_vec(),
                                                    };
                                                    let request_id = self.next_request_id;
                                                    self.next_request_id += 1;
                                                    self.pending_responses
                                                        .insert(request_id, response);
                                                    let new_state =
                                                        ConnectionState::WritingResponse {
                                                            request_id,
                                                            response_state:
                                                                HttpResponseState::Pending,
                                                            client_id: *client_id,
                                                        };
                                                    state_changes.push((*client_id, new_state));
                                                    parsing_complete = true;
                                                    break;
                                                }

                                                let method = parts[0].to_uppercase();
                                                let mut path = parts[1].to_string();
                                                let version = parts[2].to_uppercase();

                                                // Parse query parameters from path
                                                let mut query_params = HashMap::new();
                                                if let Some(query_start) = path.find('?') {
                                                    let query_string =
                                                        path[query_start + 1..].to_string(); // Extract before truncating
                                                    path.truncate(query_start); // Remove query from path

                                                    for pair in query_string.split('&') {
                                                        if let Some(eq_pos) = pair.find('=') {
                                                            let key = pair[..eq_pos].to_string();
                                                            let value =
                                                                pair[eq_pos + 1..].to_string();
                                                            query_params.insert(key, value);
                                                        } else if !pair.is_empty() {
                                                            // Key without value
                                                            query_params.insert(
                                                                pair.to_string(),
                                                                String::new(),
                                                            );
                                                        }
                                                    }
                                                }

                                                // Validate HTTP version
                                                if !version.starts_with("HTTP/1.") {
                                                    let response = HttpResponse {
                                                        status_code: 505,
                                                        status_text: "HTTP Version Not Supported".to_string(),
                                                        headers: HashMap::new(),
                                                        body: b"Only HTTP/1.0 and HTTP/1.1 are supported".to_vec(),
                                                    };
                                                    let request_id = self.next_request_id;
                                                    self.next_request_id += 1;
                                                    self.pending_responses
                                                        .insert(request_id, response);
                                                    let new_state =
                                                        ConnectionState::WritingResponse {
                                                            request_id,
                                                            response_state:
                                                                HttpResponseState::Pending,
                                                            client_id: *client_id,
                                                        };
                                                    state_changes.push((*client_id, new_state));
                                                    parsing_complete = true;
                                                    break;
                                                }

                                                *parse_state = HttpParseState::Headers {
                                                    headers: HashMap::new(),
                                                };
                                                conn_buffer.drain(0..line_end + 2);

                                                *current_request = Some(HttpRequest {
                                                    method,
                                                    path,
                                                    query_params,
                                                    path_params: HashMap::new(),
                                                    version,
                                                    headers: HashMap::new(),
                                                    body: Vec::new(),
                                                });
                                            } else {
                                                // Need more data for request line
                                                break;
                                            }
                                        }

                                        HttpParseState::Headers { headers } => {
                                            // Parse headers until we find an empty line (double CRLF)
                                            while let Some(line_end) = find_crlf(conn_buffer) {
                                                if line_end == 0 {
                                                    // Empty line - end of headers
                                                    conn_buffer.drain(0..2); // Remove the CRLF

                                                    if let Some(ref mut req) = current_request {
                                                        req.headers = headers.clone();

                                                        // Check for Transfer-Encoding or Content-Length
                                                        let transfer_encoding =
                                                            headers.get("transfer-encoding");
                                                        let content_length =
                                                            headers.get("content-length");

                                                        if transfer_encoding
                                                            .map(|v| v.to_lowercase())
                                                            == Some("chunked".to_string())
                                                        {
                                                            *parse_state =
                                                                HttpParseState::ChunkedBody {
                                                                    chunks: Vec::new(),
                                                                    chunk_size: None,
                                                                    buffer: Vec::new(),
                                                                };
                                                        } else if let Some(cl_str) = content_length
                                                        {
                                                            if let Ok(cl) = cl_str.parse::<usize>()
                                                            {
                                                                if cl > 0 {
                                                                    *parse_state =
                                                                        HttpParseState::Body {
                                                                            content_length: cl,
                                                                            received: 0,
                                                                            buffer: Vec::new(),
                                                                        };
                                                                } else {
                                                                    // No body
                                                                    parsing_complete = true;
                                                                }
                                                            } else {
                                                                // Invalid Content-Length
                                                                let response = HttpResponse {
                                                                    status_code: 400,
                                                                    status_text: "Bad Request".to_string(),
                                                                    headers: HashMap::new(),
                                                                    body: b"Invalid Content-Length header".to_vec(),
                                                                };
                                                                let request_id =
                                                                    self.next_request_id;
                                                                self.next_request_id += 1;
                                                                self.pending_responses
                                                                    .insert(request_id, response);
                                                                let new_state = ConnectionState::WritingResponse {
                                                                    request_id,
                                                                    response_state: HttpResponseState::Pending,
                                                                    client_id: *client_id,
                                                                };
                                                                state_changes
                                                                    .push((*client_id, new_state));
                                                                parsing_complete = true;
                                                                break;
                                                            }
                                                        } else {
                                                            // No body expected
                                                            parsing_complete = true;
                                                        }
                                                    }
                                                    break;
                                                } else {
                                                    // Parse header line
                                                    let line_str = match std::str::from_utf8(
                                                        &conn_buffer[0..line_end],
                                                    ) {
                                                        Ok(s) => s,
                                                        Err(_) => {
                                                            let response = HttpResponse {
                                                                status_code: 400,
                                                                status_text: "Bad Request"
                                                                    .to_string(),
                                                                headers: HashMap::new(),
                                                                body:
                                                                    b"Invalid UTF-8 in header line"
                                                                        .to_vec(),
                                                            };
                                                            let request_id = self.next_request_id;
                                                            self.next_request_id += 1;
                                                            self.pending_responses
                                                                .insert(request_id, response);
                                                            let new_state =
                                                                ConnectionState::WritingResponse {
                                                                    request_id,
                                                                    response_state:
                                                                        HttpResponseState::Pending,
                                                                    client_id: *client_id,
                                                                };
                                                            state_changes
                                                                .push((*client_id, new_state));
                                                            parsing_complete = true;
                                                            break;
                                                        }
                                                    };

                                                    if let Some((key, value)) =
                                                        parse_header_line(line_str)
                                                    {
                                                        headers.insert(key, value);
                                                    } else {
                                                        // Invalid header line
                                                        let response = HttpResponse {
                                                            status_code: 400,
                                                            status_text: "Bad Request".to_string(),
                                                            headers: HashMap::new(),
                                                            body: b"Invalid header line format"
                                                                .to_vec(),
                                                        };
                                                        let request_id = self.next_request_id;
                                                        self.next_request_id += 1;
                                                        self.pending_responses
                                                            .insert(request_id, response);
                                                        let new_state =
                                                            ConnectionState::WritingResponse {
                                                                request_id,
                                                                response_state:
                                                                    HttpResponseState::Pending,
                                                                client_id: *client_id,
                                                            };
                                                        state_changes.push((*client_id, new_state));
                                                        parsing_complete = true;
                                                        break;
                                                    }

                                                    conn_buffer.drain(0..line_end + 2);
                                                }
                                            }

                                            if parsing_complete {
                                                break;
                                            }
                                        }

                                        HttpParseState::Body {
                                            content_length,
                                            received,
                                            buffer: ref mut body_buffer,
                                        } => {
                                            let remaining = *content_length - *received;
                                            let to_read = remaining.min(conn_buffer.len());

                                            if to_read > 0 {
                                                let data_to_copy = conn_buffer[0..to_read].to_vec();
                                                body_buffer.extend_from_slice(&data_to_copy);
                                                conn_buffer.drain(0..to_read);
                                                *received += to_read;

                                                if *received >= *content_length {
                                                    // Body complete
                                                    if let Some(ref mut req) = current_request {
                                                        req.body = body_buffer.clone();
                                                    }
                                                    parsing_complete = true;
                                                }
                                            }
                                            break; // Need more data or body complete
                                        }

                                        HttpParseState::ChunkedBody {
                                            ref mut chunks,
                                            chunk_size,
                                            buffer: ref mut chunk_buffer,
                                        } => {
                                            loop {
                                                if let Some(size) = *chunk_size {
                                                    if size == 0 {
                                                        // Last chunk
                                                        let chunks_copy = chunks.clone();
                                                        *parse_state = HttpParseState::Trailers {
                                                            headers: HashMap::new(),
                                                            chunks: chunks_copy,
                                                        };
                                                        break;
                                                    }

                                                    let to_read = size.min(conn_buffer.len());
                                                    if to_read > 0 {
                                                        let data_to_copy =
                                                            conn_buffer[0..to_read].to_vec();
                                                        chunk_buffer
                                                            .extend_from_slice(&data_to_copy);
                                                        conn_buffer.drain(0..to_read);

                                                        if chunk_buffer.len() >= size {
                                                            // Chunk complete
                                                            chunks.push(chunk_buffer.clone());
                                                            chunk_buffer.clear();
                                                            *chunk_size = None;

                                                            // Skip CRLF after chunk
                                                            if conn_buffer.len() >= 2
                                                                && &conn_buffer[0..2] == b"\r\n"
                                                            {
                                                                conn_buffer.drain(0..2);
                                                            }
                                                        }
                                                    }
                                                    break; // Need more data
                                                } else {
                                                    // Read chunk size
                                                    if let Some(line_end) = find_crlf(conn_buffer) {
                                                        let size_line = match std::str::from_utf8(
                                                            &conn_buffer[0..line_end],
                                                        ) {
                                                            Ok(s) => s,
                                                            Err(_) => {
                                                                let response = HttpResponse {
                                                                    status_code: 400,
                                                                    status_text: "Bad Request".to_string(),
                                                                    headers: HashMap::new(),
                                                                    body: b"Invalid UTF-8 in chunk size".to_vec(),
                                                                };
                                                                let request_id =
                                                                    self.next_request_id;
                                                                self.next_request_id += 1;
                                                                self.pending_responses
                                                                    .insert(request_id, response);
                                                                let new_state = ConnectionState::WritingResponse {
                                                                    request_id,
                                                                    response_state: HttpResponseState::Pending,
                                                                    client_id: *client_id,
                                                                };
                                                                state_changes
                                                                    .push((*client_id, new_state));
                                                                parsing_complete = true;
                                                                break;
                                                            }
                                                        };

                                                        if let Some(size) =
                                                            parse_chunk_size(size_line)
                                                        {
                                                            *chunk_size = Some(size);
                                                            conn_buffer.drain(0..line_end + 2);
                                                        } else {
                                                            let response = HttpResponse {
                                                                status_code: 400,
                                                                status_text: "Bad Request"
                                                                    .to_string(),
                                                                headers: HashMap::new(),
                                                                body: b"Invalid chunk size"
                                                                    .to_vec(),
                                                            };
                                                            let request_id = self.next_request_id;
                                                            self.next_request_id += 1;
                                                            self.pending_responses
                                                                .insert(request_id, response);
                                                            let new_state =
                                                                ConnectionState::WritingResponse {
                                                                    request_id,
                                                                    response_state:
                                                                        HttpResponseState::Pending,
                                                                    client_id: *client_id,
                                                                };
                                                            state_changes
                                                                .push((*client_id, new_state));
                                                            parsing_complete = true;
                                                            break;
                                                        }
                                                    } else {
                                                        break; // Need more data
                                                    }
                                                }
                                            }
                                        }

                                        HttpParseState::Trailers { headers: _, chunks } => {
                                            // Combine chunks into body and complete parsing
                                            if let Some(ref mut req) = current_request {
                                                let mut body = Vec::new();
                                                for chunk in chunks {
                                                    body.extend_from_slice(chunk);
                                                }
                                                req.body = body;
                                            }
                                            parsing_complete = true;
                                            break;
                                        }

                                        HttpParseState::Complete { .. } => {
                                            parsing_complete = true;
                                            break;
                                        }
                                    }
                                }

                                if parsing_complete {
                                    let parse_error_queued =
                                        state_changes.iter().any(|(id, state)| {
                                            *id == *client_id
                                                && matches!(
                                                    state,
                                                    ConnectionState::WritingResponse { .. }
                                                )
                                        });

                                    if !parse_error_queued {
                                        if let Some(req) = current_request.take() {
                                            let new_state = ConnectionState::ProcessingRequest {
                                                request: req.clone(),
                                                client_id: *client_id,
                                            };
                                            state_changes.push((*client_id, new_state));

                                            // Add to pending requests with request ID
                                            let request_id = self.next_request_id;
                                            self.next_request_id += 1;
                                            self.pending_requests
                                                .push_back((request_id, *client_id, req));
                                            work_units += 1;
                                            context.remaining_budget -= 1;
                                            debug!(
                                                "parsed complete HTTP request from client {}",
                                                client_id
                                            );
                                        }
                                    } else {
                                        *current_request = None;
                                        debug!(
                                                "HTTP parse error for client {}, request will not be forwarded",
                                                client_id
                                            );
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

                ConnectionState::WritingResponse {
                    request_id,
                    response_state,
                    client_id,
                } => {
                    match response_state {
                        HttpResponseState::Pending => {
                            // Only start writing once the response for this specific request is available.
                            if let Some(response) = self.pending_responses.remove(request_id) {
                                *response_state = HttpResponseState::Writing { response, sent: 0 };
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!(
                                    "starting to write HTTP response for request {} to client {}",
                                    request_id, client_id
                                );
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
                                        warn!(
                                            "failed to write HTTP response to client {}: {}",
                                            client_id, e
                                        );
                                        connections_to_remove.push(*client_id);
                                    }
                                }
                            }
                        }
                        HttpResponseState::Complete => {
                            // Should not reach here in WritingResponse state
                            // This state should transition immediately to KeepAlive
                        }
                    }
                }

                ConnectionState::KeepAlive => {
                    // Connection is keep-alive, prepare to read another request
                    // Check if connection should be kept alive based on timeout
                    if connection.last_active.elapsed() > self.keep_alive_timeout {
                        // Connection timed out, close it
                        debug!("HTTP connection {} timed out, closing", client_id);
                        let new_state = ConnectionState::Closed;
                        state_changes.push((*client_id, new_state));
                        connections_to_remove.push(*client_id);
                    } else {
                        // Reset connection for next request
                        let new_state = ConnectionState::ReadingRequest {
                            parse_state: HttpParseState::RequestLine,
                            buffer: Vec::new(),
                            request: None,
                        };
                        state_changes.push((*client_id, new_state));
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("HTTP connection {} ready for next request", client_id);
                    }
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
            if let Some(connection) = self.connections.remove(&client_id) {
                if let ConnectionState::WritingResponse { request_id, .. } = connection.state {
                    self.response_wait_queue
                        .retain(|queued_id| *queued_id != request_id);
                    self.pending_responses.remove(&request_id);
                }
            }
            work_units += 1; // Count cleanup as work
            debug!("cleaned up HTTP connection {}", client_id);
        }

        // Send pending requests to FBP network
        while context.remaining_budget > 0 && !self.pending_requests.is_empty() {
            // Get the front item without borrowing
            let (request_id, client_id, request) = match self.pending_requests.front() {
                Some(item) => item.clone(),
                None => break,
            };

            // Route the request based on method and path with parameter extraction
            if let Some(routes) = &self.routes_config {
                let mut route_index = None;
                let mut extracted_params = HashMap::new();

                for (i, route) in routes.iter().enumerate() {
                    if request.method == route.method {
                        // Check for exact match first
                        if request.path == route.path {
                            route_index = Some(i);
                            break;
                        }

                        // Check for parameter match (e.g., /users/{id})
                        if let Some(params) = match_route_with_params(&request.path, &route.path) {
                            route_index = Some(i);
                            extracted_params = params;
                            break;
                        }
                    }
                }

                let mut request_to_send = request.clone();
                if !extracted_params.is_empty() {
                    request_to_send.path_params = extracted_params.clone();
                }

                if let Some(index) = route_index {
                    if index < self.req.len() {
                        // Contract for REQ payload is a newline-delimited request envelope.
                        match self.req[index].push(encode_request_for_req_port(&request_to_send)) {
                            Ok(()) => {
                                self.pending_requests.pop_front();
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!(
                                    "sent HTTP request {} to FBP network for route {}",
                                    request_id, index
                                );
                                self.response_wait_queue.push_back(request_id);

                                // Mark connection as waiting for response
                                if let Some(conn) = self.connections.get_mut(&client_id) {
                                    conn.state = ConnectionState::WritingResponse {
                                        request_id,
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
                        self.pending_responses.insert(request_id, response);
                        if let Some(conn) = self.connections.get_mut(&client_id) {
                            conn.state = ConnectionState::WritingResponse {
                                request_id,
                                response_state: HttpResponseState::Pending,
                                client_id,
                            };
                        }
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
                    self.pending_responses.insert(request_id, response);
                    if let Some(conn) = self.connections.get_mut(&client_id) {
                        conn.state = ConnectionState::WritingResponse {
                            request_id,
                            response_state: HttpResponseState::Pending,
                            client_id,
                        };
                    }
                }
            }
        }

        // Read responses from FBP network
        while context.remaining_budget > 0 {
            if let Ok(response_body) = self.resp.pop() {
                if let Some(request_id) = self.response_wait_queue.pop_front() {
                    let response = HttpResponse {
                        status_code: 200,
                        status_text: "OK".to_string(),
                        headers: HashMap::new(),
                        body: response_body,
                    };
                    self.pending_responses.insert(request_id, response);
                    work_units += 1;
                    context.remaining_budget -= 1;
                    debug!(
                        "received response from FBP network for request {}",
                        request_id
                    );
                } else {
                    warn!("received response from FBP network but no request is waiting for a response");
                    break;
                }
            } else {
                break;
            }
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
                                                match out_req_one.push(body.to_vec()) {
                                                    Ok(()) => true,
                                                    Err(flowd_component_api::PushError::Full(_)) => {
                                                        false
                                                    }
                                                }
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
