use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

// component-specific
use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};

/*
Ability to extract a value from a JSON data structure.
Use case is for example getting sensor value out of a JSON data structure loaded via HTTP from a CPS/IoT device.

structure from https://github.com/01mf02/jaq/blob/101619985b3b99707be8863727fa1c08f350559c/jaq/src/main.rs#L418
and https://docs.rs/jaq-interpret/1.2.1/jaq_interpret/

TODO add options from jaq, for example if result is a string if it should be quoted or not
*/

pub struct JSONQueryComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    query: Option<jaq_interpret::Filter<Val>>,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for JSONQueryComponent {
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
        JSONQueryComponent {
            conf: inports
                .remove("QUERY")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
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
            query: None,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("JSONQuery process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            let signal_text = ip.as_text().unwrap_or("");
            trace!("received signal ip: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(FbpMessage::from_str("pong"));
            } else {
                warn!("received unknown signal ip: {}", signal_text)
            }
        }

        // Check if we have configuration
        if self.query.is_none() {
            if let Ok(filter_msg) = self.conf.pop() {
                let filter_str = filter_msg.as_text().expect("invalid text in config IP");
                debug!("received query: {}", filter_str);

                // Configure
                let mut parse_context = ParseCtx::new(Vec::new());

                // Parse the filter
                let (filter, errors) = jaq_parse::parse(filter_str, jaq_parse::main());
                if !errors.is_empty() {
                    for err in errors {
                        error!("filter parse error: {:?}", err);
                    }
                    return ProcessResult::Finished; // Invalid config, finish
                }

                // Compile the filter
                let filter = parse_context.compile(filter.unwrap());
                if !parse_context.errs.is_empty() {
                    for (err, range) in parse_context.errs {
                        error!(
                            "filter compile error: {:?} in character {} to {}",
                            err.to_string(),
                            range.start,
                            range.end
                        );
                    }
                    return ProcessResult::Finished; // Invalid config, finish
                }

                self.query = Some(filter);
                return ProcessResult::DidWork(1); // Configuration processed
            } else {
                // No config yet
                return ProcessResult::NoWork;
            }
        }

        let filter = self.query.as_ref().unwrap();
        let inputs = RcIter::new(core::iter::empty());

        let mut work_units = 0;

        // Process available input packets within remaining budget
        while context.remaining_budget > 0 && !self.inn.is_empty() {
            if let Ok(ip) = self.inn.pop() {
                debug!("got a packet, processing...");
                let json_bytes = ip.as_bytes().unwrap_or(&[]);
                match serde_json::from_slice::<serde_json::Value>(json_bytes) {
                    Ok(input) => {
                        // Iterator over the output values
                        let result = filter.run((Ctx::new([], &inputs), Val::from(input)));

                        for value in result {
                            match value {
                                Ok(val) => {
                                    // Prepare packet
                                    let vec_out = format!("{}", val).into_bytes();

                                    // Send it
                                    debug!("sending...");
                                    let output_msg = FbpMessage::from_bytes(vec_out);
                                    if let Ok(()) = self.out.push(output_msg) {
                                        debug!("done");
                                        work_units += 1;
                                        context.remaining_budget -= 1;
                                    } else {
                                        // Output buffer full, stop processing for now
                                        break;
                                    }
                                }
                                Err(err) => {
                                    let vec_out = err.to_string().into_bytes();
                                    error!(
                                        "error while filtering: {} - discarding",
                                        std::str::from_utf8(&vec_out).expect("invalid utf-8")
                                    );
                                }
                            }
                        }
                    }
                    Err(err) => {
                        warn!("failed to parse IP as JSON: {}", err);
                    }
                }

                if context.remaining_budget == 0 {
                    break;
                }
            } else {
                break;
            }
        }

        // Check if input is abandoned
        if self.inn.is_abandoned() {
            info!("EOF on inport, shutting down");
            return ProcessResult::Finished;
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
        ComponentComponentPayload {
            name: String::from("JSONQuery"),
            description: String::from("Reads IPs containing JSON data, filters them using jaq/jq and sends the processed resp. filtered data to the OUT port."),
            icon: String::from("filter"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("QUERY"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("filter to apply to the JSON data, in jaq/jq filter syntax"),
                    values_allowed: vec![],
                    value_default: String::from(".[]")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs to process, expected to contain JSON data"),
                    values_allowed: vec![],
                    value_default: String::from("[\"Hello\", \"world\"]")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("processed IPs with filtered JSON data"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
