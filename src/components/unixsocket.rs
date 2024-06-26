use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
/*
use std::os::unix::net::UnixDatagram;
use std::os::unix::net::UnixStream;
use std::io::prelude::*;
use std::str::FromStr;
use std::os::unix::net::SocketAddr;
use std::os::linux::net::SocketAddrExt;
use std::io::ErrorKind;
use std::io::BufReader;
use uds::{UnixSeqpacketConn, UnixDatagramExt, UnixListenerExt, UnixStreamExt};
*/
use std::time::Duration;
use uds::UnixSocketAddr;
use std::io::{Write, Read};
use std::thread::{self};
use std::collections::HashMap;

pub struct UnixSocketClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

/*
Situation 2024-04:
* socket types:  https://www.man7.org/linux/man-pages/man7/unix.7.html
* the best type is seqpacket
* https://internals.rust-lang.org/t/pre-rfc-adding-sock-seqpacket-unix-sockets-to-std/7323
* https://github.com/rust-lang-nursery/unix-socket/pull/25
* https://github.com/rust-lang/rust/pull/50348
* they want to have it on crates.io first and - if popular enough - put it into std, pfff
* that crate is:  https://crates.io/crates/uds
* problem is that for example credential-passing and ancillary data has been added to std, but not do uds crate, but seqpacket is in uds but not in std
* best place to add SeqPacket socket support would be in std::os::linux::net::UnixSocketExt
* wtf
* if flowd ever moves from threading to tokio, then there is already https://crates.io/crates/tokio-seqpacket
*/
//TODO optimize - u8?
enum SocketType {
    Datagram,
    Stream,
    SeqPacket
    // NOTE: what is seqpacket, it is the way to go unless really streaming is employed and many many messages are received since 1 read syscall will yield only 1 seqpacket=message.
    // http://www.ccplusplus.com/2011/08/understanding-sockseqpacket-socket-type.html
}

const DEFAULT_READ_BUFFER_SIZE: usize = 65536;
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_millis(500);

