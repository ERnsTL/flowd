use std::{os::unix::thread::JoinHandleExt, sync::{Arc, Mutex}};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

use std::time::Duration;
use std::thread;

pub struct RedisPublisherComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for RedisPublisherComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        RedisPublisherComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports.remove("IN").expect("found no IN inport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("RedisPublisher is now run()ning!");
        let conf = &mut self.conf;
        let inn = &mut self.inn;    //TODO optimize these references, not really needed for them to be referenes, can just consume?

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
        let mut pipe_lpush = redis::pipe();

        // signal handling
        //TODO optimize - currently not used because we use connection error (connection aborted) to stop the thread
        // but there can be other reasons for connection close or an error...
        //let (eventthread_signalsink, eventthread_signalsource) = std::sync::mpsc::sync_channel::<()>(0);

        // handle connection events
        //TODO automatic reconnection
        //NOTE: currently this is not using async so we need no async handler thread
        /*
        let event_handler_thread = thread::Builder::new().name(format!("{}/EV", thread::current().name().expect("failed to get current thread name"))).spawn(move || {
            //TODO optimize - we dont want to poll for shutdown, so we currently use the Ok or Err distinction - maybe it can be more efficient with an iter() based solution?
            // but there can be other reasons for connection close or an error...
            while let Ok(event) = connection.recv() {
                match event {
                    Err(err) => {
                        trace!("Event Err = {:?}", err);
                    },
                    _ => {
                        trace!("Event = {:?}", event);
                        // nothing to do, not interested in any events
                        //TODO really?
                    }
                }
            }
            debug!("exiting");
        }).expect("failed to spawn eventloop listener thread");
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
                if let Ok(_ip) = inn.pop() {
                    debug!("got a packet, dropping it.");
                } else {
                    break;
                }
            }
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
                //debug!("got a packet, dropping it.");

                debug!("got {} packets, forwarding to redis channel.", inn.slots());
                let chunk = inn.read_chunk(inn.slots()).expect("receive as chunk failed");
                for ip in chunk.into_iter() {   //TODO is iterator faster or as_slices() or as_mut_slices() ?
                    //TODO optimize so that the channel name is already fixed - does the channel name get cloned for every push?
                    //TODO optimize - does it make sense to use PubSub object?
                    //pipe_lpush.lpush(channel, ip).ignore();  //TODO add error handling
                    pipe_lpush.publish(channel, ip).ignore();
                }
                // NOTE: no commit_all() necessary, because into_iter() does that automatically

                // send all queries at once
                pipe_lpush.query::<Vec<u8>>(&mut connection).expect("failed to publish into redis channel");
                pipe_lpush.clear();
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

        // stop redis event thread - close channel
        //drop(eventthread_signalsink);
        // close redis connection -> redis event thread will exit from special connection error ConnectionAborted
        //TODO there is no close() method anywhere - how to close the connection or the client?
        drop(pipe_lpush);
        drop(connection);
        drop(client);
        // wait for event thread to exit
        //event_handler_thread.join().expect("failed to join eventloop listener thread");

        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("RedisPublisher"),
            description: String::from("Publishes data as-is from IN port to the Redis MQ channel given in CONF."),
            icon: String::from("cloud-upload"), // or arrow-circle-down
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
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be published on given Redis MQ channel"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

pub struct RedisSubscriberComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

// how often the subscriber receive loop should check for signals from FBP network
const RECV_TIMEOUT: Option<Duration> = Some(Duration::from_millis(500));

impl Component for RedisSubscriberComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        RedisSubscriberComponent {
            conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(mut self) {
        debug!("RedisSubscriber is now run()ning!");
        let conf = &mut self.conf;    //TODO optimize
        let out = &mut self.out.sink;
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
            name: String::from("RedisSubscriber"),
            description: String::from("Subscribes to the Redis MQ channel given in CONF and forwards received message data to the OUT outport."),
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