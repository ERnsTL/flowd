use std::{io::{Read, Write}, net::TcpStream, sync::{Arc, Mutex}};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use std::net::SocketAddr;
use std::time::Duration;

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