impl Component for UnixSocketClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        UnixSocketClientComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("UnixSocketClient is now run()ning!");
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
        //let mut query_pairs = url.query_pairs();
        //TODO optimize ^ re-use the query_pairs iterator? wont find anything after first .find() call
        // get abstract y/n
        let address_is_abstract: bool;
        if url.has_host() {
            address_is_abstract = true;
        } else if url.path().len() > 0 {
            address_is_abstract = false;
        } else {
            panic!("failed to determine if socket address is abstract or path-based");
        };
        debug!("got abstract socket address: {}", address_is_abstract);
        // get address from URL
        let address_str ;
        if address_is_abstract {
            // get abstract socket address from URL host
            address_str = url.host_str().expect("failed to get abstract socket address from URL host");
        } else {
            // get path-based socket address from URL path
            address_str = url.path();
            if address_str.is_empty() || address_str == "/" {
                error!("no socket address given in config URL path, exiting");
                return;
            }
        }
        debug!("got socket address: {}", &address_str);
        // get buffer size from URL
        //TODO implement - currently variables are not used
        let _read_buffer_size;
        if let Some((_key, value)) = url.query_pairs().find(|(key, _)| key == "rbuffer") {
            _read_buffer_size = value.to_string().parse::<usize>().expect("failed to parse query pair value for read buffer as integer");
        } else {
            _read_buffer_size = DEFAULT_READ_BUFFER_SIZE;
        }
        // get read timeout from URL
        //TODO differentiate internal read timeout and read timeout when connection has to be reconnected
        let read_timeout: Duration;
        if let Some((_key, value)) = url.query_pairs().find(|(key, _)| key == "rtimeout") {
            read_timeout = Duration::from_millis(value.to_string().parse::<u64>().expect("failed to parse query pair value for read timeout as integer"));
        } else {
            read_timeout = DEFAULT_READ_TIMEOUT;
        }
        // get socket type from URL
        let socket_type: SocketType;
        if let Some((_key, value)) = url.query_pairs().find(|(key, _)| key == "socket_type") {
            socket_type = match value.to_string().as_str() {
                "seqpacket" => SocketType::SeqPacket,
                "stream" => SocketType::Stream,
                "dgram"|"datagram" => SocketType::Datagram,
                _ => { panic!("failed to parse query pair value for key socket_type into dgram|datagram|stream|seqpacket"); }
            };
        } else {
            // NOTE: not setting a default type, because this may produce "unable to connect" errors because of unexpected socket type (and the default might change in the future)
            error!("failed to get socket type from config URL, missing query key socket_type");
            return;
        }

        // configure

        // prepare socket address
        let socket_address;
        if address_is_abstract {
            // std variant
            //socket_address = SocketAddr::from_abstract_name(address_str).expect("failed to parse abstract socket address into SocketAddr");
            // uds variant
            socket_address = UnixSocketAddr::from_abstract(address_str).expect("failed to parse abstract socket address into UnixSocketAddr");
        } else {
            // std variant
            //socket_address = SocketAddr::from_pathname(address_str).expect("failed to parse path-based socket address into SocketAddr");
            // uds variant
            socket_address = UnixSocketAddr::from_path(address_str).expect("failed to parse path-based socket address into UnixSocketAddr");
        };

        // prepare socket client
        //TODO implement - support stream and datagram sockets
        let client_seqpacket;
        //let client_stream;
        //let client_dgram;
        match socket_type {
            SocketType::SeqPacket => { client_seqpacket = uds::UnixSeqpacketConn::connect_unix_addr(&socket_address).expect("failed to connect"); }
            SocketType::Stream => { unimplemented!(); } //client_stream = UnixStream::connect_addr(&socket_address).expect("failed to connect"); },
            SocketType::Datagram => { unimplemented!(); } //client_dgram = UnixDatagram::connect_addr(&socket_address).expect("failed to bind"); },
        };

        // prepare buffered reader
        // NOTE: this is needed because POSIX does not have a function to give the available bytes, therefore must read 2x at least. I would prefer 1 call for number of bytes and 2nd call to all available bytes in one exactly-fitting buffer.
        //let bufreader = BufReader::with_capacity(DEFAULT_READ_BUFFER_SIZE, &client_seqpacket);
        let mut buf = [0u8; DEFAULT_READ_BUFFER_SIZE];

        // set read timeout to avoid blocking forever and watchdog thread marking the process as non-responding
        client_seqpacket.set_read_timeout(Some(read_timeout)).expect("failed to set socket read timeout");

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

            // check in port
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
                //_ = inn.pop().ok();
                //debug!("got a packet, dropping it.");

                debug!("got {} packets, sending into socket...", inn.slots());
                let chunk = inn.read_chunk(inn.slots()).expect("receive as chunk failed");

                for ip in chunk.into_iter() {
                    match client_seqpacket.send(ip.as_ref()) {
                        Ok(_) => {},
                        Err(err) => {
                            //TODO handle disconnection and reconnection here
                            //TODO automatic reconnection - reconnect timeout of 30s, then error out.
                            //TODO which io error is returned for "connection closed because server going offline"?
                            //TODO which io error is returned for "server unreachable"?
                            error!("{:?}: {}", err.kind(), err);
                            /*
                            match err.kind() {
                                ErrorKind::AddrInUse
                                EndOfFile => break,
                                SomeOtherError => do_something(),
                                _ => panic!("Can't read from file: {}, err {}", filename, e),
                            };
                            */
                        }
                    }

                    //TODO stream
                    /*
                    let mut stream = UnixStream::connect("/path/to/my/socket")?;
                    stream.write_all(b"hello world")?;
                    let mut response = String::new();
                    stream.read_to_string(&mut response)?;
                    println!("{response}");
                    */
                }
            }

            // receive
            //TODO optimize - better to send or receive first? and send|receive first on FBP or on socket?
            //let mut ip_out = Vec::with_capacity(DEFAULT_READ_BUFFER_SIZE);
            //TODO handle case of message longer than read buffer using while loop; how to detect continuing seqpacket?
            match client_seqpacket.recv(&mut buf) {
                Ok(bytes_in) => {
                    debug!("got packet with {} bytes, repeating...", bytes_in);
                    debug!("repeating packet...");
                    out.push(Vec::from(&buf[0..bytes_in])).expect("could not push into OUT");
                    out_wakeup.unpark();
                    debug!("done");
                },
                Err(err) => {
                    //TODO handle disconnection and reconnection here
                    //TODO automatic reconnection - reconnect timeout of 30s, then error out.
                    //TODO which io error is returned for "connection closed because server going offline"?
                    //TODO which io error is returned for "server unreachable"?
                    error!("{:?}: {}", err.kind(), err);
                    /*
                    match err.kind() {
                        ErrorKind::AddrInUse
                        EndOfFile => break,
                        SomeOtherError => do_something(),
                        _ => panic!("Can't read from file: {}, err {}", filename, e),
                    };
                    */
                }
            };

            // are we done?
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
                //TODO close socket
                drop(out);
                out_wakeup.unpark();
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("UnixSocketClient"),
            description: String::from("Drops all packets received on IN port."),    //###
            icon: String::from("trash-o"),  //###
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be dropped"),    //###
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be dropped"),
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
                    description: String::from("data to be dropped"),    //###
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

