use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

pub struct HTTPClientComponent {
    req: ProcessEdgeSource,
    out_resp: ProcessEdgeSink,
    out_err: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for HTTPClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        HTTPClientComponent {
            req: inports.remove("REQ").expect("found no REQ inport"),
            out_resp: outports.remove("RESP").expect("found no RESP outport"),
            out_err: outports.remove("ERR").expect("found no ERR outport"),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("HTTPClient is now run()ning!");
        let requests = &mut self.req;    //TODO optimize
        let out_resp = &mut self.out_resp.sink;
        let out_resp_wakeup = self.out_resp.wakeup.expect("got no wakeup handle for outport RESP");
        let out_err = &mut self.out_err.sink;
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
                    self.signals_out.send(b"pong".to_vec()).expect("cloud not send pong");
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
                            out_resp.push(body).expect("could not push into RESP");  //TODO optimize conversion
                            out_resp_wakeup.unpark();
                        },
                        Err(err) => {
                            // send error
                            debug!("got HTTP error, sending...");
                            out_err.push(err.to_string()).expect("could not push into RESP");   //TODO optimize conversion
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