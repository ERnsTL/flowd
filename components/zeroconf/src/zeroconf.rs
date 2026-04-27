use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, create_io_channels,
};
use log::{debug, error, info, trace, warn};

// component-specific
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::time::Duration;


/*
Goal: Finding the flowd instance to connect to in the network, enabling "zero configuration" and dynamic setups.
Ability for multiple flowd instances to find each other AKA "where is my other peer component's inport to connect to"
in order to easily form a multi-machine, multi-network FBP network with zero configuration,
for example via a TCP <-> TCP or WebSocket <-> WebSocket bridge.
*/

#[derive(Debug)]
enum ZeroconfResponderState {
    WaitingForConfig,
    Registering,
    Registered,
    Finished,
}

pub struct ZeroconfResponderComponent {
    conf: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    // Async operation state
    state: ZeroconfResponderState,
    config: Option<ZeroconfResponderConfig>,
    // ADR-017: Bounded IO channels
    cmd_sender: std::sync::mpsc::SyncSender<ZeroconfResponderCommand>,
    result_receiver: std::sync::mpsc::Receiver<ZeroconfResponderResult>,
    #[allow(dead_code)]
    async_thread: Option<std::thread::JoinHandle<()>>,
    //graph_inout: GraphInportOutportHandle,
}

#[derive(Debug, Clone)]
struct ZeroconfResponderConfig {
    service_name: String,
    instance_name: String,
    host_name: String,
    address: String,
    port: u16,
}

#[derive(Debug)]
enum ZeroconfResponderCommand {
    Register(ZeroconfResponderConfig),
    Unregister,
}

#[derive(Debug)]
enum ZeroconfResponderResult {
    Registered,
    Error(String),
}

async fn async_responder_main(
    cmd_rx: std::sync::mpsc::Receiver<ZeroconfResponderCommand>,
    result_tx: std::sync::mpsc::SyncSender<ZeroconfResponderResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let mut responder: Option<ServiceDaemon> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            ZeroconfResponderCommand::Register(config) => {
                debug!(
                    "Registering mDNS service: {} {}",
                    config.service_name, config.instance_name
                );
                match register_service(&config).await {
                    Ok(daemon) => {
                        responder = Some(daemon);
                        let _ = result_tx.send(ZeroconfResponderResult::Registered);
                        if let Some(ref waker) = scheduler_waker {
                            waker();
                        }
                    }
                    Err(e) => {
                        let _ = result_tx.send(ZeroconfResponderResult::Error(format!(
                            "Registration failed: {}",
                            e
                        )));
                        if let Some(ref waker) = scheduler_waker {
                            waker();
                        }
                    }
                }
            }
            ZeroconfResponderCommand::Unregister => {
                debug!("Unregistering mDNS service");
                if let Some(daemon) = responder.take() {
                    if let Err(e) = daemon.shutdown() {
                        error!("Failed to shutdown mDNS daemon: {}", e);
                    }
                }
                break; // Exit the loop
            }
        }
    }
}

async fn register_service(
    config: &ZeroconfResponderConfig,
) -> Result<ServiceDaemon, Box<dyn std::error::Error + Send + Sync>> {
    let responder = ServiceDaemon::new()?;
    let my_service = ServiceInfo::new(
        &config.service_name,
        &config.instance_name,
        &config.host_name,
        &config.address,
        config.port,
        None,
    )?;
    responder.register(my_service)?;
    Ok(responder)
}

