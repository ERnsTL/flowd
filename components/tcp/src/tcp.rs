#![feature(addr_parse_ascii)]   // for TCPClientComponent -> SocketAddr::parse_ascii()
use flowd_component_api::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHandle, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};
use log::{debug, error, info, trace, warn};

// component-specific
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::{self};
use std::collections::HashMap;

pub struct TCPClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

const CONNECT_TIMEOUT: Duration = Duration::from_millis(10000);
const READ_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const READ_BUFFER: usize = 4096;
const LISTENER_POLL_SLEEP: Duration = Duration::from_millis(50);
const LISTENER_JOIN_GRACE: Duration = Duration::from_secs(2);
const CONNECTION_JOIN_GRACE: Duration = Duration::from_secs(2);

impl Component for TCPClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        TCPClientComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("TCPClient is now run()ning!");
        let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        trace!("read config IP...");
        while conf.is_empty() {
            if let Ok(sig) = self.signals_in.try_recv() {
                trace!("received signal ip: {}", std::str::from_utf8(&sig).expect("invalid utf-8"));
                if sig == b"stop" {
                    info!("got stop signal while waiting for TCP config, exiting");
                    return;
                } else if sig == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&sig).expect("invalid utf-8"));
                }
            }
            thread::yield_now();
        }
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        trace!("got config IP");

        // prepare connection arguments
        let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");
        let url = url::Url::parse(&url_str).expect("failed to parse URL");
        // socket address
        let addr = SocketAddr::parse_ascii(format!("{}:{}",
            url.host_str().expect("failed to parse host from URL"),
            url.port().expect("failed to parse port from URL"))
            .as_bytes()
            ).expect("failed to parse socket address from URL");
        // NOTE: currently no options implemented - following code remains for future use
        //let mut query_pairs = url.query_pairs();
        //TODO optimize ^ re-use the query_pairs iterator? wont find anything after first .find() call
        // get buffer size from URL
        /*
        let _read_buffer_size;
        if let Some((_key, value)) = url.query_pairs().find(|(key, _)| key == "rbuffer") {
            _read_buffer_size = value.to_string().parse::<usize>().expect("failed to parse query pair value for read buffer as integer");
        } else {
            _read_buffer_size = DEFAULT_READ_BUFFER_SIZE;
        }
        */

        // configure
        let mut client = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).expect("failed to connect to TCP server");
        client.set_read_timeout(READ_TIMEOUT).expect("failed to set read timeout on TCP client"); //TODO optimize this Some() wrapping
        client.set_write_timeout(WRITE_TIMEOUT).expect("failed to set write timeout on TCP client");
        debug!("connected to {}", addr);

        // main loop
        //let mut bytes_in;
        let mut buf = [0; READ_BUFFER]; //TODO make configurable    //TODO optimize - change to BufReader, which can read all available bytes at once and we can send 1 consolidated IP with all bytes availble on the socket
        loop {
            trace!("begin of iteration");

            // check signals
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

            // send
            /*
            loop {
                if let Ok(_ip) = inn.pop() {
                    debug!("got a packet, dropping it.");
                } else {
                    break;
                }
            }
            */
            while !inn.is_empty() {
                debug!("got {} packets, sending into socket...", inn.slots());
                let chunk = inn.read_chunk(inn.slots()).expect("receive as chunk failed");

                for ip in chunk.into_iter() {
                    //TODO handle WouldBlock error, which is not an actual error
                    client.write(&ip).expect("failed to write into TCP client");
                }

                client.flush().expect("failed to flush TCP client");
                debug!("done");
            }

            // receive
            //TODO optimize - better to send or receive first? and send|receive first on FBP or on socket?
            //TODO optimize read or read_buf()? what is this BorrowedCursor, does it allow to use one big buffer and fill that one up as far as possible = read more using one read syscall?
            match client.read(&mut buf) {
                Ok(bytes_in) => {
                    //TODO optimize allocation of bytes_in - because of this match, we cannot assign to a re-used mut bytes_in directly, but use the returned variable from read()
                    if bytes_in > 0 {
                        debug!("got bytes from socket, repeating...");
                        out.push(Vec::from(&buf[0..bytes_in])).expect("could not push into OUT");
                        out_wakeup.unpark();
                        debug!("done");
                    }
                },
                Err(e) => {
                    match e.kind() {
                        std::io::ErrorKind::WouldBlock => {
                            // nothing to be done - this is OK and prevents busy-waiting
                        },
                        _ => {
                            //TODO handle disconnection etc. - initiate reconnection
                            error!("failed to read from TCP client: {} - exiting", e);
                            break;
                        }
                    }
                }
            }

            // are we done?
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
                //TODO close socket
                drop(out);
                out_wakeup.unpark();
                break;
            }

            trace!("-- end of iteration");
            // NOTE: not needed, because the loop is blocking on the socket read() with timeout
            //std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
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
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: GraphInportOutportHandle) -> Self where Self: Sized {
        TCPServerComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            resp: inports.remove("RESP").expect("found no RESP inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
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
        //TODO optimize string conversions to on an address
        let config = conf.pop().expect("not empty but still got an error on pop");
        let listen_addr = std::str::from_utf8(&config).expect("could not parse listen_addr as utf-8");
        trace!("got listen address {}", listen_addr);

        // set configuration
        let listener = TcpListener::bind(listen_addr).expect("failed to bind tcp listener socket");
        listener.set_nonblocking(true).expect("failed to set non-blocking on tcp listener socket");
        let resp = &mut self.resp;
        let out = Arc::new(Mutex::new(self.out.sink));
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");
        let shutdown = Arc::new(AtomicBool::new(false));

        // prepare variables for listen thread
        let sockets: Arc<Mutex<HashMap<u32, TcpStream>>> = Arc::new(Mutex::new(HashMap::new()));
        let sockets_ref = Arc::clone(&sockets);
        let out_ref = Arc::clone(&out);
        let out_wakeup_ref = out_wakeup.clone();
        let shutdown_listen = shutdown.clone();
        let connection_threads: Arc<Mutex<Vec<thread::JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));
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

            // check in port
            //TODO while !inn.is_empty() {
            loop {
                if let Ok(ip) = resp.pop() { //TODO normally the IP should be immutable and forwarded as-is into the component library
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
                                    ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::NotConnected => {
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
                "tcpserver: listener thread did not exit within {:?}, detaching",
                LISTENER_JOIN_GRACE
            );
            drop(listen_thread);
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
                    "tcpserver: connection handler thread did not exit within {:?}, detaching",
                    CONNECTION_JOIN_GRACE
                );
                drop(handle);
            }
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
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
                    description: String::from("configuration value, currently the IP and port to listen on"),
                    values_allowed: vec![],
                    value_default: String::from("localhost:1234")
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
