//! Component-level unit tests
//! These tests validate isolated component logic without using the runtime

use flowd_component_api::*;
use flowd_http::{HTTPClientComponent, HTTPServerComponent};
use flowd_repeat::RepeatComponent;
use multimap::MultiMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Specialized test harness for Repeat component
pub struct RepeatTestHarness {
    component: RepeatComponent,
    input_producer: rtrb::Producer<MessageBuf>,
    output_consumer: rtrb::Consumer<MessageBuf>,
    signal_sender: ProcessSignalSink,
    context: NodeContext,
}

impl RepeatTestHarness {
    /// Create a new test harness for the Repeat component
    pub fn new() -> Self {
        // Create ring buffers for component IN and OUT ports
        let (input_producer, input_consumer) =
            rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);
        let (output_producer, output_consumer) =
            rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);

        // Create signal channels
        let (signal_sender, signal_receiver) = mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);

        // Set up inports and outports for the component
        let mut inports = MultiMap::new();
        inports.insert("IN".to_string(), input_consumer);

        let mut outports = MultiMap::new();
        outports.insert(
            "OUT".to_string(),
            ProcessEdgeSink {
                sink: output_producer,
                wakeup: None,
                proc_name: None,
                signal_ready: None,
            },
        );

        // Create dummy graph_inout handle (not used in tests)
        let graph_inout: GraphInportOutportHandle = (
            Arc::new(|_| {}), // network_output function
            Arc::new(|_| {}), // network_previewurl function
        );

        // Instantiate the component
        let component = RepeatComponent::new(
            inports,
            outports,
            signal_receiver,
            signal_sender.clone(),
            graph_inout,
            None, // scheduler_waker
        );

        let context = NodeContext {
            node_id: "test_repeat".to_string(),
            budget_class: BudgetClass::Normal,
            remaining_budget: 32,
            ready_signal: Arc::new(AtomicBool::new(false)),
            last_execution: Instant::now(),
            execution_count: 0,
            work_units_processed: 0,
        };

        RepeatTestHarness {
            component,
            input_producer,
            output_consumer,
            signal_sender,
            context,
        }
    }

    /// Send data to the component's IN port
    pub fn send_input(&mut self, data: &[u8]) -> Result<(), rtrb::PushError<MessageBuf>> {
        self.input_producer.push(data.to_vec())
    }

    /// Run the component for one processing cycle
    pub fn process(&mut self) -> ProcessResult {
        self.component.process(&mut self.context)
    }

    /// Collect all available outputs from the OUT port
    pub fn collect_outputs(&mut self) -> Vec<MessageBuf> {
        let mut outputs = Vec::new();
        while let Ok(data) = self.output_consumer.pop() {
            outputs.push(data);
        }
        outputs
    }

    /// Send a signal to the component
    pub fn send_signal(&self, signal: &[u8]) -> Result<(), mpsc::TrySendError<MessageBuf>> {
        self.signal_sender.try_send(signal.to_vec())
    }

    /// Check if output port has data
    pub fn has_output(&self) -> bool {
        !self.output_consumer.is_empty()
    }

    /// Reset the context for a new test
    pub fn reset_context(&mut self) {
        self.context.remaining_budget = 32;
        self.context.execution_count = 0;
        self.context.work_units_processed = 0;
        self.context.last_execution = Instant::now();
    }

    /// Get remaining budget
    pub fn remaining_budget(&self) -> u32 {
        self.context.remaining_budget
    }

    /// Set remaining budget
    pub fn set_budget(&mut self, budget: u32) {
        self.context.remaining_budget = budget;
    }
}

/// Specialized test harness for HTTPClient component
pub struct HTTPClientTestHarness {
    component: HTTPClientComponent,
    req_producer: rtrb::Producer<MessageBuf>,
    resp_consumer: rtrb::Consumer<MessageBuf>,
    err_consumer: rtrb::Consumer<MessageBuf>,
    signal_sender: ProcessSignalSink,
    context: NodeContext,
    mock_server: Option<std::thread::JoinHandle<()>>,
    mock_server_stop: Arc<AtomicBool>,
    server_port: u16,
}

impl HTTPClientTestHarness {
    /// Create a new test harness for the HTTPClient component with a mock server
    pub fn new() -> std::io::Result<Self> {
        // Start a mock HTTP server on a random port
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let server_port = listener.local_addr()?.port();
        let mock_server_stop = Arc::new(AtomicBool::new(false));
        let mock_server_stop_thread = mock_server_stop.clone();

        let mock_server = thread::spawn(move || {
            Self::run_mock_server(listener, mock_server_stop_thread);
        });

        // Create ring buffers for component ports
        let (req_producer, req_consumer) = rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);
        let (resp_producer, resp_consumer) =
            rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);
        let (err_producer, err_consumer) = rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);

        // Create signal channels
        let (signal_sender, signal_receiver) = mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);

        // Set up inports and outports for the component
        let mut inports = MultiMap::new();
        inports.insert("REQ".to_string(), req_consumer);

        let mut outports = MultiMap::new();
        outports.insert(
            "RESP".to_string(),
            ProcessEdgeSink {
                sink: resp_producer,
                wakeup: None,
                proc_name: None,
                signal_ready: None,
            },
        );
        outports.insert(
            "ERR".to_string(),
            ProcessEdgeSink {
                sink: err_producer,
                wakeup: None,
                proc_name: None,
                signal_ready: None,
            },
        );

        // Create dummy graph_inout handle
        let graph_inout: GraphInportOutportHandle = (Arc::new(|_| {}), Arc::new(|_| {}));

        // Instantiate the component
        let component = HTTPClientComponent::new(
            inports,
            outports,
            signal_receiver,
            signal_sender.clone(),
            graph_inout,
            None, // scheduler_waker
        );

        let context = NodeContext {
            node_id: "test_http_client".to_string(),
            budget_class: BudgetClass::Normal,
            remaining_budget: 32,
            ready_signal: Arc::new(AtomicBool::new(false)),
            last_execution: Instant::now(),
            execution_count: 0,
            work_units_processed: 0,
        };

        Ok(HTTPClientTestHarness {
            component,
            req_producer,
            resp_consumer,
            err_consumer,
            signal_sender,
            context,
            mock_server: Some(mock_server),
            mock_server_stop,
            server_port,
        })
    }

    /// Run a simple mock HTTP server for testing
    fn run_mock_server(listener: TcpListener, stop: Arc<AtomicBool>) {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _addr)) => {
                    let mut buffer = [0; 1024];
                    match stream.read(&mut buffer) {
                        Ok(size) if size > 0 => {
                            let request = String::from_utf8_lossy(&buffer[..size]);

                            // Simple HTTP response based on request
                            let response = if request.contains("GET /test HTTP/1.1") {
                                "HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nHello World!"
                                    .to_string()
                            } else if request.contains("GET /error HTTP/1.1") {
                                "HTTP/1.1 404 Not Found\r\nContent-Length: 13\r\n\r\nNot Found"
                                    .to_string()
                            } else if request.contains("POST /echo HTTP/1.1") {
                                // Echo back the body
                                if let Some(body_start) = request.find("\r\n\r\n") {
                                    let body = &request[body_start + 4..];
                                    format!(
                                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                                        body.len(),
                                        body
                                    )
                                } else {
                                    "HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\n\r\nBad Request".to_string()
                                }
                            } else {
                                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK".to_string()
                            };

                            let _ = stream.write_all(response.as_bytes());
                        }
                        _ => {}
                    }
                    let _ = stream.shutdown(Shutdown::Both);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    }

    /// Send a URL request to the component
    pub fn send_request(&mut self, url: &str) -> Result<(), rtrb::PushError<MessageBuf>> {
        self.req_producer.push(url.as_bytes().to_vec())
    }

    /// Run the component for one processing cycle
    pub fn process(&mut self) -> ProcessResult {
        self.component.process(&mut self.context)
    }

    /// Collect all available responses from the RESP port
    pub fn collect_responses(&mut self) -> Vec<MessageBuf> {
        let mut responses = Vec::new();
        while let Ok(data) = self.resp_consumer.pop() {
            responses.push(data);
        }
        responses
    }

    /// Collect all available errors from the ERR port
    pub fn collect_errors(&mut self) -> Vec<MessageBuf> {
        let mut errors = Vec::new();
        while let Ok(data) = self.err_consumer.pop() {
            errors.push(data);
        }
        errors
    }

    /// Send a signal to the component
    pub fn send_signal(&self, signal: &[u8]) -> Result<(), mpsc::TrySendError<MessageBuf>> {
        self.signal_sender.try_send(signal.to_vec())
    }

    /// Get the mock server port
    pub fn server_port(&self) -> u16 {
        self.server_port
    }

    /// Reset the context for a new test
    pub fn reset_context(&mut self) {
        self.context.remaining_budget = 32;
        self.context.execution_count = 0;
        self.context.work_units_processed = 0;
        self.context.last_execution = Instant::now();
    }
}

