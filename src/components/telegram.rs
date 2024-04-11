use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, ProcessEdgeSinkConnection, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use teloxide::prelude::*;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::{thread, thread::Thread};
use std::future::IntoFuture;

/*
TODO harden - the bot's Telegram username associated with the bot token is a global identifier for the bot, making it possible for anybody to open a chat to the bot and thus send data into your flowd network
TODO currently replying messages to the chat ID the latest message came from - add support for multiple chats, effectively turing this component into a server-type component in the senso of a TCP server component handling multiple clients
TODO add support for sending and receiving images, files, documents, also for command list, buttons, selection possibilities etc.
*/

pub struct TelegramBotComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for TelegramBotComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        TelegramBotComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("TelegramBot is now run()ning!");
        let mut conf = self.conf;
        let mut inn = self.inn;
        let out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        trace!("read config IPs");
        /*
        while conf.is_empty() {
            thread::yield_now();
        }
        */
        let Ok(token) = conf.pop() else { trace!("no config IP received - exiting"); return; };
        let token_str = std::str::from_utf8(&token).expect("failed to parse token as utf-8");

        // configure
        // tokio runtime
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)  //TODO optimize - still starts additional thread for rt.spawn() thus 2 threads for tokio, seems useless
            .enable_all()
            .thread_name(format!("{}/EV", thread::current().name().expect("failed to get current thread name")))
            .build()
            .expect("failed to build tokio runtime");
        // configure bot
        let bot = Bot::new(token_str);    //TODO harden - this can panic
        // variables for the bot
        let bot_ref = bot.clone();
        let messages_total = Arc::new(AtomicU64::new(0));
        let chat_id: Arc<AtomicI64> = Arc::new(AtomicI64::new(0));
        let chat_id_ref = chat_id.clone();
        let out_ref = Arc::new(Mutex::new(out));    //TODO optimize
        let out_wakeup_ref = out_wakeup.clone();
        // handler for incoming messages - which can be text, images, etc.
        let handler = Update::filter_message().endpoint(
            |_bot: Bot, msg: Message, out: Arc<Mutex<ProcessEdgeSinkConnection>>, out_wakeup: Thread, chat_id: Arc<AtomicI64>, messages_total: Arc<AtomicU64>| async move {
                // increase message counter
                let _previous = messages_total.fetch_add(1, Ordering::Relaxed);

                // store chat ID for being able to respond
                chat_id.store(msg.chat.id.0, Ordering::Relaxed);

                // send it
                debug!("sending...");
                out.lock().expect("lock poisoned").push(msg.text().expect("failed to get text from Telegram message").as_bytes().to_vec()).expect("could not push into OUT");
                out_wakeup.unpark();
                debug!("done");

                ResponseResult::Ok(())
            },
        );
        // start bot on the separate tokio runtime thread
        debug!("starting bot message dispatcher...");
        rt.spawn(async move {
            let mut builder = Dispatcher::builder(bot, handler)
            .dependencies(dptree::deps![out_ref, out_wakeup_ref, chat_id_ref, messages_total])   // pass the shared state to the handler as a dependency
            .build();
            let bot_dispatcher = builder.dispatch();
            bot_dispatcher.await;
        });
        debug!("done");

        // main loop
        loop {
            trace!("begin of iteration");
            // check signals
            if let Ok(ip) = self.signals_in.try_recv() {
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {   //TODO optimize comparison
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
            loop {
                //TODO add support for chunks
                if let Ok(ip) = inn.pop() {
                    // prepare packet
                    debug!("got a packet, sending to Telegram...");
                    // nothing to do

                    // process
                    let chat_id = chat_id.load(Ordering::Relaxed);
                    if chat_id != 0 {
                        //TODO add suport for handling multiple chat IDs ("clients")
                        let req = bot_ref.send_message(ChatId(chat_id), String::from_utf8(ip).expect("failed to convert IP to UTF-8")).send().into_future();
                        rt.block_on(req).expect("failed to send message");
                    } else {
                        warn!("no chat ID set - discarding IP");
                    }

                    debug!("done");
                } else {
                    break;
                }
            }

            // are we done?
            if inn.is_abandoned() {
                info!("EOF on inport, shutting down");
                rt.shutdown_background();
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