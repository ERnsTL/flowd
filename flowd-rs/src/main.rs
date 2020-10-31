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
    stream.set_nodelay(true).expect("set_nodelay call failed");

    let callback = |req: &Request, mut response: Response| {
        debug!("Received a new ws handshake");
        debug!("The request's path is: {}", req.uri().path());
        debug!("The request's headers are:");
        for (ref key, value) in req.headers() {
            debug!("  {}: {:?}", key, value);
        }

        // Let's add an additional header to our response to the client.
        let headers = response.headers_mut();
        //TODO check for noflo on Request
        headers.insert("sec-websocket-protocol", "noflo".parse().unwrap());
        headers.append("MyCustomHeader", ":)".parse().unwrap());
        headers.append("SOME_TUNGSTENITE_HEADER", "header_value".parse().unwrap());

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
                debug!("message data: {}", msg.into_text().unwrap());
                info!("sending back runtime/runtime message");
                websocket
                    .write_message(Message::text(
                        "{
                      \"protocol\": \"runtime\",
                      \"command\": \"runtime\",
                      \"payload\": {
                        \"id\": \"f18a4924-9d4f-414d-a37c-deadbeef0000\",
                        \"label\": \"human-readable description of the runtime\",
                        \"version\": \"0.7\",
                        \"allCapabilities\": [
                            \"protocol:runtime\",
                            \"protocol:network\",
                            \"graph:readonly\",
                            \"protocol:component\",
                            \"network:status\",
                            \"network:persist\"
                        ],
                        \"capabilities\": [
                            \"protocol:runtime\",
                            \"protocol:network\",
                            \"graph:readonly\",
                            \"protocol:component\",
                            \"network:status\"
                        ],
                        \"graph\": \"service-main\",
                        \"type\": \"flowd\",
                        \"namespace\": \"my-project-foo\",
                        \"repository\": \"https://github.com/flowbased/fbp-protocol.git\",
                        \"repositoryVersion\": \"0.6.3-8-g90edcfc\"
                      }
                    }",
                    ))
                    .expect("failed to write message into websocket");
                info!("sending back runtime/ports message");
                websocket
                    .write_message(Message::text(
                        "{
                  \"protocol\": \"runtime\",
                  \"command\": \"ports\",
                  \"payload\": {
                    \"graph\": \"service-main\",
                    \"inPorts\": [],
                    \"outPorts\": []
                  }
                }",
                    ))
                    .expect("failed to write message into websocket");
                /*
                info!("sending clear graph message");
                websocket
                    .write_message(Message::text(
                        "{\"protocol\": \"graph\",\"command\": \"clear\",\"payload\": {
                                \"idstring\":\"f18a4924-9d4f-414d-a37c-deadbeef0001\",
                                \"namestring\":\"service-main\",
                                \"librarystring\":\"main\",
                                \"main\":true,
                                \"icon\":\"circle\",
                                \"description\":\"tolle Beschreibung!\",
                                \"secret\":\"soso\"
                            }}",
                    ))
                    .expect("failed to write message into websocket");
                    */
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

    let server = TcpListener::bind("localhost:3000").unwrap();

    info!("listening");
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
