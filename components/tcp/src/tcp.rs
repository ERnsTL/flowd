#![feature(addr_parse_ascii)] // for TCPClientComponent -> SocketAddr::parse_ascii()
use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, error, info, trace, warn};

// component-specific
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self};
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
const LISTENER_POLL_SLEEP: Duration = Duration::from_millis(50);
const LISTENER_JOIN_GRACE: Duration = Duration::from_secs(2);
const CONNECTION_JOIN_GRACE: Duration = Duration::from_secs(2);

fn handoff_join(handle: thread::JoinHandle<()>, label: &'static str) {
    thread::Builder::new()
        .name(format!("tcp-join-{}", label))
        .spawn(move || {
            if let Err(err) = handle.join() {
                warn!("{}: deferred thread join returned error: {:?}", label, err);
            }
        })
        .expect("failed to spawn tcp deferred join thread");
}

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
                    Ok(mut stream) => {
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
                                Err(PushError::Full(returned_data)) => {
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

pub struct TCPServerComponent {
    conf: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
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
        }
    }

    fn run(mut self) {
        debug!("TCPServer is now run()ning!");
        let mut conf = self.conf;

        // get configuration IP
        trace!("spinning for configuration IP...");
        loop {
            if !conf.is_empty() {
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
        //TODO optimize string conversions to on an address
        let config = conf.pop().expect("not empty but still got an error on pop");
        let listen_addr =
            std::str::from_utf8(&config).expect("could not parse listen_addr as utf-8");
        trace!("got listen address {}", listen_addr);

        // set configuration
        let listener = TcpListener::bind(listen_addr).expect("failed to bind tcp listener socket");
        listener
            .set_nonblocking(true)
            .expect("failed to set non-blocking on tcp listener socket");
        let resp = &mut self.resp;
        let out = Arc::new(Mutex::new(self.out.sink));
        let out_wakeup = self
            .out
            .wakeup
            .expect("got no wakeup handle for outport OUT");
        let shutdown = Arc::new(AtomicBool::new(false));

        // prepare variables for listen thread
        let sockets: Arc<Mutex<HashMap<u32, TcpStream>>> = Arc::new(Mutex::new(HashMap::new()));
        let sockets_ref = Arc::clone(&sockets);
        let out_ref = Arc::clone(&out);
        let out_wakeup_ref = out_wakeup.clone();
        let shutdown_listen = shutdown.clone();
        let connection_threads: Arc<Mutex<Vec<thread::JoinHandle<()>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let connection_threads_ref = connection_threads.clone();
        let listen_thread = thread::Builder::new().name(format!("{}_listen", thread::current().name().expect("could not get component thread name"))).spawn(move || {   //TODO optimize better way to get the current thread's name as String?
            // listener loop
            let mut socketnum: u32 = 0;
            loop {
                if shutdown_listen.load(Ordering::Relaxed) {
                    debug!("listener got shutdown signal, exiting");
                    break;
                }
                debug!("listening for a client");
                match listener.accept() {
                    Ok((mut socket, addr)) => {
                        println!("handling client: {addr:?}");
                        socketnum += 1;
                        socket.set_read_timeout(READ_TIMEOUT).expect("failed to set read timeout on client socket");
                        socket.set_write_timeout(WRITE_TIMEOUT).expect("failed to set write timeout on client socket");
                        sockets_ref.as_ref().lock().expect("lock poisoned").insert(socketnum, socket.try_clone().expect("cloud not clone socket"));
                        let sockets_ref2 = Arc::clone(&sockets_ref);
                        let out_ref2 = Arc::clone(&out_ref);
                        let out_wakeup_ref2 = out_wakeup_ref.clone();
                        let shutdown_conn = shutdown_listen.clone();
                        let connection_thread = thread::Builder::new().name(format!("{}_{}", thread::current().name().expect("could not get component thread name"), socketnum)).spawn(move || {
                            let socketnum_inner = socketnum;

                            // receive loop and send to component OUT tagged with socketnum
                            debug!("handling client connection");
                            loop {
                                if shutdown_conn.load(Ordering::Relaxed) {
                                    debug!("connection handler got shutdown signal, exiting");
                                    break;
                                }
                                // read from client
                                trace!("reading from client");
                                let mut buf = vec![0; 1024];   //TODO optimize with_capacity(1024);
                                match socket.read(&mut buf) {
                                    Ok(bytes) => {
                                        if bytes == 0 {
                                            // correctly closed (or given buffer had size 0)
                                            debug!("connection closed ok, exiting connection handler");
                                            break;
                                        }

                                        debug!("got data from client, pushing data to OUT");
                                        buf.truncate(bytes);    // otherwise we always hand over the full size of the buffer with many nul bytes
                                        //TODO optimize ^ do not truncate, but re-use the buffer and copy the bytes into a new Vec = the output IP
                                        out_ref2.lock().expect("lock poisoned").push(buf).expect("cloud not push IP into FBP network");   //TODO optimize really consume here? well, it makes sense since we are not responsible and never will be again for this IP; it is really handed over to the next process

                                        trace!("unparking OUT thread");
                                        out_wakeup_ref2.unpark();
                                    }
                                    Err(err) => {
                                        if err.kind() == ErrorKind::WouldBlock || err.kind() == ErrorKind::TimedOut {
                                            continue;
                                        }
                                        debug!("connection non-ok result, exiting connection handler");
                                        break;
                                    }
                                }
                                trace!("-- end of iteration")
                            }

                            // when socket closed, remove myself from list of known/open sockets resp. socket handlers
                            let _ = sockets_ref2.lock().expect("lock poisoned").remove(&socketnum_inner);
                            debug!("connections left: {}", sockets_ref2.lock().expect("lock poisoned").len());
                        }).expect("could not start connection handler thread");
                        connection_threads_ref.lock().expect("lock poisoned").push(connection_thread);
                    },
                    Err(e) => {
                        if e.kind() == ErrorKind::WouldBlock {
                            thread::sleep(LISTENER_POLL_SLEEP);
                            continue;
                        }
                        error!("accept failed: {e:?} - exiting");
                        break;
                    },
                }
            }
        }).expect("could not start listener thread");

        // main loop
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

            // check in port
            //TODO while !inn.is_empty() {
            loop {
                if let Ok(ip) = resp.pop() {
                    //TODO normally the IP should be immutable and forwarded as-is into the component library
                    // output the packet data with newline
                    debug!("got a packet, writing into client socket...");

                    // send into client socket
                    //TODO add support for multiple client connections - TODO need way to hand over metadata -> IP framing
                    let mut sockets_locked = sockets.lock().expect("lock poisoned");
                    if let Some(socket_id) = sockets_locked.keys().next().copied() {
                        let write_res = sockets_locked
                            .get_mut(&socket_id)
                            .expect("socket id vanished unexpectedly while writing")
                            .write_all(&ip);
                        match write_res {
                            Ok(_) => {}
                            Err(err) => {
                                match err.kind() {
                                    ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                                        warn!("tcpserver: timed out writing response to client {}, dropping packet", socket_id);
                                    }
                                    ErrorKind::BrokenPipe
                                    | ErrorKind::ConnectionReset
                                    | ErrorKind::NotConnected => {
                                        warn!("tcpserver: dropping disconnected client {} after write error: {}", socket_id, err);
                                        let _ = sockets_locked.remove(&socket_id);
                                    }
                                    _ => {
                                        warn!("tcpserver: write to client {} failed: {}, dropping client", socket_id, err);
                                        let _ = sockets_locked.remove(&socket_id);
                                    }
                                }
                            }
                        }
                    } else {
                        debug!("no connected clients, dropping response packet");
                    }
                    debug!("done");
                } else {
                    break;
                }
            }

            // check socket
            //NOTE: happens in connection handler threads, see above

            trace!("end of iteration");
            std::thread::park();
        }
        shutdown.store(true, Ordering::Relaxed);
        {
            let mut sockets_locked = sockets.lock().expect("lock poisoned");
            for socket in sockets_locked.values_mut() {
                let _ = socket.shutdown(Shutdown::Both);
            }
        }
        let listen_join_started = Instant::now();
        while !listen_thread.is_finished() && listen_join_started.elapsed() < LISTENER_JOIN_GRACE {
            thread::sleep(Duration::from_millis(10));
        }
        if listen_thread.is_finished() {
            if let Err(err) = listen_thread.join() {
                warn!("failed to join tcp listener thread: {:?}", err);
            }
        } else {
            warn!(
                "tcpserver: listener thread did not exit within {:?}, handing off to join reaper",
                LISTENER_JOIN_GRACE
            );
            handoff_join(listen_thread, "tcpserver-listener");
        }

        let handles = {
            let mut handles_guard = connection_threads.lock().expect("lock poisoned");
            handles_guard.drain(..).collect::<Vec<_>>()
        };
        for handle in handles {
            let conn_join_started = Instant::now();
            while !handle.is_finished() && conn_join_started.elapsed() < CONNECTION_JOIN_GRACE {
                thread::sleep(Duration::from_millis(10));
            }
            if handle.is_finished() {
                if let Err(err) = handle.join() {
                    warn!("failed to join tcp connection handler thread: {:?}", err);
                }
            } else {
                warn!(
                    "tcpserver: connection handler thread did not exit within {:?}, handing off to join reaper",
                    CONNECTION_JOIN_GRACE
                );
                handoff_join(handle, "tcpserver-connection");
            }
        }
        info!("exiting");
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
