use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

// component-specific imports
//use assoc::AssocExt;

/*
TODO this is a draft for a component that can handle S-expressions (Sexp) as input and output
*/

// component-specific structs
/*  TODO implement
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
*/

pub struct SexpComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for SexpComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        SexpComponent {
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Sexp process() called");

        let mut work_units = 0u32;

        // Check signals first (signals are handled regardless of budget)
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        // Process input within budget
        while context.remaining_budget > 0 {
            if let Ok(ip) = self.inn.pop() {
                // read packet - expecting UTF-8 string
                let mut text = ip.as_text()
                    .or_else(|| ip.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                    .unwrap_or("");
                debug!("got a text to trim: {}", &text);

                // trim string
                debug!("len before trim: {}", text.len());
                text = text.trim();
                debug!("len after trim: {}", text.len());

                // send it
                debug!("forwarding trimmed string...");
                let output_msg = FbpMessage::from_str(&text);
                if let Err(_) = self.out.push(output_msg) {
                    error!("Failed to send trimmed string to output");
                    return ProcessResult::Finished;
                }
                work_units += 1;
                context.remaining_budget -= 1;
            } else {
                break;
            }
        }

        // Check if we're done
        if self.inn.is_abandoned() {
            info!("EOF on inport, finishing");
            return ProcessResult::Finished;
        }

        // Signal readiness if we have pending input work
        if self.inn.slots() > 0 {
            context.signal_ready();
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    //TODO
    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("Sexp"),
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
