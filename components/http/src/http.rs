use flowd_component_api::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHandle, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};
use log::{debug, trace, info, warn};

// component-specific
use std::convert::Infallible;
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self};
use std::time::{Duration, Instant};
use hyper::{Body as HyperBody, Request as HyperRequest, Response as HyperResponse, Server as HyperServer, StatusCode};
use hyper::service::{make_service_fn, service_fn};
use tokio::sync::oneshot;

const SERVER_JOIN_GRACE: Duration = Duration::from_secs(5);

pub struct HTTPClientComponent {
    req: ProcessEdgeSource,
    out_resp: ProcessEdgeSink,
    out_err: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for HTTPClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        HTTPClientComponent {
            req: inports.remove("REQ").expect("found no REQ inport").pop().unwrap(),
            out_resp: outports.remove("RESP").expect("found no RESP outport").pop().unwrap(),
            out_err: outports.remove("ERR").expect("found no ERR outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("HTTPClient is now run()ning!");
        let mut requests = self.req;
        let mut out_resp = self.out_resp.sink;
        let out_resp_wakeup = self.out_resp.wakeup.expect("got no wakeup handle for outport RESP");
        let mut out_err = self.out_err.sink;
        let out_err_wakeup = self.out_err.wakeup.expect("got no wakeup handle for outport ERR");
        loop {
            trace!("begin of iteration");
            // check signals
            //TODO optimize, there is also try_recv() and recv_timeout()
            if let Ok(ip) = self.signals_in.try_recv() {
                //TODO optimize string conversions
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {   //TODO optimize comparison
                    info!("got stop signal, exiting");
                    break;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }
            // check in port
            //TODO while !inn.is_empty() {
            loop {
                if let Ok(ip) = requests.pop() {
                    // read filename on inport
                    //TODO support POST etc.
                    //TODO support sending a body
                    let file_path = std::str::from_utf8(&ip).expect("non utf-8 data");
                    debug!("got a request: {}", &file_path);

                    // make HTTP request
                    //TODO may be big file - add chunking
                    //TODO enclose files in brackets to know where its stream of chunks start and end
                    //TODO enable use of async and/or timeout
                    debug!("making HTTP request...");
                    match reqwest::blocking::get(file_path) {
                        Ok(resp) => {
                            // send it
                            //TODO forward response headers? useful?
                            debug!("forwarding HTTP results...");
                            let body = resp.bytes().expect("should have been able to read the HTTP response");
                            out_resp.push(body.to_vec()).expect("could not push into RESP");  //TODO optimize conversion
                            out_resp_wakeup.unpark();
                        },
                        Err(err) => {
                            // send error
                            debug!("got HTTP error, sending...");
                            out_err.push(err.to_string().into()).expect("could not push into RESP");   //TODO optimize conversion
                            out_err_wakeup.unpark();
                        }
                    };
                    debug!("done");
                } else {
                    break;
                }
            }

            // are we done?
            if requests.is_abandoned() {
                info!("EOF on inport REQ, shutting down");
                drop(out_resp);
                out_resp_wakeup.unpark();
                drop(out_err);
                out_err_wakeup.unpark();
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("HTTPClient"),
            description: String::from("Reads URLs and sends the response body out via RESP or ERR"),   //TODO change according to new features
            icon: String::from("web"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("REQ"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("URLs, one per IP"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("RESP"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("response body if response is non-error"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("ERR"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("error responses in human-readable error format"),    //TODO some machine-readable format better?
                    values_allowed: vec![],
                    value_default: String::from("")
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
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        HTTPServerComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            routes: inports.remove("ROUTES").expect("found no ROUTES inport").pop().unwrap(),
            resp: inports.remove("RESP").expect("found no RESP inport").pop().unwrap(),
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
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                if ip == b"stop" {
                    info!("got stop signal while waiting for CONF, exiting");
                    return;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
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
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                if ip == b"stop" {
                    info!("got stop signal while waiting for ROUTES, exiting");
                    return;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }
            thread::yield_now();
        }
        //TODO optimize string conversions to listen on a path
        let config = self.conf.pop().expect("not empty but still got an error on pop");
        let listenaddress = std::str::from_utf8(&config).expect("could not parse listenpath as utf8").to_owned();   //TODO optimize
        trace!("got listen address {}", listenaddress);
        let routes_bytes = self.routes.pop().expect("not empty but still got an error on pop");
        let routes_str = std::str::from_utf8(&routes_bytes).expect("could not parse routes as utf8").split(",").collect::<Vec<_>>();   //TODO optimize
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
            .name(format!("{}_handler", thread::current().name().expect("could not get component thread name")))
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
                            Ok::<_, Infallible>(service_fn(move |mut req: HyperRequest<HyperBody>| {
                                let routes = routes.clone();
                                let out_req = out_req.clone();
                                let resp = resp.clone();
                                let shutdown = shutdown.clone();
                                async move {
                                    if shutdown.load(Ordering::Relaxed) {
                                        let mut response = HyperResponse::new(HyperBody::from("server shutting down"));
                                        *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
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
                                        let mut response = HyperResponse::new(HyperBody::from("no route matched"));
                                        *response.status_mut() = StatusCode::NOT_FOUND;
                                        return Ok::<_, Infallible>(response);
                                    }

                                    let body = match hyper::body::to_bytes(req.body_mut()).await {
                                        Ok(bytes) => bytes,
                                        Err(err) => {
                                            warn!("HTTPServer: failed to read request body: {}", err);
                                            let mut response = HyperResponse::new(HyperBody::from("failed to read request body"));
                                            *response.status_mut() = StatusCode::BAD_REQUEST;
                                            return Ok::<_, Infallible>(response);
                                        }
                                    };

                                    let pushed_ok = {
                                        let mut out_req_locked = out_req.lock().expect("poisoned lock on REQ outport");
                                        if let Some(out_req_one) = out_req_locked.get_mut(route_index) {
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
                                        let mut response = HyperResponse::new(HyperBody::from("internal route mismatch"));
                                        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                                        return Ok::<_, Infallible>(response);
                                    }

                                    // wait for response from FBP network
                                    loop {
                                        if shutdown.load(Ordering::Relaxed) {
                                            let mut response = HyperResponse::new(HyperBody::from("server shutting down"));
                                            *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
                                            return Ok::<_, Infallible>(response);
                                        }
                                        if let Ok(ip) = resp.lock().expect("poisoned lock on RESP inport").pop() {
                                            return Ok::<_, Infallible>(HyperResponse::new(HyperBody::from(ip)));
                                        }
                                        tokio::task::yield_now().await;
                                    }
                                }
                            }))
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
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {
                    info!("got stop signal, exiting");
                    break;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
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
                "HTTPServer: server thread did not exit within {:?}, detaching",
                SERVER_JOIN_GRACE
            );
            drop(server_thread);
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
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
