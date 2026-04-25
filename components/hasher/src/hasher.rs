use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, info, trace};

// component-specific
use std::hash::{BuildHasher, Hasher};
use twox_hash::RandomXxHashBuilder64;

pub struct HasherComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for HasherComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
    ) -> Self
    where
        Self: Sized,
    {
        HasherComponent {
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
        debug!("Hasher is now process()ing!");
        let mut work_units = 0u32;

        //TODO support for multiple hashing algorithms, needs OPTS inport
        //  fnv = good for small inputs (a few bytes) otherwise xx is better for large inputs, siphash (default Rust) is mediocrebust stable overall
        //  comparison:  https://cglab.ca/~abeinges/blah/hash-rs/ resp. https://github.com/Gankra/hash-rs
        //TODO support for output format selection (hex or binary or decimal)
        let hasher_factory: RandomXxHashBuilder64 = Default::default();
        let mut hasher = hasher_factory.build_hasher();

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            // stop signal
            if ip == b"stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            }
        }

        // Process input messages within budget
        while context.remaining_budget > 0 {
            if let Ok(ip) = self.inn.pop() {
                debug!("hashing packet...");
                hasher.write(&ip);
                let hash_value = hasher.finish();
                let outip = format!("{:016x}", hash_value).into_bytes();

                match self.out.push(outip) {
                    Ok(_) => {
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("hashed and sent packet");
                    }
                    Err(_) => {
                        // Output buffer full, yield control back to scheduler
                        debug!("output buffer full, yielding to scheduler");
                        break;
                    }
                }
            } else {
                // No more input available
                break;
            }
        }

        // Check if input is abandoned (EOF)
        if self.inn.is_abandoned() {
            info!("EOF on inport, finishing");
            return ProcessResult::Finished;
        }

        // Return appropriate result based on work done
        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("Hasher"),
            description: String::from(
                "Hashes each IP from IN port, sending the hash value to the OUT port.",
            ),
            icon: String::from("check"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("IN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("data to be hashed"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("hash value of the input data"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}
