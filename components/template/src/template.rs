use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, info, trace, warn};

// component-specific
use tera::Tera;

//TODO evaluate TT2 as alternative to Tera -> https://www.template-toolkit.org/#

pub struct TeraTemplateComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    template: Option<Tera>,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for TeraTemplateComponent {
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
        TeraTemplateComponent {
            conf: inports
                .remove("TEMPLATE")
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
            template: None,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("TeraTemplate process() called");

        // Check signals first
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

        // Check if we have configuration
        if self.template.is_none() {
            if let Ok(template_data) = self.conf.pop() {
                debug!("received template");
                let template_str = template_data.as_text().expect("template must be text");

                // Configure
                let mut tera = Tera::default();
                match tera.add_raw_template("a", template_str) {
                    Ok(_) => {
                        self.template = Some(tera);
                        return ProcessResult::DidWork(1); // Configuration processed
                    }
                    Err(err) => {
                        warn!("failed to add template: {}", err);
                        return ProcessResult::Finished; // Invalid config, finish
                    }
                }
            } else {
                // No config yet
                return ProcessResult::NoWork;
            }
        }

        let tera = self.template.as_ref().unwrap();

        let mut work_units = 0;

        // Process available input packets within remaining budget
        while context.remaining_budget > 0 && !self.inn.is_empty() {
            if let Ok(ip) = self.inn.pop() {
                debug!("got a packet, processing...");

                // Prepare the context with some data
                let mut template_context = tera::Context::new();
                let ip_str = ip.as_text().expect("input must be text");
                template_context.insert("ip", &ip_str);

                // Render the template with the given context
                match tera.render("a", &template_context) {
                    Ok(rendered) => {
                        trace!("{}", rendered);

                        // Send result to out port
                        debug!("sending...");
                        let msg = FbpMessage::from_text(rendered.trim().to_string());
                        if let Ok(()) = self.out.push(msg) {
                            debug!("done");
                            work_units += 1;
                            context.remaining_budget -= 1;
                        } else {
                            // Output buffer full, stop processing for now
                            break;
                        }
                    }
                    Err(err) => {
                        warn!("failed to render template: {}", err);
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
            name: String::from("TeraTemplate"),
            description: String::from("Sends IPs through the template given on TEMPLATE and the rendered result to the outport."),
            icon: String::from("file-text-o"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("TEMPLATE"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("the template source code"),
                    values_allowed: vec![],
                    value_default: String::from(r#"
                        {% set in = ip | int %}
                        {% if in < 10  %}
                            -10
                        {% elif in >= 10 and in < 50 %}
                            10
                        {% else %}
                            ERROR
                        {% endif %}
                    "#),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data inputs to be processed by the template"),
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
                    description: String::from("rendered template output"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
