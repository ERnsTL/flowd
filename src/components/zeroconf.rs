use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use mdns_sd::{ServiceDaemon, ServiceInfo, ServiceEvent};
use std::time::Duration;
use std::thread;

/*
Goal: Finding the flowd instance to connect to in the network, enabling "zero configuration" and dynamic setups.
Ability for multiple flowd instances to find each other AKA "where is my other peer component's inport to connect to"
in order to easily form a multi-machine, multi-network FBP network with zero configuration,
for example via a TCP <-> TCP or WebSocket <-> WebSocket bridge.
*/

pub struct ZeroconfResponderComponent {
    conf: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for ZeroconfResponderComponent {
    fn new(mut inports: ProcessInports, _outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        ZeroconfResponderComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("ZeroconfResponder is now run()ning!");
        let conf = &mut self.conf;

        // get configuration
        trace!("read config IP");
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url_str = std::str::from_utf8(&url_vec).expect("configuration URL is invalid utf-8");
        let url = url::Url::parse(&url_str).expect("failed to parse URL");
        // get instance name
        let instance_name = url.path().strip_prefix("/").expect("failed to strip prefix '/' from instance name in configuration URL path").to_owned();
        // get service name
        //TODO optimize re-use the query_pairs iterator? wont find anything after first .find() call
        let service_name_queryparam = url.query_pairs().find( |(key, _)| key.eq("service") ).expect("failed to get service from connection URL");
        let service_name_bytes = service_name_queryparam.1.as_bytes();
        let service_name = std::str::from_utf8(service_name_bytes).expect("failed to convert socket address to str");
        //TODO add support for publishing on defined interface

        // configure
        // set up responder
        let responder = ServiceDaemon::new().expect("failed to create mDNS-SD daemon");
        // prepare service record
        let host_name = url.host_str().expect("failed to get socket address from connection URL").to_owned() + ".local.";
        //let properties = [("property_1", "test"), ("property_2", "1234")];
        let my_service = ServiceInfo::new(
            service_name,
            &instance_name,
            &host_name,
            url.host_str().expect("failed to get socket address from connection URL"),
            url.port().expect("failed to get port from configuration URL"),
            None    //&properties[..],
        ).unwrap();
        // register service with responder, which will publish it
        responder.register(my_service).expect("Failed to register our service");
        debug!("responder started");

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

            trace!("-- end of iteration");
            std::thread::park();
        }

        // shut down
        responder.shutdown().expect("failed to shut down responder");
        thread::sleep(Duration::from_millis(500));    // give it some time to shut down
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("ZeroconfResponder"),
            description: String::from("Starts an mDNS and DNS-SD responder for the service given in CONF."),
            icon: String::from("podcast"),  // or handshake-o
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("configuration URL with options"),
                    values_allowed: vec![],
                    value_default: String::from("mdns://192.168.1.123:1234/flowd_subnetwork_abc?service=_fbp._tcp.local.")
                }
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

pub struct ZeroconfBrowserComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

const RECEIVE_TIMEOUT: Duration = Duration::from_millis(500);

impl Component for ZeroconfBrowserComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        ZeroconfBrowserComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("ZeroconfQuery is now run()ning!");
        let mut conf = self.conf;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // get configuration
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");
        let url = url::Url::parse(&url_str).expect("failed to parse URL");
        
        // get service name from URL
        let service_name = url.host_str().expect("failed to get service name from connection URL").to_owned();

        // get instance name from URL
        let instance_name: &str;
        if url.path().is_empty() {
            instance_name = "";
        } else {
            instance_name = url.path().strip_prefix("/").expect("failed to strip prefix '/' from instance name in configuration URL path");
        }

        //TODO add support for domains other than "local"
        /*
        //TODO fix - URLs are ASCII, there is .as_ascii() but requires some more conversions and is unstable feature
        let channel_queryparam = url.query_pairs().find( |kv| kv.0.eq("channel") ).expect("failed to get channel name from connection URL channel parameter");
        let channel_bytes = channel_queryparam.1.as_bytes();
        let channel = std::str::from_utf8(channel_bytes).expect("failed to convert channel name to str");
        debug!("channel: {}", channel);
        */

        //TODO add support for mode selection: one-shot, continuous, etc.
        //TODO add support for timeout in one-shot mode
        //TODO add support for selecting output format, which parameters to return

        // set configuration
        let client = ServiceDaemon::new().expect("failed to create mDNS-SD daemon");
        // set up query
        let receiver = client.browse(&service_name).expect("Failed to browse");
        debug!("query started");

        // main loop
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

            // receive responses
            if let Ok(event) = receiver.recv_timeout(RECEIVE_TIMEOUT) {
                trace!("received event from resolver: {:?}", event);

                // process
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        trace!("got a resolved service of given service name: {}", info.get_fullname());

                        // check instance name - it is part of the fullname up until the first dot
                        let instance_name_this = info.get_fullname().split('.').next().expect("failed to get instance name from fullname");
                        if instance_name_this.contains(instance_name) {
                            debug!("got a resolved service with matching instance name, sending...");

                            //TODO add support for multiple IP addresses
                            //TODO add support for IPv6 address and which is prefferred result - IPv4 or IPv6
                            //TODO add support for TXT record properties in result

                            // send it
                            // NOTE: the first address is not always the best one, it could be a local address or an IP address that is not reachable from the network like a VPN address - OTOH, that is up to the publisher to do properly. Just FYI.
                            let address = info.get_addresses().iter().next().expect("failed to get first IP address");
                            let address_str;
                            if address.is_ipv6() {
                                address_str = format!("[{}]", address); //TODO better use URL instead of this formatting exception, which does this automatically
                            } else {
                                address_str = address.to_string();
                            }
                            let result = format!(
                                "tcp://{}:{}/{}",
                                address_str,
                                info.get_port(),
                                instance_name_this
                                ).into_bytes();
                            out.push(result).expect("could not push into OUT");    //TODO optimize conversion
                            out_wakeup.unpark();
                            debug!("done");
                        } else {
                            trace!("service name matches, but instance name mismatch: {} != {}, discarding result", instance_name_this, instance_name);
                        }

                        // what to do next?
                        break;
                    }
                    other_event => {
                        // NOTE: for example, before the ServiceResolved event, there is a ServiceFound event, but it has no addresses yet so it is not useful
                        trace!("received other event: {:?} - discarding", &other_event);
                    }
                }
            }

            trace!("-- end of iteration");
            //NOTE: dont park thread here - this is achieved by recv_timeout()
        }
        client.shutdown().expect("failed to shut down client");
        thread::sleep(Duration::from_millis(500));    // give it some time to shut down
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
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