use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use std::time::Duration;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::{self};
use std::collections::HashMap;

pub struct TCPClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

const CONNECT_TIMEOUT: Duration = Duration::from_millis(10000);
const READ_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const READ_BUFFER: usize = 4096;

impl Component for TCPClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
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
        trace!("read config IPs");
        /*
        while conf.is_empty() {
            thread::yield_now();
        }
        */
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };

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
        let mut client = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).expect("failed to connect to TCP server - timeout");
        client.set_read_timeout(READ_TIMEOUT).expect("failed to set read timeout on TCP client"); //TODO optimize this Some() wrapping
        client.set_write_timeout(WRITE_TIMEOUT).expect("failed to set write timeout on TCP client");

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
                    self.signals_out.send(b"pong".to_vec()).expect("could not send pong");
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
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for TCPServerComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
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
        while conf.is_empty() {
            thread::yield_now();
        }
        //TODO optimize string conversions to on an address
        let config = conf.pop().expect("not empty but still got an error on pop");
        let listen_addr = std::str::from_utf8(&config).expect("could not parse listen_addr as utf-8");
        trace!("got listen address {}", listen_addr);

        // set configuration
        let listener = TcpListener::bind(listen_addr).expect("failed to bind tcp listener socket");
        let resp = &mut self.resp;
        let out = Arc::new(Mutex::new(self.out.sink));
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // prepare variables for listen thread
        let sockets: Arc<Mutex<HashMap<u32, TcpStream>>> = Arc::new(Mutex::new(HashMap::new()));
        let sockets_ref = Arc::clone(&sockets);
        let out_ref = Arc::clone(&out);
        let out_wakeup_ref = out_wakeup.clone();
        //TODO use that variable and properly terminate the listener thread on network stop - who tells it to stop listening?
        //TODO fix - the listener thread is not stopped when the component is stopped, it will continue to listen and accept connections and on network restart the listen address will be already in use
        let _listen_thread = thread::Builder::new().name(format!("{}_listen", thread::current().name().expect("could not get component thread name"))).spawn(move || {   //TODO optimize better way to get the current thread's name as String?
            // listener loop
            let mut socketnum: u32 = 0;
            loop {
                debug!("listening for a client");
                match listener.accept() {
                    Ok((mut socket, addr)) => {
                        println!("handling client: {addr:?}");
                        socketnum += 1;
                        sockets_ref.as_ref().lock().expect("lock poisoned").insert(socketnum, socket.try_clone().expect("cloud not clone socket"));
                        let sockets_ref2 = Arc::clone(&sockets_ref);
                        let out_ref2 = Arc::clone(&out_ref);
                        let out_wakeup_ref2 = out_wakeup_ref.clone();
                        thread::Builder::new().name(format!("{}_{}", thread::current().name().expect("could not get component thread name"), socketnum)).spawn(move || {
                            let socketnum_inner = socketnum;

                            // receive loop and send to component OUT tagged with socketnum
                            debug!("handling client connection");
                            loop {
                                // read from client
                                trace!("reading from client");
                                let mut buf = vec![0; 1024];   //TODO optimize with_capacity(1024);
                                if let Ok(bytes) = socket.read(&mut buf) {
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
                                } else {
                                    debug!("connection non-ok result, exiting connection handler");
                                    break;
                                };
                                trace!("-- end of iteration")
                            }

                            // when socket closed, remove myself from list of known/open sockets resp. socket handlers
                            sockets_ref2.lock().expect("lock poisoned").remove(&socketnum_inner).expect("could not remove my socketnum from sockets hashmap");
                            debug!("connections left: {}", sockets_ref2.lock().expect("lock poisoned").len());
                        }).expect("could not start connection handler thread");
                    },
                    Err(e) => {
                        error!("accept failed: {e:?} - exiting");
                        break;
                    },
                }
            }
        });

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
                    self.signals_out.send(b"pong".to_vec()).expect("cloud not send pong");
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
                    sockets.lock().expect("lock poisoned").iter().next().unwrap().1.write(&ip).expect("could not send data from FBP network into client socket connection");   //TODO harden write_timeout() //TODO optimize
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