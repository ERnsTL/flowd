use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
//use tungstenite::client::connect;
use tungstenite::protocol::Message;
use tungstenite::util::NonBlockingError;
use std::time::Duration;
use std::net::{TcpListener, TcpStream};
use std::thread::{self};
use std::collections::HashMap;
use tungstenite::WebSocket;

pub struct WSClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

const READ_TIMEOUT: Duration = Duration::from_millis(500);
const WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));

impl Component for WSClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        WSClientComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("WSClient is now run()ning!");
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
        // NOTE: currently no options implemented
        let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");
        //let url = url::Url::parse(&url_str).expect("failed to parse URL");
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
        let (mut client, _) = tungstenite::client::connect(url_str).expect("failed to connect to WebSocket server");    //TODO hande response

        // main loop
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
                debug!("got {} packets, sending into WebSocket...", inn.slots());
                let chunk = inn.read_chunk(inn.slots()).expect("receive as chunk failed");

                for ip in chunk.into_iter() {
                    //TODO add support for Text messages
                    client.write(Message::Binary(ip)).unwrap(); //TODO handle Result - but it is only () anyway
                }

                client.flush().expect("failed to flush WebSocket client");
            }

            // receive
            //TODO optimize - better to send or receive first? and send|receive first on FBP or on socket?
            while client.can_read() {
                debug!("got message from WebSocket, repeating...");
                let msg = client.read().expect("failed to read from WebSocket");
                out.push(msg.into_data()).expect("could not push into OUT");
                out_wakeup.unpark();
                debug!("done");
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
            //TODO optimize - hacky way to avoid busy waiting;
            //  better to use a separate thread for WebSocket read and close the connection when the inport is abandoned, leading to the read thread exiting
            //std::thread::park();
            std::thread::sleep(READ_TIMEOUT);
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("WSClient"),
            description: String::from("Sends and receives messages, transformed into IPs, via a WebSocket connection."),
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
                    value_default: String::from("wss://example.com:8080/socketpath")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("incoming WebSocket messages, transformed to IPs"),
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
                    description: String::from("IPs to be sent as WebSocket messages"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

pub struct WSServerComponent {
    conf: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}


impl Component for WSServerComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        WSServerComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            resp: inports.remove("RESP").expect("found no RESP inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("WSServer is now run()ning!");
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
        let sockets: Arc<Mutex<HashMap<u32, WebSocket<TcpStream>>>> = Arc::new(Mutex::new(HashMap::new()));
        let sockets_ref = sockets.clone();
        let out_ref = out.clone();
        let out_wakeup_ref = out_wakeup.clone();
        //TODO use that variable and properly terminate the listener thread on network stop - who tells it to stop listening?
        //TODO fix - the listener thread is not stopped when the component is stopped, it will continue to listen and accept connections and on network restart the listen address will be already in use
        let _listen_thread = thread::Builder::new().name(format!("{}_listen", thread::current().name().expect("could not get component thread name"))).spawn(move || {   //TODO optimize better way to get the current thread's name as String?
            // listener loop
            let mut socketnum: u32 = 0;
            loop {
                debug!("listening for a client");
                match listener.accept() {
                    Ok((socket, addr)) => {
                        println!("handling client: {addr:?}");
                        socketnum += 1;

                        // set options on socket
                        socket.set_read_timeout(Some(READ_TIMEOUT)).expect("failed to set read timeout on socket");
                        socket.set_write_timeout(WRITE_TIMEOUT).expect("failed to set write timeout on socket");
                        // upgrade to WebSocket
                        let websocket = tungstenite::accept(socket).expect("failed to accept/upgrade to WebSocket connection");

                        // add to list of known/open sockets resp. socket handlers
                        sockets_ref.lock().expect("lock poisoned").insert(socketnum, websocket);

                        // prepare variables for connection handler thread
                        let sockets_ref2 = sockets_ref.clone();
                        let out_ref2 = out_ref.clone();
                        let out_wakeup_ref2 = out_wakeup_ref.clone();

                        // handle the connection in a separate thread
                        thread::Builder::new().name(format!("{}_{}", thread::current().name().expect("could not get component thread name"), socketnum)).spawn(move || {
                            let socketnum_inner = socketnum;

                            // receive loop and send to component OUT tagged with socketnum
                            debug!("handling client connection");
                            loop {
                                // read from client
                                debug!("reading from client");  //TODO fix - without the debug message, Rust will keep the lock forever even when it should only be kept for the match block and not the whole loop
                                //TODO optimize - the real problem is that WebSocket is not cloneable
                                match sockets_ref2.lock().expect("lock poisoned").iter_mut().next().expect("failed to get entry from socket hashmap").1.read() {
                                    Ok(message) => {
                                        debug!("got message from client, pushing data to OUT");
                                        out_ref2.lock().expect("lock poisoned").push(message.into_data()).expect("could not push IP into FBP network");   //TODO optimize really consume here? well, it makes sense since we are not responsible and never will be again for this IP; it is really handed over to the next process

                                        trace!("unparking OUT thread");
                                        out_wakeup_ref2.unpark();
                                        debug!("done");
                                    },
                                    Err(e) => {
                                        // ignore WouldBlock "errors" - they are not errors, just information that there is no data to read resp. timeout on read()
                                        if let Some(real_error) = e.into_non_blocking() {
                                            debug!("failed to read from client: {real_error:?} - exiting connection handler");
                                            break;
                                        }
                                    },
                                };
                                debug!("-- end of iteration");
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
                    self.signals_out.send(b"pong".to_vec()).expect("could not send pong");
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }

            // check in port
            //TODO while !inn.is_empty() {
            loop {
                if let Ok(ip) = resp.pop() {
                    debug!("got a packet, writing into client socket...");

                    // send into client socket
                    {
                        //TODO add support for multiple client connections - TODO need way to hand over metadata -> IP framing
                        //TODO optimize - lots of locking and indirection here
                        sockets.lock().expect("lock poisoned").iter_mut().next().unwrap().1.write(Message::binary(ip)).expect("could not send data from FBP network into client socket connection");   //TODO harden write_timeout() //TODO optimize
                        //TODO optimize - flush only when necessary. read chunks from FBP network and flush when all IPs in the chunk are sent
                        sockets.lock().expect("lock poisoned").iter_mut().next().unwrap().1.flush().expect("failed to flush WebSocket to client");
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
            name: String::from("WSServer"),
            description: String::from("WebSocket server"),
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