pub struct UnixSocketServerComponent {
    conf: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for UnixSocketServerComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        UnixSocketServerComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            resp: inports.remove("RESP").expect("found no RESP inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("UnixSocketServer is now run()ning!");
        let mut conf = self.conf;
        trace!("spinning for configuration IP...");
        while conf.is_empty() {
            thread::yield_now();
        }
        //TODO optimize string conversions to listen on a path
        let config = conf.pop().expect("not empty but still got an error on pop");
        let listenpath = std::str::from_utf8(&config).expect("could not parse listenpath as utf8");
        trace!("got path {}", listenpath);
        std::fs::remove_file(&listenpath).ok();
        let listener = std::os::unix::net::UnixListener::bind(std::path::Path::new(listenpath)).expect("bind unix listener socket");
        let resp = &mut self.resp;
        let out = Arc::new(Mutex::new(self.out.sink));
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        //listener.set_nonblocking(true).expect("set listen socket to non-blocking");
        let sockets: Arc<Mutex<HashMap<u32, std::os::unix::net::UnixStream>>> = Arc::new(Mutex::new(HashMap::new()));
        let sockets_ref = Arc::clone(&sockets);
        let out_ref = Arc::clone(&out);
        let out_wakeup_ref = out_wakeup.clone();
        //TODO use that variable and properly terminate the listener thread on network stop - who tells it to stop listening?
        let _listen_thread = thread::Builder::new().name(format!("{}_handler", thread::current().name().expect("could not get component thread name"))).spawn(move || {   //TODO optimize better way to get the current thread's name as String?
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
                    debug!("got a packet, writing into unix socket...");

                    // send into unix socket to peer
                    //TODO add support for multiple client connections - TODO need way to hand over metadata -> IP framing
                    sockets.lock().expect("lock poisoned").iter().next().unwrap().1.write(&ip).expect("could not send data from FBP network into Unix socket connection");   //TODO harden write_timeout() //TODO optimize
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
        debug!("cleaning up");
        std::fs::remove_file(listenpath).unwrap();
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("UnixSocketServer"),
            description: String::from("Unix socket server"),
            icon: String::from("server"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("configuration value, currently the path to listen on"),
                    values_allowed: vec![],
                    value_default: String::from("/tmp/server.sock")
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
                    description: String::from("signal and content data from client connections"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}