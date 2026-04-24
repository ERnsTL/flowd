use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

// component-specific
use hyper::service::{make_service_fn, service_fn};
use hyper::{
    Body as HyperBody, Request as HyperRequest, Response as HyperResponse, Server as HyperServer,
    StatusCode,
};
use std::convert::Infallible;
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self};
use std::time::{Duration, Instant};
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
    conf: ProcessEdgeSource,
    routes: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    req: Vec<ProcessEdgeSink>,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
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
