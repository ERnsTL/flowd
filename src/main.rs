#![feature(duration_constants)]
#![feature(io_error_more)]
#![feature(map_try_insert)]

use std::net::{TcpListener, TcpStream};
use std::str;
use std::sync::{Arc, RwLock, Mutex};
use std::thread::{self, Thread};
use std::time::Duration;

use tungstenite::handshake::server::{Request, Response};
use tungstenite::handshake::HandshakeRole;
use tungstenite::{accept_hdr, Error, HandshakeError, Message, Result};

#[macro_use] extern crate log;
extern crate simplelog; //TODO check the paris feature flag for tags, useful?

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use std::collections::HashMap;
use multimap::MultiMap;
//use dashmap::DashMap;

use chrono::prelude::*;


// configuration
const PROCESS_HEALTHCHECK_DUR: core::time::Duration = Duration::from_secs(7);   //NOTE: 7 * core::time::Duration::SECOND is not compile-time calculatable (mul const trait not implemented)
const NODE_WIDTH_DEFAULT: u32 = 72;
const NODE_HEIGHT_DEFAULT: u32 = 72;

fn must_not_block<Role: HandshakeRole>(err: HandshakeError<Role>) -> Error {
    match err {
        HandshakeError::Interrupted(_) => panic!("Bug: blocking socket would block"),
        HandshakeError::Failure(f) => f,
    }
}

// components
//TODO optimize - currently organized as modules, because the components need certain structs from flowd-rs crate here. if putting each component into its own crate, this would create a circular dependency, which is not allowed with crates, but very well modules.
mod components;
use components::repeat::RepeatComponent;
use components::drop::DropComponent;
use components::output::OutputComponent;
use components::libcomponent::LibComponent;
use components::unixsocketserver::UnixSocketServerComponent;
use components::file::{FileReaderComponent, FileTailerComponent, FileWriterComponent};
use components::trim::TrimComponent;
use components::splitlines::SplitLinesComponent;
use components::count::CountComponent;
use components::cron::CronComponent;
use components::cmd::CmdComponent;
use components::hasher::HasherComponent;
use components::equals::EqualsComponent;
use components::httpclient::HTTPClientComponent;
use components::httpserver::HTTPServerComponent;
use components::muxer::MuxerComponent;
use components::mqtt::{MQTTPublisherComponent, MQTTSubscriberComponent};
use components::redis::{RedisPublisherComponent, RedisSubscriberComponent};
use components::imap::{IMAPAppendComponent, IMAPFetchIdleComponent};
use components::openai::OpenAIChatComponent;
use components::template::TeraTemplateComponent;
use components::regexp::RegexpExtractComponent;
use components::text::TextReplaceComponent;
use components::zeroconf::ZeroconfResponderComponent;

fn main() {
    println!("flowd {}", env!("CARGO_PKG_VERSION"));

    //NOTE: important to show the thread name = the FBP process name
    simplelog::TermLogger::init(
        simplelog::LevelFilter::Trace,   // can locally increase this for dev, TODO make configurable via args - but better configure this in Cargo.toml
        simplelog::ConfigBuilder::default()
            .set_time_level(simplelog::LevelFilter::Off)
            .set_thread_level(simplelog::LevelFilter::Info)
            .set_target_level(simplelog::LevelFilter::Off)  // no need to see the module path, for example flowd::components::repeat
            .set_thread_mode(simplelog::ThreadLogMode::Names)
            .set_thread_padding(simplelog::ThreadPadding::Right(21))    // maximum thread name length on Linux is 15 by the way
            .set_level_padding(simplelog::LevelPadding::Right)
            .add_filter_ignore_str("rumqttc")   //TODO optimize - unfortunately the rumqttc crate sends debug! about PingReq every few seconds in https://github.com/bytebeamio/rumqtt/blob/0266b85bd5986f556b3eaedc806c964e906232b8/rumqttc/src/state.rs#L416 and https://github.com/bytebeamio/rumqtt/blob/0266b85bd5986f556b3eaedc806c964e906232b8/rumqttc/src/v5/state.rs#L589 and https://github.com/bytebeamio/rumqtt/blob/0266b85bd5986f556b3eaedc806c964e906232b8/rumqttc/src/state.rs#L357
            .add_filter_ignore_str("rustls")   //TODO optimize - unfortunately the rustls sends debug! about session parameters etc. on connection establishment
            .add_filter_ignore_str("simple_mdns")   // TODO optimize - unfortunately simle_mdns sends debug! about every packet received
            .build(),
        simplelog::TerminalMode::Mixed, // level error and above to stderr, rest to stdout
        simplelog::ColorChoice::Auto    // depending on whether interactive or not
    ).expect("logging init failed");
    info!("logging initialized");

    //TODO the runtime should manage the graphs -> add_graph() and also checking that they actually exist and should have a method switch_graph()
    let runtime: Arc<RwLock<RuntimeRuntimePayload>> = Arc::new(RwLock::new(RuntimeRuntimePayload::new(
        String::from("main_graph")
    )));
    info!("runtime initialized");

    //NOTE: is currently located inside the runtime struct above; more notes on the "processes" field there
    //let processes: Arc<RwLock<ProcessManager>> = Arc::new(RwLock::new(ProcessManager::default()));
    //info!("process manager initialized");

    //NOTE: also add new core components in runtime.start()
    let componentlib: Arc<RwLock<ComponentLibrary>> = Arc::new(RwLock::new(ComponentLibrary::new(vec![
        RepeatComponent::get_metadata(),
        DropComponent::get_metadata(),
        OutputComponent::get_metadata(),
        LibComponent::get_metadata(),
        UnixSocketServerComponent::get_metadata(),
        FileReaderComponent::get_metadata(),
        TrimComponent::get_metadata(),
        SplitLinesComponent::get_metadata(),
        CountComponent::get_metadata(),
        CronComponent::get_metadata(),
        CmdComponent::get_metadata(),
        HasherComponent::get_metadata(),
        EqualsComponent::get_metadata(),
        HTTPClientComponent::get_metadata(),
        HTTPServerComponent::get_metadata(),
        MuxerComponent::get_metadata(),
        MQTTPublisherComponent::get_metadata(),
        MQTTSubscriberComponent::get_metadata(),
        RedisPublisherComponent::get_metadata(),
        RedisSubscriberComponent::get_metadata(),
        IMAPAppendComponent::get_metadata(),
        IMAPFetchIdleComponent::get_metadata(),
        OpenAIChatComponent::get_metadata(),
        TeraTemplateComponent::get_metadata(),
        FileTailerComponent::get_metadata(),
        RegexpExtractComponent::get_metadata(),
        FileWriterComponent::get_metadata(),
        TextReplaceComponent::get_metadata(),
        ZeroconfResponderComponent::get_metadata(),
    ])));
    //TODO actually load components
    info!("component library initialized");

    //TODO graph (or runtime?) should check if the components used in the graph are actually available in the component library
    let graph: Arc<RwLock<Graph>> = Arc::new(RwLock::new(Graph::new(
        String::from("main_graph"),
        String::from("basic description"),
        String::from("usd")
    )));  //TODO check if an RwLock is OK (multiple readers possible, but what if writer deletes that thing being read?) or if Mutex needed
    // holder for a copy of the TcpStreams = connections of the clients and the graph exported inports and outports (for the graph inport and graph outport handler threads)
    let graph_inout: Arc<Mutex<GraphInportOutportHolder>> = Arc::new(Mutex::new(GraphInportOutportHolder { inports: None, outports: None, websockets: HashMap::new() }));
    info!("graph initialized");

    // add graph exported/published inport and outport
    graph.write().expect("lock poisoned").inports.insert("GRAPHIN".to_owned(), GraphPort {
        process: "Repeat_31337".to_owned(),
        port: "IN".to_owned(),
        metadata: GraphPortMetadata {
            x: 36,
            y: 72,
        }
    });
    graph.write().expect("lock poisoned").outports.insert("GRAPHOUT".to_owned(), GraphPort {
        process: "Repeat_31338".to_owned(),
        port: "OUT".to_owned(),
        metadata: GraphPortMetadata {
            x: 468,
            y: 72,
        }
    });
    graph.write().expect("lock poisoned").add_node("main_graph".to_owned(), "Repeat".to_owned(), "Repeat_31337".to_owned(), GraphNodeMetadata { x: 180, y: 72, height: Some(NODE_HEIGHT_DEFAULT), width: Some(NODE_WIDTH_DEFAULT), label: Some("Repeat".to_owned()) }).expect("add_node() failed");
    //NOTE: bug in noflo-ui, which does not allow reconnecting exported ports to other components, they just vanish then (TODO)
    graph.write().expect("lock poisoned").add_edge("main_graph".to_owned(), GraphEdge { source: GraphNodeSpec { process: "Repeat_31337".to_owned(), port: "OUT".to_owned(), index: None }, data: None, target: GraphNodeSpec { process: "Repeat_31338".to_owned(), port: "IN".to_owned(), index: None }, metadata: GraphEdgeMetadata::new(None, None, None) }).expect("add_edge() failed");
    graph.write().expect("lock poisoned").add_node("main_graph".to_owned(), "Repeat".to_owned(), "Repeat_31338".to_owned(), GraphNodeMetadata { x: 324, y: 72, height: Some(NODE_HEIGHT_DEFAULT), width: Some(NODE_WIDTH_DEFAULT), label: Some("Repeat".to_owned()) }).expect("add_node() failed");
    // add components required for test suite
    graph.write().expect("lock poisoned").add_node("main_graph".to_owned(), "Repeat".to_owned(), "Repeat_2ufmu".to_owned(), GraphNodeMetadata { x: 36, y: 216, height: Some(NODE_HEIGHT_DEFAULT), width: Some(NODE_WIDTH_DEFAULT), label: Some("Repeat".to_owned()) }).expect("add_node() failed");
    graph.write().expect("lock poisoned").add_node("main_graph".to_owned(), "Drop".to_owned(), "Drop_raux7".to_owned(), GraphNodeMetadata { x: 324, y: 216, height: Some(NODE_HEIGHT_DEFAULT), width: Some(NODE_WIDTH_DEFAULT), label: Some("Drop".to_owned()) }).expect("add_node() failed");
    graph.write().expect("lock poisoned").add_node("main_graph".to_owned(), "Output".to_owned(), "Output_mwr5y".to_owned(), GraphNodeMetadata { x: 180, y: 216, height: Some(NODE_HEIGHT_DEFAULT), width: Some(NODE_WIDTH_DEFAULT), label: Some("Output".to_owned()) }).expect("add_node() failed");
    graph.write().expect("lock poisoned").add_edge("main_graph".to_owned(), GraphEdge { source: GraphNodeSpec { process: "".to_owned(), port: "".to_owned(), index: None }, data: Some("test IIP data".to_owned()), target: GraphNodeSpec { process: "Repeat_2ufmu".to_owned(), port: "IN".to_owned(), index: None }, metadata: GraphEdgeMetadata::new(None, None, None) }).expect("add_edge() failed");
    graph.write().expect("lock poisoned").add_edge("main_graph".to_owned(), GraphEdge { source: GraphNodeSpec { process: "Repeat_2ufmu".to_owned(), port: "OUT".to_owned(), index: None }, data: None, target: GraphNodeSpec { process: "Output_mwr5y".to_owned(), port: "IN".to_owned(), index: None }, metadata: GraphEdgeMetadata::new(None, None, None) }).expect("add_edge() failed");
    graph.write().expect("lock poisoned").add_edge("main_graph".to_owned(), GraphEdge { source: GraphNodeSpec { process: "Output_mwr5y".to_owned(), port: "OUT".to_owned(), index: None }, data: None, target: GraphNodeSpec { process: "Drop_raux7".to_owned(), port: "IN".to_owned(), index: None }, metadata: GraphEdgeMetadata::new(None, None, None) }).expect("add_edge() failed");

    let bind_addr = "localhost:3569";
    let server = TcpListener::bind(bind_addr).unwrap();
    info!("management listening on {} - manage via GUI at https://app.flowhub.io/#runtime/endpoint?protocol%3Dwebsocket%26address%3Dws%3A%2F%2Flocalhost%3A3569", bind_addr);   //TODO URL escape of bind_addr in URL - currently static

    for stream_res in server.incoming() {
        if let Ok(stream) = stream_res {
            // create Arc pointers for the new thread
            let graphref = graph.clone();
            let runtimeref = runtime.clone();
            let componentlibref = componentlib.clone();
            let graph_inoutref = graph_inout.clone();
            //let processesref = processes.clone();

            // start thread
            // since the thread name can only be 15 characters on Linux and an IP address already has up to 15, the IP address is not in the name
            thread::Builder::new().name("client-handler".into()).spawn(move || {
                info!("got a client from {}", stream.peer_addr().expect("get peer address failed"));
                //if let Err(err) = handle_client(stream, graphref, runtimeref, componentlibref, processesref) {
                if let Err(err) = handle_client(stream, graphref, runtimeref, componentlibref, graph_inoutref) {
                    match err {
                        Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8 => (),
                        e => error!("test: {}", e),
                    }
                }
            }).expect("thread start for connection handler failed");
        } else if let Err(e) = stream_res {
            error!("Error accepting stream: {}", e);
        }
    }
}

