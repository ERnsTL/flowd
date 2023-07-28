use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

//component-specific
use libloading::{Library, Symbol};
use std::ffi::OsString;

pub struct LibComponent { //<'a> {
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
    //fn_process: Option<libloading::Symbol<'a, unsafe extern fn(&std::ffi::CStr) -> u32>>,
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
impl Component for LibComponent { //<'_> {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        unsafe {
            //TODO load the shared libary
            //TODO if there are any undefined symbols, this panics in some OS-specific function before it bubbles up into libloading -> cannot be caught! argh
            let lib = Library::new(libloading::library_filename("wordcounter")).expect("failed to load library 'wordcounter'");
            //TODO give the process function some shared state that it can mutate
            //TODO cannot return function at this point, can only check - because otherwise error "cannot return reference to value owned by current function" - solution?
            //TODO find way to prepare the function already here at this point
            let _func: Symbol<unsafe extern fn(OsString) -> u32> = lib.get(b"process").expect("failed to get symbol 'process'");
            //self.fn_process = func;

            //TODO get metadata from a global variable in the shared library

            //TODO take the declared inports and outports
            LibComponent {
                inn: inports.remove("IN").expect("found no IN inport"),
                out: outports.remove("OUT").expect("found no OUT outport"),
                signals_in: signals_in,
                signals_out: signals_out,
                //graph_inout: graph_inout,
                lib: lib,
                //fn_process: Some(func),  //TODO cannot return function at this point, can only check - because otherwise error "cannot return reference to value owned by current function" - solution?
            }
        }
    }

    // TODO refactor to receive on inports of the component in the shared library
    fn run(mut self) {
        debug!("LibComponent is now run()ning!");
        let inn = &mut self.inn;    //TODO optimize
        let out = &mut self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup notify handle for outport OUT");
        unsafe {
            let fn_process: libloading::Symbol<unsafe extern fn(&std::ffi::CStr) -> u32> = self.lib.get(b"process").expect("failed to re-get symbol 'process'");
            loop {
                trace!("begin of iteration");
                // check signals
                //TODO optimize, there is also try_recv() and recv_timeout()
                if let Ok(ip) = self.signals_in.try_recv() {
                    //TODO optimize string conversions
                    trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                    // stop signal
                    if ip == b"stop" {   //TODo optimize comparison
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
                    if let Ok(mut ip) = inn.pop() { //TODO normally the IP should be immutable and forwarded as-is into the component library
                        // output the packet data with newline
                        debug!("got a packet, splitting words...");

                        //TODO call library
                        //TODO what is CStr, CString, OsStringExt used for?
                        ip.insert(ip.len(), 0); // insert null byte  //TODO optimize - just for the CStr conversion below that it finds its null byte
                        let res = fn_process(std::ffi::CStr::from_bytes_with_nul_unchecked(ip.as_slice())); //TODO fix this: take care of possible null bytes. Goal: ability to transfer any binary data, incl. null bytes. But then again, the FBP protocol is JSON so would need base64-encoding (CPU intensive!) -> any solution? do usual binary serialization formats use null byte?

                        // forward split words
                        //TODO maybe more than one
                        out.push(res.to_string().into_bytes()).expect("could not push into OUT"); //TODO optimize kludgy conversion
                        //condvar_notify!(&*out_wakeup);
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
                    //condvar_notify!(&*out_wakeup);
                    out_wakeup.unpark();
                    break;
                }

                trace!("-- end of iteration");
                std::thread::park();
                //condvar_block!(&*self.wakeup_notify);
            }
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        //TODO get metadata including ports (though that could be done in new() ) from the library component
        ComponentComponentPayload {
            name: String::from("LibComponent"),
            description: String::from("Loads the given flowd component from a shared library"),
            icon: String::from("bug"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be processed by shared library"),    //TODO
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
                    description: String::from("processed data from IN port"),   //TODO
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}