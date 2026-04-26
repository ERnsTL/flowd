use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSinkConnection, ProcessEdgeSource, ProcessInports,
    ProcessOutports, ProcessResult, ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

// component-specific
use std::net::ToSocketAddrs;
use std::time::Duration;
extern crate imap;
extern crate native_tls;
use imap::extensions::idle::WaitOutcome;

const IMAP_CONNECT_TIMEOUT: Duration = Duration::from_secs(7);
const IMAP_IO_TIMEOUT: Option<Duration> = Some(Duration::from_secs(2));

#[derive(Debug)]
enum IMAPAppendState {
    WaitingForConfig,
    Connected {
        imap_session: imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
        mailbox: String,
    },
    Finished,
}

pub struct IMAPAppendComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: IMAPAppendState,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for IMAPAppendComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
    ) -> Self
    where
        Self: Sized,
    {
        IMAPAppendComponent {
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
            signals_in: signals_in,
            signals_out: signals_out,
            state: IMAPAppendState::WaitingForConfig,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("IMAPAppend process() called, state: {:?}", self.state);

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = IMAPAppendState::Finished;
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

        match &mut self.state {
            IMAPAppendState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url = std::str::from_utf8(&url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url);

                    // Parse and connect to IMAP server
                    let parsed_url = parse_url(url);
                    let (imap_session, mailbox) = login_and_connect(&parsed_url);
                    debug!("IMAP connection established to mailbox: {}", mailbox);
                    self.state = IMAPAppendState::Connected {
                        imap_session,
                        mailbox: mailbox.to_owned(),
                    };
                    return ProcessResult::DidWork(1);
                }
                // No config yet, but check if we should yield budget
                if context.remaining_budget == 0 {
                    return ProcessResult::NoWork;
                }
                context.remaining_budget -= 1;
                return ProcessResult::NoWork;
            }

            IMAPAppendState::Connected { imap_session, mailbox } => {
                let mut work_units = 0;

                // Process incoming messages for append
                while !self.inn.is_empty() && context.remaining_budget > 0 {
                    debug!("got {} packets, appending to IMAP mailbox.", self.inn.slots());
                    let chunk = self.inn
                        .read_chunk(self.inn.slots())
                        .expect("receive as chunk failed");

                    for ip in chunk.into_iter() {
                        debug!("appending message to IMAP mailbox: {}", mailbox);
                        if let Err(err) = imap_session.append(mailbox.as_str(), ip) {
                            error!("failed to append to IMAP mailbox: {}", err);
                            self.state = IMAPAppendState::Finished;
                            return ProcessResult::Finished;
                        }
                        work_units += 1;
                        context.remaining_budget -= 1;

                        if context.remaining_budget == 0 {
                            break;
                        }
                    }

                    if context.remaining_budget == 0 {
                        break;
                    }
                }

                // Check if input is abandoned
                if self.inn.is_abandoned() {
                    info!("EOF on inport, shutting down");
                    // Close the IMAP session
                    close(imap_session);
                    self.state = IMAPAppendState::Finished;
                    return ProcessResult::Finished;
                }

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    ProcessResult::NoWork
                }
            }

            IMAPAppendState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
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
                    value_default: String::from("imaps://username:password@example.com:993/INBOX")
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

#[derive(Debug)]
enum IMAPFetchIdleState {
    WaitingForConfig,
    Connected {
        imap_session: imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
    },
    Finished,
}

