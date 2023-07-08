use std::sync::{Condvar, Arc, Mutex};
use crate::{condvar_block, condvar_notify, ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

use std::io::{Write, Read};
use std::thread::{self};
use std::collections::HashMap;

pub struct UnixSocketServerComponent {
    conf: ProcessEdgeSource,
    resp: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
    wakeup_notify: Arc<(Mutex<bool>, Condvar)>,
}

impl Component for UnixSocketServerComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, graph_inout: Arc<Mutex<GraphInportOutportHolder>>, wakeup_notify: Arc<(Mutex<bool>, Condvar)>) -> Self where Self: Sized {
        UnixSocketServerComponent {
            conf: inports.remove("CONF").expect("found no CONF inport"),
            resp: inports.remove("RESP").expect("found no RESP inport"),
            out: outports.remove("OUT").expect("found no OUT outport"),
            signals_in: signals_in,
            signals_out: signals_out,
            graph_inout: graph_inout,
            wakeup_notify: wakeup_notify,
        }
    }

    fn run(mut self) {
        debug!("UnixSocketServer is now run()ning!");
        let conf = &mut self.conf;
        trace!("spinning for listen path on CONF...");
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
        let out_wakeup = self.out.wake_notify;

        //listener.set_nonblocking(true).expect("set listen socket to non-blocking");
        let sockets: Arc<Mutex<HashMap<u32, std::os::unix::net::UnixStream>>> = Arc::new(Mutex::new(HashMap::new()));
        let sockets_ref = Arc::clone(&sockets);
        let out_ref = Arc::clone(&out);
        let out_wakeup_ref = Arc::clone(&out_wakeup);
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
                        let out_wakeup_ref2 = Arc::clone(&out_wakeup_ref);
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
                                    condvar_notify!(&*out_wakeup_ref2);
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
            //###thread::park();
            condvar_block!(&*self.wakeup_notify);
        }
        debug!("cleaning up");
        std::fs::remove_file(listenpath).unwrap();
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("UnixSocketServer"),
            description: String::from("Unix socket server"),
            icon: String::from("bug"),
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
                    description: String::from("response data from downstream proess for each connection"),
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
                    description: String::from("signal and content data for Unix socket connections"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}