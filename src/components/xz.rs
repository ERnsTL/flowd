use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use std::io::prelude::*;
use xz::write::{XzEncoder, XzDecoder};

pub struct XzCompressComponent {
    //conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

const COMPRESSION_LEVEL: u32 = 9;   //TODO maybe make configurable

impl Component for XzCompressComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        XzCompressComponent {
            //conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("XzCompress is now run()ning!");
        //let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        //trace!("read config IPs");
        /*
        while conf.is_empty() {
            thread::yield_now();
        }
        */
        //let Ok(opts) = conf.pop() else { trace!("no config IP received - exiting"); return; };

        // configure
        // NOTE: nothing to be done here

        // main loop
        loop {
            trace!("begin of iteration");
            // check signals
            if let Ok(ip) = self.signals_in.try_recv() {
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
            loop {
                if let Ok(ip) = inn.pop() {
                    // prepare packet
                    debug!("got a packet, compressing...");
                    // nothing to do

                    // compress
                    //TODO optimize - currently, every packet is processed with a new Encoder instance
                    let vec_buf = Vec::new();
                    let mut compressor = XzEncoder::new(vec_buf, COMPRESSION_LEVEL);
                    compressor.write(&ip).expect("failed to write into encoder");
                    debug!("compression: {} bytes in, {} bytes out", compressor.total_in(), compressor.total_out());
                    let vec_out = compressor.finish().expect("failed to finish encoding");

                    // send it
                    debug!("forwarding...");
                    out.push(vec_out).expect("could not push into OUT");
                    out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }

            // are we done?
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
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
            name: String::from("XzCompress"),
            description: String::from("Reads IPs as UTF-8 strings, applies text replacements and forwards the processed string IPs."),  //###
            icon: String::from("cut"),  //###
            subgraph: false,
            in_ports: vec![
                /*
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("TODO"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                */
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("string IPs to process"),
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
                    description: String::from("IPs with strings, replacements applied"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

pub struct XzDecompressComponent {
    //conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for XzDecompressComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        XzDecompressComponent {
            //conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("XzDecompress is now run()ning!");
        //let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        //trace!("read config IPs");
        /*
        while conf.is_empty() {
            thread::yield_now();
        }
        */
        //let Ok(opts) = conf.pop() else { trace!("no config IP received - exiting"); return; };

        // configure
        // NOTE: nothing to be done here

        // main loop
        loop {
            trace!("begin of iteration");
            // check signals
            if let Ok(ip) = self.signals_in.try_recv() {
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
            loop {
                if let Ok(ip) = inn.pop() {
                    // prepare packet
                    // nothing to do
                    debug!("got a packet, decompressing...");

                    // decompress //###
                    //TODO optimize - currently, every packet is processed with a new Encoder instance
                    let vec_buf = Vec::new();
                    let mut compressor = XzEncoder::new(vec_buf, COMPRESSION_LEVEL);
                    compressor.write(&ip).expect("failed to write into encoder");
                    debug!("decompression: {} bytes in, {} bytes out", compressor.total_in(), compressor.total_out());
                    let vec_out = compressor.finish().expect("failed to finish encoding");

                    
                    // Round trip some bytes from a byte source, into a compressor, into a
                    // decompressor, and finally into a vector.
                    /*
                    let data = "Hello, World!".as_bytes();
                    let compressor = XzEncoder::new(data, 9);
                    let mut decompressor = XzDecoder::new(compressor);

                    let mut contents = String::new();
                    decompressor.read_to_string(&mut contents).unwrap();
                    assert_eq!(contents, "Hello, World!");
                    */
                    //###

                    // send it
                    debug!("sending...");
                    //TODO optimize .to_vec() copies the contents - is Vec::from faster?
                    out.push(ip).expect("could not push into OUT");
                    out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }

            // are we done?
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
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
            name: String::from("XzDecompress"),
            description: String::from("Reads IPs as UTF-8 strings, applies text replacements and forwards the processed string IPs."),  //###
            icon: String::from("cut"),  //###
            subgraph: false,
            in_ports: vec![
                /*
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("TODO"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                */
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("string IPs to process"),
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
                    description: String::from("IPs with strings, replacements applied"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}