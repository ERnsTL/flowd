use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
//use std::net::SocketAddr;
use std::net::{TcpListener, TcpStream};
use std::time::Duration;
use rustls::{RootCertStore, ServerConnection};
use std::thread::{self};
use std::io::{BufReader, Read, Write};
use std::collections::HashMap;
use rustls_pemfile;

pub struct TLSClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

//const CONNECT_TIMEOUT: Duration = Duration::from_millis(10000);
const READ_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));
const READ_BUFFER: usize = 65536;   // is allocated once and re-used for each read() call

impl Component for TLSClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        TLSClientComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("TLSClient is now run()ning!");
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
        // for connect_timeout a signle IP address is needed
        /*
        let addr = SocketAddr::parse_ascii(format!("{}:{}",
            url.host_str().expect("failed to parse host from URL"),
            url.port().expect("failed to parse port from URL"))
            .as_bytes()
            ).expect("failed to parse socket address from URL");
        */
        // for connect() a hostname is OK
        let addr = format!(
            "{}:{}",
            url.host_str().expect("failed to parse host from URL"),
            url.port().expect("failed to parse port from URL")
            );
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
        // certificates
        let root_store = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.into() };
        let mut config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        // allow using SSLKEYLOGFILE - TODO what is this?
        config.key_log = Arc::new(rustls::KeyLogFile::new());
        // create connection
        let server_name2 = url.host_str().expect("failed to parse host from URL").to_owned().try_into().unwrap();
        let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name2).unwrap();
        //let mut sock = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).expect("failed to connect to TCP server - timeout");
        let mut sock = TcpStream::connect(addr).expect("failed to connect to TCP server - timeout");
        sock.set_read_timeout(READ_TIMEOUT).expect("failed to set read timeout on TCP client"); //TODO optimize this Some() wrapping
        sock.set_write_timeout(WRITE_TIMEOUT).expect("failed to set write timeout on TCP client");
        let mut client = rustls::Stream::new(&mut conn, &mut sock);

        // main loop
        //let mut bytes_in;
        let mut buf = [0; READ_BUFFER]; //TODO make configurable    //TODO optimize - change to BufReader, which can read all available bytes at once and we can send 1 consolidated IP with all bytes availble on the socket
        'mainloop:
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

            // receive = read until no more bytes are available
            //TODO optimize - better to send or receive first? and send|receive first on FBP or on socket?
            //TODO optimize read or read_buf()? what is this BorrowedCursor, does it allow to use one big buffer and fill that one up as far as possible = read more using one read syscall?
            loop {  //TODO do as a while loop with a break on WouldBlock
                match client.read(&mut buf) {
                    Ok(bytes_in) => {
                        //TODO optimize allocation of bytes_in - because of this match, we cannot assign to a re-used mut bytes_in directly, but use the returned variable from read()
                        if bytes_in > 0 {
                            debug!("got bytes from socket, repeating...");
                            //TODO optimize - write chunk?
                            out.push(Vec::from(&buf[0..bytes_in])).expect("could not push into OUT");
                            out_wakeup.unpark();
                            debug!("done");
                        } else {
                            // nothing to be done here - this is OK and we have read all available bytes for now
                            break;
                        }
                    },
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::WouldBlock => {
                                // nothing to be done - this is OK and prevents busy-waiting
                                break;
                            },
                            _ => {
                                //TODO handle disconnection etc. - initiate reconnection
                                error!("failed to read from TCP client: {} - exiting", e);
                                break 'mainloop;
                            }
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
            name: String::from("TLSClient"),
            description: String::from("Sends and receives bytes, transformed into IPs, via a TLS+TCP connection."),
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
                    value_default: String::from("tls://example.com:8080")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("incoming decrypted bytes from TLS, transformed to IPs"),
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
                    description: String::from("IPs to be sent as encrypted bytes via TLS"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

pub struct TLSServerComponent {
    conf: ProcessEdgeSource,
    cert: ProcessEdgeSource,
    key: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for TLSServerComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        TLSServerComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            cert: inports.remove("CERT").expect("found no CERT inport").pop().unwrap(),
            key: inports.remove("KEY").expect("found no KEY inport").pop().unwrap(),
            resp: inports.remove("RESP").expect("found no RESP inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("TLSServer is now run()ning!");
        let mut conf = self.conf;
        let mut cert_in = self.cert;
        let mut key_in = self.key;

        // get configuration

        // get configuration IP
        trace!("spinning for configuration IP...");
        while conf.is_empty() {
            thread::yield_now();
        }
        //TODO optimize string conversions to on an address
        let config = conf.pop().expect("not empty but still got an error on pop");
        let listen_addr = std::str::from_utf8(&config).expect("could not parse listen_addr as utf-8");
        trace!("got listen address {}", listen_addr);

        // get certificate
        trace!("waiting for certificate IP...");
        while cert_in.is_empty() {
            thread::yield_now();
        }
        let cert_vec = cert_in.pop().expect("not empty but still got an error on pop");
        //let listen_addr = std::str::from_utf8(&config).expect("could not parse listen_addr as utf-8");
        //trace!("got listen address {}", listen_addr);

        // get key
        trace!("waiting for private key IP...");
        while key_in.is_empty() {
            thread::yield_now();
        }
        let key_vec = key_in.pop().expect("not empty but still got an error on pop");
        //let listen_addr = std::str::from_utf8(&config).expect("could not parse listen_addr as utf-8");
        //trace!("got listen address {}", listen_addr);

        // set configuration
        // parse certificates
        let certs = rustls_pemfile::certs(&mut BufReader::new(&cert_vec[..])).collect::<Result<Vec<_>, _>>().expect("failed to parse certificate");
        // parse private key
        let private_key = rustls_pemfile::private_key(&mut BufReader::new(&key_vec[..])).unwrap().expect("failed to parse private key");
        // put both into configuration
        let config = Arc::new(rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, private_key)
            .expect("failed to build server config"));
        // repare listener
        let listener = TcpListener::bind(listen_addr).expect("failed to bind tcp listener socket");
        // prepare FBP ports
        let resp = &mut self.resp;
        let out = Arc::new(Mutex::new(self.out.sink));
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // prepare variables for listen thread
        let sockets: Arc<Mutex<HashMap<u32, (TcpStream, Arc<Mutex<ServerConnection>>)>>> = Arc::new(Mutex::new(HashMap::new()));
        let sockets_ref = Arc::clone(&sockets);
        let out_ref = Arc::clone(&out);
        let out_wakeup_ref = out_wakeup.clone();
        let config_ref = config.clone();
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

                        // prepare variables for connection handler thread
                        let sockets_ref2 = Arc::clone(&sockets_ref);
                        let out_ref2 = Arc::clone(&out_ref);
                        let out_wakeup_ref2 = out_wakeup_ref.clone();
                        let config_ref2 = config_ref.clone();

                        // handle the individual connection in a separate thread
                        thread::Builder::new().name(format!("{}_{}", thread::current().name().expect("could not get component thread name"), socketnum)).spawn(move || {
                            let socketnum_inner = socketnum;

                            // setup TCP stream
                            socket.set_read_timeout(READ_TIMEOUT).expect("could not set read timeout on TCP connection to client");
                            socket.set_write_timeout(WRITE_TIMEOUT).expect("could not set write timeout on TCP connection to client");

                            // set up TLS
                            let conn = Arc::new(Mutex::new(rustls::ServerConnection::new(config_ref2).expect("could not create TLS server connection")));
                            conn.lock().expect("lock poisoned").complete_io(&mut socket).expect("could not complete TLS IO");
                            sockets_ref2.as_ref().lock().expect("lock poisoned").insert(socketnum_inner, (socket.try_clone().expect("cloud not clone socket"), conn.clone()));

                            // receive loop and send to component OUT tagged with socketnum
                            debug!("handling client connection");
                            loop {
                                // read from client
                                trace!("reading from client");
                                {
                                    let mut conn_locked = conn.lock().expect("lock poisoned");
                                    // pull in data from underlying unencrypted TCP socket - this is affected by the read timeout set above so that we dont busy-wait
                                    if let Ok(count) = conn_locked.read_tls(&mut socket) {
                                        if count > 0 {
                                            conn_locked.process_new_packets().expect("could not process new packets in TLS connection");
                                        }
                                    }
                                }
                                let mut buf = vec![0; 1024];   //TODO optimize with_capacity(1024); //TODO optimize this gets allocated for every try to read() !!
                                match conn.lock().expect("lock poisoned").reader().read(&mut buf) {    // TODO optimize - always getting a new reader() is not efficient
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
                                    },
                                    Err(e) => {
                                        match e.kind() {
                                            std::io::ErrorKind::WouldBlock => {
                                                // nothing to be done - this is OK and prevents busy-waiting
                                            },
                                            _ => {
                                                //TODO handle disconnection etc. - initiate reconnection
                                                debug!("failed to read from client: {e:?} - exiting connection handler");
                                                break;
                                            }
                                        }
                                    },
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
                if let Ok(ip) = resp.pop() {
                    // output the packet data with newline
                    debug!("got a packet, writing into client socket...");

                    // send into client socket
                    //TODO add support for multiple client connections, currently sending to the first one in the hashmap
                    //  TODO need way to hand over metadata -> IP framing
                    //TODO optimize - all this unpacking is unefficient
                    //TODO optimize - due to the locking, we are not able to send while the the connection handler thread above is blocking on its read() and until the read timeout has passed
                    {
                        let mut sockets_locked = sockets.lock().expect("lock poisoned");
                        let (ref mut first_socket, conn2client_arc) = sockets_locked.iter_mut().next().unwrap().1;
                        let conn2client = conn2client_arc.clone();
                        let mut conn_locked = conn2client.lock().expect("lock poisoned");
                        let mut conn_writer = conn_locked.writer();
                        conn_writer.write(&ip).expect("could not send data from FBP network into client socket connection");   //TODO harden write_timeout() //TODO optimize
                        conn_locked.complete_io(first_socket).expect("could not complete IO on client socket connection = send data via TLS and underlying TCP connection to client");
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
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("TLSServer"),
            description: String::from("TLS over TCP server"),
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
                    name: String::from("CERT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("the TLS certificate with the public key; can be multiple chained certificates; generate selfsigned with openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -sha256 -days 365 -nodes -subj '/CN=localhost'"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("KEY"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("private key fitting the public key in the TLS certificate"),
                    values_allowed: vec![],
                    value_default: String::from("")
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