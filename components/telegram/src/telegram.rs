use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, create_io_channels,
};
use log::{debug, error, info, trace, warn};

// component-specific
use std::time::Duration;
use teloxide::prelude::*;

/*
TODO harden - the bot's Telegram username associated with the bot token is a global identifier for the bot, making it possible for anybody to open a chat to the bot and thus send data into your flowd network
TODO currently replying messages to the chat ID the latest message came from - add support for multiple chats, effectively turing this component into a server-type component in the senso of a TCP server component handling multiple clients
TODO add support for sending and receiving images, files, documents, also for command list, buttons, selection possibilities etc.
*/

#[derive(Debug)]
enum TelegramBotState {
    WaitingForConfig,
    Active,
    Finished,
}

pub struct TelegramBotComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    // Async operation state
    state: TelegramBotState,
    config: Option<TelegramBotConfig>,
    // ADR-017: Bounded IO channels
    cmd_sender: std::sync::mpsc::SyncSender<TelegramBotCommand>,
    result_receiver: std::sync::mpsc::Receiver<TelegramBotResult>,
    #[allow(dead_code)]
    async_thread: Option<std::thread::JoinHandle<()>>,
    //graph_inout: GraphInportOutportHandle,
}

#[derive(Debug, Clone)]
struct TelegramBotConfig {
    #[allow(dead_code)]
    bot_token: String,
}

#[derive(Debug)]
enum TelegramBotCommand {
    InitBot(String),
    SendMessage(String),
    SetChatId(i64),
    Shutdown,
}

#[derive(Debug)]
enum TelegramBotResult {
    BotInitialized,
    MessageReceived(Vec<u8>, i64), // message bytes and chat ID
    MessageSent,
    Error(String),
}

const TELEGRAM_SEND_TIMEOUT: Duration = Duration::from_secs(10);

async fn async_telegram_main(
    cmd_rx: std::sync::mpsc::Receiver<TelegramBotCommand>,
    result_tx: std::sync::mpsc::SyncSender<TelegramBotResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let scheduler_waker = scheduler_waker;
    let mut bot: Option<Bot> = None;
    let mut chat_id: i64 = 0;
    let mut dispatcher_handle: Option<tokio::task::JoinHandle<()>> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            TelegramBotCommand::InitBot(token) => {
                debug!("Initializing Telegram bot");
                let new_bot = Bot::new(token);
                let bot_ref = new_bot.clone();
                let result_tx_clone = result_tx.clone();
                let scheduler_waker_dispatch = scheduler_waker.clone();

                // Start message dispatcher
                let handle = tokio::spawn(async move {
                    let handler =
                        Update::filter_message().endpoint(move |_bot: Bot, msg: Message| {
                            let result_tx = result_tx_clone.clone();
                            let scheduler_waker_clone = scheduler_waker_dispatch.clone();
                            async move {
                                // Store chat ID for responses
                                // Note: In a real implementation, this would need thread-safe storage
                                // For now, we'll handle this in the process() method

                                // Send message to component
                                if let Some(text) = msg.text() {
                                    let _ = result_tx.send(TelegramBotResult::MessageReceived(
                                        text.as_bytes().to_vec(),
                                        msg.chat.id.0,
                                    ));
                                    if let Some(ref waker) = scheduler_waker_clone {
                                        waker();
                                    }
                                }
                                ResponseResult::Ok(())
                            }
                        });

                    let mut builder = Dispatcher::builder(bot_ref, handler).build();
                    let _ = builder.dispatch().await;
                });

                bot = Some(new_bot);
                dispatcher_handle = Some(handle);
                let _ = result_tx.send(TelegramBotResult::BotInitialized);
                if let Some(ref waker) = scheduler_waker {
                    waker();
                }
            }
            TelegramBotCommand::SendMessage(text) => {
                if let Some(bot_ref) = &bot {
                    if chat_id != 0 {
                        debug!("Sending message to Telegram chat {}", chat_id);
                        let send_result = tokio::time::timeout(
                            TELEGRAM_SEND_TIMEOUT,
                            bot_ref.send_message(ChatId(chat_id), text).send(),
                        )
                        .await;

                        match send_result {
                            Ok(Ok(_)) => {
                                let _ = result_tx.send(TelegramBotResult::MessageSent);
                                if let Some(ref waker) = scheduler_waker {
                                    waker();
                                }
                            }
                            Ok(Err(err)) => {
                                let _ = result_tx.send(TelegramBotResult::Error(format!(
                                    "Send failed: {}",
                                    err
                                )));
                                if let Some(ref waker) = scheduler_waker {
                                    waker();
                                }
                            }
                            Err(_) => {
                                let _ = result_tx.send(TelegramBotResult::Error(format!(
                                    "Send timed out after {:?}",
                                    TELEGRAM_SEND_TIMEOUT
                                )));
                                if let Some(ref waker) = scheduler_waker {
                                    waker();
                                }
                            }
                        }
                    } else {
                        let _ =
                            result_tx.send(TelegramBotResult::Error("No chat ID set".to_string()));
                        if let Some(ref waker) = scheduler_waker {
                            waker();
                        }
                    }
                } else {
                    let _ =
                        result_tx.send(TelegramBotResult::Error("Bot not initialized".to_string()));
                    if let Some(ref waker) = scheduler_waker {
                        waker();
                    }
                }
            }
            TelegramBotCommand::SetChatId(new_chat_id) => {
                chat_id = new_chat_id;
                debug!("Chat ID set to {}", chat_id);
            }
            TelegramBotCommand::Shutdown => {
                debug!("Shutting down Telegram bot");
                if let Some(handle) = dispatcher_handle.take() {
                    handle.abort();
                }
                break;
            }
        }
    }
}

