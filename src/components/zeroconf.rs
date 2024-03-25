use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use simple_mdns::sync_discovery::ServiceDiscovery;
use simple_mdns::InstanceInformation;

pub struct ZeroconfResponderComponent {
    conf: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

const TTL_DEFAULT: u32 = 60;

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

        let socket_address = url.host_str().expect("failed to get socket address from connection URL").to_owned() + ":" + &url.port().expect("failed to get port from connection URL").to_string();
        let instance_name = url.path().strip_prefix("/").expect("failed to strip prefix '/' from instance name in configuration URL path").to_owned();

        let service_name_queryparam = url.query_pairs().find( |kv| kv.0.eq("service") ).expect("failed to get service from connection URL");
        let service_name_bytes = service_name_queryparam.1.as_bytes();
        let service_name = std::str::from_utf8(service_name_bytes).expect("failed to convert socket address to str");

        let ttl: u32;
        if let Some(ttl_queryparam)  = url.query_pairs().find( |kv| kv.0.eq("ttl") ) {
            let ttl_bytes = ttl_queryparam.1.as_bytes();
            ttl = std::str::from_utf8(ttl_bytes).expect("failed to convert channel name to str").parse::<u32>().expect("failed to parse ttl as u32");
        } else {
            trace!("failed to get ttl from connection URL");
            ttl = TTL_DEFAULT;
        }

        // configure
        // start service discovery responder
        // TODO optimize parameters
        let mut discovery = ServiceDiscovery::new(
            InstanceInformation::new(instance_name).with_socket_address(socket_address.parse().expect("invalid socket address for responder")),
            service_name,
            ttl
            ).expect("failed to start service discovery responder");
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
        discovery.remove_service_from_discovery();
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
                    value_default: String::from("mdns://192.168.1.123:8090/myweb?service=_http._tcp.local&ttl=60")
                }
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

pub struct ZeroconfQueryComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for ZeroconfQueryComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        ZeroconfQueryComponent {
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

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        let Ok(url_vec) = conf.pop() else { error!("no config IP received - exiting"); return; };
        let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");

        // prepare connection arguments
        // get destination from URL
        let url = url::Url::parse(&url_str).expect("failed to parse URL");
        //TODO fix - URLs are ASCII, there is .as_ascii() but requires some more conversions and is unstable feature
        let channel_queryparam = url.query_pairs().find( |kv| kv.0.eq("channel") ).expect("failed to get channel name from connection URL channel parameter");
        let channel_bytes = channel_queryparam.1.as_bytes();
        let channel = std::str::from_utf8(channel_bytes).expect("failed to convert channel name to str");
        debug!("channel: {}", channel);

        // connect
        let client = redis::Client::open(url_str).expect("failed to open client");
        let mut connection = client.get_connection().expect("failed to get connection on client");
        let mut pubsub = connection.as_pubsub();
        // subscribe to given topic
        //TODO enable reconnection - or is this done automatically via .iter()?
        pubsub.subscribe(channel).expect("failed to subscribe to channel");
        pubsub.set_read_timeout(RECV_TIMEOUT).expect("failed to set read timeout");    //TODO optimize Some packaging

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

            // receive packets
            while let Ok(msg) = pubsub.get_message() {
                //TODO optimize get_payload() there is some FromRedisType conversion involved
                let payload: Vec<u8> = msg.get_payload().expect("failed to get message payload");
                debug!("Received payload from redis channel: {:?}", payload);

                // send it
                debug!("forwarding redis payload...");
                out.push(payload).expect("could not push into OUT");    //TODO optimize conversion
                out_wakeup.unpark();
                debug!("done");
            }
            //TODO handle Err case - is is temporary error or permanent error?

            // are we done?
            //TODO handle EOF on redis connection? or does it automatically reconnect? what if it fails to reconnect and we better shut down?
            //TODO add automatic reconnection via crate "connection-manager" feature

            trace!("-- end of iteration");
            //NOTE: dont park thread here - this is done by recv_timeout()
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("ZeroconfQuery"),
            description: String::from("Subscribes to the Redis MQ pub/sub channel given in CONF and forwards received message data to the OUT outport."),
            icon: String::from("cloud-download"),   // or arrow-circle-down
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("connection URL which includes options, see redis crate documentation"),  // see https://github.com/redis-rs/redis-rs/blob/45973d30c70c3817856095dda0c20401a8327207/redis/src/connection.rs#L282
                    values_allowed: vec![],
                    value_default: String::from("rediss://user:pass@server.com/db_number?channel=channel_name")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("contents of received messages on given Redis MQ channel"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}