//fn handle_client(stream: TcpStream, graph: Arc<RwLock<Graph>>, runtime: Arc<RwLock<RuntimeRuntimePayload>>, components: Arc<RwLock<ComponentLibrary>>, processes: Arc<RwLock<ProcessManager>>) -> Result<()> {
fn handle_client(stream: TcpStream, graph: Arc<RwLock<Graph>>, runtime: Arc<RwLock<RuntimeRuntimePayload>>, components: Arc<RwLock<ComponentLibrary>>, graph_inout: Arc<std::sync::Mutex<GraphInportOutportHolder>>) -> Result<()> {
    stream
        .set_write_timeout(Some(Duration::SECOND))
        .expect("set_write_timeout call failed");
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
        graph_inout.lock().expect(r#"could not acquire lock for saving TcpStream for graph outport process"#).websockets.insert(peer_addr, tungstenite::WebSocket::from_raw_socket(stream.try_clone().expect("could not try_clone() TcpStream"), tungstenite::protocol::Role::Server, None));
    }

    let callback = |req: &Request, mut response: Response| {
        debug!("Received a new ws handshake");
        debug!("The request's path is: {}", req.uri().path());
        debug!("The request's headers are:");
        for (ref key, value) in req.headers() {
            debug!("  {}: {:?}", key, value);
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

    debug!("entering receive loop");
    loop {
        debug!("waiting for next message");
        match websocket.read()? {
            msg @ Message::Text(_) | msg @ Message::Binary(_) => {
                debug!("got a text|binary message");
                //debug!("message data: {}", msg.clone().into_text().unwrap());

                let fbpmsg: FBPMessage = serde_json::from_slice(msg.into_data().as_slice())
                    .expect("failed to decode JSON message"); //TODO data handover optimizable?
                                                              //TODO handle panic because of decoding error here

                match fbpmsg {
                    // runtime base
                    FBPMessage::RuntimeGetruntimeMessage(payload) => {
                        info!("got runtime:getruntime message with secret {}", payload.secret);
                        // send response = runtime:runtime message
                        info!("response: sending runtime:runtime message");
                        websocket
                            .write(Message::text(
                                //TODO handing over value inside lock would work like this:  serde_json::to_string(&*runtime.read().expect("lock poisoned"))
                                serde_json::to_string(&RuntimeRuntimeMessage::new(&runtime.read().expect("lock poisoned")))
                                .expect("failed to serialize runtime:runtime message"),
                            ))
                            .expect("failed to write message into websocket");
                        // spec: "If the runtime is currently running a graph and it is able to speak the full Runtime protocol, it should follow up with a ports message."
                        info!("response: sending runtime:ports message");
                        websocket
                            .write(Message::text(
                                serde_json::to_string(&RuntimePortsMessage::new(&runtime.read().expect("lock poisoned"), &graph.read().expect("lock poisoned")))
                                    .expect("failed to serialize runtime:ports message"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    // protocol:component
                    FBPMessage::ComponentListRequest(_payload) => {
                        info!("got component:list message");
                        //TODO check secret
                        let mut count: u32 = 0;
                        for component in components.read().expect("lock poisoned").available.iter() {
                            info!("response: sending component:component message");
                            websocket
                            .write(Message::text(
                                serde_json::to_string(&ComponentComponentMessage::new(&component))
                                    .expect("failed to serialize component:component response"),
                            ))
                            .expect("failed to write message into websocket");
                            count += 1;
                        }
                        info!("response: sending component:componentsready response");
                        websocket
                            .write(Message::text(
                                serde_json::to_string(&ComponentComponentsreadyMessage::new(count))
                                    .expect("failed to serialize component:componentsready response"),
                            ))
                            .expect("failed to write message into websocket");
                        info!("sent {} component:component responses", count);
                        }

                    FBPMessage::NetworkGetstatusMessage(_payload) => {
                        info!("got network:getstatus message");
                        //TODO check secret
                        info!("response: sending network:status message");
                        websocket
                            .write(Message::text(
                                serde_json::to_string(&NetworkStatusMessage::new(&NetworkStatusPayload::new(&runtime.read().expect("lock poisoned").status)))
                                    .expect("failed to serialize network:status message"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::NetworkPersistRequest(_payload) => {
                        info!("got network:persist message");
                        //TODO check secret
                        // persist and send either network:persist or network:error
                        match runtime.read().expect("lock poisoned").persist() {    //NOTE: lock read() is enough, because persist() does not modify state, just copies it away to persistence
                            Ok(_) => {
                                info!("response: sending network:persist message");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&NetworkPersistResponse::default())
                                            .expect("failed to serialize network:persist message"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("persist failed: {}", err);
                                info!("response: sending network:error message");
                                websocket
                                    .write(Message::text(
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

                    FBPMessage::ComponentGetsourceMessage(payload) => {
                        info!("got component:getsource message");
                        //TODO multi-graph support (runtime has the info which graph is running currently)
                        //TODO optimize: need 2 locks to get graph source - and it is not the common case
                        if graph.read().expect("lock poisoned").properties.name == payload.name {
                            // retrieve graph source
                            info!("got a request for graph source of {}", &payload.name);
                            //TODO why does Rust require a write lock here? "cannot borrow data in dereference as mutable"
                            debug!("source is: {}", graph.write().expect("lock poisoned").get_source(payload.name.clone()).expect("could not get graph source").code);
                            match graph.write().expect("lock poisoned").get_source(payload.name) {
                                Ok(source_info) => {
                                    info!("response: sending component:source message for graph");
                                    websocket
                                    .write(Message::text(
                                        serde_json::to_string(&ComponentSourceMessage::new(source_info))
                                            .expect("failed to serialize component:source message"),
                                    ))
                                    .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    error!("graph.get_source() failed: {}", err);
                                    info!("response: sending graph:error response");
                                    websocket
                                        .write(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        } else {
                            // retrieve component source from component library
                            info!("got a request for component source of {}", &payload.name);
                            match components.read().expect("lock poisoned").get_source(payload.name) {
                                Ok(source_info) => {
                                    info!("response: sending component:source message for component");
                                    websocket
                                    .write(Message::text(
                                        serde_json::to_string(&ComponentSourceMessage::new(source_info))
                                            .expect("failed to serialize component:source message"),
                                    ))
                                    .expect("failed to write message into websocket");
                                },
                                Err(err) => {
                                    error!("componentlib.get_source() failed: {}", err);
                                    info!("response: sending graph:error response");
                                    websocket
                                        .write(Message::text(
                                            serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                                .expect("failed to serialize graph:error response"),
                                        ))
                                        .expect("failed to write message into websocket");
                                }
                            }
                        }
                    }

                    FBPMessage::GraphClearRequest(payload) => {
                        info!("got graph:clear message");
                        match graph.write().expect("lock poisoned").clear(&payload, &runtime.read().expect("lock poisoned")) {
                            Ok(_) => {
                                info!("response: sending graph:clear response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphClearResponse::new(&payload))
                                            .expect("failed to serialize graph:clear response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.clear() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphAddnodeRequest(payload) => {
                        info!("got graph:addnode message");
                        match graph.write().expect("lock poisoned").add_node(payload.graph, payload.component, payload.name, payload.metadata) {
                            Ok(response) => {
                                info!("response: sending graph:addnode response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphAddnodeResponse::new(response))
                                            .expect("failed to serialize graph:addnode response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.add_node() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphRemovenodeRequest(payload) => {
                        info!("got graph:removenode message");
                        match graph.write().expect("lock poisoned").remove_node(payload.graph, payload.name) {
                            Ok(_) => {
                                info!("response: sending graph:removenode response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphRemovenodeResponse::default())
                                            .expect("failed to serialize graph:removenode response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.remove_node() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphRenamenodeRequest(payload) => {
                        info!("got graph:renamenode message");
                        match graph.write().expect("lock poisoned").rename_node(payload.graph, payload.from, payload.to) {
                            Ok(_) => {
                                info!("response: sending graph:renamenode response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphRenamenodeResponse::default())
                                            .expect("failed to serialize graph:renamenode response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.rename_node() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphChangenodeRequest(payload) => {
                        info!("got graph:changenode message");
                        match graph.write().expect("lock poisoned").change_node(payload.graph, payload.name, payload.metadata) {
                            Ok(_) => {
                                info!("response: sending graph:changenode response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphChangenodeResponse::default())
                                            .expect("failed to serialize graph:changenode response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.change_node() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphAddedgeRequest(payload) => {
                        info!("got graph:addedge message");
                        //TODO optimize clone here
                        match graph.write().expect("lock poisoned").add_edge(payload.graph.clone(), GraphEdge::from(payload.clone())) {
                            Ok(_) => {
                                info!("response: sending graph:addedge response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphAddedgeResponse::from_request(payload))
                                            .expect("failed to serialize graph:addedge response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.add_edge() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphRemoveedgeRequest(payload) => {
                        info!("got graph:removeedge message");
                        match graph.write().expect("lock poisoned").remove_edge(payload.graph.clone(), payload.src.clone(), payload.tgt.clone()) {  //TODO optimize any way to avoid these clones?
                            Ok(_) => {
                                info!("response: sending graph:removeedge response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphRemoveedgeResponse::from_request(payload))
                                            .expect("failed to serialize graph:removeedge response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.remove_edge() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphChangeedgeRequest(payload) => {
                        info!("got graph:changeedge message");
                        match graph.write().expect("lock poisoned").change_edge(payload.graph.clone(), payload.src.clone(), payload.tgt.clone(), payload.metadata.clone()) {    //TODO optimize any way to avoid these clones here?
                            Ok(_) => {
                                info!("response: sending graph:changeedge response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphChangeedgeResponse::from_request(payload))  //TODO optimize clone
                                            .expect("failed to serialize graph:changeedge response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.change_edge() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphAddinitialRequest(payload) => {
                        info!("got graph:addinitial message");
                        match graph.write().expect("lock poisoned").add_initialip(payload) {
                            Ok(_) => {
                                info!("response: sending graph:addinitial response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphAddinitialResponse::default())
                                            .expect("failed to serialize graph:addinitial response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.add_initialip() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphRemoveinitialRequest(payload) => {
                        info!("got graph:removeinitial message");
                        match graph.write().expect("lock poisoned").remove_initialip(payload) {
                            Ok(_) => {
                                info!("response: sending graph:removeinitial response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphRemoveinitialResponse::default())
                                            .expect("failed to serialize graph:removeinitial response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.remove_initialip() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphAddinportRequest(payload) => {
                        info!("got graph:addinport message");
                        //TODO check if graph name matches
                        //TODO extend to multi-graph functionality, find the correct graph to address
                        match graph.write().expect("lock poisoned").add_inport(payload.public.clone(), GraphPort::from(payload)) {
                            Ok(_) => {
                                info!("response: sending graph:addinport response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphAddinportResponse::default())
                                            .expect("failed to serialize graph:addinport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.add_inport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                        }
                    }

                    FBPMessage::GraphRemoveinportRequest(payload) => {
                        info!("got graph:removeinport message");
                        //TODO check if graph name matches
                        //TODO multi-graph support
                        match graph.write().expect("lock poisoned").remove_inport(payload.public) {
                            Ok(_) => {
                                info!("response: sending graph:removeinport response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphRemoveinportResponse::default())
                                            .expect("failed to serialize graph:removeinport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.remove_inport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                        }
                    }

                    FBPMessage::GraphRenameinportRequest(payload) => {
                        info!("got graph:renameinport message");
                        //TODO check if graph name matches
                        //TODO multi-graph support
                        info!("response: sending graph:renameinport response");
                        match graph.write().expect("lock poisoned").rename_inport(payload.from, payload.to) {
                            Ok(_) => {
                                websocket
                                .write(Message::text(
                                    serde_json::to_string(&GraphRenameinportResponse::default())
                                        .expect("failed to serialize graph:renameinport response"),
                                ))
                                .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.rename_inport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                        }
                    }

                    FBPMessage::GraphAddoutportRequest(payload) => {
                        info!("got graph:addoutport message");
                        //TODO check if graph name matches
                        //TODO multi-graph support
                        match graph.write().expect("lock poisoned").add_outport(payload.public.clone(), GraphPort::from(payload)) {
                            Ok(_) => {
                                info!("response: sending graph:addoutport response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphAddoutportResponse::default())
                                            .expect("failed to serialize graph:addoutport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.add_outport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                        }
                    }

                    FBPMessage::GraphRemoveoutportRequest(payload) => {
                        info!("got graph:removeoutport message");
                        //TODO check if graph name matches
                        //TODO multi-graph support
                        match graph.write().expect("lock poisoned").remove_outport(payload.public) {
                            Ok(_) => {
                                info!("response: sending graph:removeoutport response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphRemoveoutportResponse::default())
                                            .expect("failed to serialize graph:removeoutport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.remove_outport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                        }
                    }

                    FBPMessage::GraphRenameoutportRequest(payload) => {
                        info!("got graph:renameoutport message");
                        //TODO check if graph name matches
                        //TODO multi-graph support
                        info!("response: sending graph:renameoutport response");
                        match graph.write().expect("lock poisoned").rename_outport(payload.from, payload.to) {
                            Ok(_) => {
                                websocket
                                .write(Message::text(
                                    serde_json::to_string(&GraphRenameoutportResponse::default())
                                        .expect("failed to serialize graph:renameoutport response"),
                                ))
                                .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.rename_outport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                        }
                    }

                    FBPMessage::GraphAddgroupRequest(payload) => {
                        info!("got graph:addgroup message");
                        match graph.write().expect("lock poisoned").add_group(payload.graph, payload.name, payload.nodes, payload.metadata) {
                            Ok(_) => {
                                info!("response: sending graph:addgroup response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphAddgroupResponse::default())
                                            .expect("failed to serialize graph:addgroup response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.add_group() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphRemovegroupRequest(payload) => {
                        info!("got graph:removegroup message");
                        match graph.write().expect("lock poisoned").remove_group(payload.graph, payload.name) {
                            Ok(_) => {
                                info!("response: sending graph:removegroup response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphRemovegroupResponse::default())
                                            .expect("failed to serialize graph:removegroup response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.remove_group() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphRenamegroupRequest(payload) => {
                        info!("got graph:renamegroup message");
                        match graph.write().expect("lock poisoned").rename_group(payload.graph, payload.from, payload.to) {
                            Ok(_) => {
                                info!("response: sending graph:renamegroup response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphRenamegroupResponse::default())
                                            .expect("failed to serialize graph:renamegroup response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.rename_group() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphChangegroupRequest(payload) => {
                        info!("got graph:changegroup message");
                        match graph.write().expect("lock poisoned").change_group(payload.graph, payload.name, payload.metadata) {
                            Ok(_) => {
                                info!("response: sending graph:changegroup response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphChangegroupResponse::default())
                                            .expect("failed to serialize graph:changegroup response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.change_group() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }


                    }

                    // protocol:trace
                    FBPMessage::TraceStartRequest(payload) => {
                        info!("got trace:start message");
                        //TODO not sure why Rust requires to use a write lock here
                        match runtime.write().expect("lock poisoned").start_trace(payload.graph.as_str(), payload.buffer_size) {
                            Ok(_) => {
                                info!("response: sending trace:start response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&TraceStartResponse::new(payload.graph))
                                            .expect("failed to serialize trace:start response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.start_trace() failed: {}", err);
                                info!("response: sending trace:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&TraceErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize trace:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                        }
                    }

                    FBPMessage::TraceStopRequest(payload) => {
                        info!("got trace:stop message");
                        //TODO why does Rust require a write lock here?
                        match runtime.write().expect("lock poisoned").stop_trace(payload.graph.as_str()) {
                            Ok(_) => {
                                info!("response: sending trace:stop response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&TraceStopResponse::new(payload.graph))
                                            .expect("failed to serialize trace:stop response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.stop_trace() failed: {}", err);
                                info!("response: sending trace:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&TraceErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize trace:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                },
                        }
                    }

                    FBPMessage::TraceClearRequest(payload) => {
                        info!("got trace:clear message");
                        //TODO why does Rust require acquiring a write lock here?
                        //TODO maybe check existence of the graph and if it is the current one out here?
                        match runtime.write().expect("lock poisoned").clear_trace(payload.graph.as_str()) {
                            Ok(_) => {
                                info!("response: sending trace:clear response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&TraceClearResponse::new(payload.graph))
                                            .expect("failed to serialize trace:clear response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.tracing_start() failed: {}", err);
                                info!("response: sending trace:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&TraceErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize trace:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                        }
                    }

                    FBPMessage::TraceDumpRequest(payload) => {
                        info!("got trace:dump message");
                        //TODO why does Rust require getting a write() on the lock?
                        match runtime.write().expect("lock poisoned").dump_trace(&payload.graph) {
                            Ok(dump) => {
                                info!("response: sending trace:dump response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&TraceDumpResponse::new(payload.graph, dump))
                                            .expect("failed to serialize trace:dump response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.dump_trace() failed: {}", err);
                                info!("response: sending trace:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&TraceErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize trace:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                        }
                    }

                    // protocol:runtime
                    FBPMessage::RuntimePacketRequest(payload) => {
                        info!("got runtime:packet message");
                        //TODO or maybe better send this to graph instead of runtime? in future for multi-graph support, yes.
                        match runtime.write().expect("lock poisoned").packet(&payload, &mut graph_inout.lock().expect("lock poisoned")) {
                            Ok(_) => {
                                info!("response: sending runtime:packetsent response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&RuntimePacketsentMessage::new(RuntimePacketsentPayload::from(payload)))
                                            .expect("failed to serialize runtime:packetsent response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.packet() failed: {}", err);
                                info!("response: sending runtime:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&RuntimeErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize runtime:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    // according to fbp-protocol, this is invalid to be sent from the client (there is no input/packetsent message defined) (TODO clarify with flowbased-devs)
                    //TODO maybe handle this a level higher in list of FBPMessage variants?
                    FBPMessage::RuntimePacketsentRequest(_payload) => {
                        info!("got runtime:packetsent message");
                        warn!("response: sending runtime:error response (error case, unexpected from FBP network protocol client)");
                        websocket
                            .write(Message::text(
                                serde_json::to_string(&RuntimeErrorResponse::new(String::from("runtime:packetsent from client is an error")))
                                    .expect("failed to serialize runtime:error response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    // network:data
                    FBPMessage::NetworkEdgesRequest(payload) => {
                        info!("got network:edges message");
                        match runtime.write().expect("lock poisoned").set_debug_edges(&payload.graph, &payload.edges) {
                            Ok(_) => {
                                info!("response: sending network:edges response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&NetworkEdgesResponse::from_request(payload))
                                            .expect("failed to serialize network:edges response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.set_debug_edges() failed: {}", err);
                                info!("response: sending network:error response");
                                websocket
                                    .write(Message::text(
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
                    FBPMessage::NetworkStartRequest(payload) => {
                        info!("got network:start message");
                        //TODO check secret
                        //match runtime.write().expect("lock poisoned").start(&graph.read().expect("lock poisoned"), &mut processes.write().expect("lock poisoned")) {
                        match runtime.write().expect("lock poisoned").start(&graph.read().expect("lock poisoned"), &components.read().expect("lock poisoned"), graph_inout.clone(), runtime.clone()) {
                            Ok(status) => {
                                info!("response: sending network:started response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&NetworkStartedResponse::new(&status))
                                            .expect("failed to serialize network:started response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                /*TODO implement network debugging, see https://github.com/ERnsTL/flowd/issues/193
                                websocket
                                    .write(Message::text(serde_json::to_string(&NetworkDataResponse::new(
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
                                error!("runtime.start() failed: {}", err);
                                info!("response: sending network:error response");
                                websocket
                                    .write(Message::text(
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

                    FBPMessage::NetworkStopRequest(payload) => {
                        info!("got network:stop message");
                        //TODO check secret
                        match runtime.write().expect("lock poisoned").stop(graph_inout.clone(), false) {   //TODO optimize avoid clone here? (but it is just an Arc clone)
                            Ok(status) => {
                                info!("response: sending network:stop response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&NetworkStoppedResponse::new(status))
                                            .expect("failed to serialize network:stopped response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                },
                            Err(err) => {
                                error!("runtime.stop() failed: {}", err);
                                info!("response: sending network:error response");
                                websocket
                                    .write(Message::text(
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

                    FBPMessage::NetworkDebugRequest(payload) => {
                        info!("got network:debug message");
                        match runtime.write().expect("lock poisoned").debug_mode(payload.graph.as_str(), payload.enable) {
                            Ok(_) => {
                                info!("response: sending network:debug response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&NetworkDebugResponse::new(payload.graph))
                                            .expect("failed to serialize network:debug response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("runtime.debug_mode() failed: {}", err);
                                info!("response: sending network:error response");
                                websocket
                                    .write(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(err.to_string(), String::from(""), payload.graph))
                                            .expect("failed to serialize network:debug response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                        }
                    }

                    //TODO group and order handler blocks by capability
                    _ => {
                        info!("unknown message type received: {:?}", fbpmsg); //TODO wanted Display trait here
                        websocket.close(None).expect("could not close websocket");
                    }
                }
            }
            Message::Ping(_) | Message::Pong(_) => {
                info!("got a ping|pong");
            }
            Message::Close(_) => {
                info!("got a close, breaking");
                break;
            }
            // From documentation: Raw frame. Note, that you are not going to get this value while reading the message.
            Message::Frame(_) => todo!()
        }

        // need to flush explicitly
        websocket.flush().expect("could not flush websocket");

        info!("--- end of message handling iteration")
    }
    {
        graph_inout.lock().expect("could not acquire lock for removing WebSocket from connections list").websockets.remove(&peer_addr);
    }
    //websocket.close().expect("could not close websocket");
    info!("---");
    Ok(())
}

//TODO currently panicks if unknown variant
//TODO currently panicks if field is missing during decoding
//TODO note messages which are used multiple times
//NOTE: deny unknown fields to learn them (serde) deny_unknown_fields, but problem is that "protocol" field is still present -> panic
#[derive(Deserialize, Debug)]
#[serde(tag = "command", content = "payload")] //TODO multiple tags: protocol and command
enum FBPMessage {
    // runtime base -- no capabilities required
    #[serde(rename = "getruntime")]
    RuntimeGetruntimeMessage(RuntimeGetruntimePayload), //NOTE: tag+content -> tuple variant not struct variant
    #[serde(rename = "runtime")]
    RuntimeRuntimeMessage,

    // protocol:runtime
    #[serde(rename = "ports")]
    RuntimePortsMessage,
    #[serde(rename = "packet")]
    RuntimePacketRequest(RuntimePacketRequestPayload),
    #[serde(rename = "packetsent")]
    RuntimePacketsentRequest(RuntimePacketsentPayload),

    // network:persist
    #[serde(rename = "persist")]
    NetworkPersistRequest(NetworkPersistRequestPayload),

    // network:status
    // used for several capabilities: protocol:network (deprecated), network:status, network:control
    #[serde(rename = "getstatus")]
    NetworkGetstatusMessage(NetworkGetstatusPayload),
    #[serde(rename = "status")]
    NetworkStatusMessage,

    // network:data
    #[serde(rename = "edges")]
    NetworkEdgesRequest(NetworkEdgesRequestPayload),

    // network:control (?)
    #[serde(rename = "start")]
    NetworkStartRequest(NetworkStartRequestPayload),
    #[serde(rename = "stop")]
    NetworkStopRequest(NetworkStopRequestPayload),
    #[serde(rename = "debug")]
    NetworkDebugRequest(NetworkDebugRequestPayload),

    // component:getsource
    #[serde(rename = "getsource")]
    ComponentGetsourceMessage(ComponentGetsourcePayload),

    // component:setsource
    #[serde(rename = "source")]
    ComponentSourceMessage,

    // protocol:component
    #[serde(rename = "list")]
    ComponentListRequest(ComponentListRequestPayload),
    //NOTE: used in several capabilities as response message
    #[serde(rename = "component")]
    ComponentComponentMessage,
    #[serde(rename = "componentsready")]
    ComponentComponentsreadyMessage,

    // graph:readonly
    // protocol:graph
    #[serde(rename = "clear")]
    GraphClearRequest(GraphClearRequestPayload),
    #[serde(rename = "addnode")]
    GraphAddnodeRequest(GraphAddnodeRequestPayload),
    #[serde(rename = "changenode")]
    GraphChangenodeRequest(GraphChangenodeRequestPayload),
    #[serde(rename = "renamenode")]
    GraphRenamenodeRequest(GraphRenamenodeRequestPayload),
    #[serde(rename = "removenode")]
    GraphRemovenodeRequest(GraphRemovenodeRequestPayload),
    #[serde(rename = "addedge")]
    GraphAddedgeRequest(GraphAddedgeRequestPayload),
    #[serde(rename = "removeedge")]
    GraphRemoveedgeRequest(GraphRemoveedgeRequestPayload),
    #[serde(rename = "changeedge")]
    GraphChangeedgeRequest(GraphChangeedgeRequestPayload),
    #[serde(rename = "addinitial")]
    GraphAddinitialRequest(GraphAddinitialRequestPayload),
    #[serde(rename = "removeinitial")]
    GraphRemoveinitialRequest(GraphRemoveinitialRequestPayload),
    #[serde(rename = "addinport")]
    GraphAddinportRequest(GraphAddinportRequestPayload),
    #[serde(rename = "removeinport")]
    GraphRemoveinportRequest(GraphRemoveinportRequestPayload),
    #[serde(rename = "renameinport")]
    GraphRenameinportRequest(GraphRenameinportRequestPayload),
    #[serde(rename = "addoutport")]
    GraphAddoutportRequest(GraphAddoutportRequestPayload),
    #[serde(rename = "removeoutport")]
    GraphRemoveoutportRequest(GraphRemoveoutportRequestPayload),
    #[serde(rename = "renameoutport")]
    GraphRenameoutportRequest(GraphRenameoutportRequestPayload),
    #[serde(rename = "addgroup")]
    GraphAddgroupRequest(GraphAddgroupRequestPayload),
    #[serde(rename = "removegroup")]
    GraphRemovegroupRequest(GraphRemovegroupRequestPayload),
    #[serde(rename = "renamegroup")]
    GraphRenamegroupRequest(GraphRenamegroupRequestPayload),
    #[serde(rename = "changegroup")]
    GraphChangegroupRequest(GraphChangegroupRequestPayload),

    // protocol:trace
    #[serde(rename = "start")]
    TraceStartRequest(TraceStartRequestPayload),
    #[serde(rename = "stop")]
    TraceStopRequest(TraceStopRequestPayload),
    #[serde(rename = "clear")]
    TraceClearRequest(TraceClearRequestPayload),
    #[serde(rename = "dump")]
    TraceDumpRequest(TraceDumpRequestPayload),
}

// ----------
// runtime base -- no capabilities required
// ----------

// runtime:getruntime -> runtime:runtime | runtime:error
#[derive(Deserialize, Debug)]
struct RuntimeGetruntimePayload {
    secret: String,
}

#[derive(Serialize, Debug)]
struct RuntimeRuntimeMessage<'a> {
    protocol: String, // group of messages (and capabities)
    command: String,  // name of message within group
    payload: &'a RuntimeRuntimePayload,
}

impl Default for RuntimeRuntimeMessage<'_> {
    fn default() -> Self {
        RuntimeRuntimeMessage {
            protocol: String::from("runtime"),
            command: String::from("runtime"),
            //TODO fix - currently using recursive Default::default() because the following does not work:
            //payload: &RuntimeRuntimePayload::default(),
            ..Default::default()
        }
    }
}

impl<'a> RuntimeRuntimeMessage<'a> {
    fn new(payload: &'a RuntimeRuntimePayload) -> Self {
        RuntimeRuntimeMessage {
            protocol: String::from("runtime"),
            command: String::from("runtime"),
            payload: &payload,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeRuntimePayload {
    id: String,                        // spec: UUID of this runtime instance
    label: String,                     // spec: human-readable description of the runtime
    version: String,                   // spec: supported protocol version //TODO which versions are there? implement proper
    all_capabilities: Vec<Capability>, // spec: capabilities supported by runtime
    capabilities: Vec<Capability>, // spec: capabities for you //TODO implement privilege level restrictions
    graph: String,                 // spec: currently active graph
    #[serde(rename = "type")]
    runtime: String,    // spec: name of the runtime software, "flowd"
    namespace: String,             // spec: namespace of components for this project of top-level graph
    repository: String,            // spec: source code repository of this runtime software //TODO but it is the repo of the graph, is it?
    repository_version: String,    // spec: repository version of this software build

    // runtime state
    #[serde(skip)]
    status: NetworkStartedResponsePayload,  // for network:status, network:started, network:stopped
    //TODO ^ also contains graph = active graph, maybe replace status.graph with a pointer so that not 2 updates are neccessary?
    #[serde(skip)]
    tracing: bool,  //TODO implement
    #[serde(skip)]
    processes: ProcessManager,    // currently it is possible (with some caveats, see struct Process) to have the ProcessManager inside this struct here which is also used for Serialize and Deserialize, but in the future the may easily be some more fields in Process neccessary, which cannot be shared between threads, which cannot be cloned, which are not Sync or Send etc. -> then have to move it out into a separate processes variable and hand it over to handle_client() (already prepared) or maybe into a separate thread which owns non-shareable data structures
    #[serde(skip)]
    watchdog_thread: Option<std::thread::JoinHandle<()>>,
    #[serde(skip)]
    watchdog_channel: Option<std::sync::mpsc::SyncSender<MessageBuf>>,
}

impl Default for RuntimeRuntimePayload {
    fn default() -> Self {
        RuntimeRuntimePayload {
            id: String::from("f18a4924-9d4f-414d-a37c-deadbeef0000"), //TODO actually random UUID
            label: String::from("human-readable description of the runtime"), //TODO useful text
            version: String::from("0.7"),                             //TODO actually implement that - what about features+changes post-0.7?
            all_capabilities: vec![
                Capability::ProtocolNetwork,
                Capability::NetworkPersist,
                Capability::NetworkStatus,
                Capability::NetworkData,
                Capability::NetworkControl,
                Capability::ProtocolComponent,
                Capability::ComponentGetsource,
                Capability::ComponentSetsource,
                Capability::ProtocolRuntime,
                Capability::ProtocolGraph,
                Capability::ProtocolTrace,
            ],
            capabilities: vec![
                Capability::ProtocolNetwork,
                Capability::NetworkPersist,
                Capability::NetworkStatus,
                Capability::NetworkData,
                Capability::NetworkControl,
                Capability::ProtocolComponent,
                Capability::ComponentGetsource,
                Capability::ComponentSetsource,
                Capability::ProtocolRuntime,
                Capability::ProtocolGraph,
                Capability::ProtocolTrace,
            ],
            graph: String::from("default_graph"), // currently active graph
            runtime: String::from("flowd"), //TODO constant - optimize
            namespace: String::from("main"), // namespace of components TODO implement
            repository: String::from("https://github.com/ERnsTL/flowd.git"),  //TODO use this feature of building and saving the graph into a Git repo
            repository_version: String::from("0.0.1-ffffffff"), //TODO use actual git commit and actual version
            // runtime values
            status: NetworkStartedResponsePayload::default(),
            tracing: false,
            processes: ProcessManager::default(),
            watchdog_thread: None,
            watchdog_channel: None,
        }
    }
}

impl RuntimeRuntimePayload {
    fn new(active_graph: String) -> Self {
        RuntimeRuntimePayload{
            graph: active_graph.clone(),    //TODO any way to avoid the clone and point to the other one?
            status: NetworkStartedResponsePayload {
                time_started: UtcTime(chrono::DateTime::<Utc>::MIN_UTC), // zero value - //TODO rather use None
                graph: active_graph,
                started: false,
                running: false,
                debug: false,
            },
            ..Default::default()  //TODO mock other fields as well
        }
    }

    fn persist(&self) -> std::result::Result<(), std::io::Error> {
        //TODO implement
        Ok(())
    }

    //fn start(&mut self, graph: &Graph, process_manager: &mut ProcessManager) -> std::result::Result<&NetworkStartedResponsePayload, std::io::Error> {
    fn start(&mut self, graph: &Graph, components: &ComponentLibrary, graph_inout_arc: Arc<Mutex<GraphInportOutportHolder>>, runtime: Arc<RwLock<RuntimeRuntimePayload>>) -> std::result::Result<&NetworkStartedResponsePayload, std::io::Error> {
        info!("starting network for graph {}", graph.properties.name);
        // get all graph in and out ports
        let mut graph_inout = graph_inout_arc.lock().expect("could not acquire lock for network start()");
        //TODO implement
        //TODO implement: what to do with the old running processes, stop using signal channel? What if they dont respond?
        //TODO implement: what if the name of the node changes? then the process is not found by that name anymore in the process manager

        //TODO check if self.processes.len() == 0 AKA stopped

        //TODO optimize thread sync and packet transfer
        // use [`channel`]s, [`Condvar`]s, [`Mutex`]es or [`join`]
        // -> https://doc.rust-lang.org/std/sync/index.html
        // or even basic:  https://doc.rust-lang.org/std/thread/fn.park.html
        //    -> would have to hand over Arc<ProcessManager> to each process, then it gets the thread handle out of this.
        //    Problem:  Does not know which process is on the other end, so would also have to hand over the process name on the other end. ugly.
        // -> better to hand over something that can be pre-generated and cloned = no difference between "sender" and "receiver"
        // -> https://doc.rust-lang.org/std/sync/struct.Condvar.html
        // foreach node in graph.nodes: pre-generate condvar + mutex here so that the next foreach over the edges can put it into a tuple on each ProcessEdgeSink
        // then the sending process can wake up the next process in the graph like a flush() on the according outport
        // putting in a JoinHandle or Thread is not possible because Rust does not allow creating a "prepared empty" Thread and pre-generating these
        // more ideas:
        //   https://stackoverflow.com/questions/37964467/how-to-freeze-a-thread-and-notify-it-from-another
        //   https://doc.rust-lang.org/std/sync/struct.Condvar.html#method.wait_timeout
        //   https://github.com/kirillkh/monitor_rs
        // what is the difference in cost between mutex+condvar, SyncChannel<()> and thread.park?
        // We need a sync mechanism that is SPSC, where the writer blocks if full, the reader blocks if empty and CPU-free waiting
        // General question: Should the Component be dumb and its process() be called to work off any packets (easy case in terms of thread sync)
        //   or should the Component be in control? Yes, because it might have external dependencies that have different timing than the FBP network (connections to external services etc.) so it must be able to manage itself
        //   -> we need a way for the Component to receive a wakeup call, but that does nothing if it is already actively processing messages = no cluttering with () wakeups, so it must be a capacity 1 channel that does not block the sender if full. only full bounded connection buffer should block. Hm, but sending could already block before it has any chance to wake up the next process.
        //   -> so the sending mechanism and the wakeup mechanism must be the same thing (or it can be 2 separate things inside, but it must be 1 call from the sender's perspective)
        // sending first packet of 10 should already wake the receiver.
        // but what if the sender is slower at producing than the receiver at consuming than the receiver?
        // -> batching would make sense. but only the producer knows how much how big its batch is and how many it will produce in the next time units.
        // but batching increases latency 

        // How thread sync is currently done:
        // 1. during edge generation, for each outport, save the process name in a separate data structure
        // 2. start the threads, hand them over the thread handles hashmap, but first thing they do is park
        // 3. network start (= this fn) starts all threads for the FBP processes and stores their threads handles in the thread handles hashmap where key = FBP process name
        //    -> now all thread handles are complete in the hashmap
        // 4. then unpark/wake all threads,
        // 5. each thread replaces the FBP process name on its outports with the thread handle, then drops the Arc ref to the thread handles hashmap
        // 6. each thread instantiates its component and calls run()
        // 7. each component gets the thread handle and stores it for wake-up after it sent a packet on the outport
        //    -> each component can decide when it will wake the next FBP process (after first packet, after a few have been generated or when it is done for the iteration or when the channel is full etc.)

        //TODO optimize: check performance, maybe this could be done easier using the signal channel sending (), but since the thread_handle already has blocking feature built-in...

        // prepare graph_inout reference for watchdog stopping the network
        let watchdog_graph_inout = graph_inout_arc.clone();
        // prepare runtime reference for watchdong stopping the network
        let watchdog_runtime = runtime;
        // prepare holder of the signal back-channel for the watchdog thread
        let mut watchdog_threadandsignal: HashMap<String, (std::sync::mpsc::SyncSender<MessageBuf>, Thread)> = HashMap::new();
        let (watchdog_signalsink, watchdog_signalsource) = std::sync::mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);

        // arrayports: check if all edges having the same source process have a source port that is marked as an arry port
        let mut known_source_processes = vec![];
        for edge in graph.edges.iter() {
            if known_source_processes.contains(&edge.source.process) {
                // ignore IIP edges
                if edge.data.is_some() && edge.source.process.is_empty() {
                    // that is an IIP edge, ignore
                    continue;
                }
                // check if source port is array port
                let component_name = &graph.nodes.get(&edge.source.process).expect("source process not found").component;
                for component in components.available.iter() {
                    if component.name == *component_name {
                        for port in component.out_ports.iter() {
                            if port.name == edge.source.port {
                                if !port.is_arrayport {
                                    // error
                                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, String::from("multiple edges from source process port which is not an array port")));  //TODO say which one
                                } else {
                                    debug!("found correct in-use array port {}.{}", edge.source.process, edge.source.port);
                                }
                            }
                        }
                    }
                }
            } else {
                // add to list
                known_source_processes.push(edge.source.process.clone());
            }
        }
        drop(known_source_processes);
        // arrayports: check if all edges having the same target process have a target port that is marked as an arry port
        let mut known_target_processes: MultiMap<String, String> = MultiMap::new();
        for edge in graph.edges.iter() {
            if let Some(ports) = known_target_processes.get_vec_mut(&edge.target.process) {
                // check for multiple connections to the same target port
                if ports.contains(&edge.target.port) {
                    // check if target port is array port, since we now have multiple connections into that target port
                    let component_name = &graph.nodes.get(&edge.target.process).expect("target process not found").component;
                    for component in components.available.iter() {
                        if component.name == *component_name {
                            for port in component.in_ports.iter() {
                                if port.name == edge.target.port {
                                    if !port.is_arrayport {
                                        // error
                                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, String::from("multiple edges to target process port which is not an array port")));    //TODO say which ones
                                    } else {
                                        debug!("found correct in-use array inport {}.{}", edge.target.process, edge.target.port);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // add to list (under that process name), first use of that port
                    ports.push(edge.target.port.clone());
                }
            } else {
                // add to list, first use of that process
                known_target_processes.insert(edge.target.process.clone(), edge.target.port.clone());
            }
        }
        drop(known_target_processes);

        // generate all connections
        struct ProcPorts {
            inports: ProcessInports,    // including ports with IIPs
            outports: ProcessOutports,
            wake_notify: WakeupNotify,    // for notification of this process
        }
        impl Default for ProcPorts {
            fn default() -> Self {
                ProcPorts {
                    inports: ProcessInports::new(),
                    outports: ProcessOutports::new(),
                    wake_notify: std::thread::current(),    //TODO not sure if that makes sense
                }
            }
        }
        impl std::fmt::Debug for ProcPorts {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
               f.debug_struct("ProcPorts").field("inports", &self.inports).field("outports", &self.outports).finish()
            }
        }
        let mut ports_all: HashMap<String, ProcPorts> = HashMap::with_capacity(graph.nodes.len());
        // set up keys
        for proc_name in graph.nodes.keys().into_iter() {
            //TODO would be nice to know the name of the process
            ports_all.try_insert(proc_name.clone(), ProcPorts::default()).expect("preparing edges for process failed: process name already exists");
        }
        //TODO using graph name as fake process, but does that imply we cannot change the graph name during runtime?
        if graph.inports.len() > 0 {
            ports_all.try_insert(format!("{}-IN", graph.properties.name), ProcPorts::default()).expect("preparing inport edges for graph failed: process name already exists");
        }
        if graph.outports.len() > 0 {
            ports_all.try_insert(format!("{}-OUT", graph.properties.name), ProcPorts::default()).expect("preparing outport edges for graph failed: process name already exists");
        }
        // fill keys with connections
        for edge in graph.edges.iter() {
            if let Some(iip) = &edge.data {
                // prepare IIP edge
                debug!("preparing edge from IIP to {}.{}", edge.target.process, edge.target.port);
                //TODO sink will not be hooked up to anything when leaving this for loop; is that good?
                let (mut sink, source) = ProcessEdge::new(PROCESSEDGE_IIP_BUFSIZE);
                // send IIP
                sink.push(iip.clone().into_bytes()).expect("failed to send IIP into process channel");    //TODO optimize as_bytes() / clone or String in Edge struct
                // insert into inports of target process
                let targetproc = ports_all.get_mut(&edge.target.process).expect("process IIP target assignment process not found");
                // arrayports: insert, but remember that this is a multimap
                targetproc.inports.insert(edge.target.port.clone(), source);
                // assign into outports of source process
                // nothing to do in case of IIP - this also means that sink will go ouf ot scope and that source.is_abandoned() = Arc::strong_count() will be 1
                // in summary: IIP ports are closed/abandoned
            } else {
                // prepare edge
                debug!("preparing edge from {}.{} to {}.{}", edge.source.process, edge.source.port, edge.target.process, edge.target.port);
                let (sink, source) = ProcessEdge::new(PROCESSEDGE_BUFSIZE);

                // insert into inports of target process
                let targetproc = ports_all.get_mut(&edge.target.process).expect("process IIP target assignment process not found");
                let targetproc_wake_notify = targetproc.wake_notify.clone();
                // arrayports: insert, but remember that this is a multimap
                targetproc.inports.insert(edge.target.port.clone(), source);
                // assign into outports of source process
                let sourceproc = ports_all.get_mut(&edge.source.process).expect("process source assignment process not found");
                // arrayports: insert, but remember that this is a multimap
                sourceproc.outports.insert(edge.source.port.clone(), ProcessEdgeSink { sink: sink, wakeup: Some(targetproc_wake_notify), proc_name: Some(edge.target.process.clone()) } );
            }
        }
        for (public_name, edge) in graph.inports.iter() {
            // prepare edge
            debug!("preparing edge from graph {} to {}.{}", public_name, edge.process, edge.port);
            let (sink, source) = ProcessEdge::new(PROCESSEDGE_BUFSIZE);

            // insert into inports of target process
            let targetproc = ports_all.get_mut(&edge.process).expect("graph target assignment process not found");
            let targetproc_wake_notify = targetproc.wake_notify.clone();
            // arrayports: insert, but remember that this is a multimap
            targetproc.inports.insert(edge.port.clone(), source);
            // assign into outports of source process
            // source process name = graphname-IN
            let sourceproc: &mut ProcPorts = ports_all.get_mut(format!("{}-IN", graph.properties.name).as_str()).expect("graph source assignment process not found");
            // arrayports: insert, but remember that this is a multimap
            sourceproc.outports.insert(public_name.clone(), ProcessEdgeSink { sink: sink, wakeup: Some(targetproc_wake_notify), proc_name: Some(edge.process.clone()) } );
        }
        for (public_name, edge) in graph.outports.iter() {
            // prepare edge
            debug!("preparing edge from {}.{} to graph {}", edge.process, edge.port, public_name);
            let (sink, source) = ProcessEdge::new(PROCESSEDGE_BUFSIZE);

            // insert into inports of target process
            // target process name = graphname-OUT
            let targetproc = ports_all.get_mut(format!("{}-OUT", graph.properties.name).as_str()).expect("graph target assignment process not found");
            let targetproc_wake_notify = targetproc.wake_notify.clone();
            // arrayports: insert, but remember that this is a multimap
            targetproc.inports.insert(public_name.clone(), source);
            // assign into outports of source process
            let sourceproc = ports_all.get_mut(&edge.process).expect("graph source assignment process not found");
            // arrayports: insert, but remember that this is a multimap
            sourceproc.outports.insert(edge.port.clone(), ProcessEdgeSink { sink: sink, wakeup: Some(targetproc_wake_notify), proc_name: Some(format!("{}-OUT", graph.properties.name)) } );
        }

        // generate processes and assign prepared connections
        let thread_wake_handles: Arc<Mutex<HashMap<String, WakeupNotify>>> = Arc::new(Mutex::new(HashMap::new())); // these already exist in ports_all but they get consumed and removed so for the -IN and -OUT they wont exist anymore
        let mut found: bool;
        let mut found2: bool;
        for (proc_name, node) in graph.nodes.iter() {
            debug!("setting up process name={} component={}", proc_name, node.component);
            //TODO is there anything in .metadata that affects process setup?

            // get prepared ports for this process
            let ports_this: ProcPorts = ports_all.remove(proc_name).expect("prepared connections for a node not found, source+target nodes in edges != nodes");
            //TODO would be great to have the port name here for diagnostics
            let inports: ProcessInports = ports_this.inports;
            //TODO would be great to have the port name here for diagnostics
            let mut outports: ProcessOutports = ports_this.outports;

            // check if all ports exist
            found = false;
            for component in &components.available {
                if component.name == node.component {
                    // check inports
                    /*
                    if inports.len() != component.in_ports.len() {
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, String::from(format!("unconnected port checking: inport count on component metadata != used in graph for process={} component={}", proc_name, component.name))));
                    }
                    */
                    //TODO this provides the exact port that is missing and allows for checking of required ports
                    for inport in &component.in_ports {
                        if !inports.contains_key(&inport.name) {
                            //TODO check if port is required, maybe add strict checking true/false as parameter

                            // check if connected to a graph inport
                            found2 = false;
                            for (_graph_inport_name, graph_inport) in graph.inports.iter() {
                                if graph_inport.process.as_str() == proc_name.as_str() && graph_inport.port == inport.name {
                                    // is connected to graph inport
                                    found2 = true;
                                    break; //TODO optimize condition flow, is mix of break+continue
                                }
                            }
                            if found2 { continue; } //TODO optimize condition flow, is mix of break+continue

                            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from(format!("unconnected port checking: process {} is missing required port {} for component {}", proc_name, &inport.name, component.name))));
                        }
                    }

                    // check outports
                    /*
                    if outports.len() != component.out_ports.len() {
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, String::from(format!("unconnected port checking: outport count on component metadata != used in graph for process={} component={}", proc_name, component.name))));
                    }
                    */
                    //TODO this provides the exact port that is missing and allows for checking of required ports
                    for outport in &component.out_ports {
                        if !outports.contains_key(&outport.name) {
                            //TODO check if port is required, maybe add strict checking true/false as parameter

                            // check if connected to a graph outport
                            found2 = false;
                            for (_graph_outport_name, graph_outport) in graph.outports.iter() {
                                if graph_outport.process.as_str() == proc_name.as_str() && graph_outport.port == outport.name {
                                    // is connected to graph outport
                                    found2 = true;
                                    break; //TODO optimize condition flow, is mix of break+continue
                                }
                            }
                            if found2 { continue; } //TODO optimize condition flow, is mix of break+continue

                            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from(format!("unconnected port checking: process {} is missing required outport {} on component {}", proc_name, &outport.name, component.name))));
                        }
                    }

                    // look no further
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("unconnected port checking could not find component in component library")));
            }

            // prepare process signal channel
            let (signalsink, signalsource) = std::sync::mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);

            // process itself in thread
            let component_name = node.component.clone();
            let joinhandlesref = thread_wake_handles.clone();
            //let ports_this_wake_notify = ports_this.wake_notify.clone();
            let watchdog_signalsink_clone = watchdog_signalsink.clone();
            let graph_inout_ref = graph_inout_arc.clone();
            let joinhandle = thread::Builder::new().name(proc_name.clone()).spawn(move || {
                debug!("this is process thread, waiting for Thread replacement");
                thread::park();
                debug!("replacing Thread objects");

                // replace all process names with Thread handles
                // assumption that process names are unique but that is guaranteed by the HashMap key uniqueness
                // arrayports: do a flat iteration over all outports because of multimap
                for outport in outports.flat_iter_mut() {
                    let proc_name = outport.1.proc_name.as_ref().expect("wtf no proc_name is None during outport Thread handle replacement");
                    let joinhandles_locked = joinhandlesref.lock().expect("failed to get lock for Thread handle replacement");
                    let thr = joinhandles_locked.get(proc_name).expect("wtf sink process not found during outport Thread handle replacement");
                    // now the actual replacement - replace wakeup from None with the handle and the process name with None
                    outport.1.wakeup = Some(thr.clone());   // before this was None, now replaced with Some(Thread)
                    //NOTE: technically, the process name is not needed any more, but we leave it in for debugging
                    //outport.1.proc_name = None; // before this was Some(String), now we could replace it with None
                }
                drop(joinhandlesref);   // not needed anymore, we got the handles

                // component
                info!("starting");
                //TODO make it generic instead of if
                //let component: Component where Component: Sized;
                match component_name.as_str() {
                    // core components
                    "Repeat" => { RepeatComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "Drop" => { DropComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "Output" => { OutputComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "LibComponent" => { LibComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "UnixSocketServer" => { UnixSocketServerComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "FileReader" => { FileReaderComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "Trim" => { TrimComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "SplitLines" => { SplitLinesComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "Count" => { CountComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "Cron" => { CronComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "Cmd" => { CmdComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "Hasher" => { HasherComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "Equals" => { EqualsComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "HTTPClient" => { HTTPClientComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "HTTPServer" => { HTTPServerComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "Muxer" => { MuxerComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "MQTTPublisher" => { MQTTPublisherComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "MQTTSubscriber" => { MQTTSubscriberComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "RedisPublisher" => { RedisPublisherComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "RedisSubscriber" => { RedisSubscriberComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "IMAPAppend" => { IMAPAppendComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "IMAPFetchIdle" => { IMAPFetchIdleComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "OpenAIChat" => { OpenAIChatComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "TeraTemplate" => { TeraTemplateComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "FileTailer" => { FileTailerComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "RegexpExtract" => { RegexpExtractComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "FileWriter" => { FileWriterComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "TextReplace" => { TextReplaceComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    "ZeroconfResponder" => { ZeroconfResponderComponent::new(inports, outports, signalsource, watchdog_signalsink_clone, graph_inout_ref).run(); },
                    _ => {
                        error!("unknown component in network start! exiting thread.");
                    }
                }
            }).expect("thread start failed");

            // store thread handle for wakeup in components
            thread_wake_handles.lock().expect("failed to get lock posting thread handle").insert(proc_name.clone(), joinhandle.thread().clone());
            //thread_wake_handles.insert(proc_name.clone(), ports_this.wake_notify);
            // store process signal channel and thread handle for watchdog thread
            watchdog_threadandsignal.insert(proc_name.clone(), (signalsink.clone(), joinhandle.thread().clone()));
            // store process signal channel and join handle
            self.processes.insert(proc_name.clone(), Process {
                signal: signalsink,
                joinhandle: joinhandle,
            });
        }
        // work off graphname-IN and graphname-OUT special processes for graph inports and graph outports
        //TODO the signal channel and joinhandle of the graph outport process/thread could also simply be stored in the processes variable with all other FBP processes
        graph_inout.inports = None;
        graph_inout.outports = None;
        if ports_all.len() > 0 {
            // insert graph inport handler
            if ports_all.contains_key(format!("{}-IN", graph.properties.name).as_str()) {
                // target datastructure
                let mut outports: HashMap<String, ProcessEdgeSink> = HashMap::new();
                // get ports for this special component
                let ports_this: ProcPorts = ports_all.remove(format!("{}-IN", graph.properties.name).as_str()).expect("prepared connections for graph inports not found");
                // add wakeup handles and sinks of all target processes (translate target proc_name into join_handle)
                for (port_name, mut edge) in ports_this.outports {
                    // arrayports: check if there are multiple edges going out of the graph inport, which is not allowed
                    if edge.len() != 1 {
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, String::from("multiple edges from graph inport")));
                    }
                    // get targetproc
                    let targetproc_wake_notify = thread_wake_handles.lock().expect("could not lock thread wake handles for getting Thread wake handle").get(edge[0].proc_name.as_ref().expect("target process for graph inport missing name").as_str()).expect("target process for graph inport not found").clone();
                    // insert that port
                    let edge0 = edge.pop().unwrap();
                    outports.insert(port_name, ProcessEdgeSink { sink: edge0.sink, wakeup: Some(targetproc_wake_notify), proc_name: edge0.proc_name });
                }
                // save the inports (where we put packets into) as the graph inport channel handles; they are "outport handles" because they are being written into (packet sink)
                graph_inout.inports = Some(outports);
            }
            if ports_all.contains_key(format!("{}-OUT", graph.properties.name).as_str()) {
                // get ports for this special component, of interest here are the inports (source channels)
                let ports_this: ProcPorts = ports_all.remove(format!("{}-OUT", graph.properties.name).as_str()).expect("prepared connections for graph outports not found");
                let mut inports = ports_this.inports;
                // prepare process signal channel
                let (signalsink, signalsource): (ProcessSignalSink, ProcessSignalSource) = std::sync::mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);
                // start thread, will move signalsource, inports
                let graph_name = graph.properties.name.clone(); //TODO cannot change graph name during runtime because of this
                //TODO optimize; WebSocket is not Copy, but a WebSocket can be re-created from the inner TcpStream, which has a try_clone()
                let graph_inoutref = graph_inout_arc.clone();
                //let ports_this_wake_notify = ports_this.wake_notify.clone();
                let joinhandle = thread::Builder::new().name(format!("{}-OUT", graph.properties.name)).spawn(move || {
                    let signals = signalsource;
                    // arrayports: Note that this does currently not handle arrayports, but since this is forbidden via the graph definition, we can ignore that for now //TODO add check
                    if inports.len() == 0 {
                        error!("no graph inports found, exiting");
                        return;
                    }
                    //let mut websocket = tungstenite::WebSocket::from_raw_socket(websocket_stream, tungstenite::protocol::Role::Server, None);
                    debug!("GraphOutports is now run()ning!");

                    // inform FBP Network Protocol clients that graphout ports are now connected (runtime:packet event type = connect)
                    //NOTE: inports is from the perspective of the GraphOut handler thread, so these are the graph outports
                    for port_name in inports.keys() {
                        graph_inoutref.lock().expect("lock poisoned").send_runtime_packet(&RuntimePacketResponse::new_connect(graph_name.clone(), port_name.clone(), None, None));  //TODO can we save cloning graph_name several times?
                    }

                    // read IPs
                    loop {
                        trace!("begin of iteration");
                        // check signals
                        //TODO optimize, there is also try_recv() and recv_timeout()
                        if let Ok(ip) = signals.try_recv() {
                            //TODO optimize string conversions
                            trace!("received signal ip: {}", str::from_utf8(&ip).expect("invalid utf-8"));    //TODO optimize conversion
                            // stop signal
                            if ip == b"stop" {    //TODO optimize conversion
                                info!("got stop signal, exiting");
                                break;
                            }
                        }
                        // receive on all inports
                        for (port_name, inport) in inports.iter_mut() {
                            //TODO while !inport.is_empty() {
                            loop {
                                if let Ok(ip) = inport.pop() {
                                    // output the packet data with newline
                                    debug!("got a packet for graph outport {}", port_name);
                                    trace!("{}", str::from_utf8(&ip).expect("non utf-8 data")); //TODO optimize avoid conversion here or use from_raw_parts?

                                    // send out to FBP network protocol client
                                    debug!("sending out to client...");
                                    //TODO optimize lock only once for all packets available in inport buffer
                                    graph_inoutref.lock().expect("lock poisoned").send_runtime_packet(&RuntimePacketResponse::new(RuntimePacketResponsePayload {
                                        port: port_name.clone(),    //TODO optimize
                                        event: RuntimePacketEvent::Data,
                                        typ: None,   //TODO implement properly, OTOH it is an optional field
                                        schema: None,
                                        graph: graph_name.clone(),
                                        payload: Some(str::from_utf8(&ip).expect("non utf-8 data").to_owned()),   //TODO optimize useless conversions here - we could make RuntimePacketResponse separate from RuntimePacketRequest and make payload on the Response &str
                                                // TODO optimize conversion; just handing over Some(String::from_utf8(ip) = move causes "this reinitialization might get skipped" -> https://github.com/rust-lang/rust/issues/92858
                                    }));
                                    debug!("done");
                                } else {
                                    //TODO optimize unlock graph_inout here
                                    break;
                                }
                            }
                        }
                        trace!("-- end of iteration");
                        std::thread::park();
                        //condvar_block!(&*ports_this_wake_notify);
                    }
                    // inform FBP Network Protocol clients that graphout ports are now disconnected (runtime:packet event type = disconnect)
                    //NOTE: inports is from the perspective of the GraphOut handler thread, so these are the graph outports
                    info!("notifying clients of graph outports disconnect");
                    for port_name in inports.keys() {
                        graph_inoutref.lock().expect("lock poisoned").send_runtime_packet(&RuntimePacketResponse::new_disconnect(graph_name.clone(), port_name.clone(), None, None));  //TODO can we save cloning graph_name several times?
                    }

                    info!("exiting");
                }).expect("thread start failed");

                // store thread handle for wakeup in components
                thread_wake_handles.lock().expect("failed to get lock posting graph outport thread handle").insert(format!("{}-OUT", graph.properties.name), joinhandle.thread().clone());
                //thread_wake_handles.insert(format!("{}-OUT", graph.properties.name), ports_this.wake_notify);
                // store process signal channel and join handle so that the other processes writing into this graph outport component can find it
                self.processes.insert(format!("{}-OUT", graph.properties.name), Process {
                    signal: signalsink,
                    joinhandle: joinhandle,
                });

                // save single joinhandle and signal for that component
                //TODO optimize, cannot clone joinhandle
                //TODO currentcy graph_inout.outports is unused
                /*
                graph_inout.outports = Some(Process {
                    signal: signalsink,
                    joinhandle: joinhandle,
                })
                */
            }
            //TODO put graph_inout into runtime struct?  self.graph_inout = graph_inout;
        }

        // sanity check
        if ports_all.len() != 0 {
            // reset to known state
            self.processes.clear();
            // report error
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, String::from("there are ports for processes left over, source+target nodes in edges != nodes")));
        }

        // unpark all processes since all joinhandles are now known and so that they can replace the process names with the join handles and instantiate their components
        for proc in self.processes.iter() {
            proc.1.joinhandle.thread().unpark();
        }

        // start background thread for regular process health check
        // not possible to kill a thread:  https://stackoverflow.com/questions/26199926/how-to-terminate-or-suspend-a-rust-thread-from-another-thread
        // TODO optimize use condvar or mpsc channel?
        //TODO add ability to change interval after network start
        //TODO allow reconfiguration of network, currently this is basically a subset copy of self.processes (signal channel sink and thread handle)
        //TODO use the Component.support_health bool there!
        // sink2 and source2 are separate for signaling between runtime and watchdog thread only so that there can be no mixup between runtime<->watchdog and watchdog<->processes communication
        let (watchdog_signalsink2, watchdog_signalsource2) = std::sync::mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);
        let watchdog_thread = thread::Builder::new().name("watchdog".to_owned()).spawn( move || {
            debug!("watchdog is running");
            loop {
                //TODO optimize, there is also try_recv() and recv_timeout()
                if let Ok(ip) = watchdog_signalsource2.try_recv() {
                    if ip == b"stop" {
                        debug!("got stop signal, exiting");
                        break;
                    }
                }

                // health check of all components
                trace!("running health check...");
                let mut now: chrono::DateTime<Utc>;   //TODO any way to not initialize this with a throwaway value?
                let mut ok = true;
                let mut exited_count: usize = 0;
                for (name, proc) in watchdog_threadandsignal.iter() {
                    trace!("process {}...", name);
                    // send query to process
                    match proc.0.try_send(b"ping".to_vec()) {
                        Ok(_) => {},
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            warn!("process {} signal channel full", name);
                            ok = false;
                            continue;
                        },
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            warn!("process {} signal channel disconnected", name);
                            exited_count += 1;
                            ok = false;
                            continue;
                        },
                    }

                    // check if process is still running
                    //TODO currently we have only Thread, not a JoinHandle here -> is it possible to get a JoinHandle here, then it would be directly possible to check.

                    // wake process
                    proc.1.unpark();
                    now = chrono::Utc::now();   //TODO is it really useful to measure 0.000005ms response time?

                    // read response
                    match watchdog_signalsource.recv_timeout(core::time::Duration::from_millis(1000)) {
                        Ok(ip) => {
                            if ip == b"pong" {    //TODO harden - may be a response from some other process -> send a nonce?
                                trace!("process {} OK ({}ms)", name, chrono::Utc::now() - now);
                                debug!("process {} OK", name);
                            } else {
                                warn!("process {} sent a spurious response: {}", name, str::from_utf8(&ip).expect("got non-utf8 data"));
                                //TODO one spurious response currently trips up the logic
                            }
                        },
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            warn!("process {} failed to respond within 1000ms!", name);
                            ok = false;
                        },
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            warn!("process {} disconnected signal channel!", name);
                            ok = false;
                        }
                    }
                }
                if ok {
                    debug!("process health check OK");
                } else if exited_count == watchdog_threadandsignal.len() {
                    // network has effectively shut down
                    info!("process health check: all processes exited, shutting down network");
                    // signal runtime
                    watchdog_runtime.write().expect("watchdog failed to acquire lock for runtime").stop(watchdog_graph_inout.clone(), true).expect("watchdog failed to runtime.stop()");
                    //TODO runtime should set the time_stopped etc. values on runtime.status
                    // TODO not watchdog, but runtime.stop() should signal the FBP protocol clients
                    // send network stop notification to all FBP protocol clients//###
                    watchdog_graph_inout.lock().expect("lock poisoned").send_network_stopped(&NetworkStoppedResponse::new(&watchdog_runtime.read().expect("watchdog failed to acquire lock for runtime").status));
                    // exit thread
                    break;
                }
                thread::sleep(PROCESS_HEALTHCHECK_DUR);
            }
        }).expect("failed to spawn watchdog thread");

        // all set, now "open the doors" = inform FBP Network Protocol clients / remote runtimes that the graph inports are now connected as well (runtime:packet event type = connect)
        // check if graph has inports
        if let Some(inports) = &graph_inout.inports {
            for port_name in inports.keys().cloned().collect::<Vec<_>>() {  //TODO optimize wow, but works:  https://stackoverflow.com/a/45312076/5120003
                graph_inout.send_runtime_packet(&RuntimePacketResponse::new_connect(graph.properties.name.clone(), port_name.clone(), None, None)); //TODO can we avoid cloning here?
            }
        }

        // return status
        self.watchdog_thread = Some(watchdog_thread);
        self.watchdog_channel = Some(watchdog_signalsink2);
        self.status.time_started = UtcTime(chrono::Utc::now()); //TODO why convert to UtcTime?
        self.status.graph = self.graph.clone();
        self.status.started = true;
        self.status.running = true;
        info!("network started");
        Ok(&self.status)
    }

    fn stop(&mut self, graph_inout: Arc<std::sync::Mutex<GraphInportOutportHolder>>, watchdog_all_exited: bool) -> std::result::Result<&NetworkStartedResponsePayload, std::io::Error> {
        //TODO implement in full detail

        // if true, the network is simply marked shut down because watchdog informed us that all processes have exited
        if !watchdog_all_exited {
            // close front door early - inform FBP Network Protocol clients that graph inports are now disconnected (runtime:packet event type = connect)
            //NOTE: this happens before the GraphOutports handler thread is notified below among all the processes, so processing can be finished but no new packets are sent in anymore by the FBP client(s)
            //NOTE: this is inside a scope so that the lock goes out of scope and thus unlocks before the GraphOutHandler thread is signaled to shut down and notifies all FBP lients and therefore tries to lock graph_inout also
            {
                let mut graph_inout_inner = graph_inout.lock().expect("lock poisoned");
                // check if this graph actually has graph inports
                if let Some(inports) = &graph_inout_inner.inports {
                    info!("notifying clients of graph inports disconnect");
                    let keys = inports.keys().cloned().collect::<Vec<_>>();  //TODO optimize wow, but works:  https://stackoverflow.com/a/45312076/5120003
                    for port_name in keys {
                        graph_inout_inner.send_runtime_packet(&RuntimePacketResponse::new_disconnect(self.graph.clone(), port_name.clone(), None, None));  //TODO can we save cloning here?
                    }
                }
            }

            // signal all threads
            info!("stop: signaling all processes...");
            for (name, proc) in self.processes.iter() {
                info!("stop: signaling {}", name);
                proc.signal.send(b"stop".to_vec()).expect("channel send failed");   //TODO change to try_send() for reliability  //TODO optimize conversion of "stop"
                proc.joinhandle.thread().unpark();  // wake up for reception
            }
            // signal watchdog thread
            info!("stop: signaling watchdog");
            self.watchdog_channel.take().expect("watchdog channel is None? wtf").send(b"stop".to_vec()).expect("could not send stop signal to watchdog thread");
            let watchdog_thread = self.watchdog_thread.take().expect("watchdog thread is None? wtf");
            watchdog_thread.thread().unpark();
            info!("done");

            // join all threads
            //TODO what if one of them wont join? hangs? -> kill, how much time to give?
            info!("stop: joining all threads...");
            for (name, proc) in self.processes.drain() {
                info!("stop: joining {}", name);
                proc.joinhandle.join().expect("thread join failed"); //TODO there is .thread() -> for killing
            }
            info!("done");
            // join watchdog thread
            //info!("stop: joining watchdog");
            //TODO cannot wait for join because otherwise noflo-ui shows a timeout
            //watchdog_thread.join().expect("watchdog thread join failed");
        }

        // set status
        info!("network is shut down.");
        self.status.graph = self.graph.clone();
        self.status.started = true;
        self.status.running = false;    // was started, but not running any more
        Ok(&self.status)
    }

    fn debug_mode(&mut self, graph: &str, mode: bool) -> std::result::Result<(), std::io::Error> {
        //TODO check if the given graph exists
        //TODO check if the given graph is the currently selected one
        //TODO implement
        self.status.debug = mode;
        Ok(())
    }

    //TODO optimize: better to hand over String or &str? Difference between Vec and vec?
    fn set_debug_edges(&mut self, graph: &str, edges: &Vec<GraphEdgeSpec>) -> std::result::Result<(), std::io::Error> {
        //TODO clarify spec: what to do with this message's information behavior-wise? Dependent on first setting network into debug mode or independent?
        //TODO implement
        info!("got following debug edges:");
        for edge in edges {
            info!("  edge: src={:?} tgt={:?}", edge.src, edge.tgt);
        }
        info!("--- end");
        Ok(())
    }

    fn start_trace(&mut self, graph: &str, buffer_size: u32) -> std::result::Result<(), std::io::Error> {
        //TODO implement
        //TODO check if graph exists and is current graph
        if self.tracing {
            // wrong state
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, String::from("tracing already started")));
        }
        self.tracing = true;
        Ok(())
    }

    fn stop_trace(&mut self, graph: &str) -> std::result::Result<(), std::io::Error> {
        //TODO implement
        //TODO check if graph exists and is current graph
        if !self.tracing {
            // wrong state
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("tracing not started")));
        }
        self.tracing = false;
        Ok(())
    }

    //TODO can this function fail, at all? can the error response be removed?
    //TODO clarify spec: when is clear() allowed? in running state or in stopped state?
    fn clear_trace(&mut self, graph: &str) -> std::result::Result<(), std::io::Error> {
        //TODO implement
        //TODO check if graph exists and is current graph
        Ok(())
    }

    //TODO can this function fail, at all? can the error response be removed?
    //TODO clarify spec: when is dump() allowed? in running state or in stopped state?
    fn dump_trace(&mut self, graph: &str) -> std::result::Result<String, std::io::Error> {
        //TODO implement
        //TODO check if graph exists and is current graph
        //TODO implement Flowtrace format?
        Ok(String::from(""))    //TODO how to indicate "empty"? Does it maybe require at least "[]" or "{}"?
    }

    fn packet(&mut self, payload: &RuntimePacketRequestPayload, graph_inout: &mut GraphInportOutportHolder) -> std::result::Result<(), std::io::Error> {
        //TODO check if graph exists and if that port actually exists
        //TODO check payload datatype, schema, event (?) etc.
        //TODO implement and deliver to destination process
        info!("runtime: got a packet for port {}: {:?}", payload.port, payload.payload);
        // deliver to destination process
        if let Some(inports) = graph_inout.inports.as_mut() {
            if let Some(inport) = inports.get_mut(payload.port.as_str()) {
                while inport.sink.is_full() {
                    // wait until non-full
                    //TODO optimize
                    //condvar_notify!(&*inport.wake_notify);
                    inport.wakeup.as_ref().unwrap().unpark();

                    // sleep some time resp. give up CPU timeslot
                    thread::yield_now();
                }
                //TODO optimize String -> Vec<u8> conversion? is clone+into_bytes better or as_bytes+to_vec ?
                inport.sink.push(payload.payload.as_ref().expect("graph inport runtime:packet is missing payload").clone().into_bytes()).expect("push packet from graph inport into component failed");
                //condvar_notify!(&*inport.wake_notify);
                inport.wakeup.as_ref().unwrap().unpark();
                return Ok(());
            } else {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("graph inport with that name not found")));
            }
        } else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("no graph inports exist")));
        }
    }

    //TODO return path: process that sends to an outport -> send to client. TODO clarify spec: which client should receive it?

    //TODO runtime: command to connect an outport to a remote runtime as remote subgraph.
}

// runtime state of graph inports and outports
#[derive(Debug)]
struct GraphInportOutportHolder {
    // inports
    // the edge sinks are stored here because the connection handler in handle_client() needs to send into these
    inports: Option<HashMap<String, ProcessEdgeSink>>,

    // outports are handled by 1 special component that needs to be signaled and joined on network stop()
    // sink and wakeup are given to the processes that write into the graph outport process, so they are not stored here
    outports: Option<Process>,

    // connected client websockets ready to send responses to connected clients, for graphout process
    websockets: HashMap<std::net::SocketAddr, tungstenite::WebSocket<TcpStream>>
}

impl GraphInportOutportHolder {
    // should not be used by components but only by GraphOutHandler (TODO enforce that - just inline that method there? but the field websockets is still visible - both GraphOutHandler and processes...)
    fn send_runtime_packet(&mut self, packet: &RuntimePacketResponse) {
        //TODO add capabilities check for each client!
        for client in self.websockets.iter_mut() {
            client.1.write(Message::text(
                serde_json::to_string(packet)
                .expect("failed to serialize runtime:packet response"),
            ))
            .expect("failed to write message into websocket");
        }
    }

    //###
    // should not be used by components but only by watchdog (TODO enforce that - just inline that method there? but the field websockets is still visible - both GraphOutHandler and processes...)
    fn send_network_stopped(&mut self, packet: &NetworkStoppedResponse) {
        //TODO add capabilities check for each client!
        for client in self.websockets.iter_mut() {
            client.1.write(Message::text(
                serde_json::to_string(packet)
                .expect("failed to serialize network:stopped response"),
            ))
            .expect("failed to write message into websocket");
        }
    }

    // like STDOUT.println() - to be used by processes
    fn send_network_output(&mut self, packet: &NetworkOutputResponse) {
        //TODO add capabilities check for each client!
        for client in self.websockets.iter_mut() {
            client.1.write(Message::text(
                serde_json::to_string(packet)
                .expect("failed to serialize network:output response"),
            ))
            .expect("failed to write message into websocket");
        }
    }

    // like STDERR.println() - to be used by processes
    fn send_network_error(&mut self, packet: &NetworkErrorResponse) {
        //TODO add capabilities check for each client!
        for client in self.websockets.iter_mut() {
            client.1.write(Message::text(
                serde_json::to_string(packet)
                .expect("failed to serialize network:error response"),
            ))
            .expect("failed to write message into websocket");
        }
    }

    // inform client(s) about IP transfer on an edge, with copy of data - to be used by processes (TODO maybe later runtime for mandatory debugging?)
    fn send_network_data(&mut self, packet: &NetworkDataResponse) {
        //TODO add capabilities check for each client!
        //TODO add debug mode check for the graph (network:debug)
        //TODO add check if edge was selected for debugging (network:edges)
        for client in self.websockets.iter_mut() {
            client.1.write(Message::text(
                serde_json::to_string(packet)
                .expect("failed to serialize network:data response"),
            ))
            .expect("failed to write message into websocket");
        }
    }
}

#[derive(Serialize, Debug)]
enum Capability {
    // spec: deprecated. Implies capabilities network:status, network:data, network:control. Does not imply capability network:persist.
    #[serde(rename = "protocol:network")]
    ProtocolNetwork,
    //TODO implement, implied messages
    #[serde(rename = "network:persist")]
    NetworkPersist,
    //TODO implement, implied messages
    #[serde(rename = "network:status")]
    NetworkStatus,
    //TODO implement, implied messages
    #[serde(rename = "network:data")]
    NetworkData,
    //TODO implement, implied messages
    #[serde(rename = "network:control")]
    NetworkControl,

    // spec: can list components of the runtime using the component:list message.
    #[serde(rename = "protocol:component")]
    ProtocolComponent,
    #[serde(rename = "component:getsource")]
    ComponentGetsource,
    #[serde(rename = "component:setsource")]
    ComponentSetsource,

    // spec: can expose ports of main graph and transmit packet information to/from them
    // input messages: runtime:packet
    #[serde(rename = "protocol:runtime")]
    ProtocolRuntime,

    // spec: read and follow changes to runtime graphs (but not modify)
    #[serde(rename = "graph:readonly")]
    GraphReadonly, //TODO add access key management (store hashed version not the original) and capabilities management of each access key (see https://security.stackexchange.com/questions/63435/why-use-an-authentication-token-instead-of-the-username-password-per-request)
    // spec: read & modify runtime graphs using the Graph protocol.
    //input messages  graph:clear graph:addnode graph:removenode graph:renamenode graph:changenode graph:addedge graph:removeedge graph:changeedge graph:addinitial graph:removeinitial graph:addinport graph:removeinport graph:renameinport graph:addoutport graph:removeoutport graph:renameoutport graph:addgroup graph:removegroup graph:renamegroup graph:changegroup
    // output messages graph:clear graph:addnode graph:removenode graph:renamenode graph:changenode graph:addedge graph:removeedge graph:changeedge graph:addinitial graph:removeinitial graph:addinport graph:removeinport graph:renameinport graph:addoutport graph:removeoutport graph:renameoutport graph:addgroup graph:removegroup graph:renamegroup graph:changegroup graph:error
    #[serde(rename = "protocol:graph")]
    ProtocolGraph,

    // spec: runtime is able to record and send over flowtraces, used for retroactive debugging
    #[serde(rename = "protocol:trace")]
    ProtocolTrace,
}

// runtime:error response
#[derive(Serialize, Debug)]
struct RuntimeErrorResponse {
    protocol: String,
    command: String,
    payload: RuntimeErrorResponsePayload,
}

#[derive(Serialize, Debug)]
struct RuntimeErrorResponsePayload {
    message: String,
}

impl Default for RuntimeErrorResponse {
    fn default() -> Self {
        RuntimeErrorResponse {
            protocol: String::from("runtime"),
            command: String::from("error"),
            payload: RuntimeErrorResponsePayload::default(),
        }
    }
}

impl Default for RuntimeErrorResponsePayload {
    fn default() -> Self {
        RuntimeErrorResponsePayload {
            message: String::from("default runtime error message"),
        }
    }
}

impl RuntimeErrorResponse {
    fn new(msg: String) -> Self {
        RuntimeErrorResponse {
            protocol: String::from("runtime"),
            command: String::from("error"),
            payload: RuntimeErrorResponsePayload{
                message: msg,
            },
        }
    }
}

// graph:error response
#[derive(Serialize, Debug)]
struct GraphErrorResponse {
    //TODO spec: graph:error response message is not defined in spec!
    protocol: String,
    command: String,
    payload: GraphErrorResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphErrorResponsePayload {
    //TODO spec: graph:error response message payload is not defined in spec!
    message: String,
}

impl Default for GraphErrorResponse {
    fn default() -> Self {
        GraphErrorResponse {
            protocol: String::from("graph"),
            command: String::from("error"),
            payload: GraphErrorResponsePayload::default(),
        }
    }
}

impl Default for GraphErrorResponsePayload {
    fn default() -> Self {
        GraphErrorResponsePayload {
            message: String::from("default graph error message"),
        }
    }
}

impl GraphErrorResponse {
    fn new(err: String) -> Self {
        GraphErrorResponse {
            protocol: String::from("graph"),
            command: String::from("error"),
            payload: GraphErrorResponsePayload {
                message: err,
            },
        }
    }
}

// network:error response
#[derive(Serialize, Debug)]
struct NetworkErrorResponse {
    protocol: String,
    command: String,
    payload: NetworkErrorResponsePayload,
}

#[derive(Serialize, Debug)]
struct NetworkErrorResponsePayload {
    message: String, // spec: roughly similar to STDERR output of a Unix process, or a line of console.error in JavaScript.
    stack: String,   // stack trace
    graph: String,   // spec: graph the action targets
}

impl Default for NetworkErrorResponse {
    fn default() -> Self {
        NetworkErrorResponse {
            protocol: String::from("network"),
            command: String::from("error"),
            payload: NetworkErrorResponsePayload::default(),
        }
    }
}

impl Default for NetworkErrorResponsePayload {
    fn default() -> Self {
        NetworkErrorResponsePayload {
            message: String::from("default network error message"),
            stack: String::from("no stack trace given"),
            graph: String::from("default_graph"),
        }
    }
}

impl NetworkErrorResponse {
    fn new(err: String, stacktrace: String, graph_name: String) -> Self {
        NetworkErrorResponse {
            protocol: String::from("network"),
            command: String::from("error"),
            payload: NetworkErrorResponsePayload {
                message: err,
                stack: stacktrace,
                graph: graph_name,
            },
        }
    }
}

// component:error response
#[derive(Serialize, Debug)]
struct ComponentErrorResponse {
    protocol: String,
    command: String,
    payload: ComponentErrorResponsePayload,
}

#[derive(Serialize, Debug)]
struct ComponentErrorResponsePayload {
    message: String,
}

impl Default for ComponentErrorResponse {
    fn default() -> Self {
        ComponentErrorResponse {
            protocol: String::from("component"),
            command: String::from("error"),
            payload: ComponentErrorResponsePayload::default(),
        }
    }
}

impl Default for ComponentErrorResponsePayload {
    fn default() -> Self {
        ComponentErrorResponsePayload {
            message: String::from("default component error message"),
        }
    }
}

// ----------
// protocol:runtime
// ----------

// runtime:packet -> runtime:packetsent | runtime:error
// spec: 2018-03-21: Added packetsent response for runtime:packet input message
// spec: use runtime as remote subgraphs when they support protocol:runtime = packet input/output
// space: also possible as status message without request message
#[derive(Deserialize, Debug)]
struct RuntimePacketRequest {
    protocol: String,
    command: String,
    payload: RuntimePacketRequestPayload,
}

#[derive(Deserialize, Debug)]
struct RuntimePacketRequestPayload {  // protocol spec shows it as non-optional, but fbp-protocol says only port, event, graph are required at https://github.com/flowbased/fbp-protocol/blob/555880e1f42680bf45e104b8c25b97deff01f77e/schema/yaml/runtime.yml#L46
    port: String,
    event: RuntimePacketEvent, //TODO spec what does this do? format is string, but with certain allowed values: TODO
    #[serde(rename = "type")]
    typ: Option<String>, // spec: the basic data type send, example "array" -- TODO which values are allowed here? TODO serde rename correct?
    schema: Option<String>, // spec: URL to JSON schema describing the format of the data
    graph: String,
    payload: Option<String>, // spec: payload for the packet. Used only with begingroup (for group names) and data packets. //TODO type "any" allowed
    secret: String,  // only present on the request payload
}

#[derive(Serialize, Debug)]
struct RuntimePacketResponse {
    protocol: String,
    command: String,
    payload: RuntimePacketResponsePayload,
}

//TODO serde: RuntimePacketRequestPayload is the same as RuntimePacketResponsePayload except the payload -- any possibility to mark this optional for the response?
#[serde_with::skip_serializing_none]    // fbp-protocol thus noflo-ui does not like "" or null values for schema, type
#[derive(Serialize, Deserialize, Debug)]
struct RuntimePacketResponsePayload {
    port: String,
    event: RuntimePacketEvent, //TODO spec what does this do? format? fbp-protocol says: string enum
    #[serde(rename = "type")]
    typ: Option<String>, // spec: the basic data type send, example "array" -- TODO which values are allowed here? TODO serde rename correct?
    schema: Option<String>, // spec: URL to JSON schema describing the format of the data
    graph: String,
    payload: Option<String>, // spec: payload for the packet. Used only with begingroup (for group names) and data packets. //TODO type "any" allowed
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]  // fbp-protocol and noflo-ui expect this in lowercase
enum RuntimePacketEvent {
    Connect,
    BeginGroup,
    Data,
    EndGroup,
    Disconnect,
}

impl Default for RuntimePacketResponse {
    fn default() -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload::default(),
        }
    }
}

impl Default for RuntimePacketResponsePayload {
    fn default() -> Self {
        RuntimePacketResponsePayload {
            port: String::from("IN"),
            event: RuntimePacketEvent::Data,
            typ: Some(String::from("string")), //TODO is this correct?
            schema: None,
            graph: String::from("default_graph"),
            payload: Some(String::from("default packet payload")),
        }
    }
}

impl RuntimePacketResponse {
    fn new (payload: RuntimePacketResponsePayload) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: payload,
        }
    }

    fn new_connect (graph: String, port: String, typ: Option<String>, schema: Option<String>) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::Connect,
                typ: typ,
                schema: schema,
                payload: None,
            },
        }
    }

    fn new_begingroup (graph: String, port: String, typ: Option<String>, schema: Option<String>, group_name: String) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::BeginGroup,
                typ: typ,
                schema: schema,
                payload: Some(group_name),
            },
        }
    }

    fn new_data (graph: String, port: String, typ: Option<String>, schema: Option<String>, payload: String) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::Connect,
                typ: typ,
                schema: schema,
                payload: Some(payload),
            },
        }
    }

    fn new_endgroup (graph: String, port: String, typ: Option<String>, schema: Option<String>, group_name: String) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::EndGroup,
                typ: typ,
                schema: schema,
                payload: Some(group_name),
            },
        }
    }

    fn new_disconnect (graph: String, port: String, typ: Option<String>, schema: Option<String>) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::Disconnect,
                typ: typ,
                schema: schema,
                payload: None,
            },
        }
    }
}

// runtime:packetsent
#[derive(Serialize, Debug)]
struct RuntimePacketsentMessage {
    protocol: String,
    command: String,
    payload: RuntimePacketsentPayload, // clarify spec: echo the full runtime:packet back, with the full payload?! protocol spec looks like runtime needs to echo back oll of runtime:packet except secret, but fbp-protocol schema only requires port, event, graph @ https://github.com/flowbased/fbp-protocol/blob/555880e1f42680bf45e104b8c25b97deff01f77e/schema/yaml/runtime.yml#L194
}

#[serde_with::skip_serializing_none]    // fbp-protocol thus noflo-ui does not like "" or null values for schema, type
#[derive(Serialize, Deserialize, Debug)]    //TODO Deserialize seems useless, we are not getting that from the client? unless the client is another runtime maybe...?
struct RuntimePacketsentPayload {
    port: String,
    event: RuntimePacketEvent, //TODO spec what does this do? format? fbp-protocol says: string enum
    #[serde(rename = "type")]
    typ: Option<String>, // spec: the basic data type send, example "array" -- TODO which values are allowed here? TODO serde rename correct?
    schema: Option<String>, // spec: URL to JSON schema describing the format of the data
    graph: String,
    payload: Option<String>, // spec: payload for the packet. Used only with begingroup (for group names) and data packets. //TODO type "any" allowed
}

impl RuntimePacketsentMessage {
    //TODO for correctness, we should convert to RuntimePacketsentResponsePayload actually, but they are structurally the same
    fn new(payload: RuntimePacketsentPayload) -> Self {
        RuntimePacketsentMessage {
            protocol: String::from("runtime"),
            command: String::from("packetsent"),
            payload: payload,
        }
    }
}

impl From<RuntimePacketRequestPayload> for RuntimePacketsentPayload {
    fn from(payload: RuntimePacketRequestPayload) -> Self {
        RuntimePacketsentPayload {  // we just leave away the field secret; and many fields can be None
            port: payload.port,
            event: payload.event,
            typ: payload.typ,
            schema: payload.schema,
            graph: payload.graph,
            payload: payload.payload,
        }
    }
}

// runtime:ports response
#[derive(Serialize, Debug)]
struct RuntimePortsMessage {
    protocol: String,
    command: String,
    payload: RuntimePortsPayload,
}

impl Default for RuntimePortsMessage {
    fn default() -> Self {
        RuntimePortsMessage {
            protocol: String::from("runtime"),
            command: String::from("ports"),
            payload: RuntimePortsPayload::default(),
        }
    }
}

impl RuntimePortsMessage {
    fn new(runtime: &RuntimeRuntimePayload, graph: &Graph) -> Self {
        RuntimePortsMessage {
            protocol: String::from("runtime"),
            command: String::from("ports"),
            payload:  RuntimePortsPayload {
                graph: runtime.graph.clone(),
                in_ports: graph.ports_as_componentportsarray(&graph.inports),
                out_ports: graph.ports_as_componentportsarray(&graph.outports),
        }}
    }
}

// spec: can request both the inports and outports of a graph and a component with the same message
// beware the fields for graph inports and outports are different from component inports and outports, also array <-> object/hashmap#[derive(Serialize, Debug)]
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RuntimePortsPayload {
    graph: String,
    in_ports: Vec<ComponentPort>,
    out_ports: Vec<ComponentPort>,
}

impl Default for RuntimePortsPayload {
    fn default() -> Self {
        RuntimePortsPayload {
            graph: String::from("default_graph"),
            in_ports: vec![],
            out_ports: vec![],
        }
    }
}

// ----------
// protcol:network
// ----------

// spec: Implies capabilities network:status, network:data, network:control. Does not imply capability network:persist.

// ----------
// network:persist
// ----------

// network:persist -> network:persist | network:error
#[derive(Deserialize, Debug)]
struct NetworkPersistRequest {
    protocol: String,
    command: String,
    payload: NetworkPersistRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkPersistRequestPayload {
    secret: String,
}

#[derive(Serialize, Debug)]
struct NetworkPersistResponse {
    protocol: String,
    command: String,
    payload: NetworkPersistResponsePayload,
}

impl Default for NetworkPersistResponse {
    fn default() -> Self {
        NetworkPersistResponse {
            protocol: String::from("network"),
            command: String::from("persist"),
            payload: NetworkPersistResponsePayload::default(),
        }
    }
}

#[derive(Serialize, Debug)]
struct NetworkPersistResponsePayload {}

impl Default for NetworkPersistResponsePayload {
    fn default() -> Self {
        NetworkPersistResponsePayload {}
    }
}

// ----------
// network:status
// ----------

// spec: is a subset of network:control, implementations are there

// ----------
// network:data
// ----------

// network:edges -> network:edges | network:error
#[derive(Deserialize, Debug)]
struct NetworkEdgesRequest {
    protocol: String,
    command: String,
    payload: NetworkEdgesRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkEdgesRequestPayload {
    graph: String,
    edges: Vec<GraphEdgeSpec>,
    secret: String,
}

//NOTE: Serialize trait needed for FBP graph structs, not for the FBP network protocol
#[derive(Serialize, Deserialize, Debug)]
struct GraphEdgeSpec {
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
}

#[derive(Serialize, Debug)]
struct NetworkEdgesResponse {
    protocol: String,
    command: String,
    payload: NetworkEdgesResponsePayload,
}

#[derive(Serialize, Debug)]
struct NetworkEdgesResponsePayload {    //TODO spec: is a confirmative response of type network:edges enough or should all values be echoed back? noflo-ui definitely complains about an empty payload on a confirmative network:edges response.
    graph: String,
    edges: Vec<GraphEdgeSpec>,
}   //TODO optimize senseless duplication of structs, use serde_partial (or so?) for sending the response

impl Default for NetworkEdgesResponse {
    fn default() -> Self {
        NetworkEdgesResponse {
            protocol: String::from("network"),
            command: String::from("edges"),
            payload: NetworkEdgesResponsePayload::default(),
        }
    }
}

impl Default for NetworkEdgesResponsePayload {
    fn default() -> Self {
        NetworkEdgesResponsePayload {
            graph: String::from("default_graph"),
            edges: vec!(),
        }
    }
}

impl NetworkEdgesResponse {
    fn from_request(payload: NetworkEdgesRequestPayload) -> Self {
        NetworkEdgesResponse {
            protocol: String::from("network"),
            command: String::from("edges"),
            payload: NetworkEdgesResponsePayload {
                graph: payload.graph,
                edges: payload.edges,
            },
        }
    }
}

// network:output response
//NOTE spec: like STDOUT output of a Unix process, or a line of console.log in JavaScript. Can also be used for passing images from the runtime to the UI.
#[derive(Serialize, Debug)]
struct NetworkOutputResponse {
    protocol: String,
    command: String,
    payload: NetworkOutputResponsePayload,
}

impl Default for NetworkOutputResponse {
    fn default() -> Self {
        NetworkOutputResponse {
            protocol: String::from("network"),
            command: String::from("output"),
            payload: NetworkOutputResponsePayload::default(),
        }
    }
}

impl NetworkOutputResponse {
    fn new(msg: String) -> Self {
        NetworkOutputResponse {
            protocol: String::from("network"),
            command: String::from("output"),
            payload: NetworkOutputResponsePayload {
                typ: String::from("message"),   //TODO optimize can we &str into a const maybe instead of always generating these String::from()?
                message: Some(msg),
                url: None,
            },
        }
    }

    fn new_previewurl(url: String) -> Self {
        NetworkOutputResponse {
            protocol: String::from("network"),
            command: String::from("output"),
            payload: NetworkOutputResponsePayload {
                typ: String::from("previewurl"),
                url: Some(url),
                message: None,
            },
        }
    }
}

#[skip_serializing_none]
#[derive(Serialize, Debug)]
struct NetworkOutputResponsePayload {
    #[serde(rename = "type")]
    typ: String, // spec: either "message" or "previewurl"    //TODO convert to enum
    message: Option<String>,    //TODO optimize would &str as parameter be faster to decode into for serde?
    url: Option<String>, // spec: URL for an image generated by the runtime
    //TODO serde "rule" that either message or url must be present in case of type="message" or "previewurl", respectively
}

impl Default for NetworkOutputResponsePayload {
    fn default() -> Self {
        NetworkOutputResponsePayload {
            typ: String::from("message"),
            message: Some(String::from("default output message")),
            url: None,
        }
    }
}

// network:connect response
#[derive(Serialize, Debug)]
struct NetworkConnectResponse {
    protocol: String,
    command: String,
    payload: NetworkTransmissionPayload,
}

impl Default for NetworkConnectResponse {
    fn default() -> Self {
        NetworkConnectResponse {
            protocol: String::from("network"),
            command: String::from("connect"),
            payload: NetworkTransmissionPayload::default(),
        }
    }
}

#[skip_serializing_none]
#[derive(Serialize, Debug)]
struct NetworkTransmissionPayload { //TODO rename to NetworkEventPayload? In FBP network protocol spec the base fields (id, sr, tgt, graph, subgraph) are referenced ad "network/event":  https://github.com/flowbased/fbp-protocol/blob/555880e1f42680bf45e104b8c25b97deff01f77e/schema/yaml/network.yml#L246
    id: String, // spec: textual edge identifier, usually in form of a FBP language line
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    graph: String,
    subgraph: Option<Vec<String>>, // spec: Subgraph identifier for the event. An array of node IDs. optional according to schema. TODO what does it mean? why a list of node IDs? - check schema:  https://github.com/flowbased/fbp-protocol/blob/555880e1f42680bf45e104b8c25b97deff01f77e/schema/yaml/shared.yml#L193
    data: Option<String>,   //TODO fix do this using type system (composable traits? "inheritance"?) data is only mandatory for network:data response, not for begingroup, endgroup, connect, disconnect
}

impl Default for NetworkTransmissionPayload {
    fn default() -> Self {
        NetworkTransmissionPayload {
            id: String::from("Repeater.OUT -> Display.IN"), //TODO not sure if this is correct
            src: GraphNodeSpecNetwork::default(),
            tgt: GraphNodeSpecNetwork::default(),
            graph: String::from("main_graph"),
            subgraph: Some(vec![String::from("Repeater.OUT -> Display.IN")]), //TODO not sure of this is correct, most likely not
            data: None,
        }
    }
}

// network:data response
#[derive(Serialize, Debug)]
struct NetworkDataResponse {
    protocol: String,
    command: String,
    payload: NetworkTransmissionPayload,
}

impl Default for NetworkDataResponse {
    fn default() -> Self {
        NetworkDataResponse {
            protocol: String::from("network"),
            command: String::from("data"),
            payload: NetworkTransmissionPayload::default(),
        }
    }
}

impl NetworkDataResponse {
    fn new(payload: NetworkTransmissionPayload) -> Self {
        //TODO solve in a better way using the type system
        if payload.data == None {
            error!("constructing a NetworkDataResponse with empty data field");
        }
        NetworkDataResponse {
            protocol: String::from("network"),
            command: String::from("data"),
            payload: payload,
        }
    }
}

// network:begingroup response
#[derive(Serialize, Debug)]
struct NetworkBegingroupResponse {
    protocol: String,
    command: String,
    payload: NetworkTransmissionPayload,
}

impl Default for NetworkBegingroupResponse {
    fn default() -> Self {
        NetworkBegingroupResponse {
            protocol: String::from("network"),
            command: String::from("begingroup"),
            payload: NetworkTransmissionPayload::default(),
        }
    }
}

// network:endgroup
#[derive(Serialize, Debug)]
struct NetworkEndgroupResponse {
    protocol: String,
    command: String,
    payload: NetworkTransmissionPayload,
}

impl Default for NetworkEndgroupResponse {
    fn default() -> Self {
        NetworkEndgroupResponse {
            protocol: String::from("network"),
            command: String::from("endgroup"),
            payload: NetworkTransmissionPayload::default(),
        }
    }
}

// network:disconnect
#[derive(Serialize, Debug)]
struct NetworkDisconnectResponse {
    protocol: String,
    command: String,
    payload: NetworkTransmissionPayload,
}

impl Default for NetworkDisconnectResponse {
    fn default() -> Self {
        NetworkDisconnectResponse {
            protocol: String::from("network"),
            command: String::from("disconnect"),
            payload: NetworkTransmissionPayload::default(),
        }
    }
}

// network:icon response
#[derive(Serialize, Debug)]
struct NetworkIconResponse {
    protocol: String,
    command: String,
    payload: NetworkIconResponsePayload,
}

impl Default for NetworkIconResponse {
    fn default() -> Self {
        NetworkIconResponse {
            protocol: String::from("network"),
            command: String::from("icon"),
            payload: NetworkIconResponsePayload::default(),
        }
    }
}

#[derive(Serialize, Debug)]
struct NetworkIconResponsePayload {
    id: String, // spec: identifier of the node
    icon: String,
    graph: String,
}

impl Default for NetworkIconResponsePayload {
    fn default() -> Self {
        NetworkIconResponsePayload {
            id: String::from("Repeater"),
            icon: String::from("fa-usd"),
            graph: String::from("main_graph"),
        }
    }
}

// network:processerror
// spec: When in debug mode, a network can signal an error happening inside a process.
#[derive(Serialize, Debug)]
struct NetworkProcesserrorResponse {
    protocol: String,
    command: String,
    payload: NetworkProcesserrorResponsePayload,
}

impl Default for NetworkProcesserrorResponse {
    fn default() -> Self {
        NetworkProcesserrorResponse {
            protocol: String::from("network"),
            command: String::from("processerror"),
            payload: NetworkProcesserrorResponsePayload::default(),
        }
    }
}

#[derive(Serialize, Debug)]
struct NetworkProcesserrorResponsePayload {
    id: String, // spec: identifier of the node
    error: String,
    graph: String,
}

impl Default for NetworkProcesserrorResponsePayload {
    fn default() -> Self {
        NetworkProcesserrorResponsePayload {
            id: String::from("Repeater"),
            error: String::from("default network process error response"),
            graph: String::from("main_graph"),
        }
    }
}

// ----------
// network:control
// ----------

// network:start -> network:started | network:error
#[derive(Deserialize, Debug)]
struct NetworkStartRequest {
    protocol: String,
    command: String,
    payload: NetworkStartRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkStartRequestPayload {
    graph: String,
    secret: String,
}

#[derive(Serialize, Debug)]
struct NetworkStartedResponse<'a> {
    protocol: String,
    command: String,
    payload: &'a NetworkStartedResponsePayload,
}

#[derive(Serialize, Debug)]
struct NetworkStartedResponsePayload {
    #[serde(rename = "time")]
    time_started: UtcTime, //TODO clarify spec: defined as just a String. But what time format? meaning of the field anyway?
    graph: String,
    started: bool, // spec: see network:status response for meaning of started and running //TODO spec: shouldn't this always be true?
    running: bool,
    debug: bool,
}

//NOTE: this type alias allows us to implement Serialize (a trait from another crate) for DateTime (also from another crate)
#[derive(Debug)]
struct UtcTime(chrono::DateTime<Utc>);

impl Serialize for UtcTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: serde::ser::Serializer {
        return Ok(serializer.serialize_str(self.0.format("%+").to_string().as_str()).expect("fail serializing datetime"));
    }
}

impl Default for NetworkStartedResponse<'_> {
    fn default() -> Self {
        NetworkStartedResponse {
            protocol: String::from("network"),
            command: String::from("started"),
            //TODO fix; using recursive Default::default() because the following does not work:
            //payload: &NetworkStartedResponsePayload::default(),
            ..Default::default()
        }
    }
}

impl Default for NetworkStartedResponsePayload {
    fn default() -> Self {
        NetworkStartedResponsePayload {
            time_started: UtcTime(chrono::Utc::now()), //TODO is this correct?
            graph: String::from("main_graph"),
            started: false,
            running: false,
            debug: false,
        }
    }
}

impl<'a> NetworkStartedResponse<'a> {
    fn new(status: &'a NetworkStartedResponsePayload) -> Self {
        NetworkStartedResponse {
            protocol: String::from("network"),
            command: String::from("started"),
            payload: status,
        }
    }
}

// network:stop -> network:stopped | network:error
#[derive(Deserialize, Debug)]
struct NetworkStopRequest {
    protocol: String,
    command: String,
    payload: NetworkStopRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkStopRequestPayload {
    graph: String,
    secret: String,
}

#[derive(Serialize, Debug)]
struct NetworkStoppedResponse {
    protocol: String,
    command: String,
    payload: NetworkStoppedResponsePayload,
}

#[derive(Serialize, Debug)]
struct NetworkStoppedResponsePayload {
    #[serde(rename = "time")]
    time_stopped: UtcTime, //TODO spec: string. clarify spec: time format? purpose? datetime where network was stopped?
    uptime: i64, // spec: time the network was running, in seconds //TODO spec: should the time it was stopped be subtracted from this number? //TODO spec: not "time" but "duration"
    graph: String,
    started: bool, // spec: see network:status response for meaning of started and running
    running: bool, // TODO spec: shouldn't this always be false?    //TODO spec: ordering of fields is different between network:started and network:stopped -> fix in spec.
    debug: bool,
}

impl Default for NetworkStoppedResponse {
    fn default() -> Self {
        NetworkStoppedResponse {
            protocol: String::from("network"),
            command: String::from("stopped"),
            payload: NetworkStoppedResponsePayload::default(),
        }
    }
}

impl Default for NetworkStoppedResponsePayload {
    fn default() -> Self {
        NetworkStoppedResponsePayload {
            time_stopped: UtcTime(chrono::Utc::now()), //TODO is this correct?
            uptime: 123,
            graph: String::from("main_graph"),
            started: false,
            running: false,
            debug: false,
        }
    }
}

impl NetworkStoppedResponse {
    fn new(status: &NetworkStartedResponsePayload) -> Self {
        NetworkStoppedResponse {
            protocol: String::from("network"),
            command: String::from("stopped"),
            payload: NetworkStoppedResponsePayload {
                time_stopped: UtcTime(chrono::Utc::now()),
                uptime: (chrono::Utc::now() - status.time_started.0).num_seconds(),
                graph: status.graph.clone(),
                started: status.started,
                running: status.running,
                debug: status.debug,
            },
        }
    }
}

// network:getstatus -> network:status | network:error
#[derive(Deserialize, Debug)]
struct NetworkGetstatusMessage {
    protocol: String,
    command: String,
    payload: NetworkGetstatusPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkGetstatusPayload {
    graph: String,
    secret: String,
}

// ----------

#[derive(Serialize, Debug)]
struct NetworkStatusMessage<'a> {
    protocol: String,
    command: String,
    payload: &'a NetworkStatusPayload,
}

impl Default for NetworkStatusMessage<'_> {
    fn default() -> Self {
        NetworkStatusMessage {
            protocol: String::from("network"),
            command: String::from("status"),
            //TODO fix - currently using recursive Default::default() because the following does not work:
            //payload: &NetworkStatusPayload::default(),
            ..Default::default()
        }
    }
}

//TODO payload has small size, we could copy it, problem is NetworkStatusPayload cannot automatically derive Copy because of the String does not implement Copy
impl<'a> NetworkStatusMessage<'a> {
    fn new(payload: &'a NetworkStatusPayload) -> Self {
        NetworkStatusMessage {
            protocol: String::from("network"),
            command: String::from("status"),
            payload: payload,
        }
    }
}

#[derive(Serialize, Debug)]
struct NetworkStatusPayload {
    graph: String,
    uptime: i64, // spec: time the network has been running, in seconds. NOTE: seconds since start of the network. NOTE: i64 because of return type from new() chrono calculations return type, which cannot be converted to u32.
    // NOTE: started+running=is running now. started+not running=network has finished. not started+not running=network was never started. not started+running=undefined (TODO).
    started: bool, // spec: whether or not network has been started
    running: bool, // spec: boolean tells whether the network is running at the moment or not
    debug: bool,   // spec: whether or not network is in debug mode
}

impl Default for NetworkStatusPayload {
    fn default() -> Self {
        NetworkStatusPayload {
            graph: String::from("default_graph"),
            uptime: 256,
            started: true,
            running: true,
            debug: false,
        }
    }
}

impl NetworkStatusPayload {
    fn new(status: &NetworkStartedResponsePayload) -> Self {
        NetworkStatusPayload {
            graph: status.graph.clone(),
            uptime: (chrono::Utc::now() - status.time_started.0).num_seconds().into(),
            started: status.started,
            running: status.running,
            debug: status.debug,
        }
    }
}

// network:debug -> TODO spec: response not specified | network:error
#[derive(Deserialize, Debug)]
struct NetworkDebugRequest {
    protocol: String,
    command: String,
    payload: NetworkDebugRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkDebugRequestPayload {
    enable: bool,
    graph: String,
    secret: String,
}

//TODO spec: this response is not defined in the spec! What should the response be?
#[derive(Serialize, Debug)]
struct NetworkDebugResponse {
    protocol: String,
    command: String,
    payload: NetworkDebugResponsePayload,
}

#[derive(Serialize, Debug)]
struct NetworkDebugResponsePayload {
    graph: String,
}

impl Default for NetworkDebugResponse {
    fn default() -> Self {
        NetworkDebugResponse {
            protocol: String::from("network"),
            command: String::from("debug"),
            payload: NetworkDebugResponsePayload::default(),
        }
    }
}

impl Default for NetworkDebugResponsePayload {
    fn default() -> Self {
        NetworkDebugResponsePayload {
            graph: String::from("default_graph"),
        }
    }
}

impl NetworkDebugResponse {
    //TODO optimize here we could probably use &str with lifetimes
    //TODO clarify spec if enable status should be returned, does not seem required
    fn new(graph: String) -> Self {
        NetworkDebugResponse {
            protocol: String::from("network"),
            command: String::from("debug"),
            payload: NetworkDebugResponsePayload {
                graph: graph,
            },
        }
    }
}

// ----------
// protocol:component
// ----------

// component:list -> component:component (multiple possible), then a final component:componentsready | component:error
#[derive(Deserialize, Debug)]
struct ComponentListRequest {
    protocol: String,
    command: String,
    payload: ComponentListRequestPayload,
}

#[derive(Deserialize, Debug)]
struct ComponentListRequestPayload {
    secret: String,
}

// ----------

#[derive(Serialize, Debug)]
struct ComponentComponentMessage<'a> {
    protocol: String,
    command: String,
    payload: &'a ComponentComponentPayload,
}

impl Default for ComponentComponentMessage<'_> {
    fn default() -> Self {
        ComponentComponentMessage {
            protocol: String::from("component"),
            command: String::from("component"),
            //TODO fix, using reursive Default::default() because the following does not work:
            //payload: &ComponentComponentPayload::default(),
            ..Default::default()
        }
    }
}

impl<'a> ComponentComponentMessage<'a> {
    fn new(payload: &'a ComponentComponentPayload) -> Self {
        ComponentComponentMessage {
            protocol: String::from("component"),
            command: String::from("component"),
            payload: payload,
        }
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ComponentComponentPayload {
    name: String, // spec: component name in format that can be used in graphs. Should contain the component library prefix.
    description: String,
    icon: String, // spec: visual icon for the component, matching icon names in Font Awesome
    subgraph: bool, // spec: is the component a subgraph?
    in_ports: Vec<ComponentPort>, // spec: array. TODO could be modelled as a hashmap/object
    out_ports: Vec<ComponentPort>, // spec: array. TODO clould be modelled as a hashmap/object ... OTOH, tere are usually not so many ports, can just as well iterate over 1/2/3/4 ports.
    // flowd-specific
    // TODO make extensible and versioned
    #[serde(skip)]
    support_health: bool,
    #[serde(skip)]
    support_perfdata: bool,
    #[serde(skip)]
    support_reconnect: bool,    //TODO should this belong to the ports (in_ports, out_ports fields)?
}

impl Default for ComponentComponentPayload {
    fn default() -> Self {
        ComponentComponentPayload {
            name: String::from("main/Repeat"), //TODO Repeat, Drop, Output required for tests
            description: String::from("description of the Repeat component"),
            icon: String::from("fa-usd"), //TODO with fa- prefix? FontAwesome should not so much be our concern in a new()-like method
            subgraph: false,
            in_ports: vec![],
            out_ports: vec![],
            support_health: false,
            support_perfdata: false,
            support_reconnect: false,
        }
    }
}

#[serde_with::skip_serializing_none]    // fbp-protocol thus noflo-ui does not like "" or null values for schema
#[derive(Serialize, Debug)]
struct ComponentPort {
    #[serde(rename = "id")]
    name: String,
    #[serde(rename = "type")]
    allowed_type: String, //TODO clarify spec: so if we define a boolean, we can send only booleans? What about struct/object types? How should the runtime verify that? //TODO map JSON types <-> Rust types
    #[serde(default)]
    schema: Option<String>, // spec: optional
    #[serde(default)]
    required: bool, // spec: optional, whether the port needs to be connected for the component to work (TODO add checks for that and notify user (how?) that a vital port is unconnected if required=true)
    #[serde(default, rename = "addressable")]
    is_arrayport: bool, // spec: optional
    #[serde(default)]
    description: String,  // spec: optional
    #[serde(default, rename = "values")]
    values_allowed: Vec<String>,  // spec: optional, can probably be any type, but TODO how to map JSON "any values" to Rust?
    #[serde(default, rename = "default")]
    value_default: String,  // spec: optional, datatype any TODO how to map JSON any values in Rust?
}

impl Default for ComponentPort {
    fn default() -> Self {
        ComponentPort {
            name: String::from("out"),
            allowed_type: String::from("string"),
            schema: None,
            required: true,
            is_arrayport: false,
            description: String::from("a default output port"),
            values_allowed: vec!(), //TODO clarify spec: does empty array mean "no values allowed" or "all values allowed"?
            value_default: String::from(""),
        }
    }
}

impl ComponentPort {
    fn default_in() -> Self {
        ComponentPort {
            name: String::from("in"),
            allowed_type: String::from("string"),
            schema: None,
            required: true,
            is_arrayport: false,
            description: String::from("a default input port"),
            values_allowed: vec!(), //TODO clarify spec: does empty array mean "no values allowed" or "all values allowed"?
            value_default: String::from(""),
        }
    }

    fn default_out() -> Self {
        return ComponentPort::default()
    }
}

// ----------

#[derive(Serialize, Debug)]
struct ComponentComponentsreadyMessage {
    protocol: String,
    command: String,
    payload: u32, // noflo-ui expects payload to be integer -> TODO clarify spec: number of component:component messages before the component:componentsready message?
}

impl Default for ComponentComponentsreadyMessage {
    fn default() -> Self {
        ComponentComponentsreadyMessage {
            protocol: String::from("component"),
            command: String::from("componentsready"),
            payload: 1,
        }
    }
}

impl ComponentComponentsreadyMessage {
    fn new(count_ready: u32) -> Self {
        ComponentComponentsreadyMessage {
            protocol: String::from("component"),
            command: String::from("componentsready"),
            payload: count_ready,
        }
    }
}

// ----------
// component:getsource
// ----------

// component:getsource -> component:source | component:error
#[derive(Deserialize, Debug)]
struct ComponentGetsourceMessage {
    protocol: String,
    command: String,
    payload: ComponentGetsourcePayload,
}

#[derive(Deserialize, Debug)]
struct ComponentGetsourcePayload {
    name: String, // spec: Name of the component to for which to get source code. Should contain the library prefix, eg. "my-project/SomeComponent"
    secret: String,
}

// component:source
//NOTE: is used as request in setsource context and as response in getsource context
#[derive(Serialize, Debug)]
struct ComponentSourceMessage {
    protocol: String,
    command: String,
    payload: ComponentSourcePayload,
}

impl Default for ComponentSourceMessage {
    fn default() -> Self {
        ComponentSourceMessage {
            protocol: String::from("component"),
            command: String::from("source"),
            payload: ComponentSourcePayload::default(),
        }
    }
}

impl ComponentSourceMessage {
    fn default_graph() -> Self {
        ComponentSourceMessage {
            protocol: String::from("component"),
            command: String::from("source"),
            payload: ComponentSourcePayload::default_graph(),
        }
    }

    fn new(payload: ComponentSourcePayload) -> Self {
        ComponentSourceMessage {
            protocol: String::from("component"),
            command: String::from("source"),
            payload: payload,
        }
    }
}

#[derive(Serialize, Debug)]
struct ComponentSourcePayload {
    name: String, // spec: Name of the component. Must not contain library prefix
    language: String,
    library: String, // spec: Component library identifier, eg. "components-common"
    code: String,    // spec: component source code
    tests: String,   // spec: unit tests for the component
}

impl Default for ComponentSourcePayload {
    fn default() -> Self {
        ComponentSourcePayload {
            name: String::from("Repeat"),
            language: String::from("Rust"),
            library: String::from("main_library"),
            code: String::from("// source code for component Repeat"),
            tests: String::from("// unit tests for component Repeat"),
        }
    }
}

impl ComponentSourcePayload {
    fn default_graph() -> Self {
        ComponentSourcePayload {
            name: String::from("default_graph"),
            language: String::from("json"),
            library: String::from("main_library"),
            //TODO validate against schema @ https://github.com/flowbased/fbp/blob/master/schema/graph.json
            code: String::from(
                r#"{
                "caseSensitive": true,
                "properties": {
                    "name": "default_graph",
                    "environment": {
                        "type": "flowd",
                        "content": ""
                    },
                    "description": "description for default_graph",
                    "icon": "usd"
                },
                "inports": {},
                "outports": {},
                "groups": [
                    {
                        "name": "process_group1",
                        "nodes": ["Repeater"],
                        "metadata": {
                            "description": "description of process_group1"
                        }
                    }
                ],
                "processes": {
                    "Repeater": {
                        "component": "Repeat",
                        "metadata": {
                            "x": 100,
                            "y": 100
                        }
                    }
                },
                "connections": []
        }"#,
            ),
            tests: String::from("// tests for graph default_graph"),
        }
    }
}

// ----------
// component:setsource
// ----------

// component:source -> component:component | component:error
//NOTE: find implementation of component:source above in section component:getsource
//NOTE: find implementation of component:component below in section protocol:component

// ----------
// graph:readonly
// ----------

// spec: read and follow changes to runtime graphs (but not modify)
// output messages: graph:clear graph:addnode graph:removenode graph:renamenode graph:changenode graph:addedge graph:removeedge graph:changeedge graph:addinitial graph:removeinitial graph:addinport graph:removeinport graph:renameinport graph:addoutport graph:removeoutport graph:renameoutport graph:addgroup graph:removegroup graph:renamegroup graph:changegroup

// ----------
// protocol:graph
// ----------

// graph:clear -> graph:clear | graph:error
#[derive(Deserialize, Debug)]
struct GraphClearRequest {
    protocol: String,
    command: String,
    payload: GraphClearRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphClearRequestPayload {
    #[serde(rename = "id")]
    name: String,   // name of the graph
    #[serde(rename = "name")]
    label: String, // human-readable label of the graph
    library: String,    //TODO clarify spec
    main: bool, // TODO clarify spec
    icon: String,
    description: String,
    secret: String,
}

#[derive(Serialize, Debug)]
struct GraphClearResponse {
    protocol: String,
    command: String,
    payload: GraphClearResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphClearResponsePayload {
    #[serde(rename = "id")]
    name: String,   // name of the graph
    #[serde(rename = "name")]
    label: String, // human-readable label of the graph
    library: String,    //TODO clarify spec
    main: bool, // TODO clarify spec
    icon: String,
    description: String,
}

impl Default for GraphClearResponse {
    fn default() -> Self {
        GraphClearResponse {
            protocol: String::from("graph"),
            command: String::from("clear"),
            payload: GraphClearResponsePayload::default(),
        }
    }
}

impl Default for GraphClearResponsePayload {
    fn default() -> Self {
        GraphClearResponsePayload {
            name: String::from("001"),
            label: String::from("main_graph"),
            library: String::from("main_library"),
            main: true,
            icon: String::from("fa-gbp"),
            description: String::from("the main graph"),
        }
    }
}

impl GraphClearResponse {
    fn new(payload: &GraphClearRequestPayload) -> Self {
        GraphClearResponse {
            protocol: String::from("graph"),
            command: String::from("clear"),
            payload: GraphClearResponsePayload {
                //TODO unify GraphClearRequest and GraphClearResponse -> optimize this
                name: payload.name.clone(),
                label: payload.label.clone(),
                library: payload.library.clone(),
                main: payload.main,
                icon: payload.icon.clone(),
                description: payload.description.clone(),
            },
        }
    }
}

// graph:addnode -> graph:addnode | graph:error
#[derive(Deserialize, Debug)]
struct GraphAddnodeRequest {
    protocol: String,
    command: String,
    payload: GraphAddnodeRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphAddnodeRequestPayload {
    #[serde(rename = "id")]
    name: String,                  // name of the node/process
    component: String,           // component name to be used for this node/process
    //TODO spec: the metadata object is optional according to schema - make it Option<>
    metadata: GraphNodeMetadata, //TODO spec: key-value pairs (with some well-known values)
    graph: String,                 // name of the graph
    secret: String,
}

// NOTE: Serialize because used in GraphNode -> Graph which needs to be serialized
#[derive(Serialize, Deserialize, Debug)]
struct GraphNodeMetadata {
    x: i32, // TODO check spec: can x and y be negative? -> i32 or u32? TODO in specs is range not defined, but noflo-ui uses negative coordinates as well
    y: i32,
    width: Option<u32>,  // not mentioned in specs, but used by noflo-ui, usually 72
    height: Option<u32>,  // not mentioned in specs, but used by noflo-ui, usually 72
    label: Option<String>,  // not mentioned in specs, but used by noflo-ui, used for the process name in bigger letters than component name
}

impl Clone for GraphNodeMetadata {
    fn clone(&self) -> Self {
        GraphNodeMetadata {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            label: self.label.clone(),
        }
    }
}

impl Default for GraphNodeMetadata {
    fn default() -> Self {
        GraphNodeMetadata {
            x: 0,
            y: 0,
            width: Some(NODE_WIDTH_DEFAULT),
            height: Some(NODE_HEIGHT_DEFAULT),
            label: None,
        }
    }

}

#[derive(Serialize, Debug)]
struct GraphAddnodeResponse {
    protocol: String,
    command: String,
    payload: GraphAddnodeResponsePayload,
}

impl Default for GraphAddnodeResponse {
    fn default() -> Self {
        GraphAddnodeResponse {
            protocol: String::from("graph"),
            command: String::from("addnode"),
            payload: GraphAddnodeResponsePayload::default(),
        }
    }
}

impl GraphAddnodeResponse {
    fn new(payload: GraphAddnodeResponsePayload) -> Self {
        GraphAddnodeResponse {
            protocol: String::from("graph"),
            command: String::from("addnode"),
            payload: payload,
        }
    }
}

#[derive(Serialize, Debug)]
struct GraphAddnodeResponsePayload {    // TODO check spec: should the sent values be echoed back as confirmation or is empty graph:addnode vs. a graph:error enough?
    #[serde(rename = "id")]
    name: String,
    component: String,
    metadata: GraphNodeMetadata,
    graph: String,
}

impl Default for GraphAddnodeResponsePayload {
    fn default() -> Self {
        GraphAddnodeResponsePayload {
            name: "".to_owned(),
            component: "".to_owned(),
            metadata: GraphNodeMetadata::default(),
            graph: String::from("default_graph"),
        }
    }

}

// graph:removenode -> graph:removenode | graph:error
#[derive(Deserialize, Debug)]
struct GraphRemovenodeRequest {
    protocol: String,
    command: String,
    payload: GraphChangenodeRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemovenodeRequestPayload {
    #[serde(rename = "id")]
    name: String,
    graph: String,
    secret: String,
}

#[derive(Serialize, Debug)]
struct GraphRemovenodeResponse {
    protocol: String,
    command: String,
    payload: GraphRemovenodeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemovenodeResponsePayload {} // TODO should we echo back the graph:removenode message values or is empty graph:removenode OK?

impl Default for GraphRemovenodeResponse {
    fn default() -> Self {
        GraphRemovenodeResponse {
            protocol: String::from("graph"),
            command: String::from("removenode"),
            payload: GraphRemovenodeResponsePayload::default(),
        }
    }
}

impl Default for GraphRemovenodeResponsePayload {
    fn default() -> Self {
        GraphRemovenodeResponsePayload {}
    }
}

// graph:renamenode -> graph:renamenode | graph:error
#[derive(Deserialize, Debug)]
struct GraphRernamenodeRequest {
    protocol: String,
    command: String,
    payload: GraphRenamenodeRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRenamenodeRequestPayload {
    from: String,
    to: String,
    graph: String,
    secret: String,
}

#[derive(Serialize, Debug)]
struct GraphRenamenodeResponse {
    protocol: String,
    command: String,
    payload: GraphRenamenodeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRenamenodeResponsePayload {} // TODO should we echo back the graph:renamenode message values or is empty graph:renamenode OK?

impl Default for GraphRenamenodeResponse {
    fn default() -> Self {
        GraphRenamenodeResponse {
            protocol: String::from("graph"),
            command: String::from("renamenode"),
            payload: GraphRenamenodeResponsePayload::default(),
        }
    }
}

impl Default for GraphRenamenodeResponsePayload {
    fn default() -> Self {
        GraphRenamenodeResponsePayload {}
    }
}

// graph:changenode -> graph:changenode | graph:error
#[derive(Deserialize, Debug)]
struct GraphChangenodeRequest {
    protocol: String,
    command: String,
    payload: GraphChangenodeRequestPayload, //TODO spec: key-value pairs (with some well-known values)
}

#[derive(Deserialize, Debug)]
struct GraphChangenodeRequestPayload {
    #[serde(rename = "id")]
    name: String,
    metadata: GraphChangenodeMetadata,
    graph: String,
    secret: String, // if using a single GraphChangenodeMessage struct, this field would be sent in response message
}

#[derive(Deserialize, Serialize, Debug)]
struct GraphChangenodeMetadata {
    x: i32,
    y: i32,
    height: u32,   // non-specified, but sent by noflo-ui (TODO clarify spec, TODO extend metadata structs to store these)
    width: u32,    // non-specified
    label: String, // non-specified
}

#[derive(Serialize, Debug)]
struct GraphChangenodeResponse {
    protocol: String,
    command: String,
    payload: GraphChangenodeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphChangenodeResponsePayload {
    id: String,
    metadata: GraphChangenodeMetadata,
    graph: String,
}

impl Default for GraphChangenodeResponse {
    fn default() -> Self {
        GraphChangenodeResponse {
            protocol: String::from("graph"),
            command: String::from("changenode"),
            payload: GraphChangenodeResponsePayload::default(),
        }
    }
}

impl Default for GraphChangenodeResponsePayload {
    fn default() -> Self {
        GraphChangenodeResponsePayload {
            id: String::from("Repeater"),
            metadata: GraphChangenodeMetadata::default(),
            graph: String::from("default_graph"),
        }
    }
}

impl Default for GraphChangenodeMetadata {
    fn default() -> Self {
        GraphChangenodeMetadata {
            x: 0,
            y: 0,
            height: 50,
            width: 50,
            label: String::from("Repeater"),
        }
    }
}

// graph:addedge -> graph:addedge | graph:error
#[derive(Deserialize, Debug)]
struct GraphAddedgeRequest {
    protocol: String,
    command: String,
    payload: GraphAddedgeRequestPayload,
}

#[derive(Deserialize, Debug, Clone)]
struct GraphAddedgeRequestPayload {
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    graph: String,
    secret: String, // only present in the request payload
}

#[skip_serializing_none]
//NOTE: PartialEq is for graph.remove_edge() and graph.change_edge()
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
struct GraphNodeSpecNetwork {
    node: String,
    port: String,
    index: Option<String>, // spec: connection index, for addressable ports //TODO spec: string or number -- how to handle in Rust? // NOTE: noflo-ui leaves index away if it is not an indexable port
}

impl Default for GraphNodeSpecNetwork {
    fn default() -> Self {
        GraphNodeSpecNetwork {
            node: String::from("Repeater"),
            port: String::from("IN"),
            index: Some(String::from("1")),
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Deserialize, Serialize, Debug, Clone)]
struct GraphEdgeMetadata {
    route: Option<i32>, //TODO clarify spec: Route identifier of a graph edge
    schema: Option<String>, //TODO clarify spec: JSON schema associated with a graph edge (TODO check schema)
    secure: Option<bool>, //TODO clarify spec: Whether edge data should be treated as secure
}

impl Default for GraphEdgeMetadata {
    fn default() -> Self {
        GraphEdgeMetadata { //TODO clarify spec: totally unsure what these mean or if these are sensible defaults or if better to leave fields undefined if no value
            route: Some(0),
            schema: Some(String::from("")),
            secure: Some(false),
        }
    }
}

impl GraphEdgeMetadata {
    fn new(route: Option<i32>, schema: Option<String>, secure: Option<bool>) -> Self {
        GraphEdgeMetadata {
            route: route,
            schema: schema,
            secure: secure,
        }
    }
}

#[derive(Serialize, Debug)]
struct GraphAddedgeResponse {
    protocol: String,
    command: String,
    payload: GraphAddedgeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphAddedgeResponsePayload {    //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addedge instead of graph:error enough? sending no response -> timeout and sending graph:addedge with empty payload -> error.
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    graph: String,
}   //TODO optimize struct duplication with GraphAddedgeRequestPayload

impl GraphAddedgeResponse {
    fn from_request(payload: GraphAddedgeRequestPayload) -> Self {
        GraphAddedgeResponse {
            protocol: String::from("graph"),
            command: String::from("addedge"),
            payload: GraphAddedgeResponsePayload {
                src: payload.src,
                tgt: payload.tgt,
                metadata: payload.metadata,
                graph: payload.graph,
            },
        }
    }
}

impl Default for GraphAddedgeResponse {
    fn default() -> Self {
        GraphAddedgeResponse {
            protocol: String::from("graph"),
            command: String::from("addedge"),
            payload: GraphAddedgeResponsePayload::default(),
        }
    }
}

impl Default for GraphAddedgeResponsePayload {
    fn default() -> Self {
        GraphAddedgeResponsePayload {
            src: GraphNodeSpecNetwork::default(),
            tgt: GraphNodeSpecNetwork::default(),
            metadata: GraphEdgeMetadata::default(),
            graph: String::from("default_graph"),
        }
    }
}

// graph:removeedge -> graph:removeedge | graph:error
#[derive(Deserialize, Debug)]
struct GraphRemoveedgeRequest {
    protocol: String,
    command: String,
    payload: GraphRemoveedgeRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemoveedgeRequestPayload {
    graph: String, //TODO spec: for graph:addedge the graph attricbute is after src,tgt but for removeedge it is first
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRemoveedgeResponse {
    protocol: String,
    command: String,
    payload: GraphRemoveedgeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemoveedgeResponsePayload { //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addedge instead of graph:error enough? if not sending a response, then comes a timeout. sending empty payload gives an error.
    graph: String, //TODO spec: for graph:addedge the graph attricbute is after src,tgt but for removeedge it is first
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
}   //TODO optimize useless duplication

impl Default for GraphRemoveedgeResponse {
    fn default() -> Self {
        GraphRemoveedgeResponse {
            protocol: String::from("graph"),
            command: String::from("removeedge"),
            payload: GraphRemoveedgeResponsePayload::default(),
        }
    }
}

impl Default for GraphRemoveedgeResponsePayload {
    fn default() -> Self {
        GraphRemoveedgeResponsePayload {
            graph: String::from("default_graph"),
            src: GraphNodeSpecNetwork::default(),
            tgt: GraphNodeSpecNetwork::default(),
        }
    }
}

impl GraphRemoveedgeResponse {
    fn from_request(payload: GraphRemoveedgeRequestPayload) -> Self {
        GraphRemoveedgeResponse {
            protocol: String::from("graph"),
            command: String::from("removeedge"),
            payload: GraphRemoveedgeResponsePayload {
                graph: payload.graph,
                src: payload.src,
                tgt: payload.tgt,
            },
        }
    }
}

// graph:changeedge -> graph:changeedge | graph:error
#[derive(Deserialize, Debug)]
struct GraphChangeedgeRequest {
    protocol: String,
    command: String,
    payload: GraphChangeedgeRequestPayload,
}

#[derive(Deserialize, Debug, Clone)]
struct GraphChangeedgeRequestPayload {
    graph: String,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphChangeedgeResponse {
    protocol: String,
    command: String,
    payload: GraphChangeedgeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphChangeedgeResponsePayload { //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:changeedge instead of graph:error enough? dont know if a response is expected, but sending one with an empty payload gives an error.
    graph: String,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
}   //TODO optimize struct duplication from GraphChangeedgeRequestPayload

impl Default for GraphChangeedgeResponse {
    fn default() -> Self {
        GraphChangeedgeResponse {
            protocol: String::from("graph"),
            command: String::from("changeedge"),
            payload: GraphChangeedgeResponsePayload::default(),
        }
    }
}

impl Default for GraphChangeedgeResponsePayload {
    fn default() -> Self {
        GraphChangeedgeResponsePayload {
            graph: String::from("default_graph"),
            metadata: GraphEdgeMetadata::default(),
            src: GraphNodeSpecNetwork::default(),
            tgt: GraphNodeSpecNetwork::default(),
        }
    }
}

impl GraphChangeedgeResponse {
    fn from_request(payload: GraphChangeedgeRequestPayload) -> Self {
        GraphChangeedgeResponse {
            protocol: String::from("graph"),
            command: String::from("changeedge"),
            payload: GraphChangeedgeResponsePayload {
                graph: payload.graph,
                src: payload.src,
                tgt: payload.tgt,
                metadata: payload.metadata,
            },
        }
    }
}

// graph:addinitial -> graph:addinitial | graph:error
#[derive(Deserialize, Debug)]
struct GraphAddinitialRequest {
    protocol: String,
    command: String,
    payload: GraphAddinitialRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphAddinitialRequestPayload {
    graph: String,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    src: GraphIIPSpecNetwork,   //TODO spec: object,array,string,number,integer,boolean,null. //NOTE: this is is for the IIP structure from the FBP Network protocol, it is different in the FBP Graph spec schema!
    tgt: GraphNodeSpecNetwork,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphAddinitialResponse {
    protocol: String,
    command: String,
    payload: GraphAddinitialResponsePayload,
}

//NOTE: Serialize for graph:addinitial which makes use of the "data" field in graph -> connections -> data according to FBP JSON graph spec.
//NOTE: PartialEq are for graph.remove_initialip()
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct GraphIIPSpecNetwork {
    data: String,   // spec: can put JSON object, array, string, number, integer, boolean, null in there TODO how to handle this in Rust / serde?
}

#[derive(Serialize, Debug)]
struct GraphAddinitialResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addinitial instead of graph:error enough?

impl Default for GraphAddinitialResponse {
    fn default() -> Self {
        GraphAddinitialResponse {
            protocol: String::from("graph"),
            command: String::from("addinitial"),
            payload: GraphAddinitialResponsePayload::default(),
        }
    }
}

impl Default for GraphAddinitialResponsePayload {
    fn default() -> Self {
        GraphAddinitialResponsePayload {}
    }
}

// graph:removeinitial -> graph:removeinitial | graph:error
#[derive(Deserialize, Debug)]
struct GraphRemoveinitialRequest {
    protocol: String,
    command: String,
    payload: GraphRemoveinitialRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemoveinitialRequestPayload {
    graph: String,
    src: GraphIIPSpecNetwork, //TODO spec: object,array,string,number,integer,boolean,null. //NOTE: this is is for the IIP structure from the FBP Network protocol, it is different in the FBP Graph spec schema!
    tgt: GraphNodeSpecNetwork,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRemoveinitialResponse {
    protocol: String,
    command: String,
    payload: GraphRemoveinitialResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemoveinitialResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:removeinitial instead of graph:error enough?

impl Default for GraphRemoveinitialResponse {
    fn default() -> Self {
        GraphRemoveinitialResponse {
            protocol: String::from("graph"),
            command: String::from("removeinitial"),
            payload: GraphRemoveinitialResponsePayload::default(),
        }
    }
}

impl Default for GraphRemoveinitialResponsePayload {
    fn default() -> Self {
        GraphRemoveinitialResponsePayload {}
    }
}

// graph:addinport -> graph:addinport | graph:error
#[derive(Deserialize, Debug)]
struct GraphAddinportRequest {
    protocol: String,
    command: String,
    payload: GraphAddinportRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphAddinportRequestPayload {
    graph: String,
    public: String, // public name of the exported port
    node: String,
    port: String,
    metadata: GraphNodeMetadata, //TODO spec: key-value pairs (with some well-known values)
    secret: String,              // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphAddinportResponse {
    protocol: String,
    command: String,
    payload: GraphAddinportResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphAddinportResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addinport instead of graph:error enough?

impl Default for GraphAddinportResponse {
    fn default() -> Self {
        GraphAddinportResponse {
            protocol: String::from("graph"),
            command: String::from("addinport"),
            payload: GraphAddinportResponsePayload::default(),
        }
    }
}

impl Default for GraphAddinportResponsePayload {
    fn default() -> Self {
        GraphAddinportResponsePayload {}
    }
}

impl GraphAddinportResponse {
    fn new() -> Self {
        GraphAddinportResponse {
            protocol: String::from("graph"),
            command: String::from("addinport"),
            payload: GraphAddinportResponsePayload::default(),  //TODO clarify spec: what values should be sent back?
        }
    }
}

// graph:removeinport -> graph:removeinport | graph:error
#[derive(Deserialize, Debug)]
struct GraphRemoveinportRequest {
    protocol: String,
    command: String,
    payload: GraphRemoveinportRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemoveinportRequestPayload {
    graph: String,
    public: String, // public name of the exported port
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRemoveinportResponse {
    protocol: String,
    command: String,
    payload: GraphRemoveinportResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemoveinportResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:removeinport instead of graph:error enough?

impl Default for GraphRemoveinportResponse {
    fn default() -> Self {
        GraphRemoveinportResponse {
            protocol: String::from("graph"),
            command: String::from("removeinport"),
            payload: GraphRemoveinportResponsePayload::default(),
        }
    }
}

impl Default for GraphRemoveinportResponsePayload {
    fn default() -> Self {
        GraphRemoveinportResponsePayload {}
    }
}

// graph:renameinport -> graph:renameinport | graph:error
#[derive(Deserialize, Debug)]
struct GraphRenameinportRequest {
    protocol: String,
    command: String,
    payload: GraphRenameinportRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRenameinportRequestPayload {
    graph: String,
    from: String,
    to: String,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRenameinportResponse {
    protocol: String,
    command: String,
    payload: GraphRenameinportResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRenameinportResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:renameinport instead of graph:error enough?

impl Default for GraphRenameinportResponse {
    fn default() -> Self {
        GraphRenameinportResponse {
            protocol: String::from("graph"),
            command: String::from("renameinport"),
            payload: GraphRenameinportResponsePayload::default(),
        }
    }
}

impl Default for GraphRenameinportResponsePayload {
    fn default() -> Self {
        GraphRenameinportResponsePayload {}
    }
}

// graph:addoutport -> graph:addoutport | graph:error
#[derive(Deserialize, Debug)]
struct GraphAddoutportRequest {
    protocol: String,
    command: String,
    payload: GraphAddoutportRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphAddoutportRequestPayload {
    graph: String,
    public: String, // public name of the exported port
    node: String,
    port: String,
    metadata: GraphNodeMetadata, //TODO spec: key-value pairs (with some well-known values)
    secret: String,              // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphAddoutportResponse {
    protocol: String,
    command: String,
    payload: GraphAddoutportResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphAddoutportResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addoutport instead of graph:error enough?

impl Default for GraphAddoutportResponse {
    fn default() -> Self {
        GraphAddoutportResponse {
            protocol: String::from("graph"),
            command: String::from("addoutport"),
            payload: GraphAddoutportResponsePayload::default(),
        }
    }
}

impl Default for GraphAddoutportResponsePayload {
    fn default() -> Self {
        GraphAddoutportResponsePayload {}
    }
}

// graph:removeoutport -> graph:removeoutport | graph:error
#[derive(Deserialize, Debug)]
struct GraphRemoveoutportRequest {
    protocol: String,
    command: String,
    payload: GraphRemoveoutportRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemoveoutportRequestPayload {
    graph: String,
    public: String, // public name of the exported port
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRemoveoutportResponse {
    protocol: String,
    command: String,
    payload: GraphRemoveoutportResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemoveoutportResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:removeoutport instead of graph:error enough?

impl Default for GraphRemoveoutportResponse {
    fn default() -> Self {
        GraphRemoveoutportResponse {
            protocol: String::from("graph"),
            command: String::from("removeoutport"),
            payload: GraphRemoveoutportResponsePayload::default(),
        }
    }
}

impl Default for GraphRemoveoutportResponsePayload {
    fn default() -> Self {
        GraphRemoveoutportResponsePayload {}
    }
}

// graph:renameoutport -> graph:renameoutport | graph:error
#[derive(Deserialize, Debug)]
struct GraphRenameoutportRequest {
    protocol: String,
    command: String,
    payload: GraphRenameoutportRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRenameoutportRequestPayload {
    graph: String,
    from: String,
    to: String,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRenameoutportResponse {
    protocol: String,
    command: String,
    payload: GraphRenameoutportResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRenameoutportResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:renameoutport instead of graph:error enough?

impl Default for GraphRenameoutportResponse {
    fn default() -> Self {
        GraphRenameoutportResponse {
            protocol: String::from("graph"),
            command: String::from("renameoutport"),
            payload: GraphRenameoutportResponsePayload::default(),
        }
    }
}

impl Default for GraphRenameoutportResponsePayload {
    fn default() -> Self {
        GraphRenameoutportResponsePayload {}
    }
}

// graph:addgroup -> graph:addgroup | graph:error
#[derive(Deserialize, Debug)]
struct GraphAddgroupRequest {
    protocol: String,
    command: String,
    payload: GraphAddgroupRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphAddgroupRequestPayload {
    name: String,
    graph: String,
    nodes: Vec<String>,           // array of node IDs
    metadata: GraphGroupMetadata, //TODO spec: key-value pairs (with some well-known values)
    secret: String,               // only present in the request payload
}

//NOTE: Serialize trait needed for FBP graph structs, not for the FBP network protocol
#[derive(Deserialize, Serialize, Debug)]
struct GraphGroupMetadata {
    description: String,
}

#[derive(Serialize, Debug)]
struct GraphAddgroupResponse {
    protocol: String,
    command: String,
    payload: GraphAddgroupResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphAddgroupResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addgroup instead of graph:error enough?

impl Default for GraphAddgroupResponse {
    fn default() -> Self {
        GraphAddgroupResponse {
            protocol: String::from("graph"),
            command: String::from("addgroup"),
            payload: GraphAddgroupResponsePayload::default(),
        }
    }
}

impl Default for GraphAddgroupResponsePayload {
    fn default() -> Self {
        GraphAddgroupResponsePayload {}
    }
}

// graph:removegroup -> graph:removegroup | graph:error
#[derive(Deserialize, Debug)]
struct GraphRemovegroupRequest {
    protocol: String,
    command: String,
    payload: GraphRemovegroupRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemovegroupRequestPayload {
    graph: String,
    name: String,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRemovegroupResponse {
    protocol: String,
    command: String,
    payload: GraphRemovegroupResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemovegroupResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:removegroup instead of graph:error enough?

impl Default for GraphRemovegroupResponse {
    fn default() -> Self {
        GraphRemovegroupResponse {
            protocol: String::from("graph"),
            command: String::from("removegroup"),
            payload: GraphRemovegroupResponsePayload::default(),
        }
    }
}

impl Default for GraphRemovegroupResponsePayload {
    fn default() -> Self {
        GraphRemovegroupResponsePayload {}
    }
}

// graph:renamegroup -> graph:renamegroup | graph:error
#[derive(Deserialize, Debug)]
struct GraphRenamegroupRequest {
    protocol: String,
    command: String,
    payload: GraphRenamegroupRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRenamegroupRequestPayload {
    graph: String,
    from: String,
    to: String,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRenamegroupResponse {
    protocol: String,
    command: String,
    payload: GraphRenamegroupResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRenamegroupResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:renamegroup instead of graph:error enough?

impl Default for GraphRenamegroupResponse {
    fn default() -> Self {
        GraphRenamegroupResponse {
            protocol: String::from("graph"),
            command: String::from("renamegroup"),
            payload: GraphRenamegroupResponsePayload::default(),
        }
    }
}

impl Default for GraphRenamegroupResponsePayload {
    fn default() -> Self {
        GraphRenamegroupResponsePayload {}
    }
}

// graph:changegroup -> graph:changegroup | graph:error
#[derive(Deserialize, Debug)]
struct GraphChangegroupRequest {
    protocol: String,
    command: String,
    payload: GraphChangegroupRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphChangegroupRequestPayload {
    graph: String,
    name: String,
    metadata: GraphGroupMetadata,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphChangegroupResponse {
    protocol: String,
    command: String,
    payload: GraphChangegroupResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphChangegroupResponsePayload {} //TODO spec: should we echo the request values for confirmation or is message type graph:changegroup (and no graph:error) enough?

impl Default for GraphChangegroupResponse {
    fn default() -> Self {
        GraphChangegroupResponse {
            protocol: String::from("graph"),
            command: String::from("changegroup"),
            payload: GraphChangegroupResponsePayload::default(),
        }
    }
}

impl Default for GraphChangegroupResponsePayload {
    fn default() -> Self {
        GraphChangegroupResponsePayload {}
    }
}

// ----------
// protocol:trace
// ----------

// spec: This protocol is utilized for triggering and transmitting Flowtraces, see https://github.com/flowbased/flowtrace

// trace:start -> trace:start | trace:error
#[derive(Deserialize, Debug)]
struct TraceStartRequest {
    protocol: String,
    command: String,
    payload: TraceStartRequestPayload,
}

#[derive(Deserialize, Debug)]
struct TraceStartRequestPayload {
    graph: String,
    #[serde(rename = "buffersize")]
    buffer_size: u32, // spec: size of tracing buffer to keep in bytes, TODO unconsistent: no camelCase here
    secret: String,  // only present in the request payload
}

#[derive(Serialize, Debug)]
struct TraceStartResponse {
    protocol: String,
    command: String,
    payload: TraceStartResponsePayload,
}

#[derive(Serialize, Debug)]
struct TraceStartResponsePayload {
    graph: String,
} //TODO clarify spec: should request values be echoed back as confirmation or is message type trace:start instead of trace:error enough?

impl Default for TraceStartResponse {
    fn default() -> Self {
        TraceStartResponse {
            protocol: String::from("trace"),
            command: String::from("start"),
            payload: TraceStartResponsePayload::default(),
        }
    }
}

impl Default for TraceStartResponsePayload {
    fn default() -> Self {
        TraceStartResponsePayload {
            graph: String::from("default_graph")
        }
    }
}

impl TraceStartResponse {
    fn new(graph: String) -> Self {
        TraceStartResponse {
            protocol: String::from("trace"),
            command: String::from("start"),
            payload: TraceStartResponsePayload {
                graph: graph.clone(),
            },
        }
    }
}

// trace:stop -> trace:stop | trace:error
#[derive(Deserialize, Debug)]
struct TraceStopRequest {
    protocol: String,
    command: String,
    payload: TraceStopRequestPayload,
}

#[derive(Deserialize, Debug)]
struct TraceStopRequestPayload {
    graph: String,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct TraceStopResponse {
    protocol: String,
    command: String,
    payload: TraceStopResponsePayload,
}

#[derive(Serialize, Debug)]
struct TraceStopResponsePayload {
    graph: String,
} //TODO clarify spec: should request values be echoed back as confirmation or is message type trace:stop instead of trace:error enough?

impl Default for TraceStopResponse {
    fn default() -> Self {
        TraceStopResponse {
            protocol: String::from("trace"),
            command: String::from("stop"),
            payload: TraceStopResponsePayload::default(),
        }
    }
}

impl Default for TraceStopResponsePayload {
    fn default() -> Self {
        TraceStopResponsePayload {
            graph: String::from("default_graph"),
        }
    }
}

impl TraceStopResponse {
    fn new(graph: String) -> Self {
        TraceStopResponse {
            protocol: String::from("trace"),
            command: String::from("stop"),
            payload: TraceStopResponsePayload {
                graph: graph,
            },
        }
    }
}

// trace:clear -> trace:clear | trace:error
#[derive(Deserialize, Debug)]
struct TraceClearRequest {
    protocol: String,
    command: String,
    payload: TraceClearRequestPayload,
}

#[derive(Deserialize, Debug)]
struct TraceClearRequestPayload {
    graph: String,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct TraceClearResponse {
    protocol: String,
    command: String,
    payload: TraceClearResponsePayload,
}

#[derive(Serialize, Debug)]
struct TraceClearResponsePayload {
    graph: String,
} //TODO clarify spec: should request values be echoed back as confirmation or is message type trace:clear instead of trace:error enough?

impl Default for TraceClearResponse {
    fn default() -> Self {
        TraceClearResponse {
            protocol: String::from("trace"),
            command: String::from("clear"),
            payload: TraceClearResponsePayload::default(),
        }
    }
}

impl Default for TraceClearResponsePayload {
    fn default() -> Self {
        TraceClearResponsePayload {
            graph: String::from("default_graph"),
        }
    }
}

impl TraceClearResponse {
    fn new(graph: String) -> Self {
        TraceClearResponse {
            protocol: String::from("trace"),
            command: String::from("clear"),
            payload: TraceClearResponsePayload {
                graph: graph,
            },
        }
    }
}

// trace:dump -> trace:dump | trace:error
#[derive(Deserialize, Debug)]
struct TraceDumpRequest {
    protocol: String,
    command: String,
    payload: TraceDumpRequestPayload,
}

#[derive(Deserialize, Debug)]
struct TraceDumpRequestPayload {
    graph: String,
    #[serde(rename = "type")] //TODO is this correct?
    typ: String, //TODO spec which types are possible? // spec calls this field "type"
    flowtrace: String, // spec: a Flowtrace file of the type given in attribute "type" -- TODO format defined there:  https://github.com/flowbased/flowtrace
    secret: String,    // only present in the request payload
}

#[derive(Serialize, Debug)]
struct TraceDumpResponse {
    protocol: String,
    command: String,
    payload: TraceDumpResponsePayload,
}

#[derive(Serialize, Debug)]
struct TraceDumpResponsePayload {
    graph: String,
    flowtrace: String,  //TODO any better format than String - a data structure?
} //TODO clarify spec: should request values be echoed back as confirmation or is message type trace:dump instead of trace:error enough?

impl Default for TraceDumpResponse {
    fn default() -> Self {
        TraceDumpResponse {
            protocol: String::from("trace"),
            command: String::from("dump"),
            payload: TraceDumpResponsePayload::default(),
        }
    }
}

impl Default for TraceDumpResponsePayload {
    fn default() -> Self {
        TraceDumpResponsePayload {
            graph: String::from("default_graph"),
            flowtrace: String::from(""),    //TODO clarify spec: how to indicate an empty tracefile? Does it need "[]" or "{}" at least?
        }
    }
}

impl TraceDumpResponse {
    fn new(graph: String, dump: String) -> Self {
        TraceDumpResponse {
            protocol: String::from("trace"),
            command: String::from("dump"),
            payload: TraceDumpResponsePayload {
                graph: graph,
                flowtrace: dump,
            },
        }
    }
}

// trace:error response
//TODO spec if this does not require any capabilities for this then move up into "base" section
#[derive(Serialize, Debug)]
struct TraceErrorResponse {
    protocol: String,
    command: String,
    payload: TraceErrorResponsePayload,
}

#[derive(Serialize, Debug)]
struct TraceErrorResponsePayload {
    message: String,
}

impl Default for TraceErrorResponse {
    fn default() -> Self {
        TraceErrorResponse {
            protocol: String::from("trace"),
            command: String::from("error"),
            payload: TraceErrorResponsePayload::default(),
        }
    }
}

impl Default for TraceErrorResponsePayload {
    fn default() -> Self {
        TraceErrorResponsePayload {
            message: String::from("default trace error message"),
        }
    }
}

impl TraceErrorResponse {
    fn new(err: String) -> Self {
        TraceErrorResponse {
            protocol: String::from("trace"),
            command: String::from("error"),
            payload: TraceErrorResponsePayload {
                message: err,
            },
        }
    }
}

// ----------
// graph structs for FBP network protocol and FBP graph import/export
// ----------

//TODO how to serialize/deserialize as object/hashtable in JSON, but use Vec internally? TODO performance tests Vec <-> HashMap.
//TODO optimize what is faster for a few entries: Hashmap or Vec @ https://www.reddit.com/r/rust/comments/7mqwjn/hashmapstringt_vs_vecstringt/
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")] // spec: for example the field "caseSensitive"
struct Graph {
    case_sensitive: bool, // always true for flowd TODO optimize
    properties: GraphProperties,
    inports: HashMap<String, GraphPort>, // spec: object/hashmap. TODO will not be accessed concurrently - to be used inside Arc<RwLock<>>
    outports: HashMap<String, GraphPort>, // spec: object/hashmap. TODO will not be accessed concurrently - to be used inside Arc<RwLock<>>
    groups: Vec<GraphGroup>, // TODO for internal representation this should be a hashmap, but if there are few groups a vec might be ok
    #[serde(rename = "processes")]
    nodes: HashMap<String, GraphNode>,
    #[serde(rename = "connections")]
    edges: Vec<GraphEdge>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GraphProperties {
    name: String,
    environment: GraphPropertiesEnvironment,
    description: String,
    icon: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct GraphPropertiesEnvironment {
    #[serde(rename = "type")]
    typ: String,
    content: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct GraphPort {
    process: String,
    port: String,
    //index: u32, //TODO clarify spec: does not exist in spec here, but for the connections it exists
    metadata: GraphPortMetadata,
}

#[derive(Serialize, Deserialize, Debug)]
struct GraphPortMetadata {
    x: i32,
    y: i32,
}

#[derive(Serialize, Deserialize, Debug)]
struct GraphGroup {
    name: String,
    nodes: Vec<String>, // spec: node IDs (.name)
    metadata: GraphGroupMetadata,
}

#[derive(Serialize, Deserialize, Debug)]
struct GraphNode {
    component: String,
    metadata: GraphNodeMetadata,
}

#[serde_with::skip_serializing_none]    // noflo-ui interprets even "data": null as "this is an IIP". not good but we can disable serializing None //TODO make issue in noflo-ui
#[derive(Serialize, Deserialize, Debug)]
struct GraphEdge {
    #[serde(rename = "src")]
    source: GraphNodeSpec,
    //TODO enable sending of object/hashmap IIPs also, currently allows only string
    data: Option<String>,  // spec: inconsistency between Graph spec schema and Network Protocol spec! Graph: data outside here, but Network protocol says "data" is field inside src and remaining fields are removed.
    #[serde(rename = "tgt")]
    target: GraphNodeSpec,
    metadata: GraphEdgeMetadata,
}

#[serde_with::skip_serializing_none]    // do not serialize index if it is None
#[derive(Deserialize, Serialize, Debug)]
struct GraphNodeSpec {
    process: String,
    port: String,
    index: Option<String>, // spec: connection index, for addressable ports //TODO spec: string or number -- how to handle in Rust? // NOTE: noflo-ui leaves index away if it is not an indexable port
}

impl PartialEq<GraphNodeSpecNetwork> for GraphNodeSpec {
    fn eq(&self, other: &GraphNodeSpecNetwork) -> bool {
        self.process == other.node && self.port == other.port && self.index == other.index
    }
}

impl Graph {
    fn new(name: String, description: String, icon: String) -> Self {
        Graph {
            case_sensitive: true, //TODO always true - optimize
            properties: GraphProperties {
                name: name,
                environment: GraphPropertiesEnvironment::default(),
                description: description,
                icon: icon,
            },
            inports: HashMap::new(),
            outports: HashMap::new(),
            groups: vec!(),
            nodes: HashMap::new(),
            edges: vec!(),
        }
    }

    //TODO very lossy conversion
    fn ports_as_componentportsarray(&self, inports_or_outports: &HashMap<String,GraphPort>) -> Vec<ComponentPort> {
        let mut out = Vec::with_capacity(inports_or_outports.len());
        for (name, info) in inports_or_outports.iter() {
            out.push(ComponentPort {
                name: name.clone(),
                allowed_type: String::from(""), //TODO clarify spec: not available from FBP JSON Graph port TODO what happens if we return a empty allowed type (because we dont know from Graph inport)
                schema: None, //TODO clarify spec: not available from FBP JSON Graph port
                required: true, //TODO clarify spec: not available from FBP JSON Graph port
                is_arrayport: false, //TODO clarify spec: not available from FBP JSON Graph port
                description: String::from(""), //TODO clarify spec: not available from FBP JSON Graph port
                values_allowed: vec!(), //TODO clarify spec: not available from FBP JSON Graph port
                value_default: String::from(""), //TODO clarify spec: not available from FBP JSON Graph port
                //TODO clarify spec: cannot return Graph inport fields process, port, metadata (x,y)
            });
        }
        return out;
    }

    fn clear(&mut self, payload: &GraphClearRequestPayload, runtime: &RuntimeRuntimePayload) -> Result<(), std::io::Error> {
        if runtime.status.running {
            // not allowed at the moment (TODO), theoretically graph and network could be different and the graph could be modified while the network is still running in the old config, then stop network and immediately start the network again according to the new graph structure, having only short downtime.
            return Err(std::io::Error::new(std::io::ErrorKind::ResourceBusy, String::from("network still running")));
            //TODO ^ requires the feature "io_error_more", is this OK or risky, or bloated?
        }
        if payload.name != runtime.graph {
            // multiple graphs currently not supported
            //TODO implement
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("wrong graph addressed, currently one graph supported")));
        }
        //TODO implement some semantics like fields "library", "main" and subgraph feature - also need multiple graph support
        // actually clear
        self.groups.clear();
        self.edges.clear();
        self.inports.clear();
        self.outports.clear();
        self.nodes.clear();
        Ok(())
    }

    fn add_inport(&mut self, name: String, portdef: GraphPort) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should adding an inport be allowed?
        match self.inports.try_insert(name, portdef) {
            Ok(_) => {
                return Ok(());
            },
            Err(_) => {
                //TODO we could pass on the std::collections::hash_map::OccupiedError
                return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, String::from("inport with that name already exists")));
            },
        }
    }

    fn add_outport(&mut self, name: String, portdef: GraphPort) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should adding an outport be allowed?
        match self.outports.try_insert(name, portdef) {
            Ok(_) => {
                return Ok(());
            },
            Err(_) => {
                //TODO we could pass on the std::collections::hash_map::OccupiedError
                return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, String::from("outport with that name already exists")));
            },
        }
    }

    fn remove_inport(&mut self, name: String) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should removing an inport be allowed?
        match self.inports.remove(&name) {
            Some(_) => {
                return Ok(());
            },
            None => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("inport not found")));
            },
        }
    }

    fn remove_outport(&mut self, name: String) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should removing an outport be allowed?
        match self.outports.remove(&name) {
            Some(_) => {
                return Ok(());
            },
            None => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("outport not found")));
            },
        }
    }

    fn rename_inport(&mut self, old: String, new: String) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should manipulating inports be allowed?

        //TODO optimize: both variants work, which is faster?
        /*
        method 1:
        get = contains
        get+return = remove(incl. get)
        insert

        method 2:
        insert(get)
        get+return = remove
        */
        if self.inports.contains_key(&new) {
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, String::from("inport already exists")));
        }
        match self.inports.remove(&old) {
            Some(v) => {
                self.inports.try_insert(new, v).expect("wtf key occupied on insertion");    // should not happen
                return Ok(());
            },
            None => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("inport not found")));
            }
        }


        //NOTE: below does not work because of "shared reference"
        /*
        // get current value
        let val: GraphPort;
        if let Some(v) = self.inports.get(old) {
            val = *v;
        } else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("inport not found")));
        }

        // insert under new key
        match self.inports.try_insert(new, val) {
            Ok(_) => {
                // remove old key
                self.inports.remove(old).expect("should not happen: could not remove old entry during rename");
                return Ok(());
            },
            Err(err) => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("new key already exists")));
            }
        }
        */
    }

    fn rename_outport(&mut self, old: String, new: String) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should manipulating outports be allowed?

        //TODO optimize: which is faster? see above.
        if self.outports.contains_key(&new) {
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, String::from("outport already exists")));
        }
        match self.outports.remove(&old) {
            Some(v) => {
                self.outports.try_insert(new, v).expect("wtf key occupied on insertion");    // should not happen
                return Ok(());
            },
            None => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("outport not found")));
            }
        }
    }

    fn add_node(&mut self, graph: String, component: String, name: String, metadata: GraphNodeMetadata) -> Result<GraphAddnodeResponsePayload, std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change the nodeset?
        //TODO check graph name and state, multi-graph support

        // TODO check if that node already exists
        // TODO check if that component exists

        let nodedef: GraphNode;
        // TODO check spec: check for noflo-ui behavior sending missing data
        if metadata.label.is_some() && !metadata.label.as_ref().unwrap().eq(&name) && metadata.width.is_none() && metadata.height.is_none() {
            debug!("add_node(): doing noflo-ui data fixup");
            nodedef = GraphNode {
                component: component,
                metadata: GraphNodeMetadata {
                    x: metadata.x,
                    y: metadata.y,
                    width: Some(NODE_WIDTH_DEFAULT),
                    height: Some(NODE_HEIGHT_DEFAULT),
                    label: metadata.label.clone(),  // should be Some(name.clone()) but noflo-ui does not display it right away
                }
            };
        } else {
            // normal client
            nodedef = GraphNode {
                component: component,
                metadata: metadata,
            };
        }
        //TODO optimize - constructing here because try_insert() moves the value
        let ret = GraphAddnodeResponsePayload {
            name: name.clone(),
            component: nodedef.component.clone(),
            metadata: nodedef.metadata.clone(),
            graph: graph.clone(),
        };

        // insert and return
        match self.nodes.try_insert(name, nodedef) {
            Ok(_) => {
                return Ok(ret);
            },
            Err(_) => {
                //TODO we could pass on the std::collections::hash_map::OccupiedError
                return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, String::from("node with that name already exists")));
            },
        }
    }

    fn remove_node(&mut self, graph: String, name: String) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should removing a node be allowed?
        //TODO check graph name, multi-graph support
        match self.nodes.remove(&name) {
            Some(_) => {
                return Ok(());
            },
            None => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("node not found")));
            },
        }
    }

    fn rename_node(&mut self, graph: String, old: String, new: String) -> Result<(), std::io::Error> {
        //TODO in which state should manipulating nodes be allowed?
        //TODO check graph name and state, multi-graph support

        // check if new name already exists
        if self.nodes.contains_key(&new) {
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, String::from("node with that name already exists")));
        }
        // remove, then re-insert (there is no rename in HashMap)
        match self.nodes.remove(&old) {
            Some(v) => {
                self.nodes.try_insert(new.clone(), v).expect("wtf key occupied on insertion");    // should not happen

                // rename in edges
                for edge in self.edges.iter_mut() {
                    if edge.source.process == old {
                        edge.source.process = new.clone();
                    }
                    if edge.target.process == old {
                        edge.target.process = new.clone();
                    }
                };

                return Ok(());
            },
            None => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("wtf node not found")));  // should not happen either
            }
        }
    }

    fn change_node(&mut self, graph: String, name: String, metadata: GraphChangenodeMetadata) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should manipulating nodes be allowed?
        //TODO check graph name and state, multi-graph support

        //TODO currently we discard additional fields! -> issue #188
        //TODO clarify spec: should the whole metadata hashmap be replaced (but then how to delete metadata entries?) or should only the given fields be overwritten?
        if let Some(node) = self.nodes.get_mut(&name) { //TODO optimize: borrowing a String here
            node.metadata.x = metadata.x;
            node.metadata.y = metadata.y;
            // known additional fields from noflo-ui
            node.metadata.width = Some(metadata.width);
            node.metadata.height = Some(metadata.height);
            return Ok(());
        } else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("node by that name not found")));
        }
    }

    fn add_edge(&mut self, graph: String, edge: GraphEdge) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change the edgeset?
        //TODO check graph name and state, multi-graph support

        //TODO check if that edge already exists! There is a dedup(), helpful?
        //TODO check for OOM by extending first?
        self.edges.push(edge);
        //TODO optimize: if it cannot fail, then no need for returning Result
        Ok(())
    }

    fn remove_edge(&mut self, graph: String, source: GraphNodeSpecNetwork, target: GraphNodeSpecNetwork) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change the edgeset?
        //TODO check graph name and state, multi-graph support

        // find correct index and remove
        for (i, edge) in self.edges.iter().enumerate() {
            if edge.source == source && edge.target == target {
                self.edges.remove(i);
                return Ok(());
            }
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("edge with that src+tgt not found")));
    }

    fn change_edge(&mut self, graph: String, source: GraphNodeSpecNetwork, target: GraphNodeSpecNetwork, metadata: GraphEdgeMetadata) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change the edgeset?
        //TODO check graph name and state, multi-graph support

        // find correct index and set metadata
        //TODO clarify spec: should the whole metadata hashmap be replaced (but then how to delete metadata entries?) or should only the given fields be overwritten?
        for (i, edge) in self.edges.iter().enumerate() {
            if edge.source == source && edge.target == target {
                //TODO optimize, maybe direct assignment without [i] is possible
                self.edges[i].metadata = metadata;
                return Ok(());
            }
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("edge with that src+tgt not found")));
    }

    // spec: According to the FBP JSON graph format spec, initial IPs are declared as a special-case of a graph edge in the "connections" part of the graph.
    fn add_initialip(&mut self, payload: GraphAddinitialRequestPayload) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change initial IPs (which are similar to edges)?
        //TODO check graph name and state, multi-graph support

        //TODO check for OOM by extending first?
        self.edges.push(GraphEdge::from(payload));
        //TODO optimize: if it cannot fail, then no need for returning Result
        Ok(())
    }

    fn remove_initialip(&mut self, payload: GraphRemoveinitialRequestPayload) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed to change initial IPs (which are similar to edges)?
        //TODO check graph name and state, multi-graph support

        //TODO clarify spec: should we remove first or last match? currently removing first match
        //NOTE: if finding and removing last match first, therefore using self.edges.iter().rev().enumerate(), then rev() reverses the order, but enumerate's index of 0 is the last element!
        for (i, edge) in self.edges.iter().enumerate() {
            //TODO optimize: match for IIP match first or for target match? Target has more values to compare, but there may be more IIPs than target matches and IIPs might be longer thus more expensive to compare...
            //TODO optimize the clone here and the GraphIIPSpec
            // check for IIP
            if let Some(iipdata) = &edge.data {
                // IIP data must be the same
                // for rev() iteration
                //info!("index {}: comparing iipdata {} == {} ?", self.edges.len()-1-i, iipdata, payload.src.data);
                // for normal, non-rev() iteration
                //info!("index {}: comparing iipdata {} == {} ?", i, iipdata, payload.src.data);
                if iipdata.as_bytes() == payload.src.data.as_bytes() {  //TODO optimize, that is supposed to be a string comparison, but .as_str() = .as_str() did not work
                    //info!("yes");
                    // target must match
                    if edge.target == payload.tgt {
                        // for rev() iteration
                        //self.edges.remove(self.edges.len()-1-i);
                        // for normal non-rev() iteration
                        self.edges.remove(i);
                        return Ok(());
                    }
                }
            }
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("edge with that data+tgt not found")));
    }

    fn add_group(&mut self, graph: String, name: String, nodes: Vec<String>, metadata: GraphGroupMetadata) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed to change node groups?
        //TODO check graph name and state, multi-graph support
        //TODO check nodes if they actually exist; check for duplicates; and node can only be part of a single group
        //TODO check for OOm by extending first?
        self.groups.push(GraphGroup { name: name, nodes: nodes, metadata: metadata });
        //TODO optimize: if it cannot fail then no need to return Result
        Ok(())
    }

    fn remove_group(&mut self, graph: String, name: String) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change node groups?
        //TODO check graph name and state, multi-graph support

        // find correct index and remove
        for (i, group) in self.groups.iter().enumerate() {
            if group.name == name {
                self.groups.remove(i);
                return Ok(());
            }
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("group with that name not found")));
    }

    fn rename_group(&mut self, graph: String, old: String, new: String) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change node groups?
        //TODO check graph name and state, multi-graph support

        // find correct index and rename
        for (i, group) in self.groups.iter().enumerate() {
            if group.name == old {
                //TODO is it possible to do directly:  group.name = new  ?
                self.groups[i].name = new;
                return Ok(());
            }
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("group with that name not found")));
    }

    fn change_group(&mut self, graph: String, name: String, metadata: GraphGroupMetadata) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change the groups?
        //TODO check graph name and state, multi-graph support

        // find correct index and set metadata
        //TODO clarify spec: should the whole metadata hashmap be replaced (but then how to delete metadata entries?) or should only the given fields be overwritten?
        for (i, group) in self.groups.iter().enumerate() {
            if group.name == name {
                //TODO optimize, maybe direct assignment without [i] is possible
                self.groups[i].metadata = metadata;
                return Ok(());
            }
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("group with that name not found")));
    }

    fn get_source(&mut self, name: String) -> Result<ComponentSourcePayload, std::io::Error> {
        //TODO optimize: the message handler has already checked the graph name outside
        if name == self.properties.name {
            return Ok(ComponentSourcePayload {
                name: name,
                language: String::from("json"), //TODO clarify spec: what to set here?
                library: String::from(""), //TODO clarify spec: what to set here?
                code: serde_json::to_string(self).expect("failed to serialize graph"),
                tests: String::from("// tests for graph here"), //TODO clarify spec: what to set here?
            });
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("graph with that name not found")));
    }
}

impl Default for GraphPropertiesEnvironment {
    fn default() -> Self {
        GraphPropertiesEnvironment {
            typ: String::from("flowd"), //TODO constant value - optimize
            content: String::from(""), //TODO always empty for flowd - optimize
        }
    }
}

//TODO optimize, graph:addinport request payload is very similar to GraphPort -> possible re-use without conversion?
impl From<GraphAddinportRequestPayload> for GraphPort {
    fn from(payload: GraphAddinportRequestPayload) -> Self {
        GraphPort { //TODO optimize structure very much the same -> use one for both?
            process: payload.node,
            port: payload.port,
            metadata: GraphPortMetadata {   //TODO optimize: GraphPortMetadata and GraphNodeMetadata are structurally the same
                x: payload.metadata.x,
                y: payload.metadata.y,
            }
        }
    }
}

//TODO optimize, graph:addoutport is also very similar
impl From<GraphAddoutportRequestPayload> for GraphPort {
    fn from(payload: GraphAddoutportRequestPayload) -> Self {
        GraphPort { //TODO optimize structure very much the same -> use one for both?
            process: payload.node,
            port: payload.port,
            metadata: GraphPortMetadata {   //TODO optimize: GraphPortMetadata and GraphNodeMetadata are structurally the same
                x: payload.metadata.x,
                y: payload.metadata.y,
            }
        }
    }
}

//TODO optimize, make the useless -- actually only the graph field is too much -> filter that out by serde?
impl From<GraphAddedgeRequestPayload> for GraphEdge {
    fn from(payload: GraphAddedgeRequestPayload) -> Self {
        GraphEdge {
            source: GraphNodeSpec::from(payload.src),
            target: GraphNodeSpec::from(payload.tgt),
            data: None,
            metadata: payload.metadata,
        }
    }
}

// spec: IIPs are special cases of a graph connection/edge
impl<'a> From<GraphAddinitialRequestPayload> for GraphEdge {
    fn from(payload: GraphAddinitialRequestPayload) -> Self {
        GraphEdge {
            source: GraphNodeSpec { //TODO clarify spec: what to set as "src" if it is an IIP?
                process: String::from(""),
                port: String::from(""),
                index: Some(String::from("")),  //TODO clarify spec: what to save here when noflo-ui does not send this field?
            },
            data: if payload.src.data.len() > 0 { Some(payload.src.data) } else { None },   //NOTE: there is an inconsistency between FBP network protocol and FBP graph schema
            target: GraphNodeSpec::from(payload.tgt),
            metadata: payload.metadata, //TODO defaults may be unsensible -> clarify spec
        }
    }
}

impl From<GraphNodeSpecNetwork> for GraphNodeSpec {
    fn from(nodespec_network: GraphNodeSpecNetwork) -> Self {
        GraphNodeSpec {
            process: nodespec_network.node,
            port: nodespec_network.port,
            index: nodespec_network.index,
        }
    }
}

// ----------
// processes
// ----------

struct Process {
    signal: ProcessSignalSink,    // signalling channel, uses mpsc channel which is lower performance but shareable and ok for signalling
    joinhandle: std::thread::JoinHandle<()>,    // for waiting for thread exit TODO what is its generic parameter?
    //TODO detect process exit
}

type ProcessManager = HashMap<String, Process>;

impl std::fmt::Debug for Process {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Process").field("signal", &self.signal).field("joinhandle", &self.joinhandle).finish()
    }
}

// ----------
// component library
// ----------

//TODO cannot think of a better name ATM, see https://stackoverflow.com/questions/1866794/naming-classes-how-to-avoid-calling-everything-a-whatevermanager
//TODO implement some functionality
#[derive(Default)]
struct ComponentLibrary {
    available: Vec<ComponentComponentPayload>,
}

impl ComponentLibrary {
    fn new(available: Vec<ComponentComponentPayload>) -> Self {
        ComponentLibrary {
            available: available,
        }
    }

    fn get_source(& self, name: String) -> Result<ComponentSourcePayload, std::io::Error> {
        // spec: Name of the component to for which to get source code.
        // spec: Should contain the library prefix, example: "my-project/SomeComponent"
        //TODO how to re-compile Rust components? how to meaningfully debug from the web? would need compiler output.
        //NOTE: components used as nodes in the current graph may be a different set than those that the component libary has available!
        //TODO optimize: access HashMap by &String or name.as_str()?
        //TODO optimize: for component:getsource we need to return an array, but for internal purpose a HashMap would be much more efficient
        //if let Some(node) = self.available.get(&name) {
        //TODO there is vec.binary_search() and vec.sort_by_key() - maybe as fast as hashmap?
        for component in self.available.iter() {
            if component.name == name {
                return Ok(ComponentSourcePayload {
                    //TODO implement
                    name: name,
                    language: String::from(""), //TODO implement - get real info
                    library: String::from(""),  //TODO implement - get real info
                    code: String::from("// code for component here"),   //TODO implement - get real info
                    tests: String::from("// tests for component here"), //TODO where to get the tests from? in what format?
                });
            }
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("component not found")));
    }

    //TODO currently all done in runtime.start()
    fn new_component(name: String) -> Result<Box<dyn Component>, std::io::Error> {
        //TODO implement - ports are currently totally unconnected
        let inports = ProcessInports::new();
        let outports = ProcessOutports::new();
        let (signal_sink, signal_source) = std::sync::mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);
        // TODO add dynamically-loaded components as well
        match name.as_str() {
            "Repeat" => {
                return Ok(Box::new(RepeatComponent::new(  //TODO implement - parameters are fake
                    inports,
                    outports,
                    signal_source,
                    signal_sink,
                    Arc::new(Mutex::new(GraphInportOutportHolder{  //TODO implement - parameters are fake
                        inports: None,
                        outports: None,
                        websockets: HashMap::new(),
                    })),
                )));
            },
            _ => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("component not found")));
            }
        }
    }
}

// ----------
// components
// ----------

type ProcessInports = MultiMap<String, ProcessEdgeSource>;
type ProcessOutports = MultiMap<String, ProcessEdgeSink>;
type ProcessEdge = rtrb::RingBuffer<MessageBuf>;
type ProcessEdgeSource = rtrb::Consumer<MessageBuf>;
#[derive(Debug)]
struct ProcessEdgeSink {
    sink: rtrb::Producer<MessageBuf>,
    wakeup: Option<WakeupNotify>,
    proc_name: Option<String>,
}
type WakeupNotify = Thread;
type ProcessSignalSource = std::sync::mpsc::Receiver<MessageBuf>;   // only one allowed (single consumer)
type ProcessSignalSink = std::sync::mpsc::SyncSender<MessageBuf>;   // Sender can be cloned (multiple producers) but SyncSender is even more convenient as it implements Sync and no deep clone() on the Sender is neccessary
/*
NOTE: Vec<u8> is growable; Box<[u8]> is decidedly not growable, which just brings limitations for forwarding IPs.
Vec is just 1 machine word larger than Box. There is more convenience API and From implementations for Vec.
In the wild, there also seems less use of Box<[u8]>.

NOTE: Changing to [u8] here and then having &[u8] in ProcessEdges creates an avalanche of lifetime problems where something does not live long enough, 
maybe there are some possibilities to state "this lifetime is equal to that one" but problem is that we are actually handing over 
data from thread A to thread B, so we are not allowing thread B to have a temporary look at the data, but it is actual message passing.
And there is no "master" who owns the data - then we could give threads A and B pointers and borrows into that data, but that is not the case.
*/
type MessageBuf = Vec<u8>;
const PROCESSEDGE_BUFSIZE: usize = 7*7*7*7;
const PROCESSEDGE_SIGNAL_BUFSIZE: usize = 2;
const PROCESSEDGE_IIP_BUFSIZE: usize = 1;

trait Component {
    fn new(inports: ProcessInports, outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized;
    fn run(self);   //NOTE: consume self because this method is not expected to return, and we can hand over data from self to sub-threads (lifetime of &self issue)
    fn get_metadata() -> ComponentComponentPayload where Self:Sized;
}