pub struct IMAPFetchIdleComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: IMAPFetchIdleState,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for IMAPFetchIdleComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
    ) -> Self
    where
        Self: Sized,
    {
        IMAPFetchIdleComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            state: IMAPFetchIdleState::WaitingForConfig,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("IMAPFetchIdle process() called, state: {:?}", self.state);

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = IMAPFetchIdleState::Finished;
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

        match &mut self.state {
            IMAPFetchIdleState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url = std::str::from_utf8(&url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url);

                    // Parse and connect to IMAP server
                    let parsed_url = parse_url(url);
                    let (mut imap_session, mailbox) = login_and_connect(&parsed_url);
                    debug!("IMAP connection established to mailbox: {}", mailbox);

                    // Fetch initial messages
                    if let Err(_) = fetch_initial_messages(&mut imap_session, &mut self.out.sink, context) {
                        error!("failed to fetch initial messages");
                        self.state = IMAPFetchIdleState::Finished;
                        return ProcessResult::Finished;
                    }

                    self.state = IMAPFetchIdleState::Connected {
                        imap_session,
                    };
                    return ProcessResult::DidWork(1);
                }
                // No config yet, but check if we should yield budget
                if context.remaining_budget == 0 {
                    return ProcessResult::NoWork;
                }
                context.remaining_budget -= 1;
                return ProcessResult::NoWork;
            }

            IMAPFetchIdleState::Connected { ref mut imap_session } => {
                // Perform timeout-based IDLE operation
                match idle_cooperative(imap_session, context) {
                    Ok(IdleResult::MailboxChanged) => {
                        debug!("mailbox changed, fetching messages");
                        if let Err(_) = fetch_messages(imap_session, &mut self.out.sink, context) {
                            error!("failed to fetch messages after mailbox change");
                            self.state = IMAPFetchIdleState::Finished;
                            return ProcessResult::Finished;
                        }
                        return ProcessResult::DidWork(1);
                    }
                    Ok(IdleResult::TimedOut) => {
                        debug!("idle timed out, continuing to listen");
                        // Continue idling, but yield to scheduler
                        if context.remaining_budget == 0 {
                            return ProcessResult::NoWork;
                        }
                        context.remaining_budget -= 1;
                        return ProcessResult::NoWork;
                    }
                    Err(_) => {
                        error!("idle operation failed");
                        self.state = IMAPFetchIdleState::Finished;
                        return ProcessResult::Finished;
                    }
                }
            }

            IMAPFetchIdleState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
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
                    value_default: String::from("imaps://username:password@example.com:993/INBOX")
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

fn login_and_connect(
    url_parsed: &url::Url,
) -> (
    imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
    &str,
) {
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
    let password = url_parsed
        .password()
        .expect("no password given in connection URL");
    // get host and port from URL
    let host = url_parsed
        .host_str()
        .expect("no host given in connection URL");
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
    debug!(
        "user={}  pass={}...  host={}  mailbox={}",
        user,
        password.chars().into_iter().take(3).collect::<String>(),
        host,
        mailbox
    );

    // connect with explicit socket timeouts so append/fetch operations stay stoppable.
    let tls = native_tls::TlsConnector::builder()
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .expect("failed to prepare TLS connection");
    let socket_addr = (host, port)
        .to_socket_addrs()
        .expect("failed to resolve IMAP server host")
        .next()
        .expect("IMAP server host resolved to no address");
    let tcp = std::net::TcpStream::connect_timeout(&socket_addr, IMAP_CONNECT_TIMEOUT)
        .expect("failed to connect to IMAP server");
    tcp.set_read_timeout(IMAP_IO_TIMEOUT)
        .expect("failed to set IMAP TCP read timeout");
    tcp.set_write_timeout(IMAP_IO_TIMEOUT)
        .expect("failed to set IMAP TCP write timeout");
    // we pass in the domain twice to check that the server's TLS certificate is valid for the domain we're connecting to.
    //NOTE: if getting authentication errors, check for special characters in URL parts, maybe a .to_ascii() is necessary because of URL-escaping
    let tls_stream = native_tls::TlsConnector::connect(&tls, host, tcp)
        .expect("failed to establish TLS to IMAP server");
    let mut client = imap::Client::new(tls_stream);
    client
        .read_greeting()
        .expect("failed to read IMAP server greeting");

    // the client we have here is unauthenticated.
    // to do anything useful with the e-mails, we need to log in
    let mut imap_session = client
        .login(user, password)
        .map_err(|e| e.0)
        .expect("failed to login to IMAP server");

    // we want to fetch the first email in the given mailbox
    imap_session
        .select(mailbox)
        .expect("failed to select mailbox");

    // return
    (imap_session, mailbox)
}

