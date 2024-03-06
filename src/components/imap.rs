use std::{os::unix::thread::JoinHandleExt, sync::{Arc, Mutex}};
use tungstenite::protocol;
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

use std::time::Duration;
use std::thread;
use std::thread::Thread;
extern crate imap;
extern crate native_tls;

pub struct IMAPAppendComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for IMAPAppendComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        IMAPAppendComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("IMAPAppend is now run()ning!");
        let conf = &mut self.conf;
        let inn = &mut self.inn;    //TODO optimize these references, not really needed for them to be referenes, can just consume?

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url = std::str::from_utf8(&url_vec).expect("invalid utf-8");

        // parse and connect to IMAP server
        let parsed_url = parse_url(url);
        let (mut imap_session, mailbox) = login_and_connect(&parsed_url);

        // handle connection events
        //TODO automatic reconnection
        //TODO handle lost TCP connection without TCP reset (proper close)
        //TODO are there any events we need to handle?
        /*
        let event_handler_thread = thread::Builder::new().name(format!("{}/EV", thread::current().name().expect("failed to get current thread name"))).spawn(move || {
            while let Ok(event) = connection.recv() {
                //###
            }
            debug!("exiting");
        }).expect("failed to spawn event handler thread");
        */

        // FBP main loop
        loop {
            trace!("begin of iteration");

            // check signals
            //TODO optimize, there is also try_recv() and recv_timeout()
            if let Ok(ip) = self.signals_in.try_recv() {
                //TODO optimize string conversions
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {   //TODO optimize comparison
                    info!("got stop signal, exiting");
                    break;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    self.signals_out.send(b"pong".to_vec()).expect("could not send pong");
                }
            }

            // check in port
            /*
            loop {
                if let Ok(ip) = inn.pop() {
                    debug!("repeating packet...");
                    out.push(ip).expect("could not push into OUT");
                    out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }
            */
            while !inn.is_empty() {
                //_ = inn.pop().ok();
                debug!("got {} packets, appending to IMAP mailbox.", inn.slots());
                let chunk = inn.read_chunk(inn.slots()).expect("receive as chunk failed");
                for ip in chunk.into_iter() {   //TODO is iterator faster or as_slices() or as_mut_slices() ?
                    // TODO for objects and open brackets, we need headers and a body - "Type: OpenBracket\r\nProcess: Drop_xxxxx\r\nPort: IN\r\n\r\n..."
                    imap_session.append(mailbox, ip).expect("append failed");
                }
                // NOTE: no commit_all() necessary, because into_iter() does that automatically
            }

            // are we done?
            if inn.is_abandoned() {
                // input closed, nothing more to do
                info!("EOF on inport, shutting down");
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }

        // close connection -> event handler thread will exit from connection error
        close(&mut imap_session);

        // wait for event thread to exit
        //event_handler_thread.join().expect("failed to join event handler thread");

        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("IMAPAppend"),
            description: String::from("Appends data as-is from IN port to the mailbox given in CONF."),
            icon: String::from("cloud-upload"), // or arrow-circle-down
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL which includes encryption, server, username, password, mailbox name"),
                    values_allowed: vec![],
                    value_default: String::from("imaps://username:password@example.com:993/mailbox")
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be appended to given mailbox"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

pub struct IMAPFetchIdleComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for IMAPFetchIdleComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        IMAPFetchIdleComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("IMAPFetchIdle is now run()ning!");
        let conf = &mut self.conf;    //TODO optimize
        let mut outport = self.out; // will be moved into event handler thread and it will unpack it

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url = std::str::from_utf8(&url_vec).expect("invalid utf-8");

        // parse and connect to IMAP server
        let parsed_url = parse_url(url);
        let (mut imap_session, mailbox) = login_and_connect(&parsed_url);

        // handle connection events
        //TODO automatic reconnection
        let event_handler_thread = thread::Builder::new().name(format!("{}/EV", thread::current().name().expect("failed to get current thread name"))).spawn(move || {
            // unpack outport
            let mut out = outport.sink;
            let out_wakeup = outport.wakeup.as_mut().expect("got no wakeup handle for outport OUT");

            // first fetch
            fetch(&mut imap_session, &mut out, &out_wakeup).expect("failed to fetch");
            // main loop of idle and fetch
            while let Ok(_) = idle(&mut imap_session, &mut out, &out_wakeup) {
                // idle already sends received messages
            }
            debug!("closing connection");
            close(&mut imap_session);

            debug!("exiting");
        }).expect("failed to spawn event handler thread");

        // FBP main loop
        loop {
            trace!("begin of iteration");
            // check signals
            //TODO optimize, there is also try_recv() and recv_timeout()
            if let Ok(ip) = self.signals_in.try_recv() {
                //TODO optimize string conversions
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

            // receive from IMAP connection is done in separate thread because we dont control the events

            // are we done?
            //TODO how do we know about EOF on IMAP connection?
            //TODO how can we signal the event handler thread to exit?
            //TODO how does the event handler thread know about EOF on IMAP connection?
            //###
            /*
            if inn.is_abandoned() {
                //TODO EOF on IMAP connection
                info!("EOF on inport NAMES, shutting down");
                drop(out);
                out_wakeup.unpark();
                break;
            }
            */

            trace!("-- end of iteration");
            thread::park();
        }

        // close connection -> event handler thread will exit from connection error
        //close(&mut imap_session);

        // wait for event thread to exit
        event_handler_thread.join().expect("failed to join event handler thread");

        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("IMAPFetchIdle"),
            description: String::from("Fetches and then idles on the IMAP mailbox given in CONF and forwards received messages to the OUT outport."),
            icon: String::from("cloud-download"),   // or arrow-circle-down
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL which includes encryption, server, username, password, mailbox name"),
                    values_allowed: vec![],
                    value_default: String::from("imaps://username:password@example.com:993/mailbox")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("contents of received message in given mailbox"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

fn parse_url(url: &str) -> url::Url {
    // parse URL
    url::Url::parse(url).expect("failed to parse connection URL")
}

fn login_and_connect(url_parsed: &url::Url) -> (imap::Session<native_tls::TlsStream<std::net::TcpStream>>, &str) {
    // prepare connection arguments
    // get protocol from URL
    let protocol = url_parsed.scheme();
    if protocol != "imaps" {
        error!("only imaps:// protocol is supported, exiting");
        panic!();
    }
    // get username and password from URL
    //TODO add correcet percent-encoding for special characters - in this case the https://url.spec.whatwg.org/#userinfo-percent-encode-set
    //  this is to be done using https://docs.rs/percent-encoding/latest/percent_encoding/
    //  for now we just handle %40 = @ sign to be able to give user@example.com as username in the URL
    let user = url_parsed.username().replace("%40", "@");
    let password = url_parsed.password().expect("no password given in connection URL");
    // get host and port from URL
    let host = url_parsed.host_str().expect("no host given in connection URL");
    let port = url_parsed.port().unwrap_or(993);
    // get mailbox from URL
    let mut mailbox = url_parsed.path();
    if mailbox.is_empty() || mailbox == "/" {
        error!("no mailbox given in connection URL path, exiting");
        panic!();
    }
    mailbox = mailbox.trim_start_matches('/');
    //TODO optimize the password substring, see https://users.rust-lang.org/t/how-to-get-a-substring-of-a-string/1351/16
    //  also see https://users.rust-lang.org/t/rust-format-max-width/100096
    debug!("user={}  pass={}...  host={}  mailbox={}", user, password.chars().into_iter().take(3).collect::<String>(), host, mailbox);

    // connect
    let tls = native_tls::TlsConnector::builder().min_protocol_version(Some(native_tls::Protocol::Tlsv12)).build().expect("failed to prepare TLS connection");

    // we pass in the domain twice to check that the server's TLS certificate is valid for the domain we're connecting to.
    //NOTE: if getting authentication errors, check for special characters in URL parts, maybe a .to_ascii() is necessary because of URL-escaping
    let client = imap::connect((host, port), host, &tls).expect("failed to connect to IMAP server");

    // the client we have here is unauthenticated.
    // to do anything useful with the e-mails, we need to log in
    let mut imap_session = client
        .login(user, password)
        .map_err(|e| e.0).expect("failed to login to IMAP server");

    // we want to fetch the first email in the INBOX mailbox
    imap_session.select(mailbox).expect("failed to select mailbox");

    // return
    (imap_session, mailbox)
}

fn close(imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>) {
    // expunge the mailbox
    imap_session.expunge().expect("expunge failed");

    // be nice to the server and log out
    imap_session.logout().expect("logout failed");
}

fn fetch(imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>, out: &mut rtrb::Producer<Vec<u8>>, out_wakeup: &Thread) -> Result<(), ()> {
    // find unseen messages in the INBOX mailbox
    let uids_set = imap_session.uid_search("UNSEEN").expect("search failed");

    if uids_set.is_empty() {
        debug!("no unseen messages");
        return Ok(());
    }

    // get first unseen message
    //let uid_unseen = uids.iter().next().expect("no uids found");

    // sort
    //TODO optimize - no clue why uid_search is returning unordered HashSet
    // ordering is important for the stream of IPs
    let mut uids: Vec<&u32> = uids_set.iter().collect();
    debug!("unread messages:  {:?}", uids);
    uids.sort_by(|a, b| a.cmp(b));

    // fetch message number 1 in this mailbox, along with its RFC822 field
    // RFC 822 dictates the format of the body of e-mails
    //let query = "RFC822";
    let query = "BODY[]";
    //let query = "BODY[TEXT]";
    //let uid_set = "(".to_owned() + uids.iter().map(|val| val.to_string()).collect::<Vec<_>>().join(" ").as_str() + ")";
    let uid_set = uids.iter().map(|val| val.to_string()).collect::<Vec<_>>().join(",");
    debug!("fetching messages {}", uid_set);
    // TODO possible race condition of there are multiple IMAP idlers running trying to fetch the same messages
    //   might lead to more-than-once delivery of messages
    let messages = imap_session.uid_fetch(&uid_set, query).expect("fetch failed");
    for message in messages.iter() {
        // extract the message's body
        let body = message.body().expect("message did not have a body!");
        // we dont need that
        /*
        let body = std::str::from_utf8(body)
            .expect("message was not valid utf-8")
            .to_string();

        debug!("unseen email in INBOX:\n{}", body[0..std::cmp::min(body.len(),512)].to_string());
        */

        // send it
        debug!("forwarding message...");
        out.push(body.to_vec()).expect("could not push into OUT");    //TODO optimize Vec conversion - is it free?
        out_wakeup.unpark();
        debug!("done");

        // delete from server
        imap_session.uid_store(&uid_set, "+FLAGS (\\Deleted)").expect("delete failed");
        debug!("deleted messages {}", uid_set);
    }

    // OK
    Ok(())
}

fn idle(imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>, out: &mut rtrb::Producer<Vec<u8>>, out_wakeup: &Thread) -> Result<(), ()> {
    // wait for changed mailbox
    debug!("idling");
    //imap_session.idle()?.wait_with_timeout(timeout);
    imap_session.idle().expect("failed to idle").wait_keepalive().expect("failed to wait_keepalive");
    debug!("idling done");
    return fetch(imap_session, out, out_wakeup);
}