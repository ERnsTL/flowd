use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

//component-specific
use libloading::{Library, Symbol};

pub struct LibComponent {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    lib: libloading::Library,
}

/*

// paste into libwordcounter.v and compile with:
// v -shared -skip-unused libwordcounter.v
// then symlink into same directory as the flowd-rs binary, probably ./target/debug/

[export: 'process']
fn flowd_process(words string) u64 {
        return u64(words.len - 1)
}

// not sure who inserts this function into the library, either V or TCC, but it is undefined:
// nm -D --undefined-only ./wordcounter.so |grep -v GLIBC

[export: '__bt_init']
fn flowd_init() {
        // nothing to do
        return
}

*/

//TODO how can a component in a shared library become "active", meaning it can wait for some external event and decide by itself when it will generate some output?
//TODO outputs are not only input-driven, but can also come from an external source...
impl Component for LibComponent {
    //<'_> {
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
        unsafe {
            // Load the shared library
            let lib = Library::new(libloading::library_filename("wordcounter"))
                .expect("failed to load library 'wordcounter'");

            // Verify the process function exists
            let _func: Symbol<unsafe extern "C" fn(&std::ffi::CStr) -> u32> =
                lib.get(b"process").expect("failed to get symbol 'process'");

            LibComponent {
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
                lib: lib,
            }
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("LibComponent process() called");

        let mut work_units = 0u32;

        // Check signals first (signals are handled regardless of budget)
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!(
                    "received unknown signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                )
            }
        }

        // Process input within budget
        while context.remaining_budget > 0 {
            if let Ok(mut ip) = self.inn.pop() {
                debug!("got a packet, calling library function...");

                // Call library function (unsafe)
                let res = unsafe {
                    let fn_process: Symbol<unsafe extern "C" fn(&std::ffi::CStr) -> u32> = self
                        .lib
                        .get(b"process")
                        .expect("failed to get symbol 'process'");

                    // Add null byte for C string
                    ip.push(0);
                    let cstr = std::ffi::CStr::from_bytes_with_nul_unchecked(&ip);
                    fn_process(cstr)
                };

                // Send result to output
                if let Err(_) = self.out.push(res.to_string().into_bytes()) {
                    error!("Failed to send library result to output");
                    return ProcessResult::Finished;
                }

                work_units += 1;
                context.remaining_budget -= 1;
                debug!("library function called, result sent");
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

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        //TODO get metadata including ports (though that could be done in new() ) from the library component
        ComponentComponentPayload {
            name: String::from("LibComponent"),
            description: String::from("Loads the given flowd component from a shared library"),
            icon: String::from("bug"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("IN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("data to be processed by shared library"), //TODO
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("processed data from IN port"), //TODO
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}
