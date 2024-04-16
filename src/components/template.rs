use std::{sync::{Arc, Mutex}, thread};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use tera::Tera;

/*
TODO if too much time on hand - evaluate TT2 as alternative to Tera -> https://www.template-toolkit.org/#
*/

pub struct TeraTemplateComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for TeraTemplateComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        TeraTemplateComponent {
            conf: inports.remove("TEMPLATE").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("TeraTemplate is now run()ning!");
        let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // check config port
        trace!("read config IP");
        while conf.is_empty() {
            thread::yield_now();
        }
        let Ok(template) = conf.pop() else { trace!("no config IP received - exiting"); return; };

        // configure
        // create a new Tera instance and add a template from a string
        //let mut tera = Tera::new("templates/**/*").unwrap();
        let mut tera = Tera::default();
        unsafe {
            //let ip_str = String::from_utf8(ip).expect("invalid utf-8 in IP");
            tera.add_raw_template("a", std::str::from_utf8_unchecked(&template)).unwrap();
        }

        // main loop
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
                    self.signals_out.send(b"pong".to_vec()).expect("could not send pong");
                }
            }

            // check in port
            loop {
                if let Ok(ip) = inn.pop() {
                    debug!("got a packet, processing...");

                    // prepare the context with some data
                    let mut context = tera::Context::new();
                    unsafe {
                        //let ip_str = String::from_utf8(ip).expect("invalid utf-8 in IP");
                        let ip_str = std::str::from_utf8_unchecked(&ip);
                        context.insert("ip", &ip_str);
                    }

                    // render the template with the given context
                    let rendered = tera.render("a", &context).expect("failed to render");
                    trace!("{}", rendered);

                    // send result to out port
                    out.push(rendered.trim().as_bytes().to_vec()).expect("could not push into OUT");   //TODO optimize - allocations?
                    out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }

            // are we done?
            if inn.is_abandoned() {
                // input closed, nothing more to do
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