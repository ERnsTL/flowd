use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

// component-specific
use redis::ErrorKind as RedisErrorKind;
use std::time::{Duration, Instant};

const REDIS_IO_TIMEOUT: Option<Duration> = Some(Duration::from_millis(0));

enum RedisPublisherState {
    WaitingForConfig,
    Connected {
        connection: redis::Connection,
        channel: String,
        pipeline: redis::Pipeline,
    },
    Finished,
}

pub struct RedisPublisherComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: RedisPublisherState,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for RedisPublisherComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        RedisPublisherComponent {
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
            state: RedisPublisherState::WaitingForConfig,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("RedisPublisher process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = RedisPublisherState::Finished;
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
            RedisPublisherState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url_str);

                    // Parse connection arguments and get channel from URL
                    let url = url::Url::parse(&url_str).expect("failed to parse URL");
                    let channel_queryparam = url
                        .query_pairs()
                        .find(|(key, _)| key.eq("channel"))
                        .expect("failed to get channel name from connection URL channel parameter");
                    let channel_bytes = channel_queryparam.1.as_bytes();
                    let channel = std::str::from_utf8(channel_bytes)
                        .expect("failed to convert channel name to str");
                    debug!("channel: {}", channel);

                    // Connect to Redis
                    let client = redis::Client::open(url_str).expect("failed to open client");
                    let connection = client
                        .get_connection()
                        .expect("failed to get connection on client");
                    connection
                        .set_read_timeout(REDIS_IO_TIMEOUT)
                        .expect("failed to set redis read timeout");
                    connection
                        .set_write_timeout(REDIS_IO_TIMEOUT)
                        .expect("failed to set redis write timeout");

                    let pipeline = redis::pipe();