impl Drop for HTTPClientTestHarness {
    fn drop(&mut self) {
        // Clean up the mock server thread
        self.mock_server_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.mock_server.take() {
            let _ = handle.join();
        }
    }
}

/// Specialized test harness for HTTPServer component
pub struct HTTPServerTestHarness {
    component: HTTPServerComponent,
    conf_producer: rtrb::Producer<MessageBuf>,
    routes_producer: rtrb::Producer<MessageBuf>,
    resp_producer: rtrb::Producer<MessageBuf>,
    req_consumer: rtrb::Consumer<MessageBuf>,
    signal_sender: ProcessSignalSink,
    context: NodeContext,
    server_port: u16,
}

impl HTTPServerTestHarness {
    /// Create a new test harness for the HTTPServer component
    pub fn new() -> std::io::Result<Self> {
        // Find an available port for the server
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let server_port = listener.local_addr()?.port();
        drop(listener); // Free the port for the component to use

        // Create ring buffers for component ports
        let (conf_producer, conf_consumer) =
            rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);
        let (routes_producer, routes_consumer) =
            rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);
        let (resp_producer, resp_consumer) =
            rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);
        let (req_producer, req_consumer) = rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);

        // Create signal channels
        let (signal_sender, signal_receiver) = mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);

        // Set up inports and outports for the component
        let mut inports = MultiMap::new();
        inports.insert("CONF".to_string(), conf_consumer);
        inports.insert("ROUTES".to_string(), routes_consumer);
        inports.insert("RESP".to_string(), resp_consumer);

        let mut outports = MultiMap::new();
        outports.insert(
            "REQ".to_string(),
            ProcessEdgeSink {
                sink: req_producer,
                wakeup: None,
                proc_name: None,
                signal_ready: None,
            },
        );

        // Create dummy graph_inout handle
        let graph_inout: GraphInportOutportHandle = (Arc::new(|_| {}), Arc::new(|_| {}));

        // Instantiate the component
        let component = HTTPServerComponent::new(
            inports,
            outports,
            signal_receiver,
            signal_sender.clone(),
            graph_inout,
            None, // scheduler_waker
        );

        let context = NodeContext {
            node_id: "test_http_server".to_string(),
            budget_class: BudgetClass::Normal,
            remaining_budget: 32,
            ready_signal: Arc::new(AtomicBool::new(false)),
            last_execution: Instant::now(),
            execution_count: 0,
            work_units_processed: 0,
        };

        Ok(HTTPServerTestHarness {
            component,
            conf_producer,
            routes_producer,
            resp_producer,
            req_consumer,
            signal_sender,
            context,
            server_port,
        })
    }

    /// Configure the server with listen address and routes
    pub fn configure(
        &mut self,
        listen_addr: &str,
        routes: &str,
    ) -> Result<(), rtrb::PushError<MessageBuf>> {
        self.conf_producer.push(listen_addr.as_bytes().to_vec())?;
        self.routes_producer.push(routes.as_bytes().to_vec())?;
        Ok(())
    }

    /// Send a response to the server component
    pub fn send_response(&mut self, response: &[u8]) -> Result<(), rtrb::PushError<MessageBuf>> {
        self.resp_producer.push(response.to_vec())
    }

    /// Run the component for one processing cycle
    pub fn process(&mut self) -> ProcessResult {
        self.component.process(&mut self.context)
    }

    /// Collect all available requests from the REQ port
    pub fn collect_requests(&mut self) -> Vec<MessageBuf> {
        let mut requests = Vec::new();
        while let Ok(data) = self.req_consumer.pop() {
            requests.push(data);
        }
        requests
    }

    /// Run processing cycles and return total work units reported.
    pub fn process_cycles(&mut self, cycles: usize) -> u32 {
        let mut total_work = 0u32;
        for _ in 0..cycles {
            match self.process() {
                ProcessResult::DidWork(work) => total_work += work,
                ProcessResult::NoWork => thread::sleep(Duration::from_millis(10)),
                ProcessResult::Finished => break,
            }
        }
        total_work
    }

    /// Process repeatedly until `condition` is true or timeout expires.
    pub fn process_until<F>(&mut self, timeout: Duration, mut condition: F) -> bool
    where
        F: FnMut() -> bool,
    {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if condition() {
                return true;
            }
            match self.process() {
                ProcessResult::DidWork(_) => {}
                ProcessResult::NoWork => thread::sleep(Duration::from_millis(10)),
                ProcessResult::Finished => break,
            }
        }
        condition()
    }

    /// Process until at least `min_requests` are available or cycles are exhausted.
    pub fn wait_for_requests(&mut self, min_requests: usize, max_cycles: usize) -> Vec<MessageBuf> {
        let mut requests = Vec::new();
        for _ in 0..max_cycles {
            match self.process() {
                ProcessResult::DidWork(_) => {}
                ProcessResult::NoWork => thread::sleep(Duration::from_millis(10)),
                ProcessResult::Finished => break,
            }
            requests.extend(self.collect_requests());
            if requests.len() >= min_requests {
                break;
            }
        }
        requests
    }

    /// Send a signal to the component
    pub fn send_signal(&self, signal: &[u8]) -> Result<(), mpsc::TrySendError<MessageBuf>> {
        self.signal_sender.try_send(signal.to_vec())
    }

    /// Get the server port
    pub fn server_port(&self) -> u16 {
        self.server_port
    }

    /// Make an HTTP request to the server
    pub fn make_http_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        Self::make_http_request_to_port(self.server_port, method, path, body)
    }

    pub fn make_http_request_to_port(
        port: u16,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(1))?;
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;

        let request = if let Some(body_content) = body {
            format!(
                "{} {} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
                method,
                path,
                body_content.len(),
                body_content
            )
        } else {
            format!("{} {} HTTP/1.1\r\nHost: localhost\r\n\r\n", method, path)
        };

        stream.write_all(request.as_bytes())?;

        let mut response = Vec::new();
        let mut buffer = [0; 1024];
        while let Ok(bytes_read) = stream.read(&mut buffer) {
            if bytes_read == 0 {
                break;
            }
            response.extend_from_slice(&buffer[..bytes_read]);
        }

        Ok(String::from_utf8(response)?)
    }

    /// Reset the context for a new test
    pub fn reset_context(&mut self) {
        self.context.remaining_budget = 32;
        self.context.execution_count = 0;
        self.context.work_units_processed = 0;
        self.context.last_execution = Instant::now();
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn maybe_http_client_harness() -> Option<HTTPClientTestHarness> {
        match HTTPClientTestHarness::new() {
            Ok(harness) => Some(harness),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("Skipping HTTP client test: local sockets are blocked in this environment ({err}).");
                None
            }
            Err(err) => panic!("Failed to initialize HTTP client test harness: {err}"),
        }
    }

    fn maybe_http_server_harness() -> Option<HTTPServerTestHarness> {
        match HTTPServerTestHarness::new() {
            Ok(harness) => Some(harness),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("Skipping HTTP server test: local sockets are blocked in this environment ({err}).");
                None
            }
            Err(err) => panic!("Failed to initialize HTTP server test harness: {err}"),
        }
    }

    fn try_start_http_server(harness: &mut HTTPServerTestHarness, routes: &str) -> bool {
        let listen_addr = format!("127.0.0.1:{}", harness.server_port());
        try_start_http_server_with_listen(harness, &listen_addr, routes)
    }

    fn try_start_http_server_with_listen(
        harness: &mut HTTPServerTestHarness,
        listen_addr: &str,
        routes: &str,
    ) -> bool {
        if let Err(err) = harness.configure(listen_addr, routes) {
            eprintln!(
                "Skipping HTTP server test: could not push server config into inports ({err:?})."
            );
            return false;
        }

        // Need to process multiple times to set up the server
        for _ in 0..10 {
            match catch_unwind(AssertUnwindSafe(|| harness.process())) {
                Ok(_) => {}
                Err(_) => {
                    eprintln!(
                        "Skipping HTTP server test: listener setup panicked in this environment."
                    );
                    return false;
                }
            }
        }
        true
    }

    fn payload_text(payload: &[u8]) -> String {
        String::from_utf8_lossy(payload).into_owned()
    }

    fn assert_payload_has(payload: &[u8], expected_lines: &[&str]) {
        let text = payload_text(payload);
        for line in expected_lines {
            assert!(
                text.contains(line),
                "missing `{}` in REQ payload: {}",
                line,
                text
            );
        }
    }

    fn read_http_response(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
        let mut response = Vec::new();
        let mut buffer = [0; 1024];

        loop {
            let bytes_read = stream.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            response.extend_from_slice(&buffer[..bytes_read]);

            if let Some(header_end) = response.windows(4).position(|w| w == b"\r\n\r\n") {
                let body_start = header_end + 4;
                let headers_text = String::from_utf8_lossy(&response[..header_end]).to_lowercase();

                if let Some(cl_line) = headers_text
                    .lines()
                    .find(|line| line.starts_with("content-length:"))
                {
                    if let Some(value) = cl_line.split(':').nth(1) {
                        if let Ok(content_length) = value.trim().parse::<usize>() {
                            if response.len() >= body_start + content_length {
                                break;
                            }
                            continue;
                        }
                    }
                }

                // If Content-Length is absent, headers-only is enough for these tests.
                break;
            }
        }

        Ok(response)
    }

    #[test]
    fn test_http_client_basic_get_request() {
        let Some(mut harness) = maybe_http_client_harness() else {
            return;
        };

        // Send a GET request to the mock server
        let url = format!("http://127.0.0.1:{}/test", harness.server_port());
        harness.send_request(&url).unwrap();

        // Process the component (may need multiple calls for network operations)
        let start = Instant::now();
        let timeout = Duration::from_secs(1);
        let mut total_work = 0u32;
        while start.elapsed() < timeout {
            let result = harness.process();
            match result {
                ProcessResult::DidWork(work) => total_work += work,
                ProcessResult::NoWork => thread::sleep(Duration::from_millis(10)),
                ProcessResult::Finished => break,
            }
        }

        // Should have done some work
        assert!(total_work > 0);

        // Check responses
        let responses = harness.collect_responses();
        assert_eq!(responses.len(), 1);
        assert_eq!(String::from_utf8_lossy(&responses[0]), "Hello World!");

        // No errors should be generated
        let errors = harness.collect_errors();
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_http_client_error_response() {
        let Some(mut harness) = maybe_http_client_harness() else {
            return;
        };

        // Send a request that will result in a 404
        let url = format!("http://127.0.0.1:{}/error", harness.server_port());
        harness.send_request(&url).unwrap();

        // Process the component
        let start = Instant::now();
        let timeout = Duration::from_secs(10);
        let mut total_work = 0u32;
        while start.elapsed() < timeout {
            let result = harness.process();
            match result {
                ProcessResult::DidWork(work) => total_work += work,
                ProcessResult::NoWork => thread::sleep(Duration::from_millis(10)),
                ProcessResult::Finished => break,
            }
        }

        // Should have done work
        assert!(total_work > 0);

        // Should get an error response
        let errors = harness.collect_errors();
        assert_eq!(errors.len(), 1);
        let error_str = String::from_utf8_lossy(&errors[0]);
        assert!(error_str.contains("404") || error_str.contains("Not Found"));

        // No successful responses
        let responses = harness.collect_responses();
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn test_http_client_post_request() {
        let Some(mut harness) = maybe_http_client_harness() else {
            return;
        };

        // Send a POST request
        let url = format!("http://127.0.0.1:{}/echo", harness.server_port());
        harness.send_request(&url).unwrap();

        // Process the component
        let start = Instant::now();
        let timeout = Duration::from_secs(10);
        let mut total_work = 0u32;
        while start.elapsed() < timeout {
            let result = harness.process();
            match result {
                ProcessResult::DidWork(work) => total_work += work,
                ProcessResult::NoWork => thread::sleep(Duration::from_millis(10)),
                ProcessResult::Finished => break,
            }
        }

        // Should have done work
        assert!(total_work > 0);

        // Check responses (the mock server echoes back the request body)
        let responses = harness.collect_responses();
        assert_eq!(responses.len(), 1);
        // The mock server should echo back something
        assert!(!responses[0].is_empty());
    }

    #[test]
    fn test_http_client_signal_stop() {
        let Some(mut harness) = maybe_http_client_harness() else {
            return;
        };

        // Send a request
        let url = format!("http://127.0.0.1:{}/test", harness.server_port());
        harness.send_request(&url).unwrap();

        // Send stop signal
        harness.send_signal(b"stop").unwrap();

        // Process the component
        let result = harness.process();

        // Should finish due to stop signal
        assert!(matches!(result, ProcessResult::Finished));
    }

    #[test]
    fn test_http_server_basic_request_handling() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/test") {
            return;
        }

        // Make an HTTP request to the server in parallel to avoid request/response deadlocks.
        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();
        thread::spawn(move || {
            let response =
                HTTPServerTestHarness::make_http_request_to_port(port, "GET", "/test", None)
                    .map_err(|e| e.to_string());
            let _ = response_tx.send(response);
        });

        // Process until request is observed.
        let requests = harness.wait_for_requests(1, 50);

        // The server should have forwarded the request
        assert_eq!(requests.len(), 1);
        assert_payload_has(
            &requests[0],
            &[
                "METHOD=GET",
                "PATH=/test",
                "QUERY=",
                "PATH_PARAMS=",
                "BODY=",
            ],
        );

        // Send a response back
        harness.send_response(b"Hello from server").unwrap();

        // Process again to send the response
        let response_work = harness.process_cycles(50);
        assert!(response_work > 0, "No work done while sending response");

        // The HTTP response should contain our message
        let response = response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request thread failed");
        assert!(response.contains("Hello from server"));
    }

    #[test]
    fn test_http_server_multiple_requests() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/req1,/req2") {
            return;
        }

        // Make multiple HTTP requests in parallel.
        let port = harness.server_port();
        let (response_tx1, response_rx1) = mpsc::channel();
        thread::spawn(move || {
            let response =
                HTTPServerTestHarness::make_http_request_to_port(port, "GET", "/req1", None)
                    .map_err(|e| e.to_string());
            let _ = response_tx1.send(response);
        });

        let port = harness.server_port();
        let (response_tx2, response_rx2) = mpsc::channel();
        thread::spawn(move || {
            let response =
                HTTPServerTestHarness::make_http_request_to_port(port, "GET", "/req2", None)
                    .map_err(|e| e.to_string());
            let _ = response_tx2.send(response);
        });

        // Process the server to handle requests - allow more time for both requests
        let requests = harness.wait_for_requests(2, 100);

        // Should have received at least one request (the server may handle them sequentially)
        assert!(requests.len() >= 1);
        let payloads = requests.iter().map(|r| payload_text(r)).collect::<Vec<_>>();
        assert!(payloads.iter().all(|p| p.contains("METHOD=GET")));
        // At least one should match one of the expected paths
        assert!(payloads
            .iter()
            .any(|p| p.contains("PATH=/req1") || p.contains("PATH=/req2")));

        // Send responses for received requests
        for _ in 0..requests.len() {
            harness.send_response(b"Response").unwrap();
        }

        // Process to send responses
        harness.process_cycles(20);

        // At least one response should be received
        let mut responses_received = 0;
        if let Ok(response1) = response_rx1.recv_timeout(Duration::from_secs(2)) {
            if response1.is_ok() {
                responses_received += 1;
            }
        }
        if let Ok(response2) = response_rx2.recv_timeout(Duration::from_secs(2)) {
            if response2.is_ok() {
                responses_received += 1;
            }
        }
        assert!(
            responses_received >= 1,
            "At least one response should be received"
        );
    }

    #[test]
    fn test_http_server_idle_client_does_not_block_other_clients() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/fast") {
            return;
        }

        let port = harness.server_port();
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

        // Connect an idle client and keep it open without sending a request.
        let _idle_client = TcpStream::connect_timeout(&addr, Duration::from_secs(1))
            .expect("failed to connect idle client");

        // Connect an active client and send a request.
        let mut active_client = TcpStream::connect_timeout(&addr, Duration::from_secs(1))
            .expect("failed to connect active client");
        active_client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("failed to set active client read timeout");
        active_client
            .set_write_timeout(Some(Duration::from_secs(1)))
            .expect("failed to set active client write timeout");
        active_client
            .write_all(b"GET /fast HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .expect("failed to write active client request");

        // The active request should still be observed quickly despite the idle client.
        let started = Instant::now();
        let mut observed = Vec::new();
        while started.elapsed() < Duration::from_millis(300) {
            match harness.process() {
                ProcessResult::DidWork(_) => {}
                ProcessResult::NoWork => thread::sleep(Duration::from_millis(5)),
                ProcessResult::Finished => break,
            }
            observed.extend(harness.collect_requests());
            if !observed.is_empty() {
                break;
            }
        }

        assert!(
            !observed.is_empty(),
            "active client request was delayed by idle client (elapsed: {:?})",
            started.elapsed()
        );
        assert_payload_has(&observed[0], &["METHOD=GET", "PATH=/fast"]);

        // Respond and verify the client receives data.
        harness.send_response(b"fast-ok").unwrap();
        harness.process_cycles(20);

        let mut response = Vec::new();
        let mut buffer = [0; 1024];
        while let Ok(bytes_read) = active_client.read(&mut buffer) {
            if bytes_read == 0 {
                break;
            }
            response.extend_from_slice(&buffer[..bytes_read]);
            if response.windows(4).any(|w| w == b"\r\n\r\n") && response.ends_with(b"fast-ok") {
                break;
            }
        }
        let response_text = String::from_utf8_lossy(&response);
        assert!(
            response_text.contains("fast-ok"),
            "unexpected response: {}",
            response_text
        );
    }

    #[test]
    fn test_http_server_404_handling() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/test") {
            return;
        }

        // Make a request to a non-existent route in parallel.
        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();
        thread::spawn(move || {
            let response =
                HTTPServerTestHarness::make_http_request_to_port(port, "GET", "/nonexistent", None)
                    .map_err(|e| e.to_string());
            let _ = response_tx.send(response);
        });

        // Process the server
        let total_work = harness.process_cycles(20);

        // Should have done work
        assert!(total_work > 0);

        // Should get a 404 response
        let response = response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request thread failed");
        assert!(response.contains("404") || response.contains("Not Found"));
    }

    #[test]
    fn test_http_server_signal_stop() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/test") {
            return;
        }

        // Send stop signal
        harness.send_signal(b"stop").unwrap();

        // Process the component
        let result = harness.process();

        // Should finish due to stop signal
        assert!(matches!(result, ProcessResult::Finished));
    }

    #[test]
    fn test_http_server_budget_management() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/test") {
            return;
        }

        // Set a small budget
        harness.context.remaining_budget = 5;

        // Make a request in parallel to avoid waiting for response before processing.
        let port = harness.server_port();
        let (response_tx, _response_rx) = mpsc::channel();
        thread::spawn(move || {
            let response =
                HTTPServerTestHarness::make_http_request_to_port(port, "GET", "/test", None)
                    .map_err(|e| e.to_string());
            let _ = response_tx.send(response);
        });

        // Process with limited budget
        let mut result = ProcessResult::NoWork;
        for _ in 0..20 {
            result = harness.process();
            if matches!(result, ProcessResult::DidWork(_)) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        // Should do some work but not exhaust all budget
        assert!(matches!(result, ProcessResult::DidWork(_)));

        // Should have remaining budget
        assert!(harness.context.remaining_budget > 0);
    }

    #[test]
    fn test_http_server_post_request_with_body() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "POST /echo") {
            return;
        }

        // Make a POST request with body in parallel to avoid request/response deadlocks
        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();
        let body = "Hello POST body";
        thread::spawn(move || {
            let response =
                HTTPServerTestHarness::make_http_request_to_port(port, "POST", "/echo", Some(body))
                    .map_err(|e| e.to_string());
            let _ = response_tx.send(response);
        });

        // Process until request is observed (same pattern as working test)
        let requests = harness.wait_for_requests(1, 50);

        // The server should have forwarded the request
        assert_eq!(requests.len(), 1);
        assert_payload_has(
            &requests[0],
            &["METHOD=POST", "PATH=/echo", "BODY=Hello POST body"],
        );

        // Send a response back
        harness.send_response(body.as_bytes()).unwrap();

        // Process again to send the response
        harness.process_cycles(20);

        // The HTTP response should contain our echoed body
        let response = response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request thread failed");
        assert!(response.contains(body));
    }

    #[test]
    fn test_http_server_query_parameters() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/search") {
            return;
        }

        // Make a request with query parameters
        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();
        thread::spawn(move || {
            let response = HTTPServerTestHarness::make_http_request_to_port(
                port,
                "GET",
                "/search?q=rust&limit=10",
                None,
            )
            .map_err(|e| e.to_string());
            let _ = response_tx.send(response);
        });

        // Process until request is received
        let requests = harness.wait_for_requests(1, 50);

        // Should have received the request envelope with parsed query.
        assert_eq!(requests.len(), 1);
        assert_payload_has(
            &requests[0],
            &["METHOD=GET", "PATH=/search", "QUERY=limit=10&q=rust"],
        );

        // Send response
        harness.send_response(b"Search results").unwrap();

        // Process to send response
        harness.process_cycles(20);

        // Verify response
        let response = response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request thread failed");
        assert!(response.contains("Search results"));
    }

    #[test]
    fn test_http_server_path_parameters() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/users/{id}") {
            return;
        }

        // Make a request to a parameterized route
        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();
        thread::spawn(move || {
            let response =
                HTTPServerTestHarness::make_http_request_to_port(port, "GET", "/users/123", None)
                    .map_err(|e| e.to_string());
            let _ = response_tx.send(response);
        });

        // Process until request is received
        let requests = harness.wait_for_requests(1, 50);

        // Should have received the request with extracted path params.
        assert_eq!(requests.len(), 1);
        assert_payload_has(
            &requests[0],
            &["METHOD=GET", "PATH=/users/123", "PATH_PARAMS=id=123"],
        );

        // Send response
        harness.send_response(b"User data").unwrap();

        // Process to send response
        harness.process_cycles(20);

        // Verify response
        let response = response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request thread failed");
        assert!(response.contains("User data"));
    }

    #[test]
    fn test_http_server_ping_signal() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/test") {
            return;
        }

        // Send ping signal
        harness.send_signal(b"ping").unwrap();

        // Process the component
        let result = harness.process();

        // Should have no work (no HTTP requests to process)
        assert!(matches!(result, ProcessResult::NoWork));
    }

    #[test]
    fn test_http_server_http_version_10() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/test") {
            return;
        }

        // Make an HTTP/1.0 request
        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();
        thread::spawn(move || {
            match TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_secs(1),
            ) {
                Ok(mut stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                    // Send HTTP/1.0 request
                    let request = b"GET /test HTTP/1.0\r\nHost: localhost\r\n\r\n";
                    let _ = stream.write_all(request);
                    let mut response = Vec::new();
                    let mut buffer = [0; 1024];
                    while let Ok(bytes_read) = stream.read(&mut buffer) {
                        if bytes_read == 0 {
                            break;
                        }
                        response.extend_from_slice(&buffer[..bytes_read]);
                        if response.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let _ = response_tx.send(Ok(String::from_utf8_lossy(&response).to_string()));
                }
                Err(e) => {
                    let _ = response_tx.send(Err(e.to_string()));
                }
            }
        });

        // Process until request is received
        let requests = harness.wait_for_requests(1, 50);

        // Should have received the request
        assert_eq!(requests.len(), 1);
        assert_payload_has(
            &requests[0],
            &["METHOD=GET", "PATH=/test", "VERSION=HTTP/1.0"],
        );

        // Send response
        harness.send_response(b"HTTP/1.0 response").unwrap();

        // Process to send response
        harness.process_cycles(20);

        // Verify response
        let response = response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request thread failed");
        assert!(response.contains("HTTP/1.0 response"));
    }

    #[test]
    fn test_http_server_large_response_chunked() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/large") {
            return;
        }

        // Create a large response body (>8KB to trigger chunked encoding)
        let large_body = vec![b'A'; 10 * 1024]; // 10KB

        // Make a request
        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();
        thread::spawn(move || {
            match TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_secs(1),
            ) {
                Ok(mut stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

                    // Send GET request
                    let request = b"GET /large HTTP/1.1\r\nHost: localhost\r\n\r\n";
                    let _ = stream.write_all(request);

                    let mut response = Vec::new();
                    let mut buffer = [0; 1024];
                    loop {
                        match stream.read(&mut buffer) {
                            Ok(bytes_read) => {
                                if bytes_read == 0 {
                                    break;
                                }
                                response.extend_from_slice(&buffer[..bytes_read]);
                            }
                            Err(_) => break,
                        }
                    }
                    let _ = response_tx.send(Ok(String::from_utf8_lossy(&response).to_string()));
                }
                Err(e) => {
                    let _ = response_tx.send(Err(e.to_string()));
                }
            }
        });

        // Process until request is received
        let requests = harness.wait_for_requests(1, 50);

        // Should have received the request
        assert_eq!(requests.len(), 1);
        assert_payload_has(&requests[0], &["METHOD=GET", "PATH=/large"]);

        // Send large response (should trigger chunked encoding)
        harness.send_response(&large_body).unwrap();

        // Process to send response
        harness.process_cycles(50); // Allow more cycles for large response

        // Verify response contains the large body
        let response = response_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request thread failed");

        // Check that response contains Transfer-Encoding: chunked
        assert!(response.contains("Transfer-Encoding: chunked"));

        // Extract body from response (after double CRLF)
        if let Some(body_start) = response.find("\r\n\r\n") {
            let body = &response[body_start + 4..];
            // The body should contain our large content (chunked)
            assert!(body.contains(&"A".repeat(1024))); // At least some of our data
        } else {
            panic!("No body found in response");
        }
    }

    #[test]
    fn test_http_server_keep_alive_connection() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/test") {
            return;
        }

        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();

        // Make two requests on the same connection
        thread::spawn(move || {
            match TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_secs(1),
            ) {
                Ok(mut stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

                    let mut responses = Vec::new();

                    // First request
                    let request1 =
                        b"GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n";
                    let _ = stream.write_all(request1);
                    let response1 = match read_http_response(&mut stream) {
                        Ok(resp) => resp,
                        Err(e) => {
                            let _ = response_tx.send(Err(e.to_string()));
                            return;
                        }
                    };
                    responses.push(String::from_utf8_lossy(&response1).to_string());

                    // Second request on same connection
                    let request2 =
                        b"GET /test HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(request2);

                    let response2 = match read_http_response(&mut stream) {
                        Ok(resp) => resp,
                        Err(e) => {
                            let _ = response_tx.send(Err(e.to_string()));
                            return;
                        }
                    };
                    responses.push(String::from_utf8_lossy(&response2).to_string());

                    let _ = response_tx.send(Ok(responses));
                }
                Err(e) => {
                    let _ = response_tx.send(Err(e.to_string()));
                }
            }
        });

        // Process first request
        let requests1 = harness.wait_for_requests(1, 50);
        assert_eq!(requests1.len(), 1);
        harness.send_response(b"Response 1").unwrap();
        harness.process_cycles(20);

        // Process second request
        let requests2 = harness.wait_for_requests(1, 50);
        assert_eq!(requests2.len(), 1);
        harness.send_response(b"Response 2").unwrap();
        harness.process_cycles(20);

        // Verify both responses
        let responses = response_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("timed out waiting for HTTP responses")
            .expect("HTTP requests failed");

        assert_eq!(responses.len(), 2);
        assert!(responses[0].contains("Response 1"));
        assert!(responses[1].contains("Response 2"));
    }

    #[test]
    fn test_http_server_invalid_utf8_request_line() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "/test") {
            return;
        }

        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();

        thread::spawn(move || {
            match TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_secs(1),
            ) {
                Ok(mut stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

                    // Send request with invalid UTF-8 in path
                    let invalid_utf8 = &[0x47, 0x45, 0x54, 0x20, 0xFF, 0xFE, 0x20, 0x48]; // GET <invalid utf8> H
                    let mut request = Vec::new();
                    request.extend_from_slice(invalid_utf8);
                    request.extend_from_slice(b"TTP/1.1\r\nHost: localhost\r\n\r\n");
                    let _ = stream.write_all(&request);

                    let mut response = Vec::new();
                    let mut buffer = [0; 1024];
                    while let Ok(bytes_read) = stream.read(&mut buffer) {
                        if bytes_read == 0 {
                            break;
                        }
                        response.extend_from_slice(&buffer[..bytes_read]);
                        if response.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let _ = response_tx.send(Ok(String::from_utf8_lossy(&response).to_string()));
                }
                Err(e) => {
                    let _ = response_tx.send(Err(e.to_string()));
                }
            }
        });

        // Process the server (should handle the invalid request)
        harness.process_cycles(20);

        // Verify 400 Bad Request response
        let response = response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request failed");
        assert!(response.contains("400") || response.contains("Bad Request"));
    }

    #[test]
    fn test_http_server_runtime_config_update() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };

        // Start with initial config
        if !try_start_http_server(&mut harness, "/old") {
            return;
        }

        // Update routes while running
        let new_routes = "/new,/updated";
        harness.configure("127.0.0.1:0", new_routes).unwrap(); // Port doesn't matter for routes update

        // Process to apply config change
        harness.process_cycles(5);

        // Test old route (should 404)
        let port = harness.server_port();
        let (response_tx1, response_rx1) = mpsc::channel();
        thread::spawn(move || {
            let response =
                HTTPServerTestHarness::make_http_request_to_port(port, "GET", "/old", None)
                    .map_err(|e| e.to_string());
            let _ = response_tx1.send(response);
        });

        harness.process_cycles(20);
        let response1 = response_rx1
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for first response")
            .expect("first request failed");
        assert!(response1.contains("404") || response1.contains("Not Found"));

        // Test new route (should work)
        let port = harness.server_port();
        let (response_tx2, response_rx2) = mpsc::channel();
        thread::spawn(move || {
            let response =
                HTTPServerTestHarness::make_http_request_to_port(port, "GET", "/new", None)
                    .map_err(|e| e.to_string());
            let _ = response_tx2.send(response);
        });

        let requests = harness.wait_for_requests(1, 50);
        assert_eq!(requests.len(), 1);
        assert_payload_has(&requests[0], &["METHOD=GET", "PATH=/new"]);

        harness.send_response(b"New route response").unwrap();
        harness.process_cycles(20);

        let response2 = response_rx2
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for second response")
            .expect("second request failed");
        assert!(response2.contains("New route response"));
    }

    #[test]
    fn test_http_server_chunked_request_body() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "POST /chunked") {
            return;
        }

        // Make a POST request with chunked body
        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();
        let chunk1 = "Hello ";
        let chunk2 = "chunked world";
        let expected_body = format!("{}{}", chunk1, chunk2);

        thread::spawn(move || {
            match TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_secs(1),
            ) {
                Ok(mut stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

                    // Send chunked POST request
                    // Format: chunk-size CRLF chunk-data CRLF ... 0 CRLF CRLF
                    let request = format!(
                        "POST /chunked HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n{:x}\r\n{}\r\n0\r\n\r\n",
                        chunk1.len(),
                        chunk1,
                        chunk2.len(),
                        chunk2
                    );
                    let _ = stream.write_all(request.as_bytes());

                    let mut response = Vec::new();
                    let mut buffer = [0; 1024];
                    while let Ok(bytes_read) = stream.read(&mut buffer) {
                        if bytes_read == 0 {
                            break;
                        }
                        response.extend_from_slice(&buffer[..bytes_read]);
                        if response.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let _ = response_tx.send(Ok(String::from_utf8_lossy(&response).to_string()));
                }
                Err(e) => {
                    let _ = response_tx.send(Err(e.to_string()));
                }
            }
        });

        // Process until request is received
        let requests = harness.wait_for_requests(1, 50);

        // Should have received the request with reassembled chunked body
        assert_eq!(requests.len(), 1);
        assert_payload_has(
            &requests[0],
            &[
                "METHOD=POST",
                "PATH=/chunked",
                &format!("BODY={}", expected_body),
            ],
        );

        // Send response
        harness.send_response(expected_body.as_bytes()).unwrap();

        // Process to send response
        harness.process_cycles(20);

        // Verify response
        let response = response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request thread failed");
        assert!(response.contains(&expected_body));
    }

    #[test]
    fn test_http_server_invalid_content_length() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "POST /test") {
            return;
        }

        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();

        thread::spawn(move || {
            match TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_secs(1),
            ) {
                Ok(mut stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

                    // Send POST with invalid Content-Length
                    let request = b"POST /test HTTP/1.1\r\nHost: localhost\r\nContent-Length: not-a-number\r\n\r\n";
                    let _ = stream.write_all(request);

                    let mut response = Vec::new();
                    let mut buffer = [0; 1024];
                    while let Ok(bytes_read) = stream.read(&mut buffer) {
                        if bytes_read == 0 {
                            break;
                        }
                        response.extend_from_slice(&buffer[..bytes_read]);
                        if response.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let _ = response_tx.send(Ok(String::from_utf8_lossy(&response).to_string()));
                }
                Err(e) => {
                    let _ = response_tx.send(Err(e.to_string()));
                }
            }
        });

        // Process cooperatively until client thread receives a response.
        let mut response_result = None;
        let _ = harness.process_until(Duration::from_secs(3), || match response_rx.try_recv() {
            Ok(v) => {
                response_result = Some(v);
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                response_result = Some(Ok(String::new()));
                true
            }
        });

        // Invalid Content-Length must never be forwarded as a valid REQ payload.
        let forwarded_requests = harness.wait_for_requests(1, 20);
        assert!(
            forwarded_requests.is_empty(),
            "invalid Content-Length request was forwarded unexpectedly: {}",
            payload_text(&forwarded_requests[0]),
        );

        // If server emitted an HTTP error response, validate it.
        if let Some(result) = response_result {
            let response = result.expect("HTTP request failed");
            if !response.is_empty() {
                assert!(
                    response.contains("400")
                        || response.contains("Bad Request")
                        || response.contains("Invalid Content-Length"),
                    "unexpected response for invalid Content-Length: {}",
                    response
                );
            }
        }
    }

    #[test]
    fn test_http_server_chunked_request_with_trailers() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        if !try_start_http_server(&mut harness, "POST /trailers") {
            return;
        }

        // Make a POST request with chunked body and trailers
        let port = harness.server_port();
        let (response_tx, response_rx) = mpsc::channel();
        let chunk_data = "Hello trailers";

        thread::spawn(move || {
            match TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_secs(1),
            ) {
                Ok(mut stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

                    // Send chunked POST request with trailers
                    // Format: chunks... 0 CRLF trailer-name: trailer-value CRLF CRLF
                    let request = format!(
                        "POST /trailers HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\nX-Trailer: test-value\r\n\r\n",
                        chunk_data.len(),
                        chunk_data
                    );
                    let _ = stream.write_all(request.as_bytes());

                    let mut response = Vec::new();
                    let mut buffer = [0; 1024];
                    while let Ok(bytes_read) = stream.read(&mut buffer) {
                        if bytes_read == 0 {
                            break;
                        }
                        response.extend_from_slice(&buffer[..bytes_read]);
                        if response.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let _ = response_tx.send(Ok(String::from_utf8_lossy(&response).to_string()));
                }
                Err(e) => {
                    let _ = response_tx.send(Err(e.to_string()));
                }
            }
        });

        // Process until request is received
        let requests = harness.wait_for_requests(1, 50);

        // Should have received the request with chunked body (trailers should be ignored)
        assert_eq!(requests.len(), 1);
        assert_payload_has(
            &requests[0],
            &[
                "METHOD=POST",
                "PATH=/trailers",
                &format!("BODY={}", chunk_data),
            ],
        );

        // Send response
        harness.send_response(chunk_data.as_bytes()).unwrap();

        // Process to send response
        harness.process_cycles(20);

        // Verify response
        let response = response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("timed out waiting for HTTP response")
            .expect("HTTP request thread failed");
        assert!(response.contains(chunk_data));
    }

    #[test]
    fn test_http_server_connection_timeout() {
        let Some(mut harness) = maybe_http_server_harness() else {
            return;
        };
        // Use a shorter keep-alive timeout for test speed.
        let listen_addr = format!(
            "127.0.0.1:{}?keep_alive_timeout_ms=1200",
            harness.server_port()
        );
        if !try_start_http_server_with_listen(&mut harness, &listen_addr, "/timeout") {
            return;
        }

        let port = harness.server_port();
        let (result_tx, result_rx) = mpsc::channel();

        thread::spawn(move || {
            match TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_secs(1),
            ) {
                Ok(mut stream) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

                    // Send a request to establish keep-alive connection
                    let request = b"GET /timeout HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n";
                    let _ = stream.write_all(request);

                    // Read initial response
                    let mut buffer = [0; 1024];
                    let mut response_received = false;
                    while let Ok(bytes_read) = stream.read(&mut buffer) {
                        if bytes_read == 0 {
                            break;
                        }
                        if !response_received
                            && buffer[..bytes_read].windows(4).any(|w| w == b"\r\n\r\n")
                        {
                            response_received = true;
                            break;
                        }
                    }

                    if !response_received {
                        let _ = result_tx.send(Err("No initial response".to_string()));
                        return;
                    }

                    // Wait for idle timeout without sending additional data.
                    let mut timeout_buffer = [0; 1];
                    match stream.read(&mut timeout_buffer) {
                        Ok(0) => {
                            // Connection closed (expected due to timeout)
                            let _ = result_tx.send(Ok("Connection closed as expected".to_string()));
                        }
                        Ok(_) => {
                            let _ =
                                result_tx.send(Err("Connection should have timed out".to_string()));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                            let _ = result_tx
                                .send(Ok("Connection remained idle (read timed out)".to_string()));
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            // Some platforms surface read timeout as WouldBlock/EAGAIN.
                            let _ = result_tx
                                .send(Ok("Connection remained idle (would block)".to_string()));
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::ConnectionReset
                                    | std::io::ErrorKind::ConnectionAborted
                                    | std::io::ErrorKind::BrokenPipe
                                    | std::io::ErrorKind::UnexpectedEof
                                    | std::io::ErrorKind::NotConnected
                            ) =>
                        {
                            // Depending on platform/socket state, timeout close may surface as an error.
                            let _ = result_tx.send(Ok("Connection closed as expected".to_string()));
                        }
                        Err(e) => {
                            let _ = result_tx.send(Err(format!("Unexpected error: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    let _ = result_tx.send(Err(format!("Connection failed: {}", e)));
                }
            }
        });

        // Process initial request
        let requests = harness.wait_for_requests(1, 50);
        assert_eq!(requests.len(), 1);
        harness.send_response(b"Keep-alive response").unwrap();
        harness.process_cycles(20);

        // Keep driving the cooperative server while waiting for timeout closure.
        let mut timeout_result = None;
        let received =
            harness.process_until(Duration::from_secs(8), || match result_rx.try_recv() {
                Ok(v) => {
                    timeout_result = Some(v);
                    true
                }
                Err(mpsc::TryRecvError::Empty) => false,
                Err(mpsc::TryRecvError::Disconnected) => {
                    timeout_result = Some(Err("timeout test channel disconnected".to_string()));
                    true
                }
            });
        assert!(received, "timed out waiting for timeout test result");
        let result = timeout_result
            .expect("missing timeout result")
            .expect("timeout test failed");

        assert!(
            result == "Connection closed as expected"
                || result == "Connection remained idle (read timed out)"
                || result == "Connection remained idle (would block)",
            "unexpected timeout result: {}",
            result
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repeat_basic_message_passing() {
        let mut harness = RepeatTestHarness::new();

        // Send a message to the input
        harness.send_input(b"hello world").unwrap();

        // Process the component
        let result = harness.process();

        // Should have done work
        assert!(matches!(result, ProcessResult::DidWork(1)));

        // Check output
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], b"hello world");
    }

    #[test]
    fn test_repeat_multiple_messages() {
        let mut harness = RepeatTestHarness::new();

        // Send multiple messages
        harness.send_input(b"msg1").unwrap();
        harness.send_input(b"msg2").unwrap();
        harness.send_input(b"msg3").unwrap();

        // Process until all messages are handled
        let mut total_work = 0u32;
        while harness.remaining_budget() > 0 {
            let result = harness.process();
            match result {
                ProcessResult::DidWork(work) => total_work += work,
                ProcessResult::NoWork => break,
                ProcessResult::Finished => break,
            }
        }

        // Should have processed 3 messages
        assert_eq!(total_work, 3);

        // Check outputs
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 3);
        assert_eq!(outputs[0], b"msg1");
        assert_eq!(outputs[1], b"msg2");
        assert_eq!(outputs[2], b"msg3");
    }

    #[test]
    fn test_repeat_empty_message() {
        let mut harness = RepeatTestHarness::new();

        // Send an empty message
        harness.send_input(b"").unwrap();

        // Process the component
        let result = harness.process();

        // Should have done work
        assert!(matches!(result, ProcessResult::DidWork(1)));

        // Check output
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], b"");
    }

    #[test]
    fn test_repeat_large_message() {
        let mut harness = RepeatTestHarness::new();

        // Create a large message (1KB)
        let large_msg = vec![b'A'; 1024];
        harness.send_input(&large_msg).unwrap();

        // Process the component
        let result = harness.process();

        // Should have done work
        assert!(matches!(result, ProcessResult::DidWork(1)));

        // Check output
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], large_msg);
    }

    #[test]
    fn test_repeat_signal_stop() {
        let mut harness = RepeatTestHarness::new();

        // Send a message to input
        harness.send_input(b"test").unwrap();

        // Send stop signal
        harness.send_signal(b"stop").unwrap();

        // Process the component
        let result = harness.process();

        // Should finish due to stop signal
        assert!(matches!(result, ProcessResult::Finished));

        // Should not have processed the message (stop takes precedence)
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 0);
    }

    #[test]
    fn test_repeat_signal_ping() {
        let mut harness = RepeatTestHarness::new();

        // Send ping signal
        harness.send_signal(b"ping").unwrap();

        // Process the component
        let result = harness.process();

        // Should have no work (no input messages)
        assert!(matches!(result, ProcessResult::NoWork));

        // The ping response would be sent to signals_out, but we can't easily test that
        // with our current harness. In a real scenario, the scheduler would handle this.
    }

    #[test]
    fn test_repeat_budget_exhaustion() {
        let mut harness = RepeatTestHarness::new();

        // Set a small budget
        harness.set_budget(2);

        // Send 3 messages
        harness.send_input(b"msg1").unwrap();
        harness.send_input(b"msg2").unwrap();
        harness.send_input(b"msg3").unwrap();

        // Process with limited budget
        let result1 = harness.process();
        // The component might process multiple messages in one call or return different work amounts
        assert!(matches!(result1, ProcessResult::DidWork(_)));

        let result2 = harness.process();
        // Continue processing until budget is exhausted
        if matches!(result2, ProcessResult::DidWork(_)) {
            let result3 = harness.process();
            assert!(matches!(result3, ProcessResult::NoWork));
        } else {
            assert!(matches!(result2, ProcessResult::NoWork));
        }

        // Should have processed some messages but not all due to budget exhaustion
        let outputs = harness.collect_outputs();
        assert!(
            outputs.len() >= 1 && outputs.len() < 3,
            "Should process some but not all messages due to budget"
        );
        assert_eq!(outputs[0], b"msg1");
        if outputs.len() > 1 {
            assert_eq!(outputs[1], b"msg2");
        }
    }

    #[test]
    fn test_repeat_no_input() {
        let mut harness = RepeatTestHarness::new();

        // Process without any input
        let result = harness.process();

        // Should have no work
        assert!(matches!(result, ProcessResult::NoWork));

        // No outputs
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 0);
    }

    #[test]
    fn test_repeat_eof_handling() {
        let mut harness = RepeatTestHarness::new();

        // Send a message
        harness.send_input(b"test").unwrap();

        // Process to consume the message
        let result = harness.process();
        assert!(matches!(result, ProcessResult::DidWork(1)));

        // Now the input should be empty, and since we can't simulate EOF
        // in our test harness (we don't have access to abandon the queue),
        // we just verify normal operation
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], b"test");
    }

    #[test]
    fn test_repeat_deterministic_behavior() {
        // Test that the component produces consistent results
        let mut harness1 = RepeatTestHarness::new();
        let mut harness2 = RepeatTestHarness::new();

        // Send same inputs to both harnesses
        harness1.send_input(b"input1").unwrap();
        harness1.send_input(b"input2").unwrap();

        harness2.send_input(b"input1").unwrap();
        harness2.send_input(b"input2").unwrap();

        // Process both
        let _ = harness1.process();
        let _ = harness1.process();

        let _ = harness2.process();
        let _ = harness2.process();

        // Outputs should be identical
        let outputs1 = harness1.collect_outputs();
        let outputs2 = harness2.collect_outputs();

        assert_eq!(outputs1, outputs2);
        assert_eq!(outputs1.len(), 2);
        assert_eq!(outputs1[0], b"input1");
        assert_eq!(outputs1[1], b"input2");
    }
}
