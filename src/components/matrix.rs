use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, ProcessEdgeSinkConnection, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use std::{thread, thread::Thread};
use std::future::IntoFuture;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::events::room::message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent};
use matrix_sdk::{Client, Room, RoomState};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId};
use matrix_sdk::event_handler::Ctx;

/*
TODO add one more filter - some other module is sending debug logging lines
TODO add support for sending and receiving more data types besides text messages
TODO add support to join the given room given in URL path - needs more testing
TODO add support for responding to verification request so that the session goes to verified state
TODO currently only supports one room, add support for multiple rooms - how to hand over the room_id to which it should send the message?

TODO clean up following:
* 1 room = 1 component? No, 1 component can handle multiple rooms
  * TODO needs to be given as metadata an an FBP object structure which room it is coming from
* Everything that is posted into this room is taken as input -> forward on port OUT into flowd network.
* Maybe add setting whether framed on unframed messages are expected and what to do with non-wellformed messages (in case of "framed FBP network protocol messages").
* What is the use-case? between flowd components or mainly used between components located in different runtimes? why not just use a TCP socket component?
* Is data history in the room useful/desired?
* Matrix has a workflow that is different from FBP:
  * it synchronizes state on room objects - and does not have flowing data like flowd has
  * it has callbacks to authorize the device, this might not work well with flowd - would need to be input into the matrix component
* Matrix bot is always reader and writer, so has IN+OUT always.
  * need to send to Drop if not used
  * need "/dev/null" component which never sends anything just to keep the port connected.

simple usage:  https://docs.rs/matrix-sdk/0.7.1/matrix_sdk/
examples:  https://github.com/matrix-org/matrix-rust-sdk/tree/main/examples
*/

