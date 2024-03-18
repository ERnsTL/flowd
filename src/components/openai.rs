use std::{sync::{Arc, Mutex}, thread};
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
        let mut conf = self.conf;
        let mut inn = self.inn;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");

        // get configuration arguments
        let url = url::Url::parse(url_str).expect("failed to parse configuration URL");
        let mut query_pairs = url.query_pairs();    // TODO optimize why mut?
        // get API key
        let api_key;
        if let Some((_key, value)) = query_pairs.find(|(key, _)| key == "apikey") {
            api_key = value.to_string();
        } else {
            error!("no API key found in configuration URL - exiting");
            return;
        }
        // get model
        let model;
        if let Some((_key, value)) = query_pairs.find(|(key, _)| key == "model") {
            model = value.to_string();  //TODO optimize and use &str
        } else {
            trace!("no model found in configuration URL - assuming default");
            model = "gpt-3.5-turbo".to_owned();
        }
        // get context
        let context: bool;
        if let Some((_key, value)) = query_pairs.find(|(key, _)| key == "context") {
            context = value.parse().expect("could not parse context value in configuration URL as boolean");
        } else {
            trace!("no context found in configuration URL - assuming default");
            context = false;
        }
        // get initial prompt
        let initialprompt: bool;
        if let Some((_key, value)) = query_pairs.find(|(key, _)| key == "initialprompt") {
            initialprompt = value.parse().expect("could not parse initialprompt value in configuration URL as boolean");
        } else {
            trace!("no initial prompt found in configuration URL - assuming default");
            initialprompt = false;
        }

        // set configuration
        // set API key
        openai::set_key(api_key);
        // set base URL
        if url.host_str().expect("no host in URL") != "default" {
            let base_url = url.scheme().to_owned() + "://" + url.host_str().expect("no host in URL") + url.path();
            openai::set_base_url(base_url);
        } else {
            debug!("using default base URL for OpenAI API");
        }
        // set initial prompt
        let mut messages = vec![];
        debug!("waiting for initial prompt IP");
        if initialprompt {
            // read initial prompt from inport
            loop {
                if let Ok(ip) = inn.pop() {
                    let initialprompt_str = String::from_utf8(ip).expect("invalid utf-8");
                    // set initial prompt
                    messages.push(ChatCompletionMessage {
                        role: ChatCompletionMessageRole::System,
                        content: Some(initialprompt_str),
                        name: None,
                        function_call: None,
                    });
                    break;
                } else {
                    thread::yield_now();
                }
            }
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
                    debug!("got a packet, sending to AI...");
                    let user_message_content = String::from_utf8(ip).expect("invalid utf-8 in IP");

                    // put into context, if desired
                    if context {
                        messages.push(ChatCompletionMessage {
                            role: ChatCompletionMessageRole::User,
                            content: Some(user_message_content),
                            name: None,
                            function_call: None,
                        });
                    } else {
                        messages = vec![ChatCompletionMessage {
                            role: ChatCompletionMessageRole::User,
                            content: Some(user_message_content),
                            name: None,
                            function_call: None,
                        }];
                    }

                    // build request
                    let chat_completion = ChatCompletion::builder(&model, messages.clone()).create();
                    // send request and wait for result
                    let returned_message_res = tokio::runtime::Runtime::new().unwrap().block_on(chat_completion); //TODO optimize
                    let returned_message_inner = returned_message_res.expect("failed to get OpenAI response");
                    let returned_message_inner2 = &returned_message_inner.choices[0];
                    let returned_message_inner3 = &returned_message_inner2.message;
                    // add returned message into context
                    if context {
                        messages.push(returned_message_inner3.clone());
                    }
                    let returned_message = returned_message_inner3.content.as_ref().expect("no content in returned message");

                    // send response to out port
                    out.push(returned_message.as_bytes().to_vec()).expect("could not push into OUT");   //TODO optimize - avoid copy, why isnt it possible to just get the message from the returned chat completion?
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
            name: String::from("OpenAIChat"),
            description: String::from("Sends IPs to an OpenAI model via the Chat API - the most popular being ChatGPT - and sends the AI response as a potentially multi-line IP to the outport."),
            icon: String::from("wechat"), // robot would be best, but there is no such icon in free font-awesome
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL which includes options in the query string"),
                    values_allowed: vec![],
                    value_default: String::from("https://default/?apikey=xxx&model=gpt-3.5-turbo&context=false&initialprompt=false"),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("chat prompts from the user"),
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
                    description: String::from("response chat completion message"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}