fn close(imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>) {
    // expunge the mailbox
    imap_session.expunge().expect("expunge failed");

    // be nice to the server and log out
    imap_session.logout().expect("logout failed");
}

#[derive(Debug)]
enum IdleResult {
    MailboxChanged,
    TimedOut,
}

fn idle_cooperative(
    imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
    _context: &mut NodeContext,
) -> Result<IdleResult, ()> {
    // Use a shorter timeout for cooperative processing
    let cooperative_timeout = Duration::from_millis(100);

    debug!("starting cooperative idle");
    match imap_session.idle().expect("failed to idle").wait_with_timeout(cooperative_timeout) {
        Ok(WaitOutcome::MailboxChanged) => {
            debug!("mailbox changed during idle");
            Ok(IdleResult::MailboxChanged)
        }
        Ok(WaitOutcome::TimedOut) => {
            debug!("idle timed out, yielding to scheduler");
            Ok(IdleResult::TimedOut)
        }
        Err(err) => {
            error!("idle operation failed: {}", err);
            Err(())
        }
    }
}

fn fetch_initial_messages(
    imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
    out: &mut ProcessEdgeSinkConnection,
    context: &mut NodeContext,
) -> Result<(), ()> {
    // Find unseen messages in the mailbox
    let uids_set = imap_session.uid_search("UNSEEN").map_err(|_| ())?;

    if uids_set.is_empty() {
        debug!("no unseen messages");
        return Ok(());
    }

    // Sort UIDs for consistent ordering
    let mut uids: Vec<&u32> = uids_set.iter().collect();
    uids.sort_by(|a, b| a.cmp(b));

    // Fetch messages with budget management
    let query = "BODY[]";
    let uid_set = uids
        .iter()
        .map(|val| val.to_string())
        .collect::<Vec<_>>()
        .join(",");

    debug!("fetching initial messages {}", uid_set);
    let messages = imap_session.uid_fetch(&uid_set, query).map_err(|_| ())?;

    for message in messages.iter() {
        if context.remaining_budget == 0 {
            break; // Yield to scheduler
        }

        let body = message.body().ok_or(())?;
        debug!("forwarding initial message...");
        out.push(body.to_vec()).map_err(|_| ())?;

        // Mark as deleted on server
        imap_session
            .uid_store(&uid_set, "+FLAGS (\\Deleted)")
            .map_err(|_| ())?;

        context.remaining_budget -= 1;
    }

    Ok(())
}

fn fetch_messages(
    imap_session: &mut imap::Session<native_tls::TlsStream<std::net::TcpStream>>,
    out: &mut ProcessEdgeSinkConnection,
    context: &mut NodeContext,
) -> Result<(), ()> {
    // Find unseen messages in the mailbox
    let uids_set = imap_session.uid_search("UNSEEN").map_err(|_| ())?;

    if uids_set.is_empty() {
        debug!("no new unseen messages");
        return Ok(());
    }

    // Sort UIDs for consistent ordering
    let mut uids: Vec<&u32> = uids_set.iter().collect();
    uids.sort_by(|a, b| a.cmp(b));

    // Fetch messages with budget management
    let query = "BODY[]";
    let uid_set = uids
        .iter()
        .map(|val| val.to_string())
        .collect::<Vec<_>>()
        .join(",");

    debug!("fetching messages {}", uid_set);
    let messages = imap_session.uid_fetch(&uid_set, query).map_err(|_| ())?;

    for message in messages.iter() {
        if context.remaining_budget == 0 {
            break; // Yield to scheduler
        }

        let body = message.body().ok_or(())?;
        debug!("forwarding message...");
        out.push(body.to_vec()).map_err(|_| ())?;

        // Mark as deleted on server
        imap_session
            .uid_store(&uid_set, "+FLAGS (\\Deleted)")
            .map_err(|_| ())?;

        context.remaining_budget -= 1;
    }

    Ok(())
}
