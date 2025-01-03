use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific imports
use assoc::AssocExt;

/*
TODO this is a draft for a component that can handle S-expressions (Sexp) as input and output
*/

// component-specific structs
enum Sexp {
    None,
    AtomU8(u8),     // small number
    AtomU32(u32),   // big number
    AtomVecU8(Vec<u8>), // "string"
    List(Vec<Sexp>),    // recursive
    // specialized
    AttributeU8(u8, Vec<u8>),   // key-value pair by u8 code
    AttributeU32(u32, Vec<u8>), // key-value pair by u32 code
    AttributeVecU8(Vec<u8>, Vec<u8>),   // key-value pair by "string" key
    //TODO optimize - wish this was possible using self-referential struct variants, but Rust (2024-04) says that a type is needed and an enum variant is not a type
    //TODO optimize - following ones are valid Sexp structures, but not validated by the type system
    //TODO optimize - any way to get rid of these Box<>?
    Message {
        header: Box<crate::Sexp>,
        body: Box<crate::Sexp>
    },    // Header and Body
    Header(Vec<crate::Sexp>), // Vec<Sexp::AttributeVecU8>
    Body(Box<crate::Sexp>), // Sexp::AtomVecU8
    XMLLike {
        attributes: Vec<crate::Sexp>,
        body: Box<crate::Sexp>
    },    // attributes as Vec<Sexp::AttributeVecU8>, body as Sexp::AtomVecU8 or Sexp::List (which can also be None)
}

struct Message {
    header: Header,
    body: Body,
}

struct Header {
    attributes: Vec<(Vec<u8>, Vec<u8>)>,
}

struct Body {
    data: Vec<u8>,  // Sexp::AtomVecU8
}

struct XMLLike {
    attributes: Vec<(Vec<u8>, Vec<u8>)>,
    body: XMLBody,
}

enum XMLBody {
    Recursive(Box<XMLBody>),    // Sexp::List
    Atom(Vec<u8>),  // Sexp::AtomVecU8
    None,
}

pub struct TrimComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for TrimComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        TrimComponent {
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("Trim is now run()ning!");
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");
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
                    // read packet - expecting UTF-8 string
                    let mut text = std::str::from_utf8(&ip).expect("non utf-8 data");
                    debug!("got a text to trim: {}", &text);

                    // trim string
                    debug!("len before trim: {}", text.len());
                    text = text.trim();
                    debug!("len after trim: {}", text.len());

                    // send it
                    debug!("forwarding trimmed string...");
                    //TODO optimize .to_vec() copies the contents - is Vec::from faster?
                    out.push(Vec::from(text)).expect("could not push into OUT");
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
            name: String::from("Trim"),
            description: String::from("Reads IPs as UTF-8 strings and trims whitespace at beginning and end, forwarding the trimmed string."),
            icon: String::from("cut"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs with strings to trim, one string per IP"),
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
                    description: String::from("trimmed strings"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}