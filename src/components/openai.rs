use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

use openai::chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole};

pub struct OpenAIChatComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for OpenAIChatComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        OpenAIChatComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("OpenAIChat is now run()ning!");
        let conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");

        // get configuration arguments
        let url = url::Url::parse(&url).expect("failed to parse configuration URL");
        //### let args = url
        //### let api_key = 

        // set configuration
        openai::set_key(api_key);

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
                    debug!("repeating packet...");
                    out.push(ip).expect("could not push into OUT");
                    out_wakeup.unpark();
                    debug!("done");

                    //###
                    // initial prompt
                    /*
                    let mut messages = vec![ChatCompletionMessage {
                        role: ChatCompletionMessageRole::System,
                        content: Some("You are a large language model built into a command line interface as an example of what the `openai` Rust library made by Valentine Briese can do.".to_string()),
                        name: None,
                        function_call: None,
                    }];
                    */
                    let mut messages = vec![];
                
                    let user_message_content = "Tell me what OS/2 is in 1 sentence.".to_owned();
                
                    //stdin().read_line(&mut user_message_content).unwrap();
                    messages.push(ChatCompletionMessage {
                        role: ChatCompletionMessageRole::User,
                        content: Some(user_message_content),
                        name: None,
                        function_call: None,
                    });
                
                    let chat_completion = ChatCompletion::builder("gpt-3.5-turbo", messages.clone())
                        .create()
                        .await
                        .unwrap();
                    let returned_message = chat_completion.choices.first().unwrap().message.clone();
                
                    println!(
                        "{:#?}: {}",
                        &returned_message.role,
                        &returned_message.content.clone().unwrap().trim()
                    );
                
                    // add returned message into context
                    //messages.push(returned_message);
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
            name: String::from("OpenAIChat"),
            description: String::from("Copies data as-is from IN port to OUT port."),//###
            icon: String::from("arrow-right"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL which includes options"),  //###
                    values_allowed: vec![],
                    value_default: String::from("?key=val")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be repeated on outport"),//###
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
                    description: String::from("repeated data from IN port"),//###
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}