                    self.state = RedisPublisherState::Connected {
                        connection,
                        channel: channel.to_owned(),
                        pipeline,
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

            RedisPublisherState::Connected {
                ref mut connection,
                channel,
                pipeline,
                ..
            } => {
                let mut work_units = 0;

                // Publish available messages within remaining budget
                while context.remaining_budget > 0 && !self.inn.is_empty() {
                    debug!(
                        "got {} packets, forwarding to redis channel.",
                        self.inn.slots()
                    );
                    let chunk = self
                        .inn
                        .read_chunk(self.inn.slots())
                        .expect("receive as chunk failed");

                    for ip in chunk.into_iter() {
                        debug!("publishing message to Redis channel: {}", channel);
                        pipeline.publish(channel.as_str(), ip).ignore();
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

                // Execute the pipeline if we have work to do
                if work_units > 0 {
                    if let Err(err) = pipeline.query::<()>(connection) {
                        if err.kind() == RedisErrorKind::Io {
                            warn!(
                                "redis publish timed out or failed with I/O error, dropping current batch and continuing: {}",
                                err
                            );
                        } else {
                            error!("failed to publish into redis channel: {}", err);
                            self.state = RedisPublisherState::Finished;
                            return ProcessResult::Finished;
                        }
                    }
                    pipeline.clear();
                }

                // Check if input is abandoned
                if self.inn.is_abandoned() {
                    info!("EOF on inport, shutting down");
                    self.state = RedisPublisherState::Finished;
                    return ProcessResult::Finished;
                }

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    context.wake_at(
                        Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                    );
                    ProcessResult::NoWork
                }
            }

            RedisPublisherState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("RedisPublisher"),
            description: String::from(
                "Publishes data as-is from IN port to the Redis MQ pub/sub channel given in CONF.",
            ),
            icon: String::from("cloud-upload"), // or arrow-circle-down
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from(
                        "connection URL which includes options, see redis crate documentation",
                    ), // see https://github.com/redis-rs/redis-rs/blob/45973d30c70c3817856095dda0c20401a8327207/redis/src/connection.rs#L282
                    values_allowed: vec![],
                    value_default: String::from(
                        "rediss://user:pass@server.com/db_number?channel=channel_name",
                    ),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be published on given Redis MQ channel"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}

enum RedisSubscriberState {
    WaitingForConfig,
    Connected {
        connection: redis::Connection,
        channel: String,
        subscribed: bool,
    },
    Finished,
}

pub struct RedisSubscriberComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    state: RedisSubscriberState,
    //graph_inout: GraphInportOutportHandle,
}

// how often the subscriber receive loop should check for signals from FBP network
const RECV_TIMEOUT: Option<Duration> = Some(Duration::from_millis(0));

impl Component for RedisSubscriberComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        RedisSubscriberComponent {
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
            state: RedisSubscriberState::WaitingForConfig,
            //graph_inout: graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("RedisSubscriber process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                self.state = RedisSubscriberState::Finished;
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
            RedisSubscriberState::WaitingForConfig => {
                // Try to get configuration
                if let Ok(url_vec) = self.conf.pop() {
                    let url_str = std::str::from_utf8(&url_vec).expect("invalid utf-8");
                    debug!("got config URL: {}", url_str);

                    // Parse connection arguments and get channel from URL
                    let url = url::Url::parse(&url_str).expect("failed to parse URL");
                    let channel_queryparam = url
                        .query_pairs()
                        .find(|(key, _)| key.eq("channel"))
                        .expect("failed to get channel name from connection URL channel parameter");
                    let channel_bytes = channel_queryparam.1.as_bytes();
                    let channel = std::str::from_utf8(channel_bytes)
                        .expect("failed to convert channel name to str");
                    debug!("channel: {}", channel);

                    // Connect to Redis. We keep the owned connection in component state and
                    // create a short-lived PubSub view per process() call to avoid unsafe
                    // lifetime extension tricks.
                    let client = redis::Client::open(url_str).expect("failed to open client");
                    let connection = client
                        .get_connection()
                        .expect("failed to get connection on client");

                    info!(
                        "Redis subscriber connected and subscribed to channel: {}",
                        channel
                    );
                    self.state = RedisSubscriberState::Connected {
                        connection,
                        channel: channel.to_owned(),
                        subscribed: false,
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

            RedisSubscriberState::Connected {
                connection,
                channel,
                subscribed,
            } => {
                let mut work_units = 0;
                let mut must_finish = false;

                // Keep PubSub borrow scoped so we can safely mutate component state afterwards.
                {
                    let mut pubsub = connection.as_pubsub();
                    if !*subscribed {
                        if let Err(err) = pubsub.subscribe(channel.as_str()) {
                            error!("failed to subscribe to channel: {}", err);
                            must_finish = true;
                        } else if let Err(err) = pubsub.set_read_timeout(RECV_TIMEOUT) {
                            error!("failed to set read timeout: {}", err);
                            must_finish = true;
                        } else {
                            *subscribed = true;
                            debug!("Redis subscriber listening on channel: {}", channel);
                        }
                    }

                    if !must_finish {
                        // Receive messages cooperatively within remaining budget
                        while context.remaining_budget > 0 {
                            match pubsub.get_message() {
                                Ok(msg) => {
                                    let payload: Vec<u8> =
                                        msg.get_payload().expect("failed to get message payload");
                                    debug!(
                                        "Received payload from redis channel '{}': {:?}",
                                        channel, payload
                                    );

                                    // Try to send it to output
                                    match self.out.push(payload) {
                                        Ok(()) => {
                                            debug!("forwarded redis payload");
                                            work_units += 1;
                                            context.remaining_budget -= 1;
                                        }
                                        Err(_) => {
                                            // Output buffer full, stop processing for now
                                            break;
                                        }
                                    }
                                }
                                Err(_) => {
                                    // No message available or timeout, yield to scheduler
                                    break;
                                }
                            }
                        }
                    }
                }

                if must_finish {
                    self.state = RedisSubscriberState::Finished;
                    return ProcessResult::Finished;
                }

                if work_units > 0 {
                    ProcessResult::DidWork(work_units)
                } else {
                    context.wake_at(
                        Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                    );
                    ProcessResult::NoWork
                }
            }

            RedisSubscriberState::Finished => ProcessResult::Finished,
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("RedisSubscriber"),
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
