use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};
use tokio::sync::mpsc;

// component-specific
use openai::{
    chat::{ChatCompletion, ChatCompletionMessage, ChatCompletionMessageRole},
    Credentials,
};
use std::time::Duration;

#[derive(Debug)]
enum OpenAIChatState {
    WaitingForConfig,
    WaitingForInitialPrompt,
    Active,
    Finished,
}

#[derive(Debug, Clone)]
struct OpenAIConfig {
    credentials: Credentials,
    model: String,
    context: bool,
}

#[derive(Debug)]
enum OpenAICommand {
    SetConfig(OpenAIConfig),  // set configuration
    SetInitialPrompt(String), // set initial system prompt
    ChatCompletion(Vec<u8>),  // user message bytes
}

#[derive(Debug)]
enum OpenAIResult {
    ChatResponse(String), // AI response
    Error(String),
}

pub struct OpenAIChatComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    // Async operation state
    state: OpenAIChatState,
    config: Option<OpenAIConfig>,
    #[allow(dead_code)]
    messages: Vec<ChatCompletionMessage>,
    cmd_sender: mpsc::UnboundedSender<OpenAICommand>,
    result_receiver: mpsc::UnboundedReceiver<OpenAIResult>,
    #[allow(dead_code)]
    async_thread: Option<std::thread::JoinHandle<()>>,
    //graph_inout: GraphInportOutportHandle,
}

const OPENAI_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

async fn async_openai_main(
    mut cmd_rx: mpsc::UnboundedReceiver<OpenAICommand>,
    result_tx: mpsc::UnboundedSender<OpenAIResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let mut config: Option<OpenAIConfig> = None;
    let mut messages: Vec<ChatCompletionMessage> = Vec::new();

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            OpenAICommand::SetConfig(new_config) => {
                config = Some(new_config);
                debug!("OpenAI config set");
            }
            OpenAICommand::SetInitialPrompt(prompt) => {
                messages.push(ChatCompletionMessage {
                    role: ChatCompletionMessageRole::System,
                    content: Some(prompt),
                    name: None,
                    function_call: None,
                    tool_call_id: None,
                    tool_calls: None,
                });
                debug!("Initial prompt set");
            }
            OpenAICommand::ChatCompletion(msg_bytes) => {
                if let Some(ref cfg) = config {
                    let user_message_content =
                        String::from_utf8(msg_bytes).expect("invalid utf-8 in message");

                    // Prepare messages for this request
                    let request_messages = if cfg.context {
                        // Add user message to context
                        messages.push(ChatCompletionMessage {
                            role: ChatCompletionMessageRole::User,
                            content: Some(user_message_content),
                            name: None,
                            function_call: None,
                            tool_call_id: None,
                            tool_calls: None,
                        });
                        messages.clone()
                    } else {
                        // Single-turn conversation
                        vec![ChatCompletionMessage {
                            role: ChatCompletionMessageRole::User,
                            content: Some(user_message_content),
                            name: None,
                            function_call: None,
                            tool_call_id: None,
                            tool_calls: None,
                        }]
                    };

                    // Build and send request
                    let chat_completion = ChatCompletion::builder(&cfg.model, request_messages)
                        .credentials(cfg.credentials.clone())
                        .create();

                    let response_result =
                        tokio::time::timeout(OPENAI_REQUEST_TIMEOUT, chat_completion).await;

                    match response_result {
                        Ok(Ok(response)) => {
                            let choice = &response.choices[0];
                            let ai_message = &choice.message;

                            // Add AI response to context if enabled
                            if cfg.context {
                                messages.push(ai_message.clone());
                            }

                            let content = ai_message
                                .content
                                .as_ref()
                                .expect("no content in AI response");

                            let _ = result_tx.send(OpenAIResult::ChatResponse(content.clone()));
                            // Wake scheduler to process the result
                            if let Some(ref waker) = scheduler_waker {
                                waker();
                            }
                        }
                        Ok(Err(err)) => {
                            let _ = result_tx
                                .send(OpenAIResult::Error(format!("OpenAI API error: {}", err)));
                            // Wake scheduler to process the error result
                            if let Some(ref waker) = scheduler_waker {
                                waker();
                            }
                        }
                        Err(_) => {
                            let _ = result_tx.send(OpenAIResult::Error(format!(
                                "OpenAI request timed out after {:?}",
                                OPENAI_REQUEST_TIMEOUT
                            )));
                            // Wake scheduler to process the error result
                            if let Some(ref waker) = scheduler_waker {
                                waker();
                            }
                        }
                    }
                } else {
                    let _ =
                        result_tx.send(OpenAIResult::Error("OpenAI not configured".to_string()));
                    // Wake scheduler to process the error result
                    if let Some(ref waker) = scheduler_waker {
                        waker();
                    }
                }
            }
        }
    }
}

