#![feature(duration_constants)]

use std::net::{TcpListener, TcpStream};
use std::thread::spawn;
use std::time::Duration;

use tungstenite::handshake::server::{Request, Response};
use tungstenite::handshake::HandshakeRole;
use tungstenite::{accept_hdr, Error, HandshakeError, Message, Result};

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

use serde::{Deserialize, Serialize};

fn must_not_block<Role: HandshakeRole>(err: HandshakeError<Role>) -> Error {
    match err {
        HandshakeError::Interrupted(_) => panic!("Bug: blocking socket would block"),
        HandshakeError::Failure(f) => f,
    }
}

fn handle_client(stream: TcpStream) -> Result<()> {
    stream
        .set_write_timeout(Some(Duration::SECOND))
        .expect("set_write_timeout call failed");
    //stream.set_nodelay(true).expect("set_nodelay call failed");

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

    info!("entering receive loop");
    loop {
        info!("waiting for next message");
        match websocket.read_message()? {
            msg @ Message::Text(_) | msg @ Message::Binary(_) => {
                info!("got a text|binary message");
                //debug!("message data: {}", msg.clone().into_text().unwrap());

                let fbpmsg: FBPMessage = serde_json::from_slice(msg.into_data().as_slice())
                    .expect("failed to decode JSON message"); //TODO data handover optimizable?
                                                              //TODO handle panic because of decoding error here

                match fbpmsg {
                    FBPMessage::RuntimeGetruntimeMessage(payload) => {
                        info!(
                            "got runtime:getruntime message with secret {}",
                            payload.secret
                        );
                        // send response = runtime:runtime message
                        info!("response: sending runtime:runtime message");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&RuntimeRuntimeMessage::default())
                                    .expect("failed to serialize runtime:runtime message"),
                            ))
                            .expect("failed to write message into websocket");
                        // (specification) "If the runtime is currently running a graph and it is able to speak the full Runtime protocol, it should follow up with a ports message."
                        info!("response: sending runtime:ports message");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&RuntimePortsMessage::default())
                                    .expect("failed to serialize runtime:ports message"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::ComponentListMessage(_payload) => {
                        info!("got component:list message");
                        info!("response: sending component:component message");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&ComponentComponentMessage::default())
                                    .expect("failed to serialize component:component message"),
                            ))
                            .expect("failed to write message into websocket");
                        info!("response: sending component:componentsready message");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&ComponentComponentsreadyMessage::default())
                                    .expect(
                                        "failed to serialize component:componentsready message",
                                    ),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::NetworkGetstatusMessage(_payload) => {
                        info!("got network:getstatus message");
                        info!("response: sending network:status message");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&NetworkStatusMessage::default())
                                    .expect("failed to serialize network:status message"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::NetworkPersistRequest(_payload) => {
                        info!("got network:persist message");
                        info!("response: sending network:persist message");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&NetworkPersistResponse::default())
                                    .expect("failed to serialize network:persist message"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::ComponentGetsourceMessage(payload) => {
                        info!("got component:getsource message");
                        if payload.name == "default_graph" {
                            info!("response: sending component:source message for graph");
                            websocket
                                .write_message(Message::text(
                                    serde_json::to_string(&ComponentSourceMessage::default_graph())
                                        .expect("failed to serialize component:source message"),
                                ))
                                .expect("failed to write message into websocket");
                        } else {
                            info!("response: sending component:source message for component");
                            websocket
                                .write_message(Message::text(
                                    serde_json::to_string(&ComponentSourceMessage::default())
                                        .expect("failed to serialize component:source message"),
                                ))
                                .expect("failed to write message into websocket");
                        }
                    }

                    FBPMessage::GraphClearRequest(_payload) => {
                        info!("got graph:clear message");
                        info!("response: sending graph:clear response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphClearResponse::default())
                                    .expect("failed to serialize graph:clear response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphAddnodeRequest(_payload) => {
                        info!("got graph:addnode message");
                        info!("response: sending graph:addnode response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphAddnodeResponse::default())
                                    .expect("failed to serialize graph:addnode response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRenamenodeRequest(_payload) => {
                        info!("got graph:renamenode message");
                        info!("response: sending graph:renamenode response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRenamenodeResponse::default())
                                    .expect("failed to serialize graph:renamenode response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphChangenodeRequest(_payload) => {
                        info!("got graph:changenode message");
                        info!("response: sending graph:changenode response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphChangenodeResponse::default())
                                    .expect("failed to serialize graph:changenode response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRemovenodeRequest(_payload) => {
                        info!("got graph:removenode message");
                        info!("response: sending graph:removenode response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRemovenodeResponse::default())
                                    .expect("failed to serialize graph:removenode response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphAddedgeRequest(_payload) => {
                        info!("got graph:addedge message");
                        info!("response: sending graph:addedge response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphAddedgeResponse::default())
                                    .expect("failed to serialize graph:addedge response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRemoveedgeRequest(_payload) => {
                        info!("got graph:removeedge message");
                        info!("response: sending graph:removeedge response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRemoveedgeResponse::default())
                                    .expect("failed to serialize graph:removeedge response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphChangeedgeRequest(_payload) => {
                        info!("got graph:changeedge message");
                        info!("response: sending graph:changeedge response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphChangeedgeResponse::default())
                                    .expect("failed to serialize graph:changeedge response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphAddinitialRequest(_payload) => {
                        info!("got graph:addinitial message");
                        info!("response: sending graph:addinitial response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphAddinitialResponse::default())
                                    .expect("failed to serialize graph:addinitial response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRemoveinitialRequest(_payload) => {
                        info!("got graph:removeinitial message");
                        info!("response: sending graph:removeinitial response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRemoveinitialResponse::default())
                                    .expect("failed to serialize graph:removeinitial response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphAddinportRequest(_payload) => {
                        info!("got graph:addinport message");
                        info!("response: sending graph:addinport response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphAddinportResponse::default())
                                    .expect("failed to serialize graph:addinport response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRemoveinportRequest(_payload) => {
                        info!("got graph:removeinport message");
                        info!("response: sending graph:removeinport response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRemoveinportResponse::default())
                                    .expect("failed to serialize graph:removeinport response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRenameinportRequest(_payload) => {
                        info!("got graph:renameinport message");
                        info!("response: sending graph:renameinport response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRenameinportResponse::default())
                                    .expect("failed to serialize graph:renameinport response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphAddoutportRequest(_payload) => {
                        info!("got graph:addoutport message");
                        info!("response: sending graph:addoutport response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphAddoutportResponse::default())
                                    .expect("failed to serialize graph:addoutport response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRemoveoutportRequest(_payload) => {
                        info!("got graph:removeoutport message");
                        info!("response: sending graph:removeoutport response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRemoveoutportResponse::default())
                                    .expect("failed to serialize graph:removeoutport response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRenameoutportRequest(_payload) => {
                        info!("got graph:renameoutport message");
                        info!("response: sending graph:renameoutport response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRenameoutportResponse::default())
                                    .expect("failed to serialize graph:renameoutport response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphAddgroupRequest(_payload) => {
                        info!("got graph:addgroup message");
                        info!("response: sending graph:addgroup response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphAddgroupResponse::default())
                                    .expect("failed to serialize graph:addgroup response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRemovegroupRequest(_payload) => {
                        info!("got graph:removegroup message");
                        info!("response: sending graph:removegroup response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRemovegroupResponse::default())
                                    .expect("failed to serialize graph:removegroup response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphRenamegroupRequest(_payload) => {
                        info!("got graph:renamegroup message");
                        info!("response: sending graph:renamegroup response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphRenamegroupResponse::default())
                                    .expect("failed to serialize graph:renamegroup response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::GraphChangegroupRequest(_payload) => {
                        info!("got graph:changegroup message");
                        info!("response: sending graph:changegroup response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&GraphChangegroupResponse::default())
                                    .expect("failed to serialize graph:changegroup response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    // protocol:trace
                    FBPMessage::TraceStartRequest(_payload) => {
                        info!("got trace:start message");
                        info!("response: sending trace:start response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&TraceStartResponse::default())
                                    .expect("failed to serialize trace:start response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::TraceStopRequest(_payload) => {
                        info!("got trace:stop message");
                        info!("response: sending trace:stop response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&TraceStopResponse::default())
                                    .expect("failed to serialize trace:stop response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::TraceClearRequest(_payload) => {
                        info!("got trace:clear message");
                        info!("response: sending trace:clear response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&TraceClearResponse::default())
                                    .expect("failed to serialize trace:clear response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::TraceDumpRequest(_payload) => {
                        info!("got trace:dump message");
                        info!("response: sending trace:dump response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&TraceDumpResponse::default())
                                    .expect("failed to serialize trace:dump response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    // protocol:runtime
                    FBPMessage::RuntimePacketRequest(_payload) => {
                        info!("got runtime:packet message");
                        //TODO print incoming packet
                    }

                    FBPMessage::RuntimePacketsentRequest(_payload) => {
                        info!("got runtime:packetsent message");
                        //TODO print and confirm/correlate to any previously sent packet to the remote runtime
                    }

                    // network:data
                    FBPMessage::NetworkEdgesRequest(_payload) => {
                        info!("got network:edges message");
                        info!("response: sending network:edges response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&NetworkEdgesResponse::default())
                                    .expect("failed to serialize network:edges response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    // network:control (?)
                    FBPMessage::NetworkStartRequest(_payload) => {
                        info!("got network:start message");
                        info!("response: sending network:started response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&NetworkStartedResponse::default())
                                    .expect("failed to serialize network:started response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::NetworkStopRequest(_payload) => {
                        info!("got network:start message");
                        info!("response: sending network:start response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&NetworkStartedResponse::default())
                                    .expect("failed to serialize network:started response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::NetworkDebugRequest(_payload) => {
                        info!("got network:debug message");
                        info!("response: sending network:debug response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&NetworkDebugResponse::default())
                                    .expect("failed to serialize network:debug response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    // protocol:component
                    FBPMessage::ComponentListRequest(_payload) => {
                        info!("got component:list message");
                        info!("response: sending component:component and component:componentsready response");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&ComponentComponentMessage::default())
                                    .expect("failed to serialize component:component response"),
                            ))
                            .expect("failed to write message into websocket");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&ComponentComponentsreadyMessage::default())
                                    .expect(
                                        "failed to serialize component:componentsready response",
                                    ),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    //TODO group and order handler blocks by capability
                    _ => {
                        info!("unknown message type received: {:?}", fbpmsg); //TODO wanted Display trait here
                        websocket.close(None).expect("could not close websocket");
                    }
                }

                //websocket.write_message(msg)?;
            }
            Message::Ping(_) | Message::Pong(_) => {
                info!("got a ping|pong");
            }
            Message::Close(_) => {
                info!("got a close, breaking");
                break;
            }
        }
    }
    //websocket.close().expect("could not close websocket");
    info!("---");
    Ok(())
}

fn main() {
    pretty_env_logger::init();

    let server = TcpListener::bind("localhost:3569").unwrap();

    info!("listening on localhost:3569");
    for stream in server.incoming() {
        spawn(move || match stream {
            Ok(stream) => {
                info!("got a client");
                if let Err(err) = handle_client(stream) {
                    match err {
                        Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8 => (),
                        e => error!("test: {}", e),
                    }
                }
            }
            Err(e) => error!("Error accepting stream: {}", e),
        });
    }
}

//TODO currently panicks if unknown variant
//TODO currently panicks if field is missing during decoding
#[derive(Deserialize, Debug)]
#[serde(tag = "command", content = "payload")] //TODO multiple tags: protocol and command
enum FBPMessage {
    // runtime base -- no capabilities required
    #[serde(rename = "getruntime")]
    RuntimeGetruntimeMessage(RuntimeGetruntimePayload), //NOTE: tag+content -> tuple variant not struct variant
    #[serde(rename = "runtime")]
    RuntimeRuntimeMessage,

    // protocol:runtime
    #[serde(rename = "packet")]
    RuntimePacketRequest(RuntimePacketRequestPayload),
    #[serde(rename = "packetsent")]
    RuntimePacketsentRequest(RuntimePacketRequestPayload), //TODO should be RuntimePacketsentRequestPayload?

    // network:persist
    #[serde(rename = "persist")]
    NetworkPersistRequest(NetworkPersistRequestPayload),

    //TODO group all request messages by capability
    //TODO order them correctly like in the spec
    #[serde(rename = "ports")]
    RuntimePortsMessage,
    #[serde(rename = "list")]
    ComponentListMessage(ComponentListPayload),
    #[serde(rename = "component")]
    ComponentComponentMessage,
    #[serde(rename = "componentsready")]
    ComponentComponentsreadyMessage,
    #[serde(rename = "getstatus")]
    NetworkGetstatusMessage(NetworkGetstatusPayload),
    #[serde(rename = "status")]
    NetworkStatusMessage,
    #[serde(rename = "getsource")]
    ComponentGetsourceMessage(ComponentGetsourcePayload),
    #[serde(rename = "source")]
    ComponentSourceMessage,

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

    // protocol:component
    #[serde(rename = "list")]
    ComponentListRequest(ComponentListRequestPayload),

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
struct RuntimeRuntimeMessage {
    protocol: String, // group of messages (and capabities)
    command: String,  // name of message within group
    payload: RuntimeRuntimePayload,
}

impl Default for RuntimeRuntimeMessage {
    fn default() -> Self {
        RuntimeRuntimeMessage {
            protocol: String::from("runtime"),
            command: String::from("runtime"),
            payload: RuntimeRuntimePayload::default(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeRuntimePayload {
    id: String,                        // UUID of this runtime instance
    label: String,                     // human-readable description of the runtime
    version: String,                   // supported protocol version
    all_capabilities: Vec<Capability>, // capabilities supported by runtime
    capabilities: Vec<Capability>, // capabities for you //TODO implement privilege level restrictions
    graph: String,                 // currently active graph
    #[serde(rename = "type")]
    runtime: String, // name of the runtime software, "flowd"
    namespace: String,             // namespace of components for this project of top-level graph
    repository: String,            // source code repository of this runtime software
    repository_version: String,    // repository version of this software build
}

impl Default for RuntimeRuntimePayload {
    fn default() -> Self {
        RuntimeRuntimePayload {
            id: String::from("f18a4924-9d4f-414d-a37c-deadbeef0000"), //TODO actually random UUID
            label: String::from("human-readable description of the runtime"), //TODO useful text
            version: String::from("0.7"),                             //TODO actually implement that
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
            runtime: String::from("flowd"),
            namespace: String::from("main"), // namespace of components
            repository: String::from("https://github.com/ERnsTL/flowd.git"),
            repository_version: String::from("0.0.1-ffffffff"), //TODO use actual git commit and acutal version
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
struct RuntimePacketRequestPayload {
    port: String,
    event: String, //TODO spec what does this do? format?
    #[serde(rename = "type")]
    typ: String, // spec: the basic data type send, example "array" -- TODO which values are allowed here? TODO serde rename correct?
    schema: String, // spec: URL to JSON schema describing the format of the data
    graph: String,
    payload: String, // spec: payload for the packet. Used only with begingroup (for group names) and data packets. //TODO type "any" allowed
    secret: String,  // only present on the request payload
}

#[derive(Serialize, Debug)]
struct RuntimePacketResponse {
    protocol: String,
    command: String,
    payload: RuntimePacketResponsePayload,
}

//TODO serde: RuntimePacketRequestPayload is the same as RuntimePacketResponsePayload except the payload -- any possibility to mark this optional for the response?
#[derive(Serialize, Deserialize, Debug)]
struct RuntimePacketResponsePayload {
    port: String,
    event: String, //TODO spec what does this do? format?
    #[serde(rename = "type")]
    typ: String, // spec: the basic data type send, example "array" -- TODO which values are allowed here? TODO serde rename correct?
    schema: String, // spec: URL to JSON schema describing the format of the data
    graph: String,
    payload: String, // spec: payload for the packet. Used only with begingroup (for group names) and data packets. //TODO type "any" allowed
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
            event: String::from("default event"),
            typ: String::from("string"), //TODO is this correct?
            schema: String::from(""),
            graph: String::from(""),
            payload: String::from("default packet payload"),
        }
    }
}

// runtime:packetsent
#[derive(Deserialize, Debug)]
struct RuntimePacketsentRequest {
    protocol: String,
    command: String,
    payload: RuntimePacketRequestPayload, //NOTE: this is structurally the same for runtime:packet and runtime:packetsent //TODO spec: missing payload?
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

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RuntimePortsPayload {
    graph: String,
    in_ports: Vec<String>,
    out_ports: Vec<String>,
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

#[derive(Deserialize, Debug)]
struct GraphEdgeSpec {
    src: GraphNodeSpec,
    tgt: GraphNodeSpec,
}

#[derive(Serialize, Debug)]
struct NetworkEdgesResponse {
    protocol: String,
    command: String,
    payload: NetworkEdgesResponsePayload,
}

#[derive(Serialize, Debug)]
struct NetworkEdgesResponsePayload {} //TODO spec: is a confirmative response of type network:edges enough or should all values be echoed beck?

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
        NetworkEdgesResponsePayload {}
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

#[derive(Serialize, Debug)]
struct NetworkOutputResponsePayload {
    message: String,
    #[serde(rename = "type")]
    typ: String, // spec: either "message" or "previewurl"    //TODO serde rename correct?  //TODO convert to enum
    url: String, // spec: URL for an image generated by the runtime
}

impl Default for NetworkOutputResponsePayload {
    fn default() -> Self {
        NetworkOutputResponsePayload {
            message: String::from("default output message"),
            typ: String::from("message"),
            url: String::from(""),
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

#[derive(Serialize, Debug)]
struct NetworkTransmissionPayload {
    id: String, // spec: textual edge identifier, usually in form of a FBP language line
    src: GraphNodeSpec,
    tgt: GraphNodeSpec,
    graph: String,
    subgraph: Vec<String>, // spec: Subgraph identifier for the event. An array of node IDs. TODO what does it mean? why a list of node IDs?
}

impl Default for NetworkTransmissionPayload {
    fn default() -> Self {
        NetworkTransmissionPayload {
            id: String::from("Repeater.OUT -> Display.IN"), //TODO not sure if this is correct
            src: GraphNodeSpec::default(),
            tgt: GraphNodeSpec::default(),
            graph: String::from("main_graph"),
            subgraph: vec![String::from("Repeater.OUT -> Display.IN")], //TODO not sure of this is correct, most likely not
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
struct NetworkStartedResponse {
    protocol: String,
    command: String,
    payload: NetworkStartedResponsePayload,
}

#[derive(Serialize, Debug)]
struct NetworkStartedResponsePayload {
    time: String, //TODO spec time format?
    graph: String,
    started: bool, // spec: see network:status response for meaning of started and running //TODO spec: shouldn't this always be true?
    running: bool,
    debug: bool,
}

impl Default for NetworkStartedResponse {
    fn default() -> Self {
        NetworkStartedResponse {
            protocol: String::from("network"),
            command: String::from("started"),
            payload: NetworkStartedResponsePayload::default(),
        }
    }
}

impl Default for NetworkStartedResponsePayload {
    fn default() -> Self {
        NetworkStartedResponsePayload {
            time: String::from("2021-01-01T19:00:00+01:00"), //TODO is this correct?
            graph: String::from("main_graph"),
            started: false,
            running: false,
            debug: false,
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
    time: String, //TODO spec time format?
    uptime: u32, // spec: time the network was running, in seconds //TODO spec: should the time it was stopped be subtracted from this number? //TODO spec: not "time" but "duration"
    graph: String,
    running: bool, //TODO spec: shouldn't this always be false?    //TODO spec: ordering of fields is different between network:started and network:stopped
    started: bool, // spec: see network:status response for meaning of started and running
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
            time: String::from("2021-01-01T19:00:00+01:00"), //TODO is this correct?
            uptime: 123,
            graph: String::from("main_graph"),
            started: false,
            running: false,
            debug: false,
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
struct NetworkStatusMessage {
    protocol: String,
    command: String,
    payload: NetworkStatusPayload,
}

impl Default for NetworkStatusMessage {
    fn default() -> Self {
        NetworkStatusMessage {
            protocol: String::from("network"),
            command: String::from("status"),
            payload: NetworkStatusPayload::default(),
        }
    }
}

#[derive(Serialize, Debug)]
struct NetworkStatusPayload {
    graph: String,
    uptime: u32, // spec: time the network has been running, in seconds. NOTE: seconds since start of the network
    // NOTE: started+running=is running now. started+not running=network has finished. not started+not running=network was never started.
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
struct NetworkDebugResponsePayload {}

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
        NetworkDebugResponsePayload {}
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
struct ComponentComponentMessage {
    protocol: String,
    command: String,
    payload: ComponentComponentPayload,
}

impl Default for ComponentComponentMessage {
    fn default() -> Self {
        ComponentComponentMessage {
            protocol: String::from("component"),
            command: String::from("component"),
            payload: ComponentComponentPayload::default(),
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ComponentListPayload {
    secret: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ComponentComponentPayload {
    name: String, // spec: component name in format that can be used in graphs. Should contain the component library prefix.
    description: String,
    icon: String, // spec: visual icon for the component, matching icon names in Font Awesome
    subgraph: bool, // spec: is the component a subgraph?
    in_ports: Vec<String>, //TODO create classes
    out_ports: Vec<String>, //TODO create classes
}

impl Default for ComponentComponentPayload {
    fn default() -> Self {
        ComponentComponentPayload {
            name: String::from("main/Repeat"), //TODO Repeat, Drop, Output required for tests
            description: String::from("description of the Repeat component"),
            icon: String::from("usd"), //TODO with fa- prefix?
            subgraph: false,
            in_ports: vec![],
            out_ports: vec![],
        }
    }
}

// ----------

#[derive(Serialize, Debug)]
struct ComponentComponentsreadyMessage {
    protocol: String,
    command: String,
    payload: u32, // noflo-ui expects payload to be integer -> TODO number of component:component messages before the component:componentsready message?
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

// componennt:source
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
    id: String,   // name of the graph
    name: String, // human-readable label of the graph
    library: String,
    main: bool, // main graph?
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
    id: String,   // name of the graph
    name: String, // human-readable label of the graph
    library: String,
    main: bool, // main graph?
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
            id: String::from("001"),
            name: String::from("main_graph"),
            library: String::from("main_library"),
            main: true,
            icon: String::from("fa-gbp"),
            description: String::from("the main graph"),
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
    id: String,                  // name of the node/process
    component: String,           // component name to be used for this node/process
    metadata: GraphNodeMetadata, //TODO spec: key-value pairs (with some well-known values)
    graph: bool,                 // name of the graph
    secret: String,
}

#[derive(Deserialize, Debug)]
struct GraphNodeMetadata {
    x: i32, // TODO check spec: can x be negative? -> i32 or u32
    y: i32,
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

#[derive(Serialize, Debug)]
struct GraphAddnodeResponsePayload {} // TODO check spec: should the sent values be echoed back as confirmation or is empty graph:addnode vs. a graph:error enough?

impl Default for GraphAddnodeResponsePayload {
    fn default() -> Self {
        GraphAddnodeResponsePayload {}
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
    id: String,
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
    id: String,
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
    id: String,
    metadata: GraphChangenodeMetadata,
    graph: String,
    secret: String, // if using a single GraphChangenodeMessage struct, this field would be sent in response message
}

#[derive(Deserialize, Serialize, Debug)]
struct GraphChangenodeMetadata {
    x: i32,
    y: i32,
    height: u32,   // non-specified
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

#[derive(Deserialize, Debug)]
struct GraphAddedgeRequestPayload {
    src: GraphNodeSpec,
    tgt: GraphNodeSpec,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    graph: String,
    secret: String, // only present in the request payload
}

#[derive(Deserialize, Serialize, Debug)]
struct GraphNodeSpec {
    node: String,
    port: String,
    index: String, // spec: connection index, for addressable ports //TODO spec: string or number -- how to handle in Rust?
}

impl Default for GraphNodeSpec {
    fn default() -> Self {
        GraphNodeSpec {
            node: String::from("Repeater"),
            port: String::from("IN"),
            index: String::from("1"),
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct GraphEdgeMetadata {
    route: i32,
    schema: String,
    secure: bool,
}

#[derive(Serialize, Debug)]
struct GraphAddedgeResponse {
    protocol: String,
    command: String,
    payload: GraphAddedgeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphAddedgeResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addedge instead of graph:error enough?

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
        GraphAddedgeResponsePayload {}
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
    src: GraphNodeSpec,
    tgt: GraphNodeSpec,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRemoveedgeResponse {
    protocol: String,
    command: String,
    payload: GraphRemoveedgeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemoveedgeResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addedge instead of graph:error enough?

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
        GraphRemoveedgeResponsePayload {}
    }
}

// graph:changeedge -> graph:changeedge | graph:error
#[derive(Deserialize, Debug)]
struct GraphChangeedgeRequest {
    protocol: String,
    command: String,
    payload: GraphChangeedgeRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphChangeedgeRequestPayload {
    graph: String,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    src: GraphNodeSpec,
    tgt: GraphNodeSpec,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphChangeedgeResponse {
    protocol: String,
    command: String,
    payload: GraphChangeedgeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphChangeedgeResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:changeedge instead of graph:error enough?

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
        GraphChangeedgeResponsePayload {}
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
    src: GraphIIPSpec,           //TODO spec: object,array,string,number,integer,boolean,null
    tgt: GraphNodeSpec,
    secret: String, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphAddinitialResponse {
    protocol: String,
    command: String,
    payload: GraphAddinitialResponsePayload,
}

#[derive(Deserialize, Debug)]
struct GraphIIPSpec {
    data: String, // can put JSON object, array, string, number, integer, boolean, null in there
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
    src: GraphIIPSpec, //TODO spec: object,array,string,number,integer,boolean,null
    tgt: GraphNodeSpec,
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

#[derive(Deserialize, Debug)]
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
    metadata: GraphChangegroupMetadata,
    secret: String, // only present in the request payload
}

#[derive(Deserialize, Serialize, Debug)]
struct GraphChangegroupMetadata {
    description: String,
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
    buffersize: u32, // spec: size of tracing buffer to keep in bytes
    secret: String,  // only present in the request payload
}

#[derive(Serialize, Debug)]
struct TraceStartResponse {
    protocol: String,
    command: String,
    payload: TraceStartResponsePayload,
}

#[derive(Serialize, Debug)]
struct TraceStartResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type trace:start instead of trace:error enough?

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
        TraceStartResponsePayload {}
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
struct TraceStopResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type trace:stop instead of trace:error enough?

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
        TraceStopResponsePayload {}
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
struct TraceClearResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type trace:clear instead of trace:error enough?

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
        TraceClearResponsePayload {}
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
struct TraceDumpResponsePayload {} //TODO clarify spec: should request values be echoed back as confirmation or is message type trace:dump instead of trace:error enough?

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
        TraceDumpResponsePayload {}
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