pub struct MatrixClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for MatrixClientComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        MatrixClientComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("MatrixClient is now run()ning!");
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
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };

        // prepare connection arguments
        // NOTE: the matrix user ID format like "@alice:example.org" does not map well to an URL and it also does not include the password, so we use URL format
        // parse URL
        let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");
        let url = url::Url::parse(&url_str).expect("failed to parse URL");
        // get configuration values from URL
        let homeserver = url.host_str().expect("failed to get homeserver from URL hostname");
        let username = url.username().to_string();  //TODO optimize - using to_string() in order to easily move into spawn closure
        let password = url.password().expect("failed to get password from URL").to_string();    //TODO optimize - using to_string() in order to easily move into spawn closure
        let room_name: Option<String>;
        if !url.path().is_empty() {
            room_name = Some(url.path().strip_prefix('/').expect("failed to get room name from URL path").to_string());
        } else {
            room_name = None;
        }

        // configure
        // tokio runtime
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)  //TODO optimize - still starts additional thread for rt.spawn() thus 2 threads for tokio, seems useless
            .enable_all()
            .thread_name(format!("{}/EV", thread::current().name().expect("failed to get current thread name")))
            .build()
            .expect("failed to build tokio runtime");
        // prepare client
        let homeserver_url = format!("https://{}/", homeserver);    //TODO optimize - use URL struct? remove username, password and query paramaters?
        // NOTE: when encryption is enabled, you should use a persistent store to be able to restore the session with a working encryption setup. TODO See the `persist_session` example in the matrix-rust-sdk repo.
        let client_fut = Client::builder().homeserver_url(homeserver_url).build().into_future();
        let client = rt.block_on(client_fut).expect("failed to create client");
        // variables for the client
        let client_ref = client.clone();
        let out_ref = Arc::new(Mutex::new(out));    //TODO optimize
        let out_wakeup_ref = out_wakeup.clone();
        let room_id: Arc<Mutex<Option<OwnedRoomId>>> = Arc::new(Mutex::new(None));
        let room_id_ref = room_id.clone();
        let user_id: Arc<Mutex<Option<OwnedUserId>>> = Arc::new(Mutex::new(None));
        let user_id_ref = user_id.clone();
        // add context = state information for event handler, otherwise it cannot interact with anything outside of the handler
        client.add_event_handler_context((out_ref, out_wakeup_ref, room_id_ref, user_id_ref));
        // start event syncing on the separate tokio runtime thread
        debug!("starting event handler...");
        rt.spawn(async move {
            // log in
            client
                .matrix_auth()
                .login_username(username.as_str(), password.as_str())
                .initial_device_display_name("flowd") //TODO add feature to make configurable or use component name
                .await.expect("failed to login");
            debug!("logged in as username={}", username);

            // get own user ID to filter out own messages
            let user_id_tmp = client.user_id().expect("failed to get user ID").to_owned();
            debug!("logged in as user_id={}", user_id_tmp);
            user_id.lock().expect("lock poisoned").replace(user_id_tmp);

            // join given room
            if let Some(room_name) = room_name {
                let room = client.join_room_by_id(&RoomId::parse(room_name).expect("failed to parse room ID given in config URL path")).await.expect("failed to join given room");
                debug!("joined given room={}", room.display_name().await.expect("failed to get room name"));
            }

            // an initial sync to set up state and so our bot does not respond to old messages
            let response = client.sync_once(SyncSettings::default()).await.unwrap();

            // add the bot to be notified of incoming messages
            // NOTE: we do this after the initial sync to avoid responding to messages before the bot was running
            client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room, context: Ctx<(Arc<Mutex<ProcessEdgeSinkConnection>>, Thread, Arc<Mutex<Option<OwnedRoomId>>>, Arc<Mutex<Option<OwnedUserId>>>)>| async move {
                // prepare variables
                let (out, out_wakeup, room_id, user_id) = context.0;
                let mut room_id = room_id.lock().expect("lock poisoned");
                let user_id = user_id.lock().expect("lock poisoned");
                let user_id = user_id.as_ref().unwrap();  // TODO optimize - unwrap is safe here, but I dont like the unwrap
    
                // check if we are ready
                if room.state() != RoomState::Joined {
                    return;
                }
                //TODO add support for verification request message type - this is sent when clicking on the "verify session" button in the Element client seems like it just has to be confirmed
                let MessageType::Text(text_content) = event.content.msgtype else {
                    debug!("got a message that is not text - discarding");
                    return;
                };
                // check for our own message coming back
                if event.sender == *user_id {
                    debug!("got our own message back - discarding");
                    return;
                }
            
                // store room ID for being able to respond in main loop
                if room_id.is_none() {
                    *room_id = Some(room.room_id().to_owned());   //TODO optimize conversion
                }
    
                // send it
                debug!("sending...");
                out.lock().expect("lock poisoned").push(text_content.body.as_bytes().to_vec()).expect("could not push into OUT");
                out_wakeup.unpark();
                debug!("done");
            });
    
            // since we called sync_once() before we entered our sync loop we must pass that sync token to sync()
            let settings = SyncSettings::default().token(response.next_batch);

            // this keeps state from the server streaming in to the event handler via the EventHandler trait
            client.sync(settings).await.expect("failed to sync");
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
                    debug!("got a packet, sending to room...");
                    let content = RoomMessageEventContent::text_plain(std::str::from_utf8(&ip).expect("failed to convert IP to UTF-8"));

                    // process
                    // nothing to be done

                    // send it
                    if let Some(room_id_inner) = room_id.lock().expect("lock poisoned").as_ref() {
                        let room = client_ref.get_room(&room_id_inner).expect("failed to get room");
                        rt.block_on(room.send(content).into_future()).expect("failed to send message");
                        debug!("done");
                    } else {
                        warn!("no room ID set - discarding IP");
                    }
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
            name: String::from("MatrixClient"),
            description: String::from("Reads messages from the Telegram Bot API, sends these into the OUT port and sends responses on IN port into the Telegram chats."),
            icon: String::from("wechat"),   // it seems like the next best thing to a Matrix icon
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("configuration IP in URL format, optional URL path is the room name if not already pre-joined from a previous session"),
                    values_allowed: vec![],
                    value_default: String::from("matrix://user:pass@matrixhomeserver.org/!roomid@matrixhomeserver.org")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("response to be sent to Matrix room"),
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
                    description: String::from("messages from Matrix room"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}