async fn async_browser_main(
    cmd_rx: std::sync::mpsc::Receiver<ZeroconfBrowserCommand>,
    result_tx: std::sync::mpsc::SyncSender<ZeroconfBrowserResult>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
) {
    let mut browser: Option<ServiceDaemon> = None;
    let mut receiver: Option<mdns_sd::Receiver<ServiceEvent>> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            ZeroconfBrowserCommand::Browse(config) => {
                debug!("Starting mDNS browse for service: {}", config.service_name);
                match start_browse(&config).await {
                    Ok((daemon, recv)) => {
                        browser = Some(daemon);
                        receiver = Some(recv);
                        debug!("mDNS browsing started successfully");
                    }
                    Err(e) => {
                        let _ = result_tx.send(ZeroconfBrowserResult::Error(format!(
                            "Browse failed: {}",
                            e
                        )));
                        if let Some(ref waker) = scheduler_waker {
                            waker();
                        }
                    }
                }
            }
            ZeroconfBrowserCommand::Stop => {
                debug!("Stopping mDNS browser");
                if let Some(daemon) = browser.take() {
                    if let Err(e) = daemon.shutdown() {
                        error!("Failed to shutdown mDNS daemon: {}", e);
                    }
                }
                break; // Exit the loop
            }
        }
    }

    // Continue browsing and sending results
    if let Some(recv) = receiver {
        while let Ok(event) = recv.recv_timeout(RECEIVE_TIMEOUT) {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    debug!("Service resolved: {}", info.get_fullname());

                    // Extract instance name from fullname
                    let instance_name_this = info.get_fullname().split('.').next().unwrap_or("");

                    // Format as URL
                    let address = info.get_addresses().iter().next();
                    if let Some(addr) = address {
                        let address_str = if addr.is_ipv6() {
                            format!("[{}]", addr)
                        } else {
                            addr.to_string()
                        };
                        let result = format!(
                            "tcp://{}:{}/{}",
                            address_str,
                            info.get_port(),
                            instance_name_this
                        );
                        let _ = result_tx.send(ZeroconfBrowserResult::ServiceFound(result));
                        if let Some(ref waker) = scheduler_waker {
                            waker();
                        }
                    }
                }
                other_event => {
                    trace!("Received other mDNS event: {:?}", other_event);
                }
            }
        }
    }
}

async fn start_browse(
    config: &ZeroconfBrowserConfig,
) -> Result<
    (ServiceDaemon, mdns_sd::Receiver<ServiceEvent>),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let client = ServiceDaemon::new()?;
    let receiver = client.browse(config.service_name.as_str())?;
    Ok((client, receiver))
}

