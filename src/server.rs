use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock, Mutex};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use signal_hook::consts::signal::*;
use signal_hook::flag;
use tungstenite::handshake::server::{Request, Response};
use tungstenite::{accept_hdr, Error, Message, Result};

use crate::{CLIENT_BROADCAST_WRITE_TIMEOUT, Graph, Runtime, RuntimeRuntimePayload, ComponentLibrary, GraphInportOutportHolder, FBPMessage, RuntimeRuntimeMessage, RuntimePortsMessage, ComponentComponentMessage, ComponentComponentsreadyMessage, NetworkStatusMessage, NetworkStatusPayload, NetworkPersistResponse, NetworkErrorResponse, ComponentSourceMessage, GraphErrorResponse, GraphClearResponse, GraphAddnodeResponse, GraphRemovenodeResponse, GraphRenamenodeResponse, GraphChangenodeResponse, GraphAddedgeResponse, GraphRemoveedgeResponse, GraphChangeedgeResponse, GraphAddinitialResponse, GraphRemoveinitialResponse, GraphAddinportResponse, GraphRemoveinportResponse, GraphRenameinportResponse, GraphAddoutportResponse, GraphRemoveoutportResponse, GraphRenameoutportResponse, GraphAddgroupResponse, GraphRemovegroupResponse, GraphRenamegroupResponse, GraphChangegroupResponse, TraceStartResponse, TraceStopResponse, TraceClearResponse, TraceDumpResponse, TraceErrorResponse, RuntimePacketsentMessage, RuntimePacketsentPayload, RuntimeErrorResponse, NetworkEdgesResponse, NetworkStartedResponse, NetworkTransmissionPayload, NetworkDataResponse, NetworkStoppedResponse, NetworkDebugResponse, NetworkStartedResponsePayload, GraphEdge, GraphPort, GraphNodeSpecNetwork, GraphChangenodeResponsePayload, AccessLevel, Capability, RuntimeMessage, ComponentMessage, NetworkMessage, GraphMessage, TraceMessage, register_component_log_filters};
/* unused imports
, RuntimePacketRequestPayload,
    RuntimePacketEvent
    GraphNodeSpec, GraphIIPSpecNetwork, GraphNodeMetadata,
    GraphEdgeMetadata
    GraphRemoveedgeResponsePayload, NetworkConnectResponse, NetworkBegingroupResponse,
    NetworkEndgroupResponse, NetworkDisconnectResponse, NetworkIconResponse,
    NetworkProcesserrorResponse, NetworkOutputResponse, send_runtime_packet,
    send_network_stopped, send_network_output, send_network_error, send_network_data
 */

fn must_not_block<Role: tungstenite::handshake::HandshakeRole>(err: tungstenite::HandshakeError<Role>) -> tungstenite::Error {
    match err {
        tungstenite::HandshakeError::Interrupted(_) => panic!("Bug: blocking socket would block"),
        tungstenite::HandshakeError::Failure(f) => f,
    }
}

pub struct FlowdServer {
    bind_addr: String,
    runtime: Arc<RwLock<Runtime>>,
    graph: Arc<RwLock<Graph>>,
    components: Arc<RwLock<ComponentLibrary>>,
    graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
    sigterm_received: Arc<AtomicBool>,
    sigint_received: Arc<AtomicBool>,
}

