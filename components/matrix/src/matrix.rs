use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};
use tokio::sync::mpsc;

// component-specific
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::events::room::message::{
    MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId};
use matrix_sdk::{Client, Room, RoomState};
use std::future::IntoFuture;
use std::time::Duration;

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

#[derive(Debug)]
enum MatrixClientState {
    WaitingForConfig,
    Active,
    Finished,
}

#[derive(Debug, Clone)]
struct MatrixClientConfig {
    homeserver: String,
    username: String,
    password: String,
    room_name: Option<String>,
}

#[derive(Debug)]
enum MatrixClientCommand {
    InitClient(MatrixClientConfig),
    SendMessage(Vec<u8>),
    Shutdown,
}

#[derive(Debug)]
enum MatrixClientResult {
    ClientInitialized,
    MessageReceived(Vec<u8>), // message bytes
    MessageSent,
    Error(String),
}

pub struct MatrixClientComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    // Async operation state
    state: MatrixClientState,
    config: Option<MatrixClientConfig>,
    cmd_sender: mpsc::UnboundedSender<MatrixClientCommand>,
    result_receiver: mpsc::UnboundedReceiver<MatrixClientResult>,
    #[allow(dead_code)]
    async_thread: Option<std::thread::JoinHandle<()>>,
    //graph_inout: GraphInportOutportHandle,
}

const MATRIX_CLIENT_INIT_TIMEOUT: Duration = Duration::from_secs(15);
const MATRIX_SEND_TIMEOUT: Duration = Duration::from_secs(10);

