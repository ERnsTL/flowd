use std::{borrow::BorrowMut, sync::{Arc, Mutex}};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use std::thread::{self};
use astra::{Body, Request, Response, Server};

pub struct HTTPServerComponent {
    conf: ProcessEdgeSource,
    routes: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    req: Vec<ProcessEdgeSink>,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for HTTPServerComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
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
        while self.conf.is_empty() {
            thread::yield_now();
        }
        trace!("spinning for routes on ROUTES...");
        while self.routes.is_empty() {
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
        //TODO optimize
        /*
        let mut out_req = vec![];
        let mut out_req_wakeup = vec![];
        for req in self.req.iter() {
            out_req.push(req.as_mut().sink);
            out_req_wakeup.push(req.wakeup.expect("got no wakeup handle for outport REQ"));
        }
        let out_req = Arc::new(Mutex::new(self.req.iter().map(|p| p.sink).collect::<Vec<_>>()));    // all sinks
        let out_req_wakeup = self.req.iter().map(|p| p.wakeup.expect("got no wakeup handle for outport REQ")).collect::<Vec<_>>();    // all wakeup handles
        */
        let out_req = Arc::new(Mutex::new(self.req));

        // start HTTP server in separate thread so we can also handle signals
        let _server = thread::Builder::new().name(format!("{}_handler", thread::current().name().expect("could not get component thread name"))).spawn(move || {   //TODO optimize better way to get the current thread's name as String?
            Server::bind(listenaddress)
                .serve(move |mut req: Request, _| {
                    debug!("request for {:?}", req.uri());

                    // decide which route to use
                    let route_req = req.uri().path();
                    let mut route_index = usize::MAX;
                    for (i, route) in routes.iter().enumerate() {
                        if route_req.starts_with(route) {   //TODO optimize, does it convert and thus clone?
                            debug!("matched route {} at index {}", route, i);
                            route_index = i;
                            break;
                        }
                    }
                    if route_index == usize::MAX {
                        warn!("no route matched, responding with 404");

                        let response = Response::new("no route matched".into());
                        let (mut parts, body) = response.into_parts();

                        parts.status = reqwest::StatusCode::NOT_FOUND;
                        let response = Response::from_parts(parts, body);

                        return response;
                    }

                    // read and send body to outport based on route index
                    let mut body = Vec::new();
                    for chunk in req.body_mut() {
                        body.extend_from_slice(&chunk.unwrap());    //TODO optimize - this clones!
                    }
                    //req.body_mut().reader().read_to_end(&mut body).expect("failed to read full request body");
                    //TODO optimize
                    let mut out_req_inner = out_req.lock().expect("poisoned lock on REQ inport");
                    unsafe {
                        let out_req_one = out_req_inner.borrow_mut().get_unchecked_mut(route_index);    // already checked this above
                        debug!("pushing to outport index {} -> {}", route_index, out_req_one.proc_name.as_ref().expect("no proc_name"));
                        out_req_one.sink.push(body).expect("could not push IP into FBP network");
                        out_req_one.wakeup.as_ref().unwrap().unpark();
                    }

                    // read back response from inport and send to client
                    debug!("waiting for response from FBP network...");
                    //TODO currently this needs a Muxer before it and the RESP port is thus intentionally not marked as an arrayport
                    while resp.lock().expect("poisoned lock on RESP outport").is_empty() {
                        //TODO optimize spinning
                        thread::yield_now();
                    }
                    debug!("got a packet, writing into HTTP response...");
                    let ip = resp.lock().expect("poisoned lock on RESP outport").pop().expect("no response data available, but said !is_empty() before");
                    Response::new(Body::new(ip))
                    
                    //Response::new(Body::from("Hello, World!"))
                })
                .expect("failed to start server");
        });

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
                    self.signals_out.send(b"pong".to_vec()).expect("cloud not send pong");
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }

            // check in port
            //TODO while !inn.is_empty() {
            /*
            loop {
                if let Ok(ip) = resp.pop() { //TODO normally the IP should be immutable and forwarded as-is into the component library
                    // output the packet data with newline
                    debug!("got a packet, writing into unix socket...");

                    // send into unix socket to peer
                    //TODO add support for multiple client connections - TODO need way to hand over metadata -> IP framing
                    sockets.lock().expect("lock poisoned").iter().next().unwrap().1.write(&ip).expect("could not send data from FBP network into Unix socket connection");   //TODO harden write_timeout() //TODO optimize
                    debug!("done");
                } else {
                    break;
                }
            }
            */

            // check socket
            //NOTE: happens in connection handler threads, see above

            trace!("end of iteration");
            std::thread::park();
        }
        debug!("cleaning up");
        //std::fs::remove_file(listenpath).unwrap();
        //TODO stop listening thread
        //TODO inform services
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