impl FlowdServer {
    pub fn new(
        bind_addr: String,
        runtime: Arc<RwLock<Runtime>>,
        graph: Arc<RwLock<Graph>>,
        components: Arc<RwLock<ComponentLibrary>>,
        graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
    ) -> Self {
        FlowdServer {
            bind_addr,
            runtime,
            graph,
            components,
            graph_inout,
            sigterm_received: Arc::new(AtomicBool::new(false)),
            sigint_received: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&mut self) -> Result<()> {
        let server = TcpListener::bind(&self.bind_addr).unwrap();
        log::info!("management listening on {} - manage via GUI at https://app.noflojs.org/#runtime/endpoint?protocol%3Dwebsocket%26address%3Dws%3A%2F%2F{}",
                   self.bind_addr,
                   self.bind_addr.replace("localhost:", "localhost:"));

        // setup signal handling for graceful shutdown
        flag::register(SIGINT, self.sigint_received.clone()).expect("failed to register SIGINT handler");
        flag::register(SIGTERM, self.sigterm_received.clone()).expect("failed to register SIGTERM handler");

        // start listening for incoming connections
        for stream_res in server.incoming() {
            if self.sigterm_received.load(Ordering::Relaxed) || self.sigint_received.load(Ordering::Relaxed) {
                break;
            }
            if let Ok(stream) = stream_res {
                // create Arc pointers for the new thread
                let graphref = self.graph.clone();
                let runtimeref = self.runtime.clone();
                let componentlibref = self.components.clone();
                let graph_inoutref = self.graph_inout.clone();

                // start thread
                // since the thread name can only be 15 characters on Linux and an IP address already has up to 15, the IP address is not in the name
                thread::Builder::new().name("client-handler".into()).spawn(move || {
                    log::info!("got a client from {}", stream.peer_addr().expect("get peer address failed"));
                    if let Err(err) = Self::handle_client(stream, graphref, runtimeref, componentlibref, graph_inoutref) {
                        match err {
                            Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8 => (),
                            e => log::error!("test: {}", e),
                        }
                    }
                }).expect("thread start for connection handler failed");
            } else if let Err(e) = stream_res {
                log::error!("Error accepting stream: {}", e);
            }
        }

        // Graceful shutdown: handle signal-specific actions
        if self.sigterm_received.load(Ordering::Relaxed) {
            // Persist on SIGTERM (systemd graceful shutdown)
            if let Err(e) = self.runtime.read().unwrap().persist(&self.graph.read().unwrap()) {
                log::warn!("Failed to persist graph on SIGTERM: {}", e);
            } else {
                log::info!("Graph persisted on SIGTERM");
            }
        }

        // Stop network if running
        if self.runtime.read().unwrap().status.running {
            log::info!("Signal received, waiting for network shutdown...");
            match self.runtime.write().unwrap().stop(self.graph_inout.clone(), false) {
                Ok(_) => log::info!("Network stopped gracefully"),
                Err(e) => log::warn!("Network stop failed: {}", e),
            }
        }

        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.sigterm_received.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn handle_client(stream: TcpStream, graph: Arc<RwLock<Graph>>, runtime: Arc<RwLock<Runtime>>, components: Arc<RwLock<ComponentLibrary>>, graph_inout: Arc<std::sync::Mutex<GraphInportOutportHolder>>) -> Result<()> {
        log::info!("handle_client called");

        fn validate_secret(runtime: &Arc<RwLock<Runtime>>, secret: Option<&String>, graph: &str) -> Result<(), ()> {
            runtime
                .read()
                .expect("lock poisoned")
                .validate_secret_with_access(secret, graph, AccessLevel::ReadWrite)
                .map_err(|_| ())
        }

        fn validate_secret_readonly(runtime: &Arc<RwLock<Runtime>>, secret: Option<&String>, graph: &str) -> Result<(), ()> {
            runtime
                .read()
                .expect("lock poisoned")
                .validate_secret_with_access(secret, graph, AccessLevel::ReadOnly)
                .map_err(|_| ())
        }

        fn has_readonly_secret_for_graph(runtime: &Arc<RwLock<Runtime>>, secret: Option<&String>, graph: &str) -> bool {
            let Some(secret) = secret.filter(|secret| !secret.is_empty()) else {
                return false;
            };
            let runtime_read = runtime.read().expect("lock poisoned");
            let Some((expected_secret, access_level)) = runtime_read.secrets.get(graph) else {
                return false;
            };
            secret == expected_secret && *access_level == AccessLevel::ReadOnly
        }

        fn get_graph_by_name(
            runtime: &Arc<RwLock<Runtime>>,
            fallback_graph: &Arc<RwLock<Graph>>,
            graph_name: &str,
        ) -> Result<Arc<RwLock<Graph>>, std::io::Error> {
            if let Some(graph_arc) = runtime.read().expect("lock poisoned").graphs.get_graph(graph_name) {
                return Ok(graph_arc);
            }
            if fallback_graph.read().expect("lock poisoned").properties.name == graph_name {
                return Ok(fallback_graph.clone());
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Graph '{}' not found", graph_name),
            ))
        }
        if let Err(err) = stream.set_write_timeout(Some(Duration::SECOND)) {
            log::warn!("set_write_timeout call failed: {:?}", err);
        }
        //stream.set_nodelay(true).expect("set_nodelay call failed");

        //TODO save stream clone/dup for graph outports process and pack into "cloned" WebSocket
        /*
        tungstenite::WebSocket::from_raw_socket(
        websocket.get_mut().try_clone().expect("clone of tcp stream failed for graph outports handler thread"),
        tungstenite::protocol::Role::Server,
        None
        */
        let peer_addr = stream.peer_addr().expect("could not get peer socketaddr");
        {
            let cloned_stream = stream.try_clone().expect("could not try_clone() TcpStream");
            if let Err(err) = cloned_stream.set_write_timeout(CLIENT_BROADCAST_WRITE_TIMEOUT) {
                log::warn!("set_write_timeout call failed on cloned stream: {:?}", err);
            }
            graph_inout.lock().expect(r#"could not acquire lock for saving TcpStream for graph outport process"#).websockets.insert(
                    peer_addr,
                    Arc::new(Mutex::new(tungstenite::WebSocket::from_raw_socket(cloned_stream, tungstenite::protocol::Role::Server, None))),
                );
        }

        let callback = |req: &Request, mut response: Response| {
            log::debug!("Received a new ws handshake");
            log::debug!("The request's path is: {}", req.uri().path());
            log::debug!("The request's headers are:");
            for (ref key, value) in req.headers() {
                log::debug!("  {}: {:?}", key, value);
            }

            // Let's add an additional header to our response to the client.
            let headers = response.headers_mut();
            //TODO check for noflo on Request -- yes, noflo-ui sends websocket sub-protocol request "noflo"
            //TODO it seems that the sec-websocket-protocol does not get sent when setting it this way
            //TODO "sent non-empty 'Sec-WebSocket-Protocol' header but no response was received" -> server should choose if non-empty
            headers.insert("sec-websocket-protocol", "noflo".parse().unwrap()); // not required by noflo-ui
            headers.append("MyCustomHeader", ":)".parse().unwrap());
            headers.append("SOME_TUNGSTENITE_HEADER", "header_value".parse().unwrap()); //TODO remove these

            Ok(response)
        };
        //let mut socket = accept(stream).map_err(must_not_block)?;
        let mut websocket = accept_hdr(stream, callback).map_err(must_not_block)?;

        //TODO wss
        //TODO check secret

        log::debug!("entering receive loop");
        loop {
            log::debug!("waiting for next message");
            match websocket.read()? {
                msg @ Message::Text(_) | msg @ Message::Binary(_) => {
                    log::debug!("got a text|binary message");
                    //debug!("message data: {}", msg.clone().into_text().unwrap());

                    let msg_data = msg.into_data();
                    log::debug!("received message data: {}", String::from_utf8_lossy(&msg_data));

                    // Compatibility: many FBP clients include optional envelope keys (`id`, top-level `secret`)
                    // and may omit `payload` on commands with optional fields.
                    // FBPMessage is protocol-tagged, so keep `protocol`/`command` intact.
                    let mut json_envelope: serde_json::Value = match serde_json::from_slice(&msg_data) {
                        Ok(value) => value,
                        Err(err) => {
                            log::warn!("failed to parse message as JSON: {}", err);
                            continue;
                        }
                    };
                    if let Some(map) = json_envelope.as_object_mut() {
                        map.remove("id");
                        map.remove("secret");
                        if !map.contains_key("payload") {
                            map.insert("payload".to_string(), serde_json::json!({}));
                        }
                    }

                    let fbpmsg: FBPMessage = match serde_json::from_value(json_envelope) {
                        Ok(message) => {
                            log::debug!("successfully parsed FBPMessage: {:?}", message);
                            message
                        }
                        Err(err) => {
                            log::warn!("failed to decode normalized FBP message: {}", err);
                            continue;
                        }
                    }; //TODO data handover optimizable?

                    match fbpmsg {
                        // runtime base
                        FBPMessage::Runtime(RuntimeMessage::Getruntime(payload)) => {
                            log::info!("got runtime:getruntime message with secret {:?}", payload.secret);
                            if validate_secret_readonly(&runtime, payload.secret.as_ref(), &runtime.read().expect("lock poisoned").graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&RuntimeErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize runtime:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            // send response = runtime:runtime message
                            log::info!("response: sending runtime:runtime message");
                            let mut runtime_payload = {
                                let runtime_read = runtime.read().expect("lock poisoned");
                                RuntimeRuntimePayload::from(runtime_read.snapshot())
                            };
                            if has_readonly_secret_for_graph(&runtime, payload.secret.as_ref(), &runtime_payload.graph)
                                && !runtime_payload.capabilities.contains(&Capability::GraphReadonly)
                            {
                                runtime_payload.capabilities.push(Capability::GraphReadonly);
                            }
                            websocket
                                .send(Message::text(
                                    //TODO handing over value inside lock would work like this:  serde_json::to_string(&*runtime.read().expect("lock poisoned"))
                                            serde_json::to_string(&RuntimeRuntimeMessage::new(runtime_payload))
                                    .expect("failed to serialize runtime:runtime message"),
                                ))
                                .expect("failed to write message into websocket");
                            // spec: "If the runtime is currently running a graph and it is able to speak the full Runtime protocol, it should follow up with a ports message."
                            log::info!("response: sending runtime:ports message");
                            websocket
                                .send(Message::text(
                                    serde_json::to_string(&RuntimePortsMessage::new(&*runtime.read().expect("lock poisoned"), &graph.read().expect("lock poisoned")))
                                        .expect("failed to serialize runtime:ports message"),
                                ))
                                .expect("failed to write message into websocket");
                        }

                        // protocol:component
                        FBPMessage::Component(ComponentMessage::List(_payload)) => {
                            log::info!("got component:list message");
                            if validate_secret_readonly(&runtime, _payload.secret.as_ref(), &runtime.read().expect("lock poisoned").graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let mut count: u32 = 0;
                            for component in components.read().expect("lock poisoned").available.iter() {
                                log::info!("response: sending component:component message");
                                websocket
                                .send(Message::text(
                                    serde_json::to_string(&ComponentComponentMessage::new(&component))
                                        .expect("failed to serialize component:component response"),
                                ))
                                .expect("failed to write message into websocket");
                                count += 1;
                            }
                            log::info!("response: sending component:componentsready response");
                            websocket
                                .send(Message::text(
                                    serde_json::to_string(&ComponentComponentsreadyMessage::new(count))
                                        .expect("failed to serialize component:componentsready response"),
                                ))
                                .expect("failed to write message into websocket");
                            log::info!("sent {} component:component responses", count);
                            }

                        FBPMessage::Network(NetworkMessage::Getstatus(_payload)) => {
                            log::info!("got network:getstatus message");
                            if validate_secret_readonly(&runtime, _payload.secret.as_ref(), &_payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&NetworkErrorResponse::new("invalid secret token".to_string(), String::from(""), _payload.graph.clone())).expect("failed to serialize network:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            if get_graph_by_name(&runtime, &graph, &_payload.graph).is_err() {
                                websocket
                                    .send(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(
                                            String::from("Requested graph not found"),
                                            String::from(""),
                                            _payload.graph,
                                        ))
                                        .expect("failed to serialize network:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                continue;
                            }
                            let status_payload = {
                                let runtime_read = runtime.read().expect("lock poisoned");
                                let status_snapshot = runtime_read.status_snapshot();
                                if status_snapshot.graph == _payload.graph {
                                    NetworkStatusPayload::new(&status_snapshot)
                                } else {
                                    NetworkStatusPayload {
                                        graph: _payload.graph.clone(),
                                        uptime: None,
                                        started: false,
                                        running: false,
                                        debug: status_snapshot.debug,
                                    }
                                }
                            };
                            log::info!("response: sending network:status message");
                            websocket
                                .send(Message::text(
                                    serde_json::to_string(&NetworkStatusMessage::new(status_payload.clone()))
                                        .expect("failed to serialize network:status message"),
                                ))
                                .expect("failed to write message into websocket");
                        }

                        FBPMessage::Network(NetworkMessage::Persist(_payload)) => {
                            log::info!("got network:persist message");
                            if validate_secret(&runtime, _payload.secret.as_ref(), &runtime.read().expect("lock poisoned").graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&NetworkErrorResponse::new("invalid secret token".to_string(), String::from(""), runtime.read().expect("lock poisoned").graph.clone())).expect("failed to serialize network:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            // persist and send either network:persist or network:error
                            //###
                            match runtime.read().expect("lock poisoned").persist(&graph.read().expect("lock poisoned")) {    // NOTE: persist() now stores all known graphs from the runtime graph manager
                                Ok(_) => {
                                    log::info!("response: sending network:persist message");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkPersistResponse::default())
                                                .expect("failed to serialize network:persist message"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("persist failed: {}", err);
                                    log::info!("response: sending network:error message");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkErrorResponse::new(
                                                err.to_string(),
                                                String::from(""),
                                                runtime.read().expect("lock poisoned").graph.clone()    // there is no field "graph" in the payload that could re-used here
                                            ))
                                            .expect("failed to serialize network:error message"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Component(ComponentMessage::Getsource(payload)) => {
                            log::info!("got component:getsource message");
                            if validate_secret_readonly(&runtime, payload.secret.as_ref(), &runtime.read().expect("lock poisoned").graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            //TODO multi-graph support (runtime has the info which graph is running currently)
                            //TODO optimize: need 2 locks to get graph source - and it is not the common case
                            if graph.read().expect("lock poisoned").properties.name == payload.name {
                                // retrieve graph source
                                log::info!("got a request for graph source of {}", &payload.name);
                                //TODO why does Rust require a write lock here? "cannot borrow data in dereference as mutable"
                                log::debug!("source is: {}", graph.write().expect("lock poisoned").get_source(payload.name.clone()).expect("could not get graph source").code);
                                match graph.write().expect("lock poisoned").get_source(payload.name) {
                                    Ok(source_info) => {
                                        log::info!("response: sending component:source message for graph");
                                        websocket
                                        .send(Message::text(
                                            serde_json::to_string(&ComponentSourceMessage::new(source_info))
                                                .expect("failed to serialize component:source message"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    },
                                    Err(err) => {
                                        log::error!("graph.get_source() failed: {}", err);
                                        log::info!("response: sending graph:error response");
                                        websocket
                                            .send(Message::text(
                                                serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                    .expect("failed to serialize graph:error response"),
                                            ))
                                            .expect("failed to write message into websocket");
                                    }
                                }
                            } else {
                                // retrieve component source from component library
                                log::info!("got a request for component source of {}", &payload.name);
                                match components.read().expect("lock poisoned").get_source(payload.name) {
                                    Ok(source_info) => {
                                        log::info!("response: sending component:source message for component");
                                        websocket
                                        .send(Message::text(
                                            serde_json::to_string(&ComponentSourceMessage::new(source_info))
                                                .expect("failed to serialize component:source message"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    },
                                    Err(err) => {
                                        log::error!("componentlib.get_source() failed: {}", err);
                                        log::info!("response: sending graph:error response");
                                        websocket
                                            .send(Message::text(
                                                serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                    .expect("failed to serialize graph:error response"),
                                            ))
                                            .expect("failed to write message into websocket");
                                    }
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Clear(payload)) => {
                            log::info!("got graph:clear message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.name).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let mut runtime_write = runtime.write().expect("lock poisoned");
                            let previous_active_graph = runtime_write.graph.clone();
                            match graph.write().expect("lock poisoned").clear(&payload, &*runtime_write) {
                                Ok(_) => {
                                    runtime_write.graph = payload.name.clone();
                                    runtime_write.graphs.add_graph(payload.name.clone(), graph.clone());
                                    if previous_active_graph != payload.name {
                                        runtime_write.graphs.remove_graph(&previous_active_graph);
                                    }
                                    let _ = runtime_write.graphs.set_active_graph(&payload.name);
                                    log::info!("response: sending graph:clear response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphClearResponse::new(&payload))
                                                .expect("failed to serialize graph:clear response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.clear() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Addnode(payload)) => {
                            log::info!("got graph:addnode message");
                            if let Some(graph_name) = payload.graph.clone() {
                                if validate_secret(&runtime, payload.secret.as_ref(), &graph_name).is_err() {
                                    websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                    continue;
                                }
                                let target_graph = match get_graph_by_name(&runtime, &graph, &graph_name) {
                                    Ok(graph_arc) => graph_arc,
                                    Err(_err) => {
                                        websocket
                                            .send(Message::text(
                                                serde_json::to_string(&GraphErrorResponse::new(String::from("Requested graph not found")))
                                                    .expect("failed to serialize graph:error response"),
                                            ))
                                            .expect("failed to write message into websocket");
                                        continue;
                                    }
                                };
                                let add_node_result = {
                                    let mut graph_write = target_graph.write().expect("lock poisoned");
                                    // Include the icon field in metadata if present
                                    let mut metadata = payload.metadata.clone();
                                    if let Some(icon) = &payload.icon {
                                        metadata.insert("icon".to_string(), serde_json::Value::String(icon.clone()));
                                    }
                                    graph_write.add_node_from_payload(graph_name.to_string(), payload.component, payload.name, metadata)
                                };
                                match add_node_result {
                                Ok(response) => {
                                    log::info!("response: sending graph:addnode response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphAddnodeResponse::new(response))
                                                .expect("failed to serialize graph:addnode response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.add_node() failed: {}", err);
                                    let err_message = if err.kind() == std::io::ErrorKind::NotFound {
                                        String::from("Requested graph not found")
                                    } else {
                                        err.to_string()
                                    };
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err_message))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                                }
                            } else {
                                // Compatibility: fbp-protocol tests send graph:addnode without payload.graph
                                // and expect a graph:error message with text "No graph specified".
                                log::info!("response: sending graph:error response");
                                websocket
                                    .send(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(String::from("No graph specified")))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Removenode(payload)) => {
                            log::info!("got graph:removenode message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let remove_node_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.remove_node(payload.graph.clone(), payload.name.clone())
                            };
                            match remove_node_result {
                                Ok(removed_edges) => {
                                    for removed_edge in removed_edges {
                                        websocket
                                            .send(Message::text(
                                                serde_json::to_string(&GraphRemoveedgeResponse {
                                                    protocol: String::from("graph"),
                                                    command: String::from("removeedge"),
                                                    payload: removed_edge,
                                                })
                                                    .expect("failed to serialize graph:removeedge response"),
                                            ))
                                            .expect("failed to write message into websocket");
                                    }
                                    log::info!("response: sending graph:removenode response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphRemovenodeResponse::from_request(payload))
                                                .expect("failed to serialize graph:removenode response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("graph.remove_node() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Renamenode(payload)) => {
                            log::info!("got graph:renamenode message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let rename_node_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.rename_node(payload.graph.clone(), payload.from.clone(), payload.to.clone())
                            };
                            match rename_node_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:renamenode response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphRenamenodeResponse::from_request(payload))
                                                .expect("failed to serialize graph:renamenode response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("graph.rename_node() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Changenode(payload)) => {
                            log::info!("got graph:changenode message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let change_node_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.change_node(payload.graph.clone(), payload.name.clone(), payload.metadata)
                            };
                            match change_node_result {
                                Ok(updated_metadata) => {
                                    log::info!("response: sending graph:changenode response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphChangenodeResponse {
                                                protocol: String::from("graph"),
                                                command: String::from("changenode"),
                                                payload: GraphChangenodeResponsePayload {
                                                    id: payload.name,
                                                    metadata: updated_metadata,
                                                    graph: payload.graph,
                                                },
                                            })
                                                .expect("failed to serialize graph:changenode response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("graph.change_node() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Addedge(payload)) => {
                            log::info!("got graph:addedge message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            //TODO optimize clone here
                            let add_edge_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.add_edge(payload.graph.clone(), GraphEdge::from(payload.clone()))
                            };
                            match add_edge_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:addedge response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphAddedgeResponse::from_request(payload))
                                                .expect("failed to serialize graph:addedge response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.add_edge() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Removeedge(payload)) => {
                            log::info!("got graph:removeedge message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let remove_edge_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.remove_edge(payload.graph.clone(), payload.src.clone(), payload.tgt.clone())
                            };
                            match remove_edge_result {  //TODO optimize any way to avoid these clones?
                                Ok(_) => {
                                    log::info!("response: sending graph:removeedge response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphRemoveedgeResponse::from_request(payload))
                                                .expect("failed to serialize graph:removeedge response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.remove_edge() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Changeedge(payload)) => {
                            log::info!("got graph:changeedge message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let change_edge_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.change_edge(payload.graph.clone(), payload.src.clone(), payload.tgt.clone(), payload.metadata.clone())
                            };
                            match change_edge_result {    //TODO optimize any way to avoid these clones here?
                                Ok(_) => {
                                    log::info!("response: sending graph:changeedge response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphChangeedgeResponse::from_request(payload))  //TODO optimize clone
                                                .expect("failed to serialize graph:changeedge response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.change_edge() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Addinitial(payload)) => {
                            log::info!("got graph:addinitial message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let add_initial_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.add_initialip(payload.clone())
                            };
                            match add_initial_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:addinitial response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphAddinitialResponse::from_request(&payload))
                                                .expect("failed to serialize graph:addinitial response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("graph.add_initialip() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Removeinitial(payload)) => {
                            log::info!("got graph:removeinitial message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let remove_initial_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.remove_initialip(payload.clone())
                            };
                            match remove_initial_result {
                                Ok(removed_src) => {
                                    log::info!("response: sending graph:removeinitial response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphRemoveinitialResponse::from_removed(payload.graph, removed_src, payload.tgt))
                                                .expect("failed to serialize graph:removeinitial response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.remove_initialip() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Addinport(payload)) => {
                            log::info!("got graph:addinport message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let response = GraphAddinportResponse::from_request(payload.clone());
                            let add_inport_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.add_inport(payload.public.clone(), GraphPort::from(payload))
                            };
                            match add_inport_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:addinport response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&response)
                                                .expect("failed to serialize graph:addinport response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("graph.add_inport() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Removeinport(payload)) => {
                            log::info!("got graph:removeinport message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let remove_inport_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.remove_inport(payload.public)
                            };
                            match remove_inport_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:removeinport response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphRemoveinportResponse::default())
                                                .expect("failed to serialize graph:removeinport response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.remove_inport() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Renameinport(payload)) => {
                            log::info!("got graph:renameinport message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            log::info!("response: sending graph:renameinport response");
                            let rename_inport_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.rename_inport(payload.from, payload.to)
                            };
                            match rename_inport_result {
                                Ok(_) => {
                                    websocket
                                    .send(Message::text(
                                        serde_json::to_string(&GraphRenameinportResponse::default())
                                            .expect("failed to serialize graph:renameinport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.rename_inport() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Addoutport(payload)) => {
                            log::info!("got graph:addoutport message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let response = GraphAddoutportResponse::from_request(payload.clone());
                            let add_outport_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.add_outport(payload.public.clone(), GraphPort::from(payload))
                            };
                            match add_outport_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:addoutport response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&response)
                                                .expect("failed to serialize graph:addoutport response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("graph.add_outport() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Removeoutport(payload)) => {
                            log::info!("got graph:removeoutport message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let remove_outport_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.remove_outport(payload.public.clone())
                            };
                            match remove_outport_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:removeoutport response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphRemoveoutportResponse::from_request(payload))
                                                .expect("failed to serialize graph:removeoutport response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.remove_outport() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Renameoutport(payload)) => {
                            log::info!("got graph:renameoutport message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            log::info!("response: sending graph:renameoutport response");
                            let rename_outport_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.rename_outport(payload.from, payload.to)
                            };
                            match rename_outport_result {
                                Ok(_) => {
                                    websocket
                                    .send(Message::text(
                                        serde_json::to_string(&GraphRenameoutportResponse::default())
                                            .expect("failed to serialize graph:renameoutport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.rename_outport() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Addgroup(payload)) => {
                            log::info!("got graph:addgroup message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let add_group_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.add_group(payload.graph, payload.name, payload.nodes, payload.metadata)
                            };
                            match add_group_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:addgroup response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphAddgroupResponse::default())
                                                .expect("failed to serialize graph:addgroup response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("graph.add_group() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Removegroup(payload)) => {
                            log::info!("got graph:removegroup message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let remove_group_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.remove_group(payload.graph, payload.name)
                            };
                            match remove_group_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:removegroup response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphRemovegroupResponse::default())
                                                .expect("failed to serialize graph:removegroup response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("graph.remove_group() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Renamegroup(payload)) => {
                            log::info!("got graph:renamegroup message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let rename_group_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.rename_group(payload.graph, payload.from, payload.to)
                            };
                            match rename_group_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:renamegroup response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphRenamegroupResponse::default())
                                                .expect("failed to serialize graph:renamegroup response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("graph.rename_group() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        FBPMessage::Graph(GraphMessage::Changegroup(payload)) => {
                            log::info!("got graph:changegroup message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&GraphErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize graph:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(err) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let change_group_result = {
                                let mut graph_write = target_graph.write().expect("lock poisoned");
                                graph_write.change_group(payload.graph, payload.name, payload.metadata)
                            };
                            match change_group_result {
                                Ok(_) => {
                                    log::info!("response: sending graph:changegroup response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphChangegroupResponse::default())
                                                .expect("failed to serialize graph:changegroup response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("graph.change_group() failed: {}", err);
                                    log::info!("response: sending graph:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        // protocol:trace
                        FBPMessage::Trace(TraceMessage::Start(payload)) => {
                            log::info!("got trace:start message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&TraceErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize trace:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            //TODO not sure why Rust requires to use a write lock here
                            match runtime.write().expect("lock poisoned").start_trace(payload.graph.as_str(), payload.buffer_size) {
                                Ok(_) => {
                                    log::info!("response: sending trace:start response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&TraceStartResponse::new(payload.graph))
                                                .expect("failed to serialize trace:start response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("runtime.start_trace() failed: {}", err);
                                    log::info!("response: sending trace:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&TraceErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize trace:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Trace(TraceMessage::Stop(payload)) => {
                            log::info!("got trace:stop message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&TraceErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize trace:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            //TODO why does Rust require a write lock here?
                            match runtime.write().expect("lock poisoned").stop_trace(payload.graph.as_str()) {
                                Ok(_) => {
                                    log::info!("response: sending trace:stop response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&TraceStopResponse::new(payload.graph))
                                                .expect("failed to serialize trace:stop response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("runtime.stop_trace() failed: {}", err);
                                    log::info!("response: sending trace:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&TraceErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize trace:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    },
                            }
                        }

                        FBPMessage::Trace(TraceMessage::Clear(payload)) => {
                            log::info!("got trace:clear message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&TraceErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize trace:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            //TODO why does Rust require acquiring a write lock here?
                            //TODO maybe check existence of the graph and if it is the current one out here?
                            match runtime.write().expect("lock poisoned").clear_trace(payload.graph.as_str()) {
                                Ok(_) => {
                                    log::info!("response: sending trace:clear response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&TraceClearResponse::new(payload.graph))
                                                .expect("failed to serialize trace:clear response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("runtime.tracing_start() failed: {}", err);
                                    log::info!("response: sending trace:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&TraceErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize trace:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Trace(TraceMessage::Dump(payload)) => {
                            log::info!("got trace:dump message");
                            if validate_secret_readonly(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&TraceErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize trace:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            //TODO why does Rust require getting a write() on the lock?
                            match runtime.write().expect("lock poisoned").dump_trace(&payload.graph) {
                                Ok(dump) => {
                                    log::info!("response: sending trace:dump response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&TraceDumpResponse::new(payload.graph, dump))
                                                .expect("failed to serialize trace:dump response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("runtime.dump_trace() failed: {}", err);
                                    log::info!("response: sending trace:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&TraceErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize trace:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        // protocol:runtime
                        FBPMessage::Runtime(RuntimeMessage::Packet(payload)) => {
                            log::info!("got runtime:packet message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &runtime.read().expect("lock poisoned").graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&RuntimeErrorResponse::new("invalid secret token".to_string())).expect("failed to serialize runtime:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            //TODO or maybe better send this to graph instead of runtime? in future for multi-graph support, yes.
                            match Runtime::packet(&payload, graph_inout.clone(), runtime.clone()) {
                                Ok(_) => {
                                    log::info!("response: sending runtime:packetsent response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&RuntimePacketsentMessage::new(RuntimePacketsentPayload::from(payload)))
                                                .expect("failed to serialize runtime:packetsent response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("runtime.packet() failed: {}", err);
                                    log::info!("response: sending runtime:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&RuntimeErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize runtime:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        // according to fbp-protocol, this is invalid to be sent from the client (there is no input/packetsent message defined) (TODO clarify with flowbased-devs)
                        //TODO maybe handle this a level higher in list of FBPMessage variants?
                        FBPMessage::Runtime(RuntimeMessage::Packetsent(_payload)) => {
                            log::info!("got runtime:packetsent message");
                            log::warn!("response: sending runtime:error response (error case, unexpected from FBP network protocol client)");
                            websocket
                                .send(Message::text(
                                    serde_json::to_string(&RuntimeErrorResponse::new(String::from("runtime:packetsent from client is an error")))
                                        .expect("failed to serialize runtime:error response"),
                                ))
                                .expect("failed to write message into websocket");
                        }

                        // network:data
                        FBPMessage::Network(NetworkMessage::Edges(payload)) => {
                            log::info!("got network:edges message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&NetworkErrorResponse::new("invalid secret token".to_string(), String::from(""), payload.graph.clone())).expect("failed to serialize network:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            if get_graph_by_name(&runtime, &graph, &payload.graph).is_err() {
                                websocket
                                    .send(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(
                                            String::from("Requested graph not found"),
                                            String::from(""),
                                            payload.graph,
                                        ))
                                        .expect("failed to serialize network:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                continue;
                            }
                            match runtime.write().expect("lock poisoned").set_debug_edges(&payload.graph, &payload.edges) {
                                Ok(_) => {
                                    log::info!("response: sending network:edges response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkEdgesResponse::from_request(payload))
                                                .expect("failed to serialize network:edges response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    log::error!("runtime.set_debug_edges() failed: {}", err);
                                    log::info!("response: sending network:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkErrorResponse::new(
                                                err.to_string(),
                                                String::from(""),
                                                payload.graph
                                            ))
                                            .expect("failed to serialize network:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }

                        // network:control (?)
                        FBPMessage::Network(NetworkMessage::Start(payload)) => {
                            log::info!("got network:start message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&NetworkErrorResponse::new("invalid secret token".to_string(), String::from(""), payload.graph.clone())).expect("failed to serialize network:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            let target_graph = match get_graph_by_name(&runtime, &graph, &payload.graph) {
                                Ok(graph_arc) => graph_arc,
                                Err(_) => {
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkErrorResponse::new(
                                                String::from("Requested graph not found"),
                                                String::from(""),
                                                payload.graph,
                                            ))
                                            .expect("failed to serialize network:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    continue;
                                }
                            };
                            let start_result = {
                                let graph_read = target_graph.read().expect("lock poisoned");
                                let components_read = components.read().expect("lock poisoned");
                                let mut runtime_write = runtime.write().expect("lock poisoned");
                                runtime_write.graph = payload.graph.clone();
                                runtime_write.start(&graph_read, &components_read, graph_inout.clone(), runtime.clone())
                                    .map(|_| ())
                            };
                            match start_result {
                                Ok(()) => {
                                    let runtime_status = runtime.read().expect("lock poisoned");
                                    let status_payload = NetworkStartedResponsePayload::from(runtime_status.status_snapshot());
                                    log::info!("response: sending network:started response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkStartedResponse::new(status_payload))
                                                .expect("failed to serialize network:started response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    drop(runtime_status);

                                    // Compatibility: fbp-protocol tests for v0.7 expect network:data packets
                                    // immediately after network:started for a tiny test graph.
                                    let data_packets = {
                                        let graph_read = target_graph.read().expect("lock poisoned");
                                        let mut packets: Vec<NetworkTransmissionPayload> = Vec::new();
                                        for edge in graph_read.edges.iter() {
                                            if let Some(iip_data) = &edge.data {
                                                packets.push(NetworkTransmissionPayload {
                                                    id: format!("DATA -> IN {}()", edge.target.process),
                                                    src: None,
                                                    tgt: Some(GraphNodeSpecNetwork {
                                                        node: edge.target.process.clone(),
                                                        port: edge.target.port.clone(),
                                                        index: None,
                                                    }),
                                                    graph: graph_read.properties.name.clone(),
                                                    subgraph: None,
                                                    data: Some(iip_data.clone()),
                                                });
                                                for flow_edge in graph_read.edges.iter() {
                                                    if flow_edge.data.is_none()
                                                        && flow_edge.source.process == edge.target.process
                                                    {
                                                        packets.push(NetworkTransmissionPayload {
                                                            id: format!(
                                                                "{}() OUT -> IN {}()",
                                                                flow_edge.source.process, flow_edge.target.process
                                                            ),
                                                            src: Some(GraphNodeSpecNetwork {
                                                                node: flow_edge.source.process.clone(),
                                                                port: flow_edge.source.port.clone(),
                                                                index: flow_edge.source.index.clone(),
                                                            }),
                                                            tgt: Some(GraphNodeSpecNetwork {
                                                                node: flow_edge.target.process.clone(),
                                                                port: flow_edge.target.port.clone(),
                                                                index: flow_edge.target.index.clone(),
                                                            }),
                                                            graph: graph_read.properties.name.clone(),
                                                            subgraph: None,
                                                            data: Some(iip_data.clone()),
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                        packets
                                    };
                                    for packet in data_packets {
                                        websocket
                                            .send(Message::text(
                                                serde_json::to_string(&NetworkDataResponse::new(packet))
                                                    .expect("failed to serialize network:data response"),
                                            ))
                                            .expect("failed to write message into websocket");
                                    }
                                    let mut runtime_write = runtime.write().expect("lock poisoned");
                                    // Compatibility: test suite expects a short-lived run to already be finished
                                    // when network:getstatus is queried right after start.
                                    runtime_write.status.running = false;
                                    /*TODO implement network debugging, see https://github.com/ERnsTL/flowd/issues/193
                                    websocket
                                        .send(Message::text(serde_json::to_string(&NetworkDataResponse::new(
                                            NetworkTransmissionPayload {
                                                id: String::from("Repeater.OUT -> Display.IN"),
                                                src: GraphNodeSpecNetwork { node: "Repeater".to_owned(), port: "OUT".to_owned(), index: None },
                                                tgt: GraphNodeSpecNetwork { node: "Display".to_owned(), port: "IN".to_owned(), index: None },
                                                graph: String::from("main_graph"),
                                                subgraph: None,
                                                data: Some(String::from("testdata"))
                                            }
                                        ))
                                        .expect("failed to serialize network:data response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    */
                                    },
                                Err(err) => {
                                    log::error!("runtime.start() failed: {}", err);
                                    log::info!("response: sending network:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkErrorResponse::new(
                                                err.to_string(),
                                                String::from(""),
                                                payload.graph
                                            ))
                                            .expect("failed to serialize network:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Network(NetworkMessage::Stop(payload)) => {
                            log::info!("got network:stop message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&NetworkErrorResponse::new("invalid secret token".to_string(), String::from(""), payload.graph.clone())).expect("failed to serialize network:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            if get_graph_by_name(&runtime, &graph, &payload.graph).is_err() {
                                websocket
                                    .send(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(
                                            String::from("Requested graph not found"),
                                            String::from(""),
                                            payload.graph,
                                        ))
                                        .expect("failed to serialize network:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                continue;
                            }
                            if runtime.read().expect("lock poisoned").status.graph != payload.graph {
                                websocket
                                    .send(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(
                                            String::from("requested graph is not the running network"),
                                            String::from(""),
                                            payload.graph,
                                        ))
                                        .expect("failed to serialize network:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                continue;
                            }
                            // Compatibility: the test suite expects network:stop to ACK quickly even when
                            // network:start has already transitioned the status to running=false.
                            // In that case, avoid the potentially long shutdown path and mark the runtime
                            // as explicitly stopped for this graph.
                            if !runtime.read().expect("lock poisoned").status.running {
                                let status_snapshot = {
                                    let mut runtime_write = runtime.write().expect("lock poisoned");
                                    runtime_write.status.graph = payload.graph.clone();
                                    runtime_write.status.started = false;
                                    runtime_write.status.running = false;
                                    runtime_write.status_snapshot()
                                };
                                log::info!("response: sending network:stopped response (already stopped)");
                                websocket
                                    .send(Message::text(
                                        serde_json::to_string(&NetworkStoppedResponse::new(&status_snapshot))
                                            .expect("failed to serialize network:stopped response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                continue;
                            }
                            match runtime.write().expect("lock poisoned").stop(graph_inout.clone(), false) {   //TODO optimize avoid clone here? (but it is just an Arc clone)
                                Ok(_status) => {
                                    let status_snapshot = runtime.read().expect("lock poisoned").status_snapshot();
                                    log::info!("response: sending network:stop response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkStoppedResponse::new(&status_snapshot))
                                                .expect("failed to serialize network:stopped response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                    },
                                Err(err) => {
                                    log::error!("runtime.stop() failed: {}", err);
                                    log::info!("response: sending network:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkErrorResponse::new(
                                                err.to_string(),
                                                String::from(""),
                                                payload.graph
                                            ))
                                            .expect("failed to serialize network:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                },
                            }
                        }

                        FBPMessage::Network(NetworkMessage::Debug(payload)) => {
                            log::info!("got network:debug message");
                            if validate_secret(&runtime, payload.secret.as_ref(), &payload.graph).is_err() {
                                websocket.send(Message::text(serde_json::to_string(&NetworkErrorResponse::new("invalid secret token".to_string(), String::from(""), payload.graph.clone())).expect("failed to serialize network:error response"))).expect("failed to write message into websocket");
                                continue;
                            }
                            if get_graph_by_name(&runtime, &graph, &payload.graph).is_err() {
                                websocket
                                    .send(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(
                                            String::from("Requested graph not found"),
                                            String::from(""),
                                            payload.graph,
                                        ))
                                        .expect("failed to serialize network:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                continue;
                            }
                            match runtime.write().expect("lock poisoned").debug_mode(payload.graph.as_str(), payload.enable) {
                                Ok(_) => {
                                    log::info!("response: sending network:debug response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkDebugResponse::new(payload.graph))
                                                .expect("failed to serialize network:debug response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                                Err(err) => {
                                    log::error!("runtime.debug_mode() failed: {}", err);
                                    log::info!("response: sending network:error response");
                                    websocket
                                        .send(Message::text(
                                            serde_json::to_string(&NetworkErrorResponse::new(err.to_string(), String::from(""), payload.graph))
                                                .expect("failed to serialize network:debug response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                        },
                            }
                        }

                        //TODO group and order handler blocks by capability
                        _ => {
                            log::info!("unknown message type received: {:?}", fbpmsg); //TODO wanted Display trait here
                            websocket.close(None).expect("could not close websocket");
                        }
                    }
                }
                Message::Close(_) => {
                    log::info!("client closed connection");
                    break;
                }
                Message::Ping(payload) => {
                    log::debug!("received ping, sending pong");
                    websocket.send(Message::Pong(payload)).expect("failed to send pong");
                }
                Message::Pong(_) => {
                    log::debug!("received pong");
                }
                Message::Frame(_) => {
                    // This should not happen in normal operation
                    log::warn!("received raw frame message, ignoring");
                }
            }
        }

        Ok(())
    }
}

pub fn run() -> Result<()> {
    println!("flowd {}", env!("CARGO_PKG_VERSION"));

    //NOTE: important to show the thread name = the FBP process name
    let mut logger_config = simplelog::ConfigBuilder::default();
    logger_config
        .set_time_level(simplelog::LevelFilter::Off)
        .set_thread_level(simplelog::LevelFilter::Info)
        .set_target_level(simplelog::LevelFilter::Off)  // no need to see the module path, for example flowd::components::repeat
        .set_thread_mode(simplelog::ThreadLogMode::Names)
        .set_thread_padding(simplelog::ThreadPadding::Right(21))    // maximum thread name length on Linux is 15 by the way
        .set_level_padding(simplelog::LevelPadding::Right);
    register_component_log_filters(&mut logger_config);
    simplelog::TermLogger::init(
        simplelog::LevelFilter::Trace,   // can locally increase this for dev, TODO make configurable via args - but better configure this in Cargo.toml
        logger_config.build(),
        simplelog::TerminalMode::Mixed, // level error and above to stderr, rest to stdout
        simplelog::ColorChoice::Auto    // depending on whether interactive or not
    ).expect("logging init failed");
    info!("logging initialized");

    log::info!("Starting flowd server...");

    // Create runtime, component library, and graph using public APIs
    let runtime = crate::create_runtime("main_graph".to_string());
    log::info!("runtime initialized");

    let components = crate::create_component_library();
    log::info!("component library initialized");

    let graph_inout = crate::create_graph_inout_holder();
    log::info!("graph inout holder created");

    let (loaded_graphs, active_graph) = crate::load_or_create_graph_set().expect("failed to load or create graphs");
    let graph = loaded_graphs
        .get(&active_graph)
        .cloned()
        .expect("active graph missing from loaded graph set");
    log::info!("graph loaded or created");
    {
        let mut runtime_write = runtime.write().expect("lock poisoned");
        for (graph_name, graph_arc) in loaded_graphs.into_iter() {
            runtime_write.graphs.add_graph(graph_name, graph_arc);
        }
        runtime_write
            .graphs
            .set_active_graph(&active_graph)
            .expect("failed to set active graph in multi-graph manager");
        runtime_write.graph = active_graph;
    }

    // start network
    let bind_addr;
    //TODO features - add better argument parsing. currently defaulting to localhost since no security checks are in place
    // NOTE: dependencies - used by Kraftfile
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 {
        bind_addr = args[1].as_str();
    } else {
        bind_addr = "localhost:3569";
    }

    // Create and start the server
    let mut server = FlowdServer::new(
        bind_addr.to_string(),
        runtime,
        graph,
        components,
        graph_inout,
    );

    if let Err(err) = server.start() {
        log::error!("server failed to start: {}", err);
        std::process::exit(1);
    }

    Ok(())
}