async fn async_matrix_main(
    mut cmd_rx: mpsc::UnboundedReceiver<MatrixClientCommand>,
    result_tx: mpsc::UnboundedSender<MatrixClientResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let scheduler_waker = scheduler_waker.map(std::sync::Arc::new);
    let mut client: Option<Client> = None;
    let mut room_id: Option<OwnedRoomId> = None;
    #[allow(unused_assignments)]
    let mut user_id: Option<OwnedUserId> = None;
    let mut dispatcher_handle: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            MatrixClientCommand::InitClient(config) => {
                debug!("Initializing Matrix client");
                let homeserver_url = format!("https://{}/", config.homeserver);
                let client_fut = Client::builder()
                    .homeserver_url(homeserver_url)
                    .build()
                    .into_future();
                let new_client =
                    match tokio::time::timeout(MATRIX_CLIENT_INIT_TIMEOUT, client_fut).await {
                        Ok(Ok(c)) => c,
                        Ok(Err(e)) => {
                            let _ = result_tx.send(MatrixClientResult::Error(format!(
                                "Client init failed: {}",
                                e
                            )));
                            if let Some(ref waker) = scheduler_waker {
                                waker();
                            }
                            continue;
                        }
                        Err(_) => {
                            let _ = result_tx.send(MatrixClientResult::Error(
                                "Client init timed out".to_string(),
                            ));
                            if let Some(ref waker) = scheduler_waker {
                                waker();
                            }
                            continue;
                        }
                    };

                // Login
                if let Err(e) = new_client
                    .matrix_auth()
                    .login_username(&config.username, &config.password)
                    .initial_device_display_name("flowd")
                    .await
                {
                    let _ =
                        result_tx.send(MatrixClientResult::Error(format!("Login failed: {}", e)));
                    if let Some(ref waker) = scheduler_waker {
                        waker();
                    }
                    continue;
                }
                debug!("logged in as username={}", config.username);

                // Get user ID
                let user_id_tmp = match new_client.user_id() {
                    Some(id) => id.to_owned(),
                    None => {
                        let _ = result_tx.send(MatrixClientResult::Error(
                            "Failed to get user ID".to_string(),
                        ));
                        if let Some(ref waker) = scheduler_waker {
                            waker();
                        }
                        continue;
                    }
                };
                debug!("logged in as user_id={}", user_id_tmp);
                user_id = Some(user_id_tmp);

                // Join room if specified
                if let Some(room_name) = config.room_name {
                    match new_client
                        .join_room_by_id(
                            &RoomId::parse(&room_name).expect("failed to parse room ID"),
                        )
                        .await
                    {
                        Ok(room) => {
                            let room_name = room
                                .display_name()
                                .await
                                .map(|n| n.to_string())
                                .unwrap_or_else(|_| "unknown".to_string());
                            debug!("joined room={}", room_name);
                            room_id = Some(room.room_id().to_owned());
                        }
                        Err(e) => {
                            let _ = result_tx.send(MatrixClientResult::Error(format!(
                                "Failed to join room: {}",
                                e
                            )));
                            if let Some(ref waker) = scheduler_waker {
                                waker();
                            }
                            continue;
                        }
                    }
                }

                // Initial sync
                let response = match new_client.sync_once(SyncSettings::default()).await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = result_tx.send(MatrixClientResult::Error(format!(
                            "Initial sync failed: {}",
                            e
                        )));
                        if let Some(ref waker) = scheduler_waker {
                            waker();
                        }
                        continue;
                    }
                };

                // Set up event handler
                let client_ref = new_client.clone();
                let result_tx_clone = result_tx.clone();
                let user_id_ref = user_id.clone();
                let scheduler_waker_clone = scheduler_waker.clone();
                let handle = tokio::spawn(async move {
                    let handler = move |event: OriginalSyncRoomMessageEvent, room: Room| async move {
                        if room.state() != RoomState::Joined {
                            return;
                        }
                        let MessageType::Text(text_content) = event.content.msgtype else {
                            debug!("got a message that is not text - discarding");
                            return;
                        };
                        if let Some(ref uid) = user_id_ref {
                            if event.sender == *uid {
                                debug!("got our own message back - discarding");
                                return;
                            }
                        }
                        let _ = result_tx_clone.send(MatrixClientResult::MessageReceived(
                            text_content.body.as_bytes().to_vec(),
                        ));
                        if let Some(ref waker) = scheduler_waker_clone {
                            waker();
                        }
                    };
                    client_ref.add_event_handler(handler);

                    let settings = SyncSettings::default().token(response.next_batch);
                    if let Err(e) = client_ref.sync(settings).await {
                        error!("Sync failed: {}", e);
                    }
                });

                client = Some(new_client);
                dispatcher_handle = Some(handle);
                let _ = result_tx.send(MatrixClientResult::ClientInitialized);
                if let Some(ref waker) = scheduler_waker {
                    waker();
                }
            }
            MatrixClientCommand::SendMessage(msg_bytes) => {
                if let (Some(ref client_ref), Some(ref room_id_inner)) = (&client, &room_id) {
                    let text =
                        String::from_utf8(msg_bytes).expect("failed to convert message to UTF-8");
                    let content = RoomMessageEventContent::text_plain(&text);
                    if let Some(room) = client_ref.get_room(room_id_inner) {
                        let send_result = tokio::time::timeout(
                            MATRIX_SEND_TIMEOUT,
                            room.send(content).into_future(),
                        )
                        .await;
                        match send_result {
                            Ok(Ok(_)) => {
                                let _ = result_tx.send(MatrixClientResult::MessageSent);
                                if let Some(ref waker) = scheduler_waker {
                                    waker();
                                }
                            }
                            Ok(Err(err)) => {
                                let _ = result_tx.send(MatrixClientResult::Error(format!(
                                    "Send failed: {}",
                                    err
                                )));
                                if let Some(ref waker) = scheduler_waker {
                                    waker();
                                }
                            }
                            Err(_) => {
                                let _ = result_tx.send(MatrixClientResult::Error(format!(
                                    "Send timed out after {:?}",
                                    MATRIX_SEND_TIMEOUT
                                )));
                                if let Some(ref waker) = scheduler_waker {
                                    waker();
                                }
                            }
                        }
                    } else {
                        let _ =
                            result_tx.send(MatrixClientResult::Error("Room not found".to_string()));
                        if let Some(ref waker) = scheduler_waker {
                            waker();
                        }
                    }
                } else {
                    let _ = result_tx.send(MatrixClientResult::Error(
                        "Client not initialized or no room set".to_string(),
                    ));
                    if let Some(ref waker) = scheduler_waker {
                        waker();
                    }
                }
            }

            MatrixClientCommand::Shutdown => {
                debug!("Shutting down Matrix client");
                if let Some(handle) = dispatcher_handle.take() {
                    handle.abort();
                }
                break;
            }
        }
    }
}

