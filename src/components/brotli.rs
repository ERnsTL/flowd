use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
//use std::io::prelude::*;

pub struct BrotliCompressComponent {
    //conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

const COMPRESSION_LEVEL: u32 = 9;   //TODO maybe make configurable

impl Component for BrotliCompressComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        BrotliCompressComponent {
            //conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("BrotliCompress is now run()ning!");
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

                    // compress//###
                    //TODO optimize - currently, every packet is processed with a new Encoder instance
                    //TODO support for chunks via open bracket, closing bracket
                    let vec_buf = Vec::new();
                    let mut compressor = XzEncoder::new(vec_buf, COMPRESSION_LEVEL);
                    compressor.write(&ip).expect("failed to write into encoder");
                    //TODO this does not take into account the final bytes written by finish(), but when requesting the totals after the call to finish(), there is a borrow checker error
                    debug!("compression: {} bytes in, {} bytes out", compressor.total_in(), compressor.total_out());
                    let vec_out = compressor.finish().expect("failed to finish encoding");

                    // send it
                    debug!("sending...");
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
            name: String::from("BrotliCompress"),
            description: String::from("Reads data IPs, compresses each using Brotli and sends the compressed data to the OUT port."),
            icon: String::from("compress"),
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
                    description: String::from("IPs to compress"),
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
                    description: String::from("compressed IPs"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

pub struct BrotliDecompressComponent {
    //conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for BrotliDecompressComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        BrotliDecompressComponent {
            //conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("BrotliDecompress is now run()ning!");
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
                    debug!("got a packet, decompressing...");
                    // nothing to do

                    // decompress
                    //###
                    //TODO optimize - currently, every packet is processed with a new Decoder instance
                    //TODO support for chunks via open bracket, closing bracket
                    let vec_buf = Vec::new();
                    let mut decompressor = XzDecoder::new(vec_buf);
                    decompressor.write(&ip).expect("failed to write into decoder");
                    debug!("decompression: {} bytes in, {} bytes out", decompressor.total_in(), decompressor.total_out());
                    let vec_out = decompressor.finish().expect("failed to finish decoding");
                    
                    // send it
                    debug!("sending...");
                    //TODO optimize .to_vec() copies the contents - is Vec::from faster?
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
            name: String::from("BrotliDecompress"),
            description: String::from("Reads IPs, applies Brotli decompression and forwards the decompressed data to OUT port."),
            icon: String::from("expand"),
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
                    description: String::from("compressed Brotli data"),
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
                    description: String::from("decompressed data"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}