impl Component for OpenAIChatComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        let (cmd_sender, cmd_receiver) = mpsc::unbounded_channel();
        let (result_sender, result_receiver) = mpsc::unbounded_channel();

        let async_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async_openai_main(
                cmd_receiver,
                result_sender,
                scheduler_waker,
            ));
        }));

        OpenAIChatComponent {
            conf: inports
                .remove("CONF")
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
            state: OpenAIChatState::WaitingForConfig,
            config: None,
            messages: Vec::new(),
            cmd_sender,
            result_receiver,
            async_thread,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("OpenAIChat process() called");

        let mut work_units = 0u32;

        // Check signals first (signals are handled regardless of budget)
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, shutting down and finishing");
                self.state = OpenAIChatState::Finished;
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

        // Handle state machine
        match self.state {
            OpenAIChatState::WaitingForConfig => {
                // Check if we have CONF configuration
                if let Ok(conf_vec) = self.conf.pop() {
                    debug!("received CONF config, parsing...");

                    // Parse configuration (extracted from original run() method)
                    let url_str = std::str::from_utf8(&conf_vec).expect("invalid utf-8");
                    let url = url::Url::parse(url_str).expect("failed to parse configuration URL");

                    // Get API key
                    let api_key = url
                        .query_pairs()
                        .find(|(key, _)| key == "apikey")
                        .map(|(_, value)| value.to_string())
                        .expect("no API key found in configuration URL");

                    // Get model
                    let model = url
                        .query_pairs()
                        .find(|(key, _)| key == "model")
                        .map(|(_, value)| value.to_string())
                        .unwrap_or_else(|| "gpt-3.5-turbo".to_string());

                    // Get context
                    let context_enabled = url
                        .query_pairs()
                        .find(|(key, _)| key == "context")
                        .and_then(|(_, value)| value.parse().ok())
                        .unwrap_or(false);

                    // Get initial prompt
                    let initialprompt = url
                        .query_pairs()
                        .find(|(key, _)| key == "initialprompt")
                        .and_then(|(_, value)| value.parse().ok())
                        .unwrap_or(false);

                    // Set credentials
                    let credentials = if url.host_str().unwrap_or("default") != "default" {
                        let base_url = format!(
                            "{}://{}{}",
                            url.scheme(),
                            url.host_str().expect("no host in URL"),
                            url.path()
                        );
                        Credentials::new(api_key, base_url)
                    } else {
                        debug!("using default base URL for OpenAI API");
                        Credentials::new(api_key, "")
                    };

                    let config = OpenAIConfig {
                        credentials,
                        model,
                        context: context_enabled,
                    };

                    self.config = Some(config.clone());

                    // Send config to async thread
                    if let Err(_) = self.cmd_sender.send(OpenAICommand::SetConfig(config)) {
                        error!("Failed to send config command");
                        return ProcessResult::Finished;
                    }

                    if initialprompt {
                        self.state = OpenAIChatState::WaitingForInitialPrompt;
                    } else {
                        self.state = OpenAIChatState::Active;
                    }

                    debug!("OpenAI configuration processed");
                    work_units += 1; // Configuration work
                    return ProcessResult::DidWork(work_units);
                } else {
                    // No CONF config yet
                    return ProcessResult::NoWork;
                }
            }
            OpenAIChatState::WaitingForInitialPrompt => {
                // Check if we have initial prompt on IN port
                if let Ok(prompt_bytes) = self.inn.pop() {
                    let prompt_str =
                        String::from_utf8(prompt_bytes).expect("invalid utf-8 in initial prompt");

                    // Send initial prompt to async thread
                    if let Err(_) = self
                        .cmd_sender
                        .send(OpenAICommand::SetInitialPrompt(prompt_str))
                    {
                        error!("Failed to send initial prompt command");
                        return ProcessResult::Finished;
                    }

                    self.state = OpenAIChatState::Active;
                    debug!("Initial prompt set, transitioning to active state");
                    work_units += 1;
                    return ProcessResult::DidWork(work_units);
                } else if self.inn.is_abandoned() {
                    info!("IN port closed while waiting for initial prompt, finishing");
                    self.state = OpenAIChatState::Finished;
                    return ProcessResult::Finished;
                } else {
                    // Still waiting for initial prompt
                    return ProcessResult::NoWork;
                }
            }
            OpenAIChatState::Active => {
                // Check for responses from async thread within budget
                if context.remaining_budget > 0 {
                    if let Ok(result) = self.result_receiver.try_recv() {
                        match result {
                            OpenAIResult::ChatResponse(response) => {
                                debug!("Received AI response, forwarding to output");
                                if let Err(_) = self.out.push(response.as_bytes().to_vec()) {
                                    error!("Failed to send response to output");
                                    return ProcessResult::Finished;
                                }
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            OpenAIResult::Error(e) => {
                                error!("OpenAI operation error: {}", e);
                                work_units += 1; // Error handling is work
                                context.remaining_budget -= 1;
                            }
                        }
                    }
                }

                // Check for incoming messages to send within remaining budget
                if context.remaining_budget > 0 {
                    if let Ok(msg_bytes) = self.inn.pop() {
                        debug!("Received message to send to OpenAI");
                        // Send chat completion command to async thread
                        if let Err(_) = self
                            .cmd_sender
                            .send(OpenAICommand::ChatCompletion(msg_bytes))
                        {
                            error!("Failed to send chat completion command");
                            return ProcessResult::Finished;
                        }
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
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
            OpenAIChatState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
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
                    value_default: String::from("https://default/?apikey=xxx&model=gpt-3.5-turbo&context=false&initialprompt=false"),   //TODO can this be minimized for the default base URL case? I tried but got RelativeUrlWithoutBase https://github.com/servo/rust-url/blob/e654efb9c19732f680f14db43a673a726b834f42/url/src/parser.rs#L384
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

impl Drop for OpenAIChatComponent {
    fn drop(&mut self) {
        debug!("OpenAIChatComponent dropping, sending shutdown command");
        // Note: OpenAI async thread will terminate when cmd_sender is dropped
    }
}