impl Component for ZeroconfResponderComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        // ADR-017: Create bounded IO channels
        let (cmd_sender, cmd_receiver, result_sender, result_receiver) = create_io_channels::<ZeroconfResponderCommand, ZeroconfResponderResult>();

        let async_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async_responder_main(
                cmd_receiver,
                result_sender,
                scheduler_waker,
            ));
        }));

        ZeroconfResponderComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            state: ZeroconfResponderState::WaitingForConfig,
            config: None,
            cmd_sender,
            result_receiver,
            async_thread,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, _context: &mut NodeContext) -> ProcessResult {
        debug!("ZeroconfResponder process() called");

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, unregistering and finishing");
                // Send unregister command to async thread
                let _ = self.cmd_sender.send(ZeroconfResponderCommand::Unregister);
                self.state = ZeroconfResponderState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        // Handle state machine
        match self.state {
            ZeroconfResponderState::WaitingForConfig => {
                // Check if we have CONF configuration
                if let Ok(conf_vec) = self.conf.pop() {
                    debug!("received CONF config, parsing...");

                    // Parse configuration (extracted from original run() method)
                    let url_str = conf_vec.as_text().expect("configuration URL is invalid text");
                    let url = url::Url::parse(&url_str).expect("failed to parse URL");

                    // get instance name
                    let instance_name = url
                        .path()
                        .strip_prefix("/")
                        .expect("failed to strip prefix '/' from instance name in configuration URL path")
                        .to_owned();

                    // get service name
                    let service_name_queryparam = url
                        .query_pairs()
                        .find(|(key, _)| key.eq("service"))
                        .expect("failed to get service from connection URL");
                    let service_name_bytes = service_name_queryparam.1.as_bytes();
                    let service_name = std::str::from_utf8(service_name_bytes)
                        .expect("failed to convert socket address to str")
                        .to_string();

                    // prepare service record
                    let host_name = url
                        .host_str()
                        .expect("failed to get socket address from connection URL")
                        .to_owned()
                        + ".local.";

                    let config = ZeroconfResponderConfig {
                        service_name,
                        instance_name,
                        host_name,
                        address: url
                            .host_str()
                            .expect("failed to get socket address from connection URL")
                            .to_owned(),
                        port: url
                            .port()
                            .expect("failed to get port from configuration URL"),
                    };

                    self.config = Some(config.clone());

                    // Send register command to async thread
                    if let Err(_) = self
                        .cmd_sender
                        .send(ZeroconfResponderCommand::Register(config))
                    {
                        error!("Failed to send register command");
                        return ProcessResult::Finished;
                    }

                    self.state = ZeroconfResponderState::Registering;
                    debug!("mDNS service registration initiated");
                    return ProcessResult::DidWork(1);
                } else {
                    // No CONF config yet
                    return ProcessResult::NoWork;
                }
            }
            ZeroconfResponderState::Registering => {
                // Check for registration result
                if let Ok(result) = self.result_receiver.try_recv() {
                    match result {
                        ZeroconfResponderResult::Registered => {
                            self.state = ZeroconfResponderState::Registered;
                            debug!("mDNS service registered successfully");
                            return ProcessResult::DidWork(1);
                        }
                        ZeroconfResponderResult::Error(e) => {
                            error!("mDNS service registration failed: {}", e);
                            return ProcessResult::Finished;
                        }
                    }
                }
                // Still registering
                return ProcessResult::NoWork;
            }
            ZeroconfResponderState::Registered => {
                // Service is registered and running, nothing to do
                return ProcessResult::NoWork;
            }
            ZeroconfResponderState::Finished => {
                return ProcessResult::Finished;
            }
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("ZeroconfResponder"),
            description: String::from(
                "Starts an mDNS and DNS-SD responder for the service given in CONF.",
            ),
            icon: String::from("podcast"), // or handshake-o
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("CONF"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("configuration URL with options"),
                values_allowed: vec![],
                value_default: String::from(
                    "mdns://192.168.1.123:1234/flowd_subnetwork_abc?service=_fbp._tcp.local.",
                ),
            }],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

impl Drop for ZeroconfResponderComponent {
    fn drop(&mut self) {
        debug!("ZeroconfResponderComponent dropping, sending unregister command");
        // Send unregister command to async thread
        let _ = self.cmd_sender.send(ZeroconfResponderCommand::Unregister);
        // Note: The async thread will handle the unregister and terminate
    }
}

impl Drop for ZeroconfBrowserComponent {
    fn drop(&mut self) {
        debug!("ZeroconfBrowserComponent dropping, sending stop command");
        // Send stop command to async thread
        let _ = self.cmd_sender.send(ZeroconfBrowserCommand::Stop);
        // Note: The async thread will handle the stop and terminate
    }
}

#[derive(Debug)]
enum ZeroconfBrowserState {
    WaitingForConfig,
    Browsing,
    Finished,
}

pub struct ZeroconfBrowserComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    // Async operation state
    state: ZeroconfBrowserState,
    config: Option<ZeroconfBrowserConfig>,
    // ADR-017: Bounded IO channels
    cmd_sender: std::sync::mpsc::SyncSender<ZeroconfBrowserCommand>,
    result_receiver: std::sync::mpsc::Receiver<ZeroconfBrowserResult>,
    #[allow(dead_code)]
    async_thread: Option<std::thread::JoinHandle<()>>,
    //graph_inout: GraphInportOutportHandle,
}

#[derive(Debug, Clone)]
struct ZeroconfBrowserConfig {
    service_name: String,
    #[allow(dead_code)]
    instance_name: String,
}

#[derive(Debug)]
enum ZeroconfBrowserCommand {
    Browse(ZeroconfBrowserConfig),
    Stop,
}