impl Component for MatrixClientComponent {
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
            rt.block_on(async_matrix_main(
                cmd_receiver,
                result_sender,
                scheduler_waker,
            ));
        }));

        MatrixClientComponent {
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
            state: MatrixClientState::WaitingForConfig,
            config: None,
            cmd_sender,
            result_receiver,
            async_thread,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("MatrixClient process() called");

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
                let _ = self.cmd_sender.send(MatrixClientCommand::Shutdown);
                self.state = MatrixClientState::Finished;
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
            MatrixClientState::WaitingForConfig => {
                // Check if we have CONF configuration
                if let Ok(conf_vec) = self.conf.pop() {
                    debug!("received CONF config, parsing...");

                    // Parse configuration (extracted from original run() method)
                    let url_str =
                        std::str::from_utf8(&conf_vec).expect("failed to parse token as utf-8");
                    let url = url::Url::parse(url_str).expect("failed to parse configuration URL");

                    let homeserver = url
                        .host_str()
                        .expect("failed to get homeserver from URL hostname")
                        .to_string();
                    let username = url.username().to_string();
                    let password = url
                        .password()
                        .expect("failed to get password from URL")
                        .to_string();
                    let room_name = if !url.path().is_empty() {
                        Some(
                            url.path()
                                .strip_prefix('/')
                                .expect("failed to get room name from URL path")
                                .to_string(),
                        )
                    } else {
                        None
                    };

                    let config = MatrixClientConfig {
                        homeserver,
                        username,
                        password,
                        room_name,
                    };

                    self.config = Some(config.clone());

                    // Send init command to async thread
                    if let Err(_) = self
                        .cmd_sender
                        .send(MatrixClientCommand::InitClient(config))
                    {
                        error!("Failed to send init client command");
                        return ProcessResult::Finished;
                    }

                    self.state = MatrixClientState::Active;
                    debug!("Matrix client initialization initiated");
                    work_units += 1; // Initialization work
                    return ProcessResult::DidWork(work_units);
                } else {
                    // No CONF config yet
                    return ProcessResult::NoWork;
                }
            }
            MatrixClientState::Active => {
                // Check for incoming messages from async thread within budget
                if context.remaining_budget > 0 {
                    if let Ok(result) = self.result_receiver.try_recv() {
                        match result {
                            MatrixClientResult::ClientInitialized => {
                                debug!("Matrix client initialized successfully");
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            MatrixClientResult::MessageReceived(msg_bytes) => {
                                debug!("Received message from Matrix, forwarding to output");
                                if let Err(_) = self.out.push(msg_bytes) {
                                    error!("Failed to send message to output");
                                    return ProcessResult::Finished;
                                }
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            MatrixClientResult::MessageSent => {
                                debug!("Message sent successfully to Matrix");
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            MatrixClientResult::Error(e) => {
                                error!("Matrix operation error: {}", e);
                                work_units += 1; // Error handling is work
                                context.remaining_budget -= 1;
                            }
                        }
                    }
                }

                // Check for outgoing messages to send within remaining budget
                if context.remaining_budget > 0 {
                    if let Ok(msg_bytes) = self.inn.pop() {
                        debug!("Received message to send to Matrix");
                        // Send message command to async thread
                        if let Err(_) = self
                            .cmd_sender
                            .send(MatrixClientCommand::SendMessage(msg_bytes))
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
                    context
                        .ready_signal
                        .store(true, std::sync::atomic::Ordering::Release);
                }

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    ProcessResult::NoWork
                }
            }
            MatrixClientState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("MatrixClient"),
            description: String::from("Reads messages from Matrix rooms, sends these into the OUT port and sends responses on IN port into Matrix rooms."),
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

impl Drop for MatrixClientComponent {
    fn drop(&mut self) {
        debug!("MatrixClientComponent dropping, sending shutdown command");
        // Send shutdown command to async thread
        let _ = self.cmd_sender.send(MatrixClientCommand::Shutdown);
        // Note: The async thread will handle the shutdown and terminate
    }
}