impl Component for TelegramBotComponent {
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
        // ADR-017: Create bounded IO channels
        let (cmd_sender, cmd_receiver, result_sender, result_receiver) = create_io_channels::<TelegramBotCommand, TelegramBotResult>();

        let async_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async_telegram_main(
                cmd_receiver,
                result_sender,
                scheduler_waker,
            ));
        }));

        TelegramBotComponent {
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
            state: TelegramBotState::WaitingForConfig,
            config: None,
            cmd_sender,
            result_receiver,
            async_thread,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("TelegramBot process() called");

        let mut work_units = 0u32;

        // Check signals first (signals are handled regardless of budget)
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, shutting down and finishing");
                // Send shutdown command to async thread
                let _ = self.cmd_sender.send(TelegramBotCommand::Shutdown);
                self.state = TelegramBotState::Finished;
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
            TelegramBotState::WaitingForConfig => {
                // Check if we have CONF configuration
                if let Ok(conf_vec) = self.conf.pop() {
                    debug!("received CONF config, parsing...");

                    // Parse configuration (extracted from original run() method)
                    let token_str =
                        std::str::from_utf8(&conf_vec).expect("failed to parse token as utf-8");

                    let config = TelegramBotConfig {
                        bot_token: token_str.to_string(),
                    };

                    self.config = Some(config.clone());

                    // Send init command to async thread
                    if let Err(_) = self
                        .cmd_sender
                        .send(TelegramBotCommand::InitBot(token_str.to_string()))
                    {
                        error!("Failed to send init bot command");
                        return ProcessResult::Finished;
                    }

                    self.state = TelegramBotState::Active;
                    debug!("Telegram bot initialization initiated");
                    work_units += 1; // Initialization work
                    return ProcessResult::DidWork(work_units);
                } else {
                    // No CONF config yet
                    return ProcessResult::NoWork;
                }
            }
            TelegramBotState::Active => {
                // Check for incoming messages from async thread within budget
                if context.remaining_budget > 0 {
                    if let Ok(result) = self.result_receiver.try_recv() {
                        match result {
                            TelegramBotResult::BotInitialized => {
                                debug!("Telegram bot initialized successfully");
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            TelegramBotResult::MessageReceived(msg_bytes, chat_id) => {
                                debug!(
                                    "Received message from Telegram chat {}, forwarding to output",
                                    chat_id
                                );
                                // Set chat ID for future responses
                                let _ =
                                    self.cmd_sender.send(TelegramBotCommand::SetChatId(chat_id));
                                if let Err(_) = self.out.push(msg_bytes) {
                                    error!("Failed to send message to output");
                                    return ProcessResult::Finished;
                                }
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            TelegramBotResult::MessageSent => {
                                debug!("Message sent successfully to Telegram");
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            TelegramBotResult::Error(e) => {
                                error!("Telegram operation error: {}", e);
                                work_units += 1; // Error handling is work
                                context.remaining_budget -= 1;
                            }
                        }
                    }
                }

                // Check for outgoing messages to send within remaining budget
                if context.remaining_budget > 0 {
                    if let Ok(msg_bytes) = self.inn.pop() {
                        debug!("Received message to send to Telegram");
                        let text = String::from_utf8(msg_bytes)
                            .expect("failed to convert message to UTF-8");

                        // Send message command to async thread
                        if let Err(_) = self.cmd_sender.send(TelegramBotCommand::SendMessage(text))
                        {
                            error!("Failed to send message command");
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
            TelegramBotState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("TelegramBot"),
            description: String::from("Reads messages from the Telegram Bot API, sends these into the OUT port and sends responses on IN port into the Telegram chats."),
            icon: String::from("telegram"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("configuration IP, currently just the Telegram Bot API token, may change to IRI/URL in the future"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("response to be sent to Telegram chat"),
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
                    description: String::from("messages from Telegram chat(s)"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

impl Drop for TelegramBotComponent {
    fn drop(&mut self) {
        debug!("TelegramBotComponent dropping, sending shutdown command");
        // Send shutdown command to async thread
        let _ = self.cmd_sender.send(TelegramBotCommand::Shutdown);
        // Note: The async thread will handle the shutdown and terminate
    }
}