#[derive(Debug)]
enum ZeroconfBrowserResult {
    ServiceFound(String), // URL format result
    Error(String),
}

const RECEIVE_TIMEOUT: Duration = Duration::from_millis(500);

impl Component for ZeroconfBrowserComponent {
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
        // ADR-017: Create bounded IO channels
        let (cmd_sender, cmd_receiver, result_sender, result_receiver) = create_io_channels::<ZeroconfBrowserCommand, ZeroconfBrowserResult>();

        let async_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async_browser_main(
                cmd_receiver,
                result_sender,
                scheduler_waker,
            ));
        }));

        ZeroconfBrowserComponent {
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
            state: ZeroconfBrowserState::WaitingForConfig,
            config: None,
            cmd_sender,
            result_receiver,
            async_thread,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, _context: &mut NodeContext) -> ProcessResult {
        debug!("ZeroconfBrowser process() called");

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, stopping browser and finishing");
                // Send stop command to async thread
                let _ = self.cmd_sender.send(ZeroconfBrowserCommand::Stop);
                self.state = ZeroconfBrowserState::Finished;
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        // Handle state machine
        match self.state {
            ZeroconfBrowserState::WaitingForConfig => {
                // Check if we have CONF configuration
                if let Ok(conf_vec) = self.conf.pop() {
                    debug!("received CONF config, parsing...");

                    // Parse configuration (extracted from original run() method)
                    let url_str = conf_vec.as_text().expect("invalid text");
                    let url = url::Url::parse(&url_str).expect("failed to parse URL");

                    // get service name from URL
                    let service_name = url
                        .host_str()
                        .expect("failed to get service name from connection URL")
                        .to_owned();

                    // get instance name from URL
                    let instance_name: String;
                    if url.path().is_empty() {
                        instance_name = "".to_string();
                    } else {
                        instance_name = url
                            .path()
                            .strip_prefix("/")
                            .expect("failed to strip prefix '/' from instance name in configuration URL path")
                            .to_owned();
                    }

                    let config = ZeroconfBrowserConfig {
                        service_name,
                        instance_name,
                    };

                    self.config = Some(config.clone());

                    // Send browse command to async thread
                    if let Err(_) = self.cmd_sender.send(ZeroconfBrowserCommand::Browse(config)) {
                        error!("Failed to send browse command");
                        return ProcessResult::Finished;
                    }

                    self.state = ZeroconfBrowserState::Browsing;
                    debug!("mDNS browsing initiated");
                    return ProcessResult::DidWork(1);
                } else {
                    // No CONF config yet
                    return ProcessResult::NoWork;
                }
            }
            ZeroconfBrowserState::Browsing => {
                // Check for browse results
                if let Ok(result) = self.result_receiver.try_recv() {
                    match result {
                        ZeroconfBrowserResult::ServiceFound(url) => {
                            debug!("Service found, sending to output: {}", url);
                            let msg = FbpMessage::from_bytes(url.into_bytes());
                            if let Err(_) = self.out.push(msg) {
                                error!("Failed to send service result to output");
                                return ProcessResult::Finished;
                            }
                            return ProcessResult::DidWork(1);
                        }
                        ZeroconfBrowserResult::Error(e) => {
                            error!("mDNS browsing error: {}", e);
                            return ProcessResult::DidWork(1); // Continue browsing despite error
                        }
                    }
                }
                // No results yet, continue browsing
                return ProcessResult::NoWork;
            }
            ZeroconfBrowserState::Finished => {
                return ProcessResult::Finished;
            }
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("ZeroconfBrowser"),
            description: String::from("Starts an mDNS-SD browser and tries to find service instance based on the given service name in CONF and sends found responses to OUT outport."),
            icon: String::from("search"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("query URL - instance in the URL path is optional"),
                    values_allowed: vec![],
                    value_default: String::from("mdns://_fbp._tcp.local./instancename")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("service response in URL format, eg. tcp://1.2.3.4:1234/instancename"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
