#![feature(duration_constants)]
#![feature(io_error_more)]
#![feature(map_try_insert)]

use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};
use std::thread::spawn;
use std::time::Duration;

use tungstenite::handshake::server::{Request, Response};
use tungstenite::handshake::HandshakeRole;
use tungstenite::{accept_hdr, Error, HandshakeError, Message, Result};

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

use serde::{Deserialize, Serialize};

use std::collections::HashMap;
//use dashmap::DashMap;

use chrono::prelude::*;

fn must_not_block<Role: HandshakeRole>(err: HandshakeError<Role>) -> Error {
    match err {
        HandshakeError::Interrupted(_) => panic!("Bug: blocking socket would block"),
        HandshakeError::Failure(f) => f,
    }
}

fn handle_client(stream: TcpStream, graph: Arc<RwLock<Graph>>, runtime: Arc<RwLock<RuntimeRuntimePayload>>, components: Arc<RwLock<ComponentLibrary>>) -> Result<()> {
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
                    // runtime base
                    FBPMessage::RuntimeGetruntimeMessage(payload) => {
                        info!("got runtime:getruntime message with secret {}", payload.secret);
                        // send response = runtime:runtime message
                        info!("response: sending runtime:runtime message");
                        websocket
                            .write_message(Message::text(
                                //TODO handing over value inside lock would work like this:  serde_json::to_string(&*runtime.read().expect("lock poisoned"))
                                serde_json::to_string(&RuntimeRuntimeMessage::new(&runtime.read().expect("lock poisoned")))
                                .expect("failed to serialize runtime:runtime message"),
                            ))
                            .expect("failed to write message into websocket");
                        // spec: "If the runtime is currently running a graph and it is able to speak the full Runtime protocol, it should follow up with a ports message."
                        info!("response: sending runtime:ports message");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&RuntimePortsMessage::new(&runtime.read().expect("lock poisoned"), &graph.read().expect("lock poisoned")))
                                    .expect("failed to serialize runtime:ports message"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    // protocol:component
                    FBPMessage::ComponentListRequest(_payload) => {
                        info!("got component:list message");
                        info!("response: sending component:component message(s) and closing component:componentsready response");
                        let mut count: u32 = 0;
                        for component in components.read().expect("lock poisoned").available.iter() {
                            websocket
                            .write_message(Message::text(
                                serde_json::to_string(&ComponentComponentMessage::new(&component))
                                    .expect("failed to serialize component:component response"),
                            ))
                            .expect("failed to write message into websocket");
                            count += 1;
                        }
                        info!("sent {} component:component responses", count);
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&ComponentComponentsreadyMessage::new(count))
                                    .expect("failed to serialize component:componentsready response"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::NetworkGetstatusMessage(_payload) => {
                        info!("got network:getstatus message");
                        info!("response: sending network:status message");
                        websocket
                            .write_message(Message::text(
                                serde_json::to_string(&NetworkStatusMessage::new(&NetworkStatusPayload::new(&runtime.read().expect("lock poisoned").status)))
                                    .expect("failed to serialize network:status message"),
                            ))
                            .expect("failed to write message into websocket");
                    }

                    FBPMessage::NetworkPersistRequest(_payload) => {
                        info!("got network:persist message");
                        // persist and send either network:persist or network:error
                        match runtime.read().expect("lock poisoned").persist() {    //NOTE: lock read() is enough, because persist() does not modify state, just copies it away to persistence
                            Ok(_) => {
                                info!("response: sending network:persist message");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&NetworkPersistResponse::default())
                                            .expect("failed to serialize network:persist message"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("persist failed: {}", err);
                                info!("response: sending network:error message");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(
                                            err.to_string(),
                                            String::from(""),
                                            runtime.read().expect("lock poisoned").graph.clone()
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
                        //TODO why does Rust require a write lock here? "cannot borrow data in dereference as mutable"
                        match graph.write().expect("lock poisoned").get_source(payload.name) {
                            Ok(source_info) => {
                                websocket
                                .write_message(Message::text(
                                    serde_json::to_string(&ComponentSourceMessage::new(source_info))
                                        .expect("failed to serialize component:source message"),
                                ))
                                .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.get_source() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphClearRequest(payload) => {
                        info!("got graph:clear message");
                        match graph.write().expect("lock poisoned").clear(&payload, &runtime.read().expect("lock poisoned")) {
                            Ok(_) => {
                                info!("response: sending graph:clear response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphClearResponse::new(&payload))
                                            .expect("failed to serialize graph:clear response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.clear() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                            Ok(_) => {
                                info!("response: sending graph:addnode response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphAddnodeResponse::default())
                                            .expect("failed to serialize graph:addnode response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.add_node() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphRemovenodeResponse::default())
                                            .expect("failed to serialize graph:removenode response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.remove_node() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphRenamenodeResponse::default())
                                            .expect("failed to serialize graph:renamenode response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.rename_node() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphChangenodeResponse::default())
                                            .expect("failed to serialize graph:changenode response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.change_node() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                        match graph.write().expect("lock poisoned").add_edge(payload.graph.clone(), GraphEdge::from(payload)) {
                            Ok(_) => {
                                info!("response: sending graph:addedge response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphAddedgeResponse::default())
                                            .expect("failed to serialize graph:addedge response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.add_edge() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphRemoveedgeRequest(payload) => {
                        info!("got graph:removeedge message");
                        match graph.write().expect("lock poisoned").remove_edge(payload.graph, payload.src, payload.tgt) {
                            Ok(_) => {
                                info!("response: sending graph:removeedge response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphRemoveedgeResponse::default())
                                            .expect("failed to serialize graph:removeedge response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.remove_edge() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize graph:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    FBPMessage::GraphChangeedgeRequest(payload) => {
                        info!("got graph:changeedge message");
                        match graph.write().expect("lock poisoned").change_edge(payload.graph, payload.src, payload.tgt, payload.metadata) {
                            Ok(_) => {
                                info!("response: sending graph:changeedge response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphChangeedgeResponse::default())
                                            .expect("failed to serialize graph:changeedge response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.change_edge() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphAddinitialResponse::default())
                                            .expect("failed to serialize graph:addinitial response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.add_initialip() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphRemoveinitialResponse::default())
                                            .expect("failed to serialize graph:removeinitial response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.remove_initialip() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphAddinportResponse::default())
                                            .expect("failed to serialize graph:addinport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.add_inport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphRemoveinportResponse::default())
                                            .expect("failed to serialize graph:removeinport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.remove_inport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                .write_message(Message::text(
                                    serde_json::to_string(&GraphRenameinportResponse::default())
                                        .expect("failed to serialize graph:renameinport response"),
                                ))
                                .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.rename_inport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphAddoutportResponse::default())
                                            .expect("failed to serialize graph:addoutport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.add_outport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphRemoveoutportResponse::default())
                                            .expect("failed to serialize graph:removeoutport response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.remove_outport() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                .write_message(Message::text(
                                    serde_json::to_string(&GraphRenameoutportResponse::default())
                                        .expect("failed to serialize graph:renameoutport response"),
                                ))
                                .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                            },
                        }
                    }

                    FBPMessage::GraphAddgroupRequest(payload) => {
                        info!("got graph:addgroup message");
                        match graph.write().expect("lock poisoned").add_group(payload.graph, payload.name, payload.nodes, payload.metadata) {
                            Ok(_) => {
                                info!("response: sending graph:addgroup response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphAddgroupResponse::default())
                                            .expect("failed to serialize graph:addgroup response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.add_group() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphRemovegroupResponse::default())
                                            .expect("failed to serialize graph:removegroup response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.remove_group() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphRenamegroupResponse::default())
                                            .expect("failed to serialize graph:renamegroup response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("graph.rename_group() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&GraphChangegroupResponse::default())
                                            .expect("failed to serialize graph:changegroup response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("graph.change_group() failed: {}", err);
                                info!("response: sending graph:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&TraceStartResponse::new(payload.graph))
                                            .expect("failed to serialize trace:start response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.start_trace() failed: {}", err);
                                info!("response: sending trace:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&TraceStopResponse::new(payload.graph))
                                            .expect("failed to serialize trace:stop response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.stop_trace() failed: {}", err);
                                info!("response: sending trace:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&TraceClearResponse::new(payload.graph))
                                            .expect("failed to serialize trace:clear response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.tracing_start() failed: {}", err);
                                info!("response: sending trace:error response");
                                websocket
                                    .write_message(Message::text(
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&TraceDumpResponse::new(payload.graph, dump))
                                            .expect("failed to serialize trace:dump response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.dump_trace() failed: {}", err);
                                info!("response: sending trace:error response");
                                websocket
                                    .write_message(Message::text(
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
                        //TODO or maybe better send this to graph?
                        match runtime.write().expect("lock poisoned").packet(&payload) {
                            Ok(_) => {
                                info!("response: sending runtime:packetsent response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&RuntimePacketsentMessage::new(payload))
                                            .expect("failed to serialize network:packetsent response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.packet() failed: {}", err);
                                info!("response: sending runtime:error response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&RuntimeErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize runtime:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                        //TODO print incoming packet
                    }

                    FBPMessage::RuntimePacketsentRequest(payload) => {
                        info!("got runtime:packetsent message");
                        match runtime.write().expect("lock poisoned").packetsent(payload) {
                            Ok(_) => {
                                //nothing to send if ok, since this is already a confirmation of a previous runtime:packet that we sent to the remote runtime acting as remote subgraph
                                info!("response: nothing, but runtime core returned OK");
                            },
                            Err(err) => {
                                error!("runtime.packetsent() failed: {}", err);
                                info!("response: sending runtime:error response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&RuntimeErrorResponse::new(err.to_string()))
                                            .expect("failed to serialize runtime:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    // network:data
                    FBPMessage::NetworkEdgesRequest(payload) => {
                        info!("got network:edges message");
                        match runtime.write().expect("lock poisoned").set_debug_edges(payload.graph, payload.edges) {
                            Ok(_) => {
                                info!("response: sending network:edges response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&NetworkEdgesResponse::default())
                                            .expect("failed to serialize network:edges response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            },
                            Err(err) => {
                                error!("runtime.set_debug_edges() failed: {}", err);
                                info!("response: sending network:error response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(
                                            err.to_string(),
                                            String::from(""),
                                            runtime.read().expect("lock poisoned").graph.clone()    //TODO can we avoid clone here?
                                        ))
                                            .expect("failed to serialize network:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                            }
                        }
                    }

                    // network:control (?)
                    FBPMessage::NetworkStartRequest(_payload) => {
                        info!("got network:start message");
                        match runtime.write().expect("lock poisoned").start() {
                            Ok(_) => {
                                info!("response: sending network:started response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&NetworkStartedResponse::new(&runtime.read().expect("lock poisoned").status))
                                            .expect("failed to serialize network:started response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                },
                            Err(err) => {
                                error!("runtime.start() failed: {}", err);
                                info!("response: sending network:error response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(
                                            err.to_string(),
                                            String::from(""),
                                            runtime.read().expect("lock poisoned").graph.clone()    //TODO can we avoid clone here?
                                        ))
                                            .expect("failed to serialize network:error response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                },
                        }
                    }

                    FBPMessage::NetworkStopRequest(_payload) => {
                        info!("got network:stop message");
                        match runtime.write().expect("lock poisoned").stop() {
                            Ok(_) => {
                                info!("response: sending network:stop response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&NetworkStoppedResponse::new(&runtime.read().expect("lock poisoned").status))
                                            .expect("failed to serialize network:stopped response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                },
                            Err(err) => {
                                error!("runtime.stop() failed: {}", err);
                                info!("response: sending network:error response");
                                websocket
                                    .write_message(Message::text(
                                        serde_json::to_string(&NetworkErrorResponse::new(
                                            err.to_string(),
                                            String::from(""),
                                            runtime.read().expect("lock poisoned").graph.clone()    //TODO can we avoid clone here?
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
                                    .write_message(Message::text(
                                        serde_json::to_string(&NetworkDebugResponse::new(payload.graph))
                                            .expect("failed to serialize network:debug response"),
                                    ))
                                    .expect("failed to write message into websocket");
                                    },
                            Err(err) => {
                                error!("runtime.debug_mode() failed: {}", err);
                                info!("response: sending network:error response");
                                websocket
                                    .write_message(Message::text(
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

                //websocket.write_message(msg)?;
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
    }
    //websocket.close().expect("could not close websocket");
    info!("---");
    Ok(())
}

fn main() {
    pretty_env_logger::init();
    info!("logging initialized");

    //TODO the runtime should manage the graphs -> add_graph() and also checking that they actually exist and should have a method switch_graph()
    let runtime: Arc<RwLock<RuntimeRuntimePayload>> = Arc::new(RwLock::new(RuntimeRuntimePayload::new(
        String::from("main_graph")
    )));
    info!("runtime initialized");

    let componentlib: Arc<RwLock<ComponentLibrary>> = Arc::new(RwLock::new(ComponentLibrary::default()));
    //TODO actually load components
    info!("component library initialized");

    //TODO graph (or runtime?) should check if the components used in the graph are actually available in the component library
    let graph: Arc<RwLock<Graph>> = Arc::new(RwLock::new(Graph::new(
        String::from("main_graph"),
        String::from("basic description"),
        String::from("usd")
    )));  //TODO check if an RwLock is OK (multiple readers possible, but what if writer deletes that thing being read?) or if Mutex needed
    info!("graph initialized");

    let server = TcpListener::bind("localhost:3569").unwrap();
    info!("management listening on localhost:3569");

    for stream in server.incoming() {
        // create Arc pointers for the new thread
        let graphref = graph.clone();
        let runtimeref = runtime.clone();
        let componentlibref = componentlib.clone();
        // spawn thread
        spawn(move || match stream {
            Ok(stream) => {
                info!("got a client");
                if let Err(err) = handle_client(stream, graphref, runtimeref, componentlibref) {
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
    RuntimePacketsentRequest(RuntimePacketRequestPayload), //TODO should be RuntimePacketsentRequestPayload?

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
        }
    }
}

impl RuntimeRuntimePayload {
    fn new(active_graph: String) -> Self {
        RuntimeRuntimePayload{
            graph: active_graph.clone(),    //TODO any way to avoid the clone and point to the other one?
            status: NetworkStartedResponsePayload {
                time_started: UtcTime(chrono::MIN_DATETIME), // zero value
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

    fn start(&mut self) -> std::result::Result<(), std::io::Error> {
        //TODO implement
        self.status.time_started = UtcTime(chrono::Utc::now());
        self.status.graph = self.graph.clone();
        self.status.started = true;
        self.status.running = true;
        Ok(())
    }

    fn stop(&mut self) -> std::result::Result<(), std::io::Error> {
        //TODO implement
        self.status.graph = self.graph.clone();
        self.status.started = true;
        self.status.running = false;    // was started, but not running any more
        Ok(())
    }

    fn debug_mode(&mut self, graph: &str, mode: bool) -> std::result::Result<(), std::io::Error> {
        //TODO check if the given graph exists
        //TODO check if the given graph is the currently selected one
        //TODO implement
        self.status.debug = mode;
        Ok(())
    }

    //TODO optimize: better to hand over String or &str? Difference between Vec and vec?
    fn set_debug_edges(&mut self, graph: String, edges: Vec<GraphEdgeSpec>) -> std::result::Result<(), std::io::Error> {
        //TODO clarify spec: what to do with this message's information behavior-wise? Dependent on first setting network into debug mode or independent?
        //TODO implement
        info!("got following debug edges:");
        for edge in edges {
            info!("  edge: src={:?} tgt={:?}", edge.src, edge.tgt);
        }
        info!("--- end");
        Ok(())
    }

    fn start_trace(&mut self, graph: &str, bufferSize: u32) -> std::result::Result<(), std::io::Error> {
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

    fn packet(&mut self, payload: &RuntimePacketRequestPayload) -> std::result::Result<(), std::io::Error> {
        //TODO check if graph exists and if that port actually exists
        //TODO implement and deliver to destination process
        info!("runtime: got a packet: {:?}", payload);
        Ok(())
    }

    //TODO the payload has unusual type -> can we really re-use it? Unify these three: RuntimePacketRequestPayload, RuntimePacketResponsePayload, RuntimePacketsentResponsePayload?
    fn packetsent(&mut self, payload: RuntimePacketRequestPayload) -> std::result::Result<(), std::io::Error> {
        //TODO implement
        //TODO confirm/correlate to any previously sent packet to the remote runtime, remote from list of awaiting packetsent confirmations
        info!("runtime: got a packetsent confirmation: {:?}", payload);
        Ok(())
    }

    //TODO return path: process that sends to an outport -> send to client. TODO clarify spec: which client should receive it?

    //TODO runtime: command to connect an outport to a remote runtime as remote subgraph.
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

#[derive(Deserialize, Serialize, Debug)]
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
#[derive(Deserialize, Serialize, Debug)]
struct RuntimePacketsentMessage {
    protocol: String,
    command: String,
    payload: RuntimePacketRequestPayload, //NOTE: this is structurally the same for runtime:packet and runtime:packetsent //TODO spec: missing payload, no there is even the payload! looks unefficient to send back the payload.
}

impl RuntimePacketsentMessage {
    //TODO for correctness, we should convert to RuntimePacketsentResponsePayload actually, but they are structurally the same
    fn new(payload: RuntimePacketRequestPayload) -> Self {
        RuntimePacketsentMessage {
            protocol: String::from("runtime"),
            command: String::from("packetsent"),
            payload: payload,
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

#[derive(Serialize, Debug)]
struct ComponentPort {
    #[serde(rename = "id")]
    name: String,
    #[serde(rename = "type")]
    allowed_type: String, //TODO clarify spec: so if we define a boolean, we can send only booleans? What about struct/object types? How should the runtime verify that? //TODO map JSON types <-> Rust types
    #[serde(default)]
    schema: String, // spec: optional
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
            schema: String::from(""), //TODO unnecessary to allocate a string to say "no schema" -> Option type or something
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
            schema: String::from(""), //TODO unnecessary to allocate a string to say "no schema" -> Option type or something
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
    metadata: GraphNodeMetadata, //TODO spec: key-value pairs (with some well-known values)
    graph: String,                 // name of the graph
    secret: String,
}

// NOTE: Serialize because used in GraphNode -> Graph which needs to be serialized
#[derive(Serialize, Deserialize, Debug)]
struct GraphNodeMetadata {
    x: i32, // TODO check spec: can x and y be negative? -> i32 or u32?
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

#[derive(Deserialize, Debug)]
struct GraphAddedgeRequestPayload {
    src: GraphNodeSpec,
    tgt: GraphNodeSpec,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    graph: String,
    secret: String, // only present in the request payload
}

//NOTE: PartialEq is for graph.remove_edge() and graph.change_edge()
#[derive(Deserialize, Serialize, Debug, PartialEq)]
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
    route: i32, //TODO clarify spec: Route identifier of a graph edge
    schema: String, //TODO clarify spec: JSON schema associated with a graph edge (TODO check schema)
    secure: bool, //TODO clarify spec: Whether edge data should be treated as secure
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

//NOTE: Serialize for graph:addinitial which makes use of the "data" field in graph -> connections -> data according to FBP JSON graph spec.
//NOTE: PartialEq are for graph.remove_initialip()
//#[derive(Serialize, Deserialize, Debug, PartialEq)]
type GraphIIPSpec = String; // spec: can put JSON object, array, string, number, integer, boolean, null in there TODO how to handle this in Rust / serde?

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

#[derive(Serialize, Deserialize, Debug)]
struct GraphEdge {
    source: GraphNodeSpec,
    data: Option<GraphIIPSpec>,
    target: GraphNodeSpec,
    metadata: GraphEdgeMetadata,
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
                schema: String::from(""), //TODO clarify spec: not available from FBP JSON Graph port
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

    fn add_node(&mut self, graph: String, component: String, name: String, metadata: GraphNodeMetadata) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change the nodeset?
        //TODO check graph name and state, multi-graph support
        let nodedef = GraphNode {
            component: component,
            metadata: metadata,
        };
        match self.nodes.try_insert(name, nodedef) {
            Ok(_) => {
                return Ok(());
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
        //TODO implement
        //TODO in which state should manipulating nodes be allowed?
        //TODO check graph name and state, multi-graph support

        //TODO optimize: which is faster? see above.
        if self.nodes.contains_key(&new) {
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, String::from("node with that name already exists")));
        }
        match self.nodes.remove(&old) {
            Some(v) => {
                self.nodes.try_insert(new, v).expect("wtf key occupied on insertion");    // should not happen
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

    fn remove_edge(&mut self, graph: String, source: GraphNodeSpec, target: GraphNodeSpec) -> Result<(), std::io::Error> {
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

    fn change_edge(&mut self, graph: String, source: GraphNodeSpec, target: GraphNodeSpec, metadata: GraphEdgeMetadata) -> Result<(), std::io::Error> {
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

        //TODO clarify spec: should we remove first or last match? currently removing last match
        for (i, edge) in self.edges.iter().rev().enumerate() {
            //TODO optimize: match for IIP match first or for target match? Target has more values to compare, but there may be more IIPs than target matches and IIPs might be longer thus more expensive to compare...
            //TODO optimize the clone here and the GraphIIPSpec
            // must be an IIP
            if let Some(thedata) = edge.data.clone() {
                // IIP data must be the same
                if thedata == payload.src {
                    // target must match
                    if edge.target == payload.tgt {
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
        //TODO implement
        //TODO how to re-compile Rust components?
        //TODO where to find the source code?
        //TODO optimize better to acquire read lock once but keep it for whole JSON serialization, or request multiple times but keep it shorter each time?
        //TODO optimize how often is source for graph requested vs. for components? -> re-order if branches
        if name == self.properties.name {
            info!("response: preparing component:source message for graph");
            return Ok(ComponentSourcePayload {
                name: name,
                language: String::from("json"), //TODO clarify spec: what to set here?
                library: String::from(""), //TODO clarify spec: what to set here?
                code: serde_json::to_string(self).expect("failed to serialize graph"),
                tests: String::from("// tests for graph here"), //TODO clarify spec: what to set here?
            });
        } else {
            if let Some(node) = self.nodes.get(&name) { //TODO optimize: &String or name.as_str()?
                info!("response: preparing component:source message for component");
                return Ok(ComponentSourcePayload {
                    name: name,
                    language: String::from(""), //TODO implement - get info from component library
                    library: String::from(""),  //TODO implement - get info from component library
                    code: String::from("// code for component here"),   //TODO implement - get from component library
                    tests: String::from("// tests for component here"), //TODO where to get the tests from? in what format?
                });
            }
        }
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from("component or graph with that name not found")));
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
            source: payload.src,
            target: payload.tgt,
            data: None,
            metadata: payload.metadata,
        }
    }
}

// spec: IIPs are special cases of a graph connection/edge
impl From<GraphAddinitialRequestPayload> for GraphEdge {
    fn from(payload: GraphAddinitialRequestPayload) -> Self {
        GraphEdge {
            source: GraphNodeSpec { //TODO clarify spec: what to set as "src" if it is an IIP?
                node: String::from(""),
                port: String::from(""),
                index: String::from(""),
            },
            data: Some(payload.src),
            target: payload.tgt,
            metadata: payload.metadata,
        }
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