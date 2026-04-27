fn message_as_utf8(msg: &MessageBuf) -> Option<&str> {
    msg.as_text()
        .or_else(|| msg.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
}

#[derive(Serialize, Deserialize, Debug)]
struct PersistedGraphSet {
    #[serde(rename = "activeGraph")]
    active_graph: String,
    graphs: HashMap<String, Graph>,
}

fn run_graph(
    runtime: Arc<RwLock<Runtime>>,
    graph: Arc<RwLock<Graph>>,
    components: Arc<RwLock<ComponentLibrary>>,
    graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
) -> std::result::Result<(), std::io::Error> {
    let graph_guard = graph.read().expect("lock poisoned");
    let components_guard = components.read().expect("lock poisoned");
    runtime.write().expect("lock poisoned").start(
        &graph_guard,
        &components_guard,
        graph_inout,
        runtime.clone(),
    )?;
    Ok(())
}

/// Create a new runtime instance
pub fn create_runtime(graph_name: String) -> Arc<RwLock<Runtime>> {
    Arc::new(RwLock::new(Runtime::new(graph_name)))
}

/// Create a component library with all available components
pub fn create_component_library() -> Arc<RwLock<ComponentLibrary>> {
    build_component_library()
}

/// Create a graph inport/outport holder
pub fn create_graph_inout_holder() -> Arc<Mutex<GraphInportOutportHolder>> {
    Arc::new(Mutex::new(GraphInportOutportHolder {
        inports: None,
        outports: None,
        websockets: HashMap::new(),
        packet_tap: None,
    }))
}

fn create_default_graph() -> std::result::Result<Graph, std::io::Error> {
    let mut graph = Graph::new(
        String::from("main_graph"),
        String::from("basic description"),
        String::from("usd"),
    );

    // add graph exported/published inport and outport
    graph.inports.insert(
        "GRAPHIN".to_owned(),
        GraphPort {
            process: "Repeat_31337".to_owned(),
            port: "IN".to_owned(),
            metadata: GraphPortMetadata { x: 36, y: 72 },
        },
    );
    graph.outports.insert(
        "GRAPHOUT".to_owned(),
        GraphPort {
            process: "Repeat_31338".to_owned(),
            port: "OUT".to_owned(),
            metadata: GraphPortMetadata { x: 468, y: 72 },
        },
    );
    graph.add_node(
        "main_graph".to_owned(),
        "Repeat".to_owned(),
        "Repeat_31337".to_owned(),
        GraphNodeMetadata {
            x: 180,
            y: 72,
            height: Some(NODE_HEIGHT_DEFAULT),
            width: Some(NODE_WIDTH_DEFAULT),
            label: Some("Repeat".to_owned()),
            icon: None,
        },
    )?;
    //NOTE: bug in noflo-ui, which does not allow reconnecting exported ports to other components, they just vanish then (TODO)
    graph.add_edge(
        "main_graph".to_owned(),
        GraphEdge {
            source: GraphNodeSpec {
                process: "Repeat_31337".to_owned(),
                port: "OUT".to_owned(),
                index: None,
            },
            data: None,
            target: GraphNodeSpec {
                process: "Repeat_31338".to_owned(),
                port: "IN".to_owned(),
                index: None,
            },
            metadata: GraphEdgeMetadata::new(None, None, None),
        },
    )?;
    graph.add_node(
        "main_graph".to_owned(),
        "Repeat".to_owned(),
        "Repeat_31338".to_owned(),
        GraphNodeMetadata {
            x: 324,
            y: 72,
            height: Some(NODE_HEIGHT_DEFAULT),
            width: Some(NODE_WIDTH_DEFAULT),
            label: Some("Repeat".to_owned()),
            icon: None,
        },
    )?;
    // add components required for test suite
    graph.add_node(
        "main_graph".to_owned(),
        "Repeat".to_owned(),
        "Repeat_2ufmu".to_owned(),
        GraphNodeMetadata {
            x: 36,
            y: 216,
            height: Some(NODE_HEIGHT_DEFAULT),
            width: Some(NODE_WIDTH_DEFAULT),
            label: Some("Repeat".to_owned()),
            icon: None,
        },
    )?;
    graph.add_node(
        "main_graph".to_owned(),
        "Drop".to_owned(),
        "Drop_raux7".to_owned(),
        GraphNodeMetadata {
            x: 324,
            y: 216,
            height: Some(NODE_HEIGHT_DEFAULT),
            width: Some(NODE_WIDTH_DEFAULT),
            label: Some("Drop".to_owned()),
            icon: None,
        },
    )?;
    graph.add_node(
        "main_graph".to_owned(),
        "Output".to_owned(),
        "Output_mwr5y".to_owned(),
        GraphNodeMetadata {
            x: 180,
            y: 216,
            height: Some(NODE_HEIGHT_DEFAULT),
            width: Some(NODE_WIDTH_DEFAULT),
            label: Some("Output".to_owned()),
            icon: None,
        },
    )?;
    graph.add_edge(
        "main_graph".to_owned(),
        GraphEdge {
            source: GraphNodeSpec {
                process: "".to_owned(),
                port: "".to_owned(),
                index: None,
            },
            data: Some("test IIP data".to_owned()),
            target: GraphNodeSpec {
                process: "Repeat_2ufmu".to_owned(),
                port: "IN".to_owned(),
                index: None,
            },
            metadata: GraphEdgeMetadata::new(None, None, None),
        },
    )?;
    graph.add_edge(
        "main_graph".to_owned(),
        GraphEdge {
            source: GraphNodeSpec {
                process: "Repeat_2ufmu".to_owned(),
                port: "OUT".to_owned(),
                index: None,
            },
            data: None,
            target: GraphNodeSpec {
                process: "Output_mwr5y".to_owned(),
                port: "IN".to_owned(),
                index: None,
            },
            metadata: GraphEdgeMetadata::new(None, None, None),
        },
    )?;
    graph.add_edge(
        "main_graph".to_owned(),
        GraphEdge {
            source: GraphNodeSpec {
                process: "Output_mwr5y".to_owned(),
                port: "OUT".to_owned(),
                index: None,
            },
            data: None,
            target: GraphNodeSpec {
                process: "Drop_raux7".to_owned(),
                port: "IN".to_owned(),
                index: None,
            },
            metadata: GraphEdgeMetadata::new(None, None, None),
        },
    )?;
    Ok(graph)
}

/// Load graph set from persistence file or create a default set.
pub fn load_or_create_graph_set(
) -> std::result::Result<(HashMap<String, Arc<RwLock<Graph>>>, String), std::io::Error> {
    if Path::new(PERSISTENCE_FILE_NAME).exists() {
        let json_data = std::fs::read_to_string(PERSISTENCE_FILE_NAME)?;
        if let Ok(graph_set) = serde_json::from_str::<PersistedGraphSet>(&json_data) {
            let mut graph_map: HashMap<String, Arc<RwLock<Graph>>> = HashMap::new();
            for (graph_id, graph) in graph_set.graphs {
                graph_map.insert(graph_id, Arc::new(RwLock::new(graph)));
            }
            if graph_map.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "persisted graph set is empty",
                ));
            }
            let active = if graph_map.contains_key(&graph_set.active_graph) {
                graph_set.active_graph
            } else {
                graph_map
                    .keys()
                    .next()
                    .expect("already checked non-empty")
                    .clone()
            };
            info!(
                "loaded multi-graph persistence with {} graph(s), active '{}'",
                graph_map.len(),
                active
            );
            return Ok((graph_map, active));
        }

        let graph: Graph = serde_json::from_str(&json_data)?;
        let graph_name = graph.properties.name.clone();
        let mut graph_map: HashMap<String, Arc<RwLock<Graph>>> = HashMap::new();
        graph_map.insert(graph_name.clone(), Arc::new(RwLock::new(graph)));
        info!("loaded single-graph persistence, graph '{}'", graph_name);
        Ok((graph_map, graph_name))
    } else {
        let graph = create_default_graph()?;
        let graph_name = graph.properties.name.clone();
        let mut graph_map: HashMap<String, Arc<RwLock<Graph>>> = HashMap::new();
        graph_map.insert(graph_name.clone(), Arc::new(RwLock::new(graph)));
        info!("graph initialized");
        Ok((graph_map, graph_name))
    }
}

/// Load active graph from persistence, keeping backward compatibility for single-graph callers.
pub fn load_or_create_graph() -> std::result::Result<Arc<RwLock<Graph>>, std::io::Error> {
    let (graph_map, active_graph) = load_or_create_graph_set()?;
    graph_map.get(&active_graph).cloned().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "active graph not found after load",
        )
    })
}

// module-based components
mod components;
// crate-based components
include!(concat!(env!("OUT_DIR"), "/build_generated.rs"));

// multi-graph support
pub mod multi_graph;

// scheduler for new execution model
pub mod scheduler;

// server module
pub mod server;

// test harness for testing strategy implementation
pub mod test_harness;

fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

//TODO currently panicks if unknown variant
//TODO currently panicks if field is missing during decoding
//TODO note messages which are used multiple times
//NOTE: deny unknown fields to learn them (serde) deny_unknown_fields, but problem is that "protocol" field is still present -> panic
#[derive(Deserialize, Debug)]
#[serde(tag = "protocol")]
enum FBPMessage {
    #[serde(rename = "runtime")]
    Runtime(RuntimeMessage),
    #[serde(rename = "network")]
    Network(NetworkMessage),
    #[serde(rename = "component")]
    Component(ComponentMessage),
    #[serde(rename = "graph")]
    Graph(GraphMessage),
    #[serde(rename = "trace")]
    Trace(TraceMessage),
}

// ----------
// runtime base -- no capabilities required
// ----------

// runtime:getruntime -> runtime:runtime | runtime:error
#[derive(Deserialize, Debug)]
struct RuntimeGetruntimePayload {
    // Spec text says required, but fbp-protocol tests send this as undefined.
    secret: Option<String>, // required per FBP protocol spec, but optional according to fbp-tests
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

impl RuntimeRuntimeMessage {
    fn new(payload: RuntimeRuntimePayload) -> Self {
        RuntimeRuntimeMessage {
            protocol: String::from("runtime"),
            command: String::from("runtime"),
            payload,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRuntimePayload {
    id: String,                        // spec: UUID of this runtime instance
    label: String,                     // spec: human-readable description of the runtime
    version: String, // spec: supported protocol version //TODO which versions are there? implement proper
    all_capabilities: Vec<Capability>, // spec: capabilities supported by runtime
    capabilities: Vec<Capability>, // spec: capabities for you //TODO implement privilege level restrictions
    graph: String,                 // spec: currently active graph
    #[serde(rename = "type")]
    runtime: String, // spec: name of the runtime software, "flowd"
    namespace: String, // spec: namespace of components for this project of top-level graph
    repository: String, // spec: source code repository of this runtime software //TODO but it is the repo of the graph, is it?
    repository_version: String, // spec: repository version of this software build
}

#[derive(Debug)]
pub struct Runtime {
    // external-facing runtime metadata
    id: String,                        // spec: UUID of this runtime instance
    label: String,                     // spec: human-readable description of the runtime
    version: String,                   // spec: supported protocol version
    all_capabilities: Vec<Capability>, // spec: capabilities supported by runtime
    capabilities: Vec<Capability>,     // spec: capabilities granted to client
    graph: String,                     // spec: currently active graph
    runtime: String,                   // spec: runtime software, "flowd"
    namespace: String,                 // spec: namespace of components for active graph
    repository: String,                // spec: source repository URL
    repository_version: String,        // spec: build/repository version

    // runtime state
    status: RuntimeStatus, // for network:status, network:started, network:stopped
    //TODO ^ also contains graph = active graph, maybe replace status.graph with a pointer so that not 2 updates are neccessary?
    tracing: bool,                           //TODO implement
    trace_sender: Option<std::sync::mpsc::Sender<FbpMessage>>,
    boundary_threads: BoundaryThreadManager, // non-component boundary handlers (for example graph outport bridge); scheduler executes components
    watchdog_thread: Option<std::thread::JoinHandle<()>>,
    watchdog_channel: Option<std::sync::mpsc::SyncSender<MessageBuf>>,
    scheduler_threads: HashMap<String, std::thread::JoinHandle<()>>, // per-graph scheduler threads
    schedulers: HashMap<String, Arc<crate::scheduler::Scheduler>>,   // per-graph schedulers
    secrets: HashMap<String, (String, AccessLevel)>, // graph name -> (secret token, access level) for token-based security
    graphs: multi_graph::MultiGraphManager,          // multi-graph support
}

#[derive(Debug)]
pub struct RuntimeSnapshot {
    id: String,
    label: String,
    version: String,
    all_capabilities: Vec<Capability>,
    capabilities: Vec<Capability>,
    graph: String,
    runtime: String,
    namespace: String,
    repository: String,
    repository_version: String,
}

#[derive(Debug, Clone)]
struct RuntimeStatus {
    time_started: chrono::DateTime<Utc>,
    graph: String,
    started: bool,
    running: bool,
    debug: Option<bool>,
}

#[derive(Debug, Clone)]
struct RuntimeStatusSnapshot {
    time_started: chrono::DateTime<Utc>,
    graph: String,
    started: bool,
    running: bool,
    debug: Option<bool>,
    scheduler_metrics: HashMap<String, SchedulerMetricsSnapshot>,
}

#[derive(Debug, Clone)]
struct SchedulerMetricsSnapshot {
    executions_per_node: HashMap<String, u64>,
    work_units_per_node: HashMap<String, u64>,
    time_since_last_execution_ms: HashMap<String, u64>,
    queue_depth: usize,
    loop_iterations: u64,
}

impl Default for RuntimeRuntimePayload {
    fn default() -> Self {
        RuntimeRuntimePayload {
            id: String::from("f18a4924-9d4f-414d-a37c-deadbeef0000"), //TODO actually random UUID
            label: String::from("human-readable description of the runtime"), //TODO useful text
            version: String::from("0.7"), //TODO actually implement that - what about features+changes post-0.7?
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
                Capability::GraphReadonly,
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
            runtime: String::from("flowd"),       //TODO constant - optimize
            namespace: String::from("main"),      // namespace of components TODO implement
            repository: String::from("https://github.com/ERnsTL/flowd.git"), //TODO use this feature of building and saving the graph into a Git repo
            repository_version: String::from("0.0.1-ffffffff"), //TODO use actual git commit and actual version
        }
    }
}

impl Default for Runtime {
    fn default() -> Self {
        let payload = RuntimeRuntimePayload::default();
        Runtime {
            id: payload.id,
            label: payload.label,
            version: payload.version,
            all_capabilities: payload.all_capabilities,
            capabilities: payload.capabilities,
            graph: payload.graph,
            runtime: payload.runtime,
            namespace: payload.namespace,
            repository: payload.repository,
            repository_version: payload.repository_version,
            status: RuntimeStatus::default(),
            tracing: false,
            trace_sender: None,
            boundary_threads: BoundaryThreadManager::default(),
            watchdog_thread: None,
            watchdog_channel: None,
            scheduler_threads: HashMap::new(),
            schedulers: HashMap::new(),
            secrets: HashMap::new(),
            graphs: multi_graph::MultiGraphManager::new(),
        }
    }
}

impl From<RuntimeSnapshot> for RuntimeRuntimePayload {
    fn from(snapshot: RuntimeSnapshot) -> Self {
        RuntimeRuntimePayload {
            id: snapshot.id,
            label: snapshot.label,
            version: snapshot.version,
            all_capabilities: snapshot.all_capabilities,
            capabilities: snapshot.capabilities,
            graph: snapshot.graph,
            runtime: snapshot.runtime,
            namespace: snapshot.namespace,
            repository: snapshot.repository,
            repository_version: snapshot.repository_version,
        }
    }
}

impl Runtime {
    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            id: self.id.clone(),
            label: self.label.clone(),
            version: self.version.clone(),
            all_capabilities: self.all_capabilities.clone(),
            capabilities: self.capabilities.clone(),
            graph: self.graph.clone(),
            runtime: self.runtime.clone(),
            namespace: self.namespace.clone(),
            repository: self.repository.clone(),
            repository_version: self.repository_version.clone(),
        }
    }

    fn status_snapshot(&self) -> RuntimeStatusSnapshot {
        let status = self.status.snapshot();
        RuntimeStatusSnapshot {
            time_started: status.time_started,
            graph: status.graph,
            started: status.started,
            running: status.running,
            debug: status.debug,
            scheduler_metrics: self.collect_scheduler_metrics(),
        }
    }

    fn collect_scheduler_metrics(&self) -> HashMap<String, SchedulerMetricsSnapshot> {
        let mut snapshots = HashMap::new();
        for (graph_name, scheduler) in &self.schedulers {
            let metrics = scheduler.metrics_snapshot();
            let time_since_last_execution_ms = metrics
                .time_since_last_execution
                .iter()
                .map(|(node_id, duration)| {
                    let millis = duration.as_millis();
                    let clamped = if millis > u64::MAX as u128 {
                        u64::MAX
                    } else {
                        millis as u64
                    };
                    (node_id.clone(), clamped)
                })
                .collect::<HashMap<_, _>>();

            snapshots.insert(
                graph_name.clone(),
                SchedulerMetricsSnapshot {
                    executions_per_node: metrics.executions_per_node,
                    work_units_per_node: metrics.work_units_per_node,
                    time_since_last_execution_ms,
                    queue_depth: metrics.queue_depth,
                    loop_iterations: metrics.loop_iterations,
                },
            );
        }
        snapshots
    }

    fn new(active_graph: String) -> Self {
        let mut graphs = multi_graph::MultiGraphManager::new();
        // Create initial graph
        let initial_graph = Arc::new(RwLock::new(Graph::new(
            active_graph.clone(),
            "Initial graph".to_string(),
            "default".to_string(),
        )));
        graphs.add_graph(active_graph.clone(), initial_graph);

        Runtime {
            graph: active_graph.clone(), //TODO any way to avoid the clone and point to the other one?
            status: RuntimeStatus {
                time_started: chrono::DateTime::<Utc>::MIN_UTC, // zero value - //TODO rather use None
                graph: active_graph,
                started: false,
                running: false,
                debug: None,
            },
            graphs,
            schedulers: HashMap::new(),
            scheduler_threads: HashMap::new(),
            trace_sender: None,
            ..Default::default() //TODO mock other fields as well
        }
    }

    // persistence
    fn persist(&self, graph: &Graph) -> std::result::Result<(), std::io::Error> {
        //###
        // get source
        //TODO is the source according to the FBP JSON Network Protocol the same as the specified FBP JSON Graph format or does it differ? In which do we want to persist?
        //TODO where in noflo-ui is the button to trigger network:persist command?

        //TODO check for valid graph - do we want to allow unconnected ports, work in progress state?
        //TODO add integrity checker:
        //  critical and non-critical errors
        //  critical - cannot load, cannot start network
        //  noncritical - found missing connections, unconnected ports, unavailable components.

        //TODO automatic saving in time intervals? how many autosaves to keep? how to handle autosave on crash and after restart?

        //TODO saving on ctrl-c? No, ctrl-c means "abort".
        //TODO saving on panic? yes. goal: never lose your data.

        //TODO handle .bak file

        let mut persisted_graphs: HashMap<String, Graph> = HashMap::new();
        for graph_id in self.graphs.list_graphs() {
            if let Some(graph_arc) = self.graphs.get_graph(&graph_id) {
                let graph_read = graph_arc.read().expect("lock poisoned");
                let graph_value = serde_json::to_value(&*graph_read).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("failed to serialize graph '{}': {}", graph_id, err),
                    )
                })?;
                let graph_obj: Graph = serde_json::from_value(graph_value).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("failed to clone graph '{}': {}", graph_id, err),
                    )
                })?;
                persisted_graphs.insert(graph_id, graph_obj);
            }
        }
        if persisted_graphs.is_empty() {
            persisted_graphs.insert(
                self.graph.clone(),
                serde_json::from_value(serde_json::to_value(graph).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("failed to serialize fallback graph: {}", err),
                    )
                })?)
                .map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("failed to clone fallback graph: {}", err),
                    )
                })?,
            );
        }
        let active_graph = if persisted_graphs.contains_key(&self.graph) {
            self.graph.clone()
        } else {
            persisted_graphs
                .keys()
                .next()
                .expect("graph set is non-empty")
                .clone()
        };
        let graph_set = PersistedGraphSet {
            active_graph,
            graphs: persisted_graphs,
        };

        let mut output = File::create(PERSISTENCE_FILE_NAME)?;
        output.write(
            serde_json::to_string_pretty(&graph_set)
                .map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("failed to encode graph set: {}", err),
                    )
                })?
                .as_bytes(),
        )?;

        Ok(())
    }

    //fn start(&mut self, graph: &Graph, boundary_threads: &mut BoundaryThreadManager) -> std::result::Result<&RuntimeStatus, std::io::Error> {
    fn start(
        &mut self,
        graph: &Graph,
        components: &ComponentLibrary,
        graph_inout_arc: Arc<Mutex<GraphInportOutportHolder>>,
        runtime: Arc<RwLock<Runtime>>,
    ) -> std::result::Result<&RuntimeStatus, std::io::Error> {
        info!("starting network for graph {}", graph.properties.name);
        if self.status.running
            || !self.boundary_threads.is_empty()
            || self.watchdog_thread.is_some()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                String::from("network already running or previous shutdown incomplete"),
            ));
        }

        // Initialize scheduler for this graph if not already done
        let graph_name = graph.properties.name.clone();
        if !self.schedulers.contains_key(&graph_name) {
            self.schedulers.insert(
                graph_name.clone(),
                Arc::new(crate::scheduler::Scheduler::new()),
            );
        }

        // Initialize trace channel and thread if tracing is enabled
        if self.tracing {
            let (trace_tx, trace_rx) = std::sync::mpsc::channel::<FbpMessage>();
            self.trace_sender = Some(trace_tx);

            // Clone graph_inout for trace thread
            let graph_inout_clone = graph_inout_arc.clone();

            // Start trace broadcasting thread
            std::thread::spawn(move || {
                debug!("trace broadcasting thread started");
                while let Ok(trace_message) = trace_rx.recv() {
                    match trace_message {
                        FbpMessage::TraceData(payload) => {
                            send_trace_data(&graph_inout_clone, payload);
                        }
                        FbpMessage::TraceConnect(payload) => {
                            send_trace_connect(&graph_inout_clone, payload);
                        }
                        FbpMessage::TraceDisconnect(payload) => {
                            send_trace_disconnect(&graph_inout_clone, payload);
                        }
                        _ => warn!("received non-trace message in trace channel"),
                    }
                }
                debug!("trace broadcasting thread exiting");
            });
        }

        // Get scheduler reference for component registration
        let scheduler_arc = self.schedulers.get(&graph_name).unwrap().clone();
        // get all graph in and out ports
        let mut graph_inout = graph_inout_arc
            .lock()
            .expect("could not acquire lock for network start()");
        //TODO implement
        //TODO implement: what to do with the old running processes, stop using signal channel? What if they dont respond?
        //TODO implement: what if the name of the node changes? then the process is not found by that name anymore in the process manager

        //TODO check if self.boundary_threads.len() == 0 AKA stopped

        //TODO optimize thread sync and packet transfer
        // use [`channel`]s, [`Condvar`]s, [`Mutex`]es or [`join`]
        // -> https://doc.rust-lang.org/std/sync/index.html
        // or even basic:  https://doc.rust-lang.org/std/thread/fn.park.html
        //    -> would have to hand over Arc<BoundaryThreadManager> to each boundary handler, then it gets the thread handle out of this.
        //    Problem:  Does not know which process is on the other end, so would also have to hand over the process name on the other end. ugly.
        // -> better to hand over something that can be pre-generated and cloned = no difference between "sender" and "receiver"
        // -> https://doc.rust-lang.org/std/sync/struct.Condvar.html
        // foreach node in graph.nodes: pre-generate condvar + mutex here so that the next foreach over the edges can put it into a tuple on each ProcessEdgeSink
        // then the sending process can wake up the next process in the graph like a flush() on the according outport
        // putting in a JoinHandle or Thread is not possible because Rust does not allow creating a "prepared empty" Thread and pre-generating these
        // more ideas:
        //   https://stackoverflow.com/questions/37964467/how-to-freeze-a-thread-and-notify-it-from-another
        //   https://doc.rust-lang.org/std/sync/struct.Condvar.html#method.wait_timeout
        //   https://github.com/kirillkh/monitor_rs
        // what is the difference in cost between mutex+condvar, SyncChannel<()> and thread.park?
        // We need a sync mechanism that is SPSC, where the writer blocks if full, the reader blocks if empty and CPU-free waiting
        // General question: Should the Component be dumb and its process() be called to work off any packets (easy case in terms of thread sync)
        //   or should the Component be in control? Yes, because it might have external dependencies that have different timing than the FBP network (connections to external services etc.) so it must be able to manage itself
        //   -> we need a way for the Component to receive a wakeup call, but that does nothing if it is already actively processing messages = no cluttering with () wakeups, so it must be a capacity 1 channel that does not block the sender if full. only full bounded connection buffer should block. Hm, but sending could already block before it has any chance to wake up the next process.
        //   -> so the sending mechanism and the wakeup mechanism must be the same thing (or it can be 2 separate things inside, but it must be 1 call from the sender's perspective)
        // sending first packet of 10 should already wake the receiver.
        // but what if the sender is slower at producing than the receiver at consuming than the receiver?
        // -> batching would make sense. but only the producer knows how much how big its batch is and how many it will produce in the next time units.
        // but batching increases latency

        // How thread sync is currently done:
        // 1. during edge generation, for each outport, save the process name in a separate data structure
        // 2. start the threads, hand them over the thread handles hashmap, but first thing they do is park
        // 3. network start (= this fn) starts all threads for the FBP processes and stores their threads handles in the thread handles hashmap where key = FBP process name
        //    -> now all thread handles are complete in the hashmap
        // 4. then unpark/wake all threads,
        // 5. each thread replaces the FBP process name on its outports with the thread handle, then drops the Arc ref to the thread handles hashmap
        // 6. each thread instantiates its component and calls run()
        // 7. each component gets the thread handle and stores it for wake-up after it sent a packet on the outport
        //    -> each component can decide when it will wake the next FBP process (after first packet, after a few have been generated or when it is done for the iteration or when the channel is full etc.)

        //TODO optimize: check performance, maybe this could be done easier using the signal channel sending (), but since the thread_handle already has blocking feature built-in...

        // prepare graph_inout reference for watchdog stopping the network
        let watchdog_graph_inout = graph_inout_arc.clone();
        // prepare runtime reference for watchdong stopping the network
        let watchdog_runtime = runtime;
        // prepare per-process watchdog control + response channels
        let mut watchdog_threadandsignal: HashMap<
            String,
            (
                std::sync::mpsc::SyncSender<MessageBuf>,
                Thread,
                Arc<AtomicBool>,
                ProcessSignalSource,
                bool,
            ),
        > = HashMap::new();

        // arrayports: check if multiple edges originate from the same source port
        // and verify that port is declared as arrayport.
        let mut known_source_processes: MultiMap<String, String> = MultiMap::new();
        for edge in graph.edges.iter() {
            // ignore IIP edges
            if edge.data.is_some() && edge.source.process.is_empty() {
                continue;
            }
            if let Some(ports) = known_source_processes.get_vec_mut(&edge.source.process) {
                if ports.contains(&edge.source.port) {
                    // check if source port is array port
                    let component_name = &graph
                        .nodes
                        .get(&edge.source.process)
                        .expect("source process not found")
                        .component;
                    for component in components.available.iter() {
                        if component.name == *component_name {
                            for port in component.out_ports.iter() {
                                if port.name == edge.source.port {
                                    if !port.is_arrayport {
                                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, String::from("multiple edges from source process port which is not an array port")));
                                    } else {
                                        debug!(
                                            "found correct in-use array outport {}.{}",
                                            edge.source.process, edge.source.port
                                        );
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // first use of that outport on this process
                    ports.push(edge.source.port.clone());
                }
            } else {
                // first use of that process
                known_source_processes
                    .insert(edge.source.process.clone(), edge.source.port.clone());
            }
        }
        drop(known_source_processes);
        // arrayports: check if all edges having the same target process have a target port that is marked as an arry port
        let mut known_target_processes: MultiMap<String, String> = MultiMap::new();
        for edge in graph.edges.iter() {
            if let Some(ports) = known_target_processes.get_vec_mut(&edge.target.process) {
                // check for multiple connections to the same target port
                if ports.contains(&edge.target.port) {
                    // check if target port is array port, since we now have multiple connections into that target port
                    let component_name = &graph
                        .nodes
                        .get(&edge.target.process)
                        .expect("target process not found")
                        .component;
                    for component in components.available.iter() {
                        if component.name == *component_name {
                            for port in component.in_ports.iter() {
                                if port.name == edge.target.port {
                                    if !port.is_arrayport {
                                        // error
                                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, String::from("multiple edges to target process port which is not an array port")));
                                    //TODO say which ones
                                    } else {
                                        debug!(
                                            "found correct in-use array inport {}.{}",
                                            edge.target.process, edge.target.port
                                        );
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // add to list (under that process name), first use of that port
                    ports.push(edge.target.port.clone());
                }
            } else {
                // add to list, first use of that process
                known_target_processes
                    .insert(edge.target.process.clone(), edge.target.port.clone());
            }
        }
        drop(known_target_processes);

        // generate all connections
        struct ProcPorts {
            inports: ProcessInports, // including ports with IIPs
            outports: ProcessOutports,
        }
        impl Default for ProcPorts {
            fn default() -> Self {
                ProcPorts {
                    inports: ProcessInports::new(),
                    outports: ProcessOutports::new(),
                }
            }
        }
        impl std::fmt::Debug for ProcPorts {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("ProcPorts")
                    .field("inports", &self.inports)
                    .field("outports", &self.outports)
                    .finish()
            }
        }
        let mut ports_all: HashMap<String, ProcPorts> = HashMap::with_capacity(graph.nodes.len());
        // set up keys
        for proc_name in graph.nodes.keys().into_iter() {
            //TODO would be nice to know the name of the process
            ports_all
                .try_insert(proc_name.clone(), ProcPorts::default())
                .expect("preparing edges for process failed: process name already exists");
        }
        //TODO using graph name as fake process, but does that imply we cannot change the graph name during runtime?
        if graph.inports.len() > 0 {
            ports_all
                .try_insert(
                    format!("{}-IN", graph.properties.name),
                    ProcPorts::default(),
                )
                .expect("preparing inport edges for graph failed: process name already exists");
        }
        if graph.outports.len() > 0 {
            ports_all
                .try_insert(
                    format!("{}-OUT", graph.properties.name),
                    ProcPorts::default(),
                )
                .expect("preparing outport edges for graph failed: process name already exists");
        }

        // Clone scheduler for signaling closures
        let scheduler_for_signaling = scheduler_arc.clone();
        // fill keys with connections
        for edge in graph.edges.iter() {
            if let Some(iip) = &edge.data {
                // prepare IIP edge
                debug!(
                    "preparing edge from IIP to {}.{}",
                    edge.target.process, edge.target.port
                );
                //TODO sink will not be hooked up to anything when leaving this for loop; is that good?
                let (mut sink, source) = ProcessEdge::new(PROCESSEDGE_IIP_BUFSIZE);
                // send IIP
                sink.push(FbpMessage::from_text(iip.clone()))
                    .expect("failed to send IIP into process channel"); //TODO optimize as_bytes() / clone or String in Edge struct
                                                                        // insert into inports of target process
                let targetproc = ports_all
                    .get_mut(&edge.target.process)
                    .expect("process IIP target assignment process not found");
                // arrayports: insert, but remember that this is a multimap
                // Compatibility: graph payloads from clients/tests often use lowercase "in"/"out",
                // but built-in components require uppercase port keys ("IN"/"OUT").
                targetproc
                    .inports
                    .insert(edge.target.port.to_ascii_uppercase(), source);
                // assign into outports of source process
                // nothing to do in case of IIP - this also means that sink will go ouf ot scope and that source.is_abandoned() = Arc::strong_count() will be 1
                // in summary: IIP ports are closed/abandoned
            } else {
                // prepare edge
                debug!(
                    "preparing edge from {}.{} to {}.{}",
                    edge.source.process, edge.source.port, edge.target.process, edge.target.port
                );
                let (sink, source) = ProcessEdge::new(PROCESSEDGE_BUFSIZE);

                // insert into inports of target process
                let targetproc = ports_all
                    .get_mut(&edge.target.process)
                    .expect("process IIP target assignment process not found");
                // arrayports: insert, but remember that this is a multimap
                // Compatibility: normalize protocol port names to component runtime port names.
                targetproc
                    .inports
                    .insert(edge.target.port.to_ascii_uppercase(), source);
                // assign into outports of source process
                let sourceproc = ports_all
                    .get_mut(&edge.source.process)
                    .expect("process source assignment process not found");
                // arrayports: insert, but remember that this is a multimap
                let target_process = edge.target.process.clone();
                let scheduler_clone = scheduler_for_signaling.clone();
                let mut edge_sink = ProcessEdgeSink::new(
                    sink,
                    None,
                    Some(target_process.clone()),
                    Some(Arc::new(move || {
                        let _ = scheduler_clone.signal_ready(&target_process);
                    })),
                );

                // Enable tracing if tracing is enabled
                if self.tracing {
                    let edge_id = format!("{}.{}-{}.{}", edge.source.process, edge.source.port, edge.target.process, edge.target.port);
                    if let Some(trace_sender) = &self.trace_sender {
                        edge_sink.enable_tracing(
                            edge_id,
                            edge.source.process.clone(),
                            edge.source.port.clone(),
                            edge.target.process.clone(),
                            edge.target.port.clone(),
                            graph.properties.name.clone(),
                            trace_sender.clone(),
                        );
                    }
                }

                sourceproc.outports.insert(
                    edge.source.port.to_ascii_uppercase(),
                    edge_sink,
                );

                // Emit trace:connect event if tracing is enabled
                if self.tracing {
                    let edge_id = format!("{}.{}-{}.{}", edge.source.process, edge.source.port, edge.target.process, edge.target.port);
                    let connect_payload = ApiTraceConnectEventPayload {
                        id: edge_id,
                        src: TraceGraphNodeSpecNetwork {
                            node: edge.source.process.clone(),
                            port: edge.source.port.clone(),
                            index: edge.source.index.clone(),
                        },
                        tgt: TraceGraphNodeSpecNetwork {
                            node: edge.target.process.clone(),
                            port: edge.target.port.clone(),
                            index: edge.target.index.clone(),
                        },
                        graph: graph.properties.name.clone(),
                    };
                    send_trace_connect(&graph_inout_arc, connect_payload);
                }
            }
        }
        for (public_name, edge) in graph.inports.iter() {
            // prepare edge
            debug!(
                "preparing edge from graph {} to {}.{}",
                public_name, edge.process, edge.port
            );
            let (sink, source) = ProcessEdge::new(PROCESSEDGE_BUFSIZE);

            // insert into inports of target process
            let targetproc = ports_all
                .get_mut(&edge.process)
                .expect("graph target assignment process not found");
            // arrayports: insert, but remember that this is a multimap
            // Compatibility: normalize graph export mapping to uppercase runtime port names.
            targetproc
                .inports
                .insert(edge.port.to_ascii_uppercase(), source);
            // assign into outports of source process
            // source process name = graphname-IN
            let sourceproc: &mut ProcPorts = ports_all
                .get_mut(format!("{}-IN", graph.properties.name).as_str())
                .expect("graph source assignment process not found");
            // arrayports: insert, but remember that this is a multimap
            let target_process = edge.process.clone();
            let scheduler_clone = scheduler_for_signaling.clone();
            sourceproc.outports.insert(
                public_name.clone(),
                ProcessEdgeSink::new(
                    sink,
                    None, // Scheduler handles signaling, no thread wakeup needed
                    Some(target_process.clone()),
                    Some(Arc::new(move || {
                        let _ = scheduler_clone.signal_ready(&target_process);
                    })),
                ),
            );
        }
        for (public_name, edge) in graph.outports.iter() {
            // prepare edge
            debug!(
                "preparing edge from {}.{} to graph {}",
                edge.process, edge.port, public_name
            );
            let (sink, source) = ProcessEdge::new(PROCESSEDGE_BUFSIZE);

            // insert into inports of target process
            // target process name = graphname-OUT
            let targetproc = ports_all
                .get_mut(format!("{}-OUT", graph.properties.name).as_str())
                .expect("graph target assignment process not found");
            // arrayports: insert, but remember that this is a multimap
            targetproc.inports.insert(public_name.clone(), source);
            // assign into outports of source process
            let sourceproc = ports_all
                .get_mut(&edge.process)
                .expect("graph source assignment process not found");
            // arrayports: insert, but remember that this is a multimap
            sourceproc.outports.insert(
                edge.port.to_ascii_uppercase(),
                ProcessEdgeSink::new(
                    sink,
                    None,
                    Some(format!("{}-OUT", graph.properties.name)),
                    None,
                ),
            );
        }

        // generate processes and assign prepared connections
        let mut found: bool;
        let mut found2: bool;
        for (proc_name, node) in graph.nodes.iter() {
            debug!(
                "setting up process name={} component={}",
                proc_name, node.component
            );
            //TODO is there anything in .metadata that affects process setup?

            // get prepared ports for this process
            let ports_this: ProcPorts = ports_all.remove(proc_name).expect(
                "prepared connections for a node not found, source+target nodes in edges != nodes",
            );
            //TODO would be great to have the port name here for diagnostics
            let inports: ProcessInports = ports_this.inports;
            //TODO would be great to have the port name here for diagnostics
            let outports: ProcessOutports = ports_this.outports;

            // check if all ports exist
            found = false;
            for component in &components.available {
                if component.name == node.component {
                    // check inports
                    /*
                    if inports.len() != component.in_ports.len() {
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, String::from(format!("unconnected port checking: inport count on component metadata != used in graph for process={} component={}", proc_name, component.name))));
                    }
                    */
                    //TODO this provides the exact port that is missing and allows for checking of required ports
                    for inport in &component.in_ports {
                        let inport_lower = inport.name.to_ascii_lowercase();
                        let has_inport = inports.contains_key(&inport.name)
                            || inports.contains_key(&inport_lower);
                        if !has_inport {
                            //TODO check if port is required, maybe add strict checking true/false as parameter

                            // check if connected to a graph inport
                            found2 = false;
                            for (_graph_inport_name, graph_inport) in graph.inports.iter() {
                                if graph_inport.process.as_str() == proc_name.as_str()
                                    && graph_inport.port.eq_ignore_ascii_case(&inport.name)
                                {
                                    // is connected to graph inport
                                    found2 = true;
                                    break; //TODO optimize condition flow, is mix of break+continue
                                }
                            }
                            if found2 {
                                continue;
                            } //TODO optimize condition flow, is mix of break+continue

                            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from(format!("unconnected port checking: process {} is missing required port {} for component {}", proc_name, &inport.name, component.name))));
                        }
                    }

                    // check outports
                    /*
                    if outports.len() != component.out_ports.len() {
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, String::from(format!("unconnected port checking: outport count on component metadata != used in graph for process={} component={}", proc_name, component.name))));
                    }
                    */
                    //TODO this provides the exact port that is missing and allows for checking of required ports
                    for outport in &component.out_ports {
                        let outport_lower = outport.name.to_ascii_lowercase();
                        let has_outport = outports.contains_key(&outport.name)
                            || outports.contains_key(&outport_lower);
                        if !has_outport {
                            //TODO check if port is required, maybe add strict checking true/false as parameter

                            // check if connected to a graph outport
                            found2 = false;
                            for (_graph_outport_name, graph_outport) in graph.outports.iter() {
                                if graph_outport.process.as_str() == proc_name.as_str()
                                    && graph_outport.port.eq_ignore_ascii_case(&outport.name)
                                {
                                    // is connected to graph outport
                                    found2 = true;
                                    break; //TODO optimize condition flow, is mix of break+continue
                                }
                            }
                            if found2 {
                                continue;
                            } //TODO optimize condition flow, is mix of break+continue

                            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, String::from(format!("unconnected port checking: process {} is missing required outport {} on component {}", proc_name, &outport.name, component.name))));
                        }
                    }

                    // look no further
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    String::from(
                        "unconnected port checking could not find component in component library",
                    ),
                ));
            }

            // prepare process signal channel
            let (_signalsink, signalsource) =
                std::sync::mpsc::sync_channel::<MessageBuf>(PROCESSEDGE_SIGNAL_BUFSIZE);

            // prepare component for scheduler execution
            let component_name = node.component.clone();
            let (watchdog_signalsink_clone, _watchdog_signalsource_clone) =
                std::sync::mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);
            let graph_inout_ref = create_graph_inout_handle(graph_inout_arc.clone());

            // determine budget class based on component type (generated from flowd.build.toml)
            let budget_class = get_component_budget_class(&component_name);

            // Register node before creating scheduler waker so async components always receive
            // a usable wake handle during construction.
            scheduler_arc.add_node(proc_name.clone(), budget_class);

            // instantiate component for scheduler
            let scheduler_waker =
                crate::scheduler::Scheduler::create_waker(&scheduler_arc, proc_name.clone());
            let component_instance: Box<dyn Component> = match instantiate_component(
                component_name.as_str(),
                inports,
                outports,
                signalsource,
                watchdog_signalsink_clone,
                graph_inout_ref,
                scheduler_waker,
            ) {
                Some(component) => component,
                None => {
                    error!("failed to instantiate component in runtime.start: component not found");
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("component '{}' not found", component_name),
                    ));
                }
            };

            // add component to scheduler
            scheduler_arc.add_component(component_instance, proc_name.clone());
        }

        // Signal all nodes as ready initially to start execution
        for node_id in scheduler_arc.node_ids() {
            scheduler_arc.signal_ready(&node_id);
        }
        // work off graphname-IN and graphname-OUT special processes for graph inports and graph outports
        //TODO the signal channel and joinhandle of the graph outport process/thread could also simply be stored in boundary_threads with all other boundary handlers
        graph_inout.inports = None;
        graph_inout.outports = None;
        if ports_all.len() > 0 {
            // insert graph inport handler
            if ports_all.contains_key(format!("{}-IN", graph.properties.name).as_str()) {
                // target datastructure
                let mut outports: HashMap<String, ProcessEdgeSink> = HashMap::new();
                // get ports for this special component
                let ports_this: ProcPorts = ports_all
                    .remove(format!("{}-IN", graph.properties.name).as_str())
                    .expect("prepared connections for graph inports not found");
                // add sinks of all target processes (no thread wakeup needed for scheduled components)
                for (port_name, mut edge) in ports_this.outports {
                    // arrayports: check if there are multiple edges going out of the graph inport, which is not allowed
                    if edge.len() != 1 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            String::from("multiple edges from graph inport"),
                        ));
                    }
                    // insert that port
                    let edge0 = edge.pop().unwrap();
                    outports.insert(port_name, edge0);
                }
                // save the inports (where we put packets into) as the graph inport channel handles; they are "outport handles" because they are being written into (packet sink)
                graph_inout.inports = Some(outports);
            }
            if ports_all.contains_key(format!("{}-OUT", graph.properties.name).as_str()) {
                // get ports for this special component, of interest here are the inports (source channels)
                let ports_this: ProcPorts = ports_all
                    .remove(format!("{}-OUT", graph.properties.name).as_str())
                    .expect("prepared connections for graph outports not found");
                let mut inports = ports_this.inports;
                // prepare process signal channel
                let (signalsink, signalsource): (ProcessSignalSink, ProcessSignalSource) =
                    std::sync::mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);
                // start thread, will move signalsource, inports
                let graph_name = graph.properties.name.clone(); //TODO cannot change graph name during runtime because of this
                                                                //TODO optimize; WebSocket is not Copy, but a WebSocket can be re-created from the inner TcpStream, which has a try_clone()
                let graph_inoutref = graph_inout_arc.clone();
                //let ports_this_wake_notify = ports_this.wake_notify.clone();
                let graph_out_exited = Arc::new(AtomicBool::new(false));
                let graph_out_exited_in_thread = graph_out_exited.clone();
                let (_watchdog_graph_out_signalsink, watchdog_graph_out_signalsource) =
                    std::sync::mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);
                let joinhandle = thread::Builder::new()
                    .name(format!("{}-OUT", graph.properties.name))
                    .spawn(move || {
                        // marks graph out handler as exited even when unwinding from panic
                        let _thread_exit_flag = ThreadExitFlag::new(graph_out_exited_in_thread);
                        let signals = signalsource;
                        // arrayports: Note that this does currently not handle arrayports, but since this is forbidden via the graph definition, we can ignore that for now //TODO add check
                        if inports.len() == 0 {
                            error!("no graph inports found, exiting");
                            return;
                        }
                        //let mut websocket = tungstenite::WebSocket::from_raw_socket(websocket_stream, tungstenite::protocol::Role::Server, None);
                        debug!("GraphOutports is now run()ning!");

                        // inform FBP Network Protocol clients that graphout ports are now connected (runtime:packet event type = connect)
                        //NOTE: inports is from the perspective of the GraphOut handler thread, so these are the graph outports
                        for port_name in inports.keys() {
                            send_runtime_packet(
                                &graph_inoutref,
                                &RuntimePacketResponse::new_connect(
                                    graph_name.clone(),
                                    port_name.clone(),
                                    None,
                                    None,
                                ),
                            ); //TODO can we save cloning graph_name several times?
                        }

                        // read IPs
                        loop {
                            trace!("begin of iteration");
                            // check signals
                            //TODO optimize, there is also try_recv() and recv_timeout()
                            if let Ok(signal) = signals.try_recv() {
                                let signal_text = message_as_utf8(&signal).unwrap_or("");
                                trace!("received signal: {}", signal_text);
                                if signal_text == "stop" {
                                    info!("got stop signal, exiting");
                                    break;
                                }
                            }
                            // receive on all inports
                            for (port_name, inport) in inports.iter_mut() {
                                //TODO while !inport.is_empty() {
                                loop {
                                    if let Ok(ip) = inport.pop() {
                                        // output the packet data with newline
                                        debug!("got a packet for graph outport {}", port_name);
                                        let payload_text = message_as_utf8(&ip)
                                            .expect("graph outport payload must be UTF-8 text");
                                        trace!("{}", payload_text);

                                        // send out to FBP network protocol client
                                        debug!("sending out to client...");
                                        //TODO optimize lock only once for all packets available in inport buffer
                                        send_runtime_packet(
                                            &graph_inoutref,
                                            &RuntimePacketResponse::new(
                                                RuntimePacketResponsePayload {
                                                    port: port_name.clone(), //TODO optimize
                                                    event: RuntimePacketEvent::Data,
                                                    typ: None, //TODO implement properly, OTOH it is an optional field
                                                    schema: None,
                                                    graph: graph_name.clone(),
                                                    payload: Some(payload_text.to_owned()), //TODO optimize useless conversions here - we could make RuntimePacketResponse separate from RuntimePacketRequest and make payload on the Response &str
                                                                                            // TODO optimize conversion; just handing over Some(String::from_utf8(ip) = move causes "this reinitialization might get skipped" -> https://github.com/rust-lang/rust/issues/92858
                                                },
                                            ),
                                        );
                                        debug!("done");
                                    } else {
                                        //TODO optimize unlock graph_inout here
                                        break;
                                    }
                                }
                            }
                            trace!("-- end of iteration");
                            std::thread::park_timeout(Duration::from_millis(5));
                            //condvar_block!(&*ports_this_wake_notify);
                        }
                        // inform FBP Network Protocol clients that graphout ports are now disconnected (runtime:packet event type = disconnect)
                        //NOTE: inports is from the perspective of the GraphOut handler thread, so these are the graph outports
                        info!("notifying clients of graph outports disconnect");
                        for port_name in inports.keys() {
                            send_runtime_packet(
                                &graph_inoutref,
                                &RuntimePacketResponse::new_disconnect(
                                    graph_name.clone(),
                                    port_name.clone(),
                                    None,
                                    None,
                                ),
                            ); //TODO can we save cloning graph_name several times?
                        }

                        info!("exiting");
                    })
                    .expect("thread start failed");

                // store process signal channel and thread handle for watchdog thread
                watchdog_threadandsignal.insert(
                    format!("{}-OUT", graph.properties.name),
                    (
                        signalsink.clone(),
                        joinhandle.thread().clone(),
                        graph_out_exited,
                        watchdog_graph_out_signalsource,
                        false,
                    ),
                );
                // store process signal channel and join handle so that the other processes writing into this graph outport component can find it
                self.boundary_threads.insert(
                    format!("{}-OUT", graph.properties.name),
                    BoundaryThread {
                        signal: signalsink,
                        joinhandle: joinhandle,
                    },
                );

                // save single joinhandle and signal for that component
                //TODO optimize, cannot clone joinhandle
                //TODO currentcy graph_inout.outports is unused
                /*
                graph_inout.outports = Some(BoundaryThread {
                    signal: signalsink,
                    joinhandle: joinhandle,
                })
                */
            }
            //TODO put graph_inout into runtime struct?  self.graph_inout = graph_inout;
        }

        // sanity check
        if ports_all.len() != 0 {
            // reset to known state
            self.boundary_threads.clear();
            // report error
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, String::from("there are ports for processes left over, source+target nodes in edges != nodes")));
        }

        // unpark all processes since all joinhandles are now known and so that they can replace the process names with the join handles and instantiate their components
        for proc in self.boundary_threads.iter() {
            proc.1.joinhandle.thread().unpark();
        }

        // start background thread for regular process health check
        // not possible to kill a thread:  https://stackoverflow.com/questions/26199926/how-to-terminate-or-suspend-a-rust-thread-from-another-thread
        // TODO optimize use condvar or mpsc channel?
        //TODO add ability to change interval after network start
        //TODO allow reconfiguration of network, currently this is basically a subset copy of self.boundary_threads (signal channel sink and thread handle)
        //TODO use the Component.support_health bool there!
        // sink2 and source2 are separate for signaling between runtime and watchdog thread only so that there can be no mixup between runtime<->watchdog and watchdog<->processes communication
        let (watchdog_signalsink2, watchdog_signalsource2) =
            std::sync::mpsc::sync_channel::<MessageBuf>(PROCESSEDGE_SIGNAL_BUFSIZE);
        let watchdog_thread = thread::Builder::new().name("watchdog".to_owned()).spawn( move || {
            debug!("watchdog is running");
            let mut missed_pongs: HashMap<String, u8> = HashMap::new();
            'watchdog_loop: loop {
                while let Ok(signal) = watchdog_signalsource2.try_recv() {
                    let signal_text = message_as_utf8(&signal).unwrap_or("");
                    if signal_text == "stop" {
                        debug!("got stop signal, exiting");
                        break 'watchdog_loop;
                    }
                }

                // health check of all components
                trace!("running health check...");
                let mut now: chrono::DateTime<Utc>;   //TODO any way to not initialize this with a throwaway value?
                let mut ok = true;
                let mut disconnected_components: Vec<String> = Vec::new();
                for (name, proc) in watchdog_threadandsignal.iter() {
                    if let Ok(ip) = watchdog_signalsource2.try_recv() {
                        if message_as_utf8(&ip) == Some("stop") {
                            debug!("got stop signal, exiting");
                            break 'watchdog_loop;
                        }
                    }
                    trace!("process {}...", name);
                    if proc.2.load(Ordering::Acquire) {
                        trace!("process {} already exited", name);
                        disconnected_components.push(name.clone());
                        missed_pongs.remove(name);
                        ok = false;
                        continue;
                    }
                    if !proc.4 {
                        // this process does not support watchdog ping/pong health checks
                        continue;
                    }
                    // send query to process
                    match proc.0.try_send(FbpMessage::from_str("ping")) {
                        Ok(_) => {},
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            warn!("process {} signal channel full", name);
                            ok = false;
                            continue;
                        },
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                            warn!("process {} signal channel disconnected", name);
                            disconnected_components.push(name.clone());
                            missed_pongs.remove(name);
                            ok = false;
                            continue;
                        },
                    }

                    // process liveness already checked via ThreadExitFlag above

                    // wake process
                    proc.1.unpark();
                    now = chrono::Utc::now();   //TODO is it really useful to measure 0.000005ms response time?

                    // read response with polling so watchdog stop stays responsive
                    let wait_started = Instant::now();
                    let mut got_pong = false;
                    let mut disconnected = false;
                    while wait_started.elapsed() < core::time::Duration::from_millis(1000) {
                        match proc.3.recv_timeout(WATCHDOG_POLL_DUR) {
                            Ok(signal) => {
                                let signal_text = message_as_utf8(&signal).unwrap_or("");
                                if signal_text == "pong" {    //TODO harden - may be a response from some other process -> send a nonce?
                                    trace!("process {} OK ({}ms)", name, chrono::Utc::now() - now);
                                    debug!("process {} OK", name);
                                    missed_pongs.remove(name);
                                } else {
                                    warn!("process {} sent a spurious response: {}", name, signal_text);
                                    //TODO one spurious response currently trips up the logic
                                }
                                got_pong = true;
                                break;
                            },
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                if let Ok(ip) = watchdog_signalsource2.try_recv() {
                                    if message_as_utf8(&ip) == Some("stop") {
                                        debug!("got stop signal, exiting");
                                        break 'watchdog_loop;
                                    }
                                }
                            },
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                warn!("process {} disconnected signal channel!", name);
                                missed_pongs.remove(name);
                                ok = false;
                                disconnected = true;
                                break;
                            }
                        }
                    }
                    if !got_pong && !disconnected {
                        let misses = missed_pongs.entry(name.clone()).or_insert(0);
                        *misses = misses.saturating_add(1);
                        if *misses >= WATCHDOG_MAX_MISSED_PONGS {
                            warn!(
                                "process {} failed to respond within 1000ms ({} consecutive misses)!",
                                name,
                                *misses
                            );
                            ok = false;
                        } else {
                            debug!(
                                "process {} missed watchdog pong once ({} / {}), tolerating transient miss",
                                name,
                                *misses,
                                WATCHDOG_MAX_MISSED_PONGS
                            );
                        }
                    }
                }
                if !disconnected_components.is_empty() {
                    for name in disconnected_components {
                        warn!("watchdog: removing exited process {}", name);
                        missed_pongs.remove(&name);
                        watchdog_threadandsignal.remove(&name);
                    }
                }
                let all_schedulers_exited = if watchdog_threadandsignal.is_empty() {
                    match watchdog_runtime.try_read() {
                        Ok(runtime_read) => {
                            !runtime_read.scheduler_threads.is_empty()
                                && runtime_read
                                    .scheduler_threads
                                    .values()
                                    .all(|scheduler_thread| scheduler_thread.is_finished())
                        }
                        Err(std::sync::TryLockError::WouldBlock) => false,
                        Err(std::sync::TryLockError::Poisoned(_)) => {
                            warn!("watchdog: runtime lock poisoned while checking scheduler completion");
                            false
                        }
                    }
                } else {
                    false
                };

                if all_schedulers_exited {
                    info!("process health check: all scheduler threads exited, shutting down network");
                    let stop_err = match watchdog_runtime.try_write() {
                        Ok(mut runtime_write) => runtime_write.stop(watchdog_graph_inout.clone(), true).err(),
                        Err(std::sync::TryLockError::WouldBlock) => {
                            info!("watchdog: runtime lock busy during scheduler-exited shutdown; external stop is likely in progress, exiting watchdog loop");
                            break;
                        }
                        Err(std::sync::TryLockError::Poisoned(_)) => {
                            warn!("watchdog: runtime lock poisoned during scheduler-exited shutdown");
                            break;
                        }
                    };
                    if let Some(err) = stop_err {
                        warn!("watchdog: runtime.stop() returned error: {}", err);
                    }
                    let stopped_packet = {
                        let runtime_read = watchdog_runtime.read().expect("watchdog failed to acquire lock for runtime");
                        NetworkStoppedResponse::new(&runtime_read.status_snapshot())
                    };
                    send_network_stopped(&watchdog_graph_inout, &stopped_packet);
                    break;
                } else if ok {
                    debug!("process health check OK");
                } else if watchdog_threadandsignal.is_empty() {
                    // network has effectively shut down
                    info!("process health check: all processes exited, shutting down network");
                    // signal runtime; if stop fails, keep watchdog alive and report best-effort status.
                    // Use non-blocking lock acquisition to avoid deadlocking with concurrent network:stop,
                    // which already holds runtime.write() and may wait for watchdog join.
                    let stop_err = match watchdog_runtime.try_write() {
                        Ok(mut runtime_write) => runtime_write.stop(watchdog_graph_inout.clone(), true).err(),
                        Err(std::sync::TryLockError::WouldBlock) => {
                            info!("watchdog: runtime lock busy during all-exited shutdown; external stop is likely in progress, exiting watchdog loop");
                            break;
                        }
                        Err(std::sync::TryLockError::Poisoned(_)) => {
                            warn!("watchdog: runtime lock poisoned during all-exited shutdown");
                            break;
                        }
                    };
                    if let Some(err) = stop_err {
                        warn!("watchdog: runtime.stop() returned error: {}", err);
                    }
                    //TODO runtime should set the time_stopped etc. values on runtime.status
                    // TODO not watchdog, but runtime.stop() should signal the FBP protocol clients
                    // send network stop notification to all FBP protocol clients//###
                    let stopped_packet = {
                        let runtime_read = watchdog_runtime.read().expect("watchdog failed to acquire lock for runtime");
                        NetworkStoppedResponse::new(&runtime_read.status_snapshot())
                    };
                    send_network_stopped(&watchdog_graph_inout, &stopped_packet);
                    // exit thread
                    break;
                }
                let sleep_started = Instant::now();
                while sleep_started.elapsed() < PROCESS_HEALTHCHECK_DUR {
                    if let Ok(ip) = watchdog_signalsource2.try_recv() {
                        if message_as_utf8(&ip) == Some("stop") {
                            debug!("got stop signal, exiting");
                            break 'watchdog_loop;
                        }
                    }
                    thread::sleep(WATCHDOG_POLL_DUR);
                }
            }
        }).expect("failed to spawn watchdog thread");

        // all set, now "open the doors" = inform FBP Network Protocol clients / remote runtimes that the graph inports are now connected as well (runtime:packet event type = connect)
        // check if graph has inports
        if let Some(inports) = &graph_inout.inports {
            let inport_names = inports.keys().cloned().collect::<Vec<_>>(); //TODO optimize wow, but works:  https://stackoverflow.com/a/45312076/5120003
            drop(graph_inout);
            for port_name in inport_names {
                send_runtime_packet(
                    &graph_inout_arc,
                    &RuntimePacketResponse::new_connect(
                        graph.properties.name.clone(),
                        port_name.clone(),
                        None,
                        None,
                    ),
                ); //TODO can we avoid cloning here?
            }
        }

        // Start scheduler thread for this graph
        let scheduler_arc_clone = scheduler_arc.clone();
        let graph_name_clone = graph_name.clone();
        let scheduler_thread = {
            thread::Builder::new()
                .name(format!("scheduler-{}", graph_name))
                .spawn(move || {
                    debug!("scheduler thread for graph {} starting", graph_name_clone);
                    scheduler_arc_clone.run();
                    debug!("scheduler thread for graph {} exiting", graph_name_clone);
                })
                .expect("failed to spawn scheduler thread")
        };

        // Store the scheduler thread
        self.scheduler_threads.insert(graph_name, scheduler_thread);

        // return status
        self.watchdog_thread = Some(watchdog_thread);
        self.watchdog_channel = Some(watchdog_signalsink2);
        self.status.time_started = chrono::Utc::now();
        self.status.graph = self.graph.clone();
        self.status.started = true;
        self.status.running = true;
        info!("network started");
        Ok(&self.status)
    }

    fn stop(
        &mut self,
        graph_inout: Arc<std::sync::Mutex<GraphInportOutportHolder>>,
        watchdog_all_exited: bool,
    ) -> std::result::Result<&RuntimeStatus, std::io::Error> {
        //TODO implement in full detail
        const STOP_SIGNAL_MAX_RETRIES: usize = 64;
        const PROCESS_JOIN_GRACE_DUR: Duration = Duration::from_secs(5);
        const WATCHDOG_JOIN_GRACE_DUR: Duration = Duration::from_secs(5);

        // if true, the watchdog informed us that all processes have already exited
        if !watchdog_all_exited {
            // close front door early - inform FBP Network Protocol clients that graph inports are now disconnected (runtime:packet event type = connect)
            //NOTE: this happens before the GraphOutports handler thread is notified below among all the processes, so processing can be finished but no new packets are sent in anymore by the FBP client(s)
            // Keep this lock scope short: collect names first, then send disconnects with per-packet locking.
            let inport_names = {
                let graph_inout_inner = graph_inout.lock().expect("lock poisoned");
                graph_inout_inner
                    .inports
                    .as_ref()
                    .map(|inports| inports.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            };
            if !inport_names.is_empty() {
                info!("notifying clients of graph inports disconnect");
                for port_name in inport_names {
                    send_runtime_packet(
                        &graph_inout,
                        &RuntimePacketResponse::new_disconnect(
                            self.graph.clone(),
                            port_name.clone(),
                            None,
                            None,
                        ),
                    ); //TODO can we save cloning here?
                }
            }

            // stop watchdog first so it cannot continue filling process signal channels with pings
            info!("stop: signaling watchdog");
            if let Some(watchdog_channel) = self.watchdog_channel.take() {
                if let Err(err) = watchdog_channel.send(FbpMessage::from_str("stop")) {
                    warn!(
                        "stop: watchdog already disconnected while sending stop signal: {}",
                        err
                    );
                }
            } else {
                warn!("stop: watchdog channel missing");
            }
            if let Some(watchdog_thread) = &self.watchdog_thread {
                watchdog_thread.thread().unpark();
            }
        } else {
            // runtime stop triggered from watchdog because all processes already exited
            self.watchdog_channel.take();
        }

        // signal all threads
        info!("stop: signaling all processes...");
        for (name, proc) in self.boundary_threads.iter() {
            if proc.joinhandle.is_finished() {
                info!("stop: process {} already exited", name);
                continue;
            }
            info!("stop: signaling {}", name);
            // wake first, then send stop non-blocking to avoid deadlock on full bounded channels
            proc.joinhandle.thread().unpark();
            let mut pending = FbpMessage::from_str("stop");
            let mut sent_or_disconnected = false;
            for _ in 0..STOP_SIGNAL_MAX_RETRIES {
                match proc.signal.try_send(pending.clone()) {
                    Ok(()) => {
                        sent_or_disconnected = true;
                        proc.joinhandle.thread().unpark();
                        break;
                    }
                    Err(std::sync::mpsc::TrySendError::Full(returned)) => {
                        pending = returned;
                        proc.joinhandle.thread().unpark();
                        thread::yield_now();
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                        sent_or_disconnected = true;
                        break;
                    }
                }
            }
            if !sent_or_disconnected {
                warn!(
                    "stop: process {} signal channel stayed full after {} retries, continuing with join",
                    name,
                    STOP_SIGNAL_MAX_RETRIES
                );
            }
        }
        info!("done");

        // Stop all scheduler threads first
        for (graph_name, scheduler) in &self.schedulers {
            info!("stop: stopping scheduler for graph {}", graph_name);
            scheduler.stop();
        }

        // Join all scheduler threads
        let mut timed_out_schedulers = Vec::new();
        for (graph_name, scheduler_thread) in std::mem::take(&mut self.scheduler_threads) {
            info!("stop: joining scheduler thread for graph {}", graph_name);
            let join_started = Instant::now();
            while !scheduler_thread.is_finished() && join_started.elapsed() < PROCESS_JOIN_GRACE_DUR
            {
                thread::sleep(Duration::from_millis(10));
            }
            if scheduler_thread.is_finished() {
                if let Err(err) = scheduler_thread.join() {
                    warn!(
                        "stop: joining scheduler thread for graph {} returned error: {:?}",
                        graph_name, err
                    );
                }
            } else {
                warn!(
                    "stop: joining scheduler thread for graph {} timed out after {:?}",
                    graph_name, PROCESS_JOIN_GRACE_DUR
                );
                timed_out_schedulers.push((graph_name, scheduler_thread));
            }
        }
        // Restore timed-out threads
        for (graph_name, thread) in timed_out_schedulers {
            self.scheduler_threads.insert(graph_name, thread);
        }

        // join all threads
        //TODO what if one of them wont join? hangs? -> kill, how much time to give?
        info!("stop: joining all threads...");
        let mut timed_out_threads: Vec<String> = Vec::new();
        let mut still_running: BoundaryThreadManager = BoundaryThreadManager::new();
        for (name, proc) in std::mem::take(&mut self.boundary_threads) {
            info!("stop: joining {}", name);
            let join_started = Instant::now();
            while !proc.joinhandle.is_finished() && join_started.elapsed() < PROCESS_JOIN_GRACE_DUR
            {
                thread::sleep(Duration::from_millis(10));
            }
            if proc.joinhandle.is_finished() {
                if let Err(err) = proc.joinhandle.join() {
                    warn!("stop: joining {} returned error: {:?}", name, err);
                }
            } else {
                warn!(
                    "stop: joining {} timed out after {:?}, keeping thread tracked for retry",
                    name, PROCESS_JOIN_GRACE_DUR
                );
                timed_out_threads.push(name.clone());
                still_running.insert(name, proc);
            } //TODO there is .thread() -> for killing
        }
        self.boundary_threads = still_running;
        info!("done");

        // join watchdog thread (unless stop() was called from watchdog itself)
        if let Some(watchdog_thread) = self.watchdog_thread.take() {
            if watchdog_all_exited {
                // called from watchdog thread itself; dropping handle detaches and avoids self-join deadlock
                info!("stop: watchdog self-stop acknowledged");
            } else {
                info!("stop: joining watchdog");
                let join_started = Instant::now();
                while !watchdog_thread.is_finished()
                    && join_started.elapsed() < WATCHDOG_JOIN_GRACE_DUR
                {
                    thread::sleep(Duration::from_millis(10));
                }
                if watchdog_thread.is_finished() {
                    if let Err(err) = watchdog_thread.join() {
                        warn!("stop: joining watchdog returned error: {:?}", err);
                    }
                } else {
                    warn!("stop: joining watchdog timed out after {:?}, keeping thread tracked for retry", WATCHDOG_JOIN_GRACE_DUR);
                    timed_out_threads.push("watchdog".to_owned());
                    self.watchdog_thread = Some(watchdog_thread);
                }
            }
        }

        if !timed_out_threads.is_empty() {
            self.status.graph = self.graph.clone();
            self.status.started = true;
            self.status.running = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "network stop incomplete, timed-out threads still running: {}",
                    timed_out_threads.join(", ")
                ),
            ));
        }

        // Emit trace:disconnect for all graph edges when tracing is active.
        if self.tracing {
            if let Some(active_graph) = self.graphs.get_graph(&self.graph) {
                let active_graph = active_graph.read().expect("graph lock poisoned");
                for edge in active_graph.edges.iter() {
                    // IIP pseudo-edges are not runtime transport edges.
                    if edge.data.is_some() && edge.source.process.is_empty() {
                        continue;
                    }
                    let edge_id = format!(
                        "{}.{}-{}.{}",
                        edge.source.process, edge.source.port, edge.target.process, edge.target.port
                    );
                    let disconnect_payload = ApiTraceDisconnectEventPayload {
                        id: edge_id,
                        src: TraceGraphNodeSpecNetwork {
                            node: edge.source.process.clone(),
                            port: edge.source.port.clone(),
                            index: edge.source.index.clone(),
                        },
                        tgt: TraceGraphNodeSpecNetwork {
                            node: edge.target.process.clone(),
                            port: edge.target.port.clone(),
                            index: edge.target.index.clone(),
                        },
                        graph: active_graph.properties.name.clone(),
                    };
                    send_trace_disconnect(&graph_inout, disconnect_payload);
                }
            }
        }

        // Drop trace channel sender so the trace relay thread can exit cleanly.
        self.trace_sender = None;

        // set status
        info!("network is shut down.");
        self.status.graph = self.graph.clone();
        // Preserve "started=true, running=false" for watchdog-driven auto-finish:
        // fbp-protocol expects this when a short-lived network already completed
        // by the time network:getstatus is queried right after network:start.
        self.status.started = watchdog_all_exited;
        self.status.running = false; // was started, but not running any more
        Ok(&self.status)
    }

    fn debug_mode(&mut self, graph: &str, mode: bool) -> std::result::Result<(), std::io::Error> {
        if self.graphs.get_graph(graph).is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Graph '{}' not found", graph),
            ));
        }
        self.graph = graph.to_string();
        self.status.graph = graph.to_string();
        self.status.debug = Some(mode);
        Ok(())
    }

    //TODO optimize: better to hand over String or &str? Difference between Vec and vec?
    fn set_debug_edges(
        &mut self,
        graph: &str,
        edges: &Vec<GraphEdgeSpec>,
    ) -> std::result::Result<(), std::io::Error> {
        if self.graphs.get_graph(graph).is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Graph '{}' not found", graph),
            ));
        }
        self.graph = graph.to_string();
        self.status.graph = graph.to_string();

        // TODO: clarify spec: what to do with this message's information behavior-wise? Dependent on first setting network into debug mode or independent?
        // TODO: implement actual debug edge handling
        info!("got following debug edges:");
        for edge in edges {
            info!("  edge: src={:?} tgt={:?}", edge.src, edge.tgt);
        }
        info!("--- end");
        Ok(())
    }

    fn start_trace(
        &mut self,
        graph: &str,
        _buffer_size: u32,
    ) -> std::result::Result<(), std::io::Error> {
        // Validate graph parameter matches current graph (single-graph mode)
        if graph != self.graph {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound,
                format!("Graph '{}' not found. Current graph is '{}'. Multi-graph support not yet implemented.",
                    graph, self.graph)));
        }

        // TODO: implement actual tracing with buffer_size parameter (FBP spec: size of tracing buffer to keep in bytes)
        // TODO: check if graph exists and is current graph (multi-graph)
        if self.tracing {
            // wrong state
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                String::from("tracing already started"),
            ));
        }
        self.tracing = true;
        info!("tracing enabled for graph '{}'", graph);
        Ok(())
    }

    fn stop_trace(&mut self, graph: &str) -> std::result::Result<(), std::io::Error> {
        // Validate graph parameter matches current graph (single-graph mode)
        if graph != self.graph {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound,
                format!("Graph '{}' not found. Current graph is '{}'. Multi-graph support not yet implemented.",
                    graph, self.graph)));
        }

        // TODO: implement actual tracing stop
        // TODO: check if graph exists and is current graph (multi-graph)
        if !self.tracing {
            // wrong state
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("tracing not started"),
            ));
        }
        self.tracing = false;
        info!("tracing disabled for graph '{}'", graph);
        Ok(())
    }

    //TODO can this function fail, at all? can the error response be removed?
    //TODO clarify spec: when is clear() allowed? in running state or in stopped state?
    fn clear_trace(&mut self, graph: &str) -> std::result::Result<(), std::io::Error> {
        // Validate graph parameter matches current graph (single-graph mode)
        if graph != self.graph {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound,
                format!("Graph '{}' not found. Current graph is '{}'. Multi-graph support not yet implemented.",
                    graph, self.graph)));
        }

        // TODO: implement actual trace clearing
        // TODO: check if graph exists and is current graph (multi-graph)
        Ok(())
    }

    //TODO can this function fail, at all? can the error response be removed?
    //TODO clarify spec: when is dump() allowed? in running state or in stopped state?
    fn dump_trace(&mut self, graph: &str) -> std::result::Result<String, std::io::Error> {
        // Validate graph parameter matches current graph (single-graph mode)
        if graph != self.graph {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound,
                format!("Graph '{}' not found. Current graph is '{}'. Multi-graph support not yet implemented.",
                    graph, self.graph)));
        }

        // TODO: implement actual trace dumping
        // TODO: check if graph exists and is current graph (multi-graph)
        // TODO: implement Flowtrace format?
        Ok(String::from("")) //TODO how to indicate "empty"? Does it maybe require at least "[]" or "{}"?
    }

    fn validate_secret(&self, secret: Option<&String>, graph: &str) -> Result<(), std::io::Error> {
        self.validate_secret_with_access(secret, graph, AccessLevel::ReadWrite)
    }

    fn validate_secret_with_access(
        &self,
        secret: Option<&String>,
        graph: &str,
        required_access: AccessLevel,
    ) -> Result<(), std::io::Error> {
        // Compatibility: many FBP clients send empty-string secrets by default.
        // Treat that the same as not providing a secret token.
        let secret = match secret {
            Some(secret) if !secret.is_empty() => secret,
            _ => return Ok(()),
        };

        if let Some((expected_secret, access_level)) = self.secrets.get(graph) {
            if secret != expected_secret {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "invalid secret token",
                ));
            }
            if *access_level == AccessLevel::ReadOnly && required_access == AccessLevel::ReadWrite {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "readonly access not sufficient for write operation",
                ));
            }
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "no secret configured for graph",
            ));
        }
        Ok(())
    }

    fn packet(
        payload: &RuntimePacketRequestPayload,
        graph_inout: Arc<std::sync::Mutex<GraphInportOutportHolder>>,
        runtime: Arc<RwLock<Runtime>>,
    ) -> std::result::Result<(), std::io::Error> {
        const INPORT_PUSH_GRACE_DUR: Duration = Duration::from_secs(2);

        // Implement token-based security: validate secret if provided
        let runtime_read = runtime.read().expect("lock poisoned");
        runtime_read.validate_secret(payload.secret.as_ref(), &payload.graph)?;

        //TODO check if graph exists and if that port actually exists
        //TODO check payload datatype, schema, event (?) etc.
        //TODO implement and deliver to destination process
        info!(
            "runtime: got a packet for port {}: {:?}",
            payload.port, payload.payload
        );
        let mut packet = FbpMessage::from_str(
            payload
                .payload
                .as_ref()
                .expect("graph inport runtime:packet is missing payload"),
        );
        let wait_started = Instant::now();
        loop {
            let mut graph_inout_locked = graph_inout.lock().expect("lock poisoned");
            // deliver to destination process
            if let Some(inports) = graph_inout_locked.inports.as_mut() {
                if let Some(inport) = inports.get_mut(payload.port.as_str()) {
                    match inport.push(packet) {
                        Ok(()) => {
                            return Ok(());
                        }
                        Err(err) => {
                            packet = match err {
                                PushError::Full(data) => data,
                            };
                        }
                    }
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        String::from("graph inport with that name not found"),
                    ));
                }
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    String::from("no graph inports exist"),
                ));
            }
            drop(graph_inout_locked);

            if wait_started.elapsed() >= INPORT_PUSH_GRACE_DUR {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "graph inport {} backpressured for {:?}",
                        payload.port, INPORT_PUSH_GRACE_DUR
                    ),
                ));
            }

            // sleep some time resp. give up CPU timeslot
            thread::yield_now();
        }
    }

    //TODO return path: process that sends to an outport -> send to client. TODO clarify spec: which client should receive it?

    //TODO runtime: command to connect an outport to a remote runtime as remote subgraph.
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        RuntimeStatus {
            time_started: chrono::Utc::now(),
            graph: String::from("main_graph"),
            started: false,
            running: false,
            debug: None,
        }
    }
}

impl RuntimeStatus {
    fn snapshot(&self) -> RuntimeStatusSnapshot {
        RuntimeStatusSnapshot {
            time_started: self.time_started,
            graph: self.graph.clone(),
            started: self.started,
            running: self.running,
            debug: self.debug,
            scheduler_metrics: HashMap::new(),
        }
    }
}

// runtime state of graph inports and outports
#[derive(Debug)]
pub struct GraphInportOutportHolder {
    // inports
    // the edge sinks are stored here because the connection handler in handle_client() needs to send into these
    inports: Option<HashMap<String, ProcessEdgeSink>>,

    // outports are handled by 1 special component that needs to be signaled and joined on network stop()
    // sink and wakeup are given to the processes that write into the graph outport process, so they are not stored here
    outports: Option<BoundaryThread>,

    // connected client websockets ready to send responses to connected clients, for graphout process
    websockets: HashMap<std::net::SocketAddr, Arc<Mutex<tungstenite::WebSocket<TcpStream>>>>,
    // benchmark hook to observe runtime packets without websocket transport
    packet_tap: Option<std::sync::mpsc::SyncSender<RuntimePacketResponsePayload>>,
}

impl GraphInportOutportHolder {
    fn set_packet_tap(
        &mut self,
        tap: Option<std::sync::mpsc::SyncSender<RuntimePacketResponsePayload>>,
    ) {
        self.packet_tap = tap;
    }
}

fn broadcast_to_clients<T: Serialize>(
    graph_inout: &Arc<Mutex<GraphInportOutportHolder>>,
    packet: &T,
    packet_name: &str,
) {
    let msg =
        serde_json::to_string(packet).expect("failed to serialize packet for websocket clients");
    let clients = {
        let graph_inout_locked = graph_inout.lock().expect("lock poisoned");
        graph_inout_locked
            .websockets
            .iter()
            .map(|(addr, client)| (*addr, Arc::clone(client)))
            .collect::<Vec<_>>()
    };

    let mut failed_clients = Vec::new();
    for (addr, client) in clients {
        let mut client = match client.lock() {
            Ok(client) => client,
            Err(err) => {
                warn!(
                    "dropping websocket client {} after {} lock poison: {}",
                    addr, packet_name, err
                );
                failed_clients.push(addr);
                continue;
            }
        };
        if let Err(err) = client
            .get_mut()
            .set_write_timeout(CLIENT_BROADCAST_WRITE_TIMEOUT)
        {
            warn!(
                "failed to set websocket client {} write timeout before {} send: {}",
                addr, packet_name, err
            );
        }
        match client.write(Message::text(msg.clone())) {
            Ok(_) => {}
            Err(err) => {
                warn!(
                    "dropping websocket client {} after failed {} send: {}",
                    addr, packet_name, err
                );
                failed_clients.push(addr);
            }
        }
    }

    if !failed_clients.is_empty() {
        let mut graph_inout_locked = graph_inout.lock().expect("lock poisoned");
        for addr in failed_clients {
            graph_inout_locked.websockets.remove(&addr);
        }
    }
}

fn send_runtime_packet(
    graph_inout: &Arc<Mutex<GraphInportOutportHolder>>,
    packet: &RuntimePacketResponse,
) {
    {
        let graph_inout_locked = graph_inout.lock().expect("lock poisoned");
        if let Some(tap) = &graph_inout_locked.packet_tap {
            let _ = tap.try_send(packet.payload.clone());
        }
    }
    //TODO add capabilities check for each client!
    broadcast_to_clients(graph_inout, packet, "runtime:packet");
}

fn send_network_stopped(
    graph_inout: &Arc<Mutex<GraphInportOutportHolder>>,
    packet: &NetworkStoppedResponse,
) {
    //TODO add capabilities check for each client!
    broadcast_to_clients(graph_inout, packet, "network:stopped");
}

fn send_network_output(
    graph_inout: &Arc<Mutex<GraphInportOutportHolder>>,
    packet: &NetworkOutputResponse,
) {
    //TODO add capabilities check for each client!
    broadcast_to_clients(graph_inout, packet, "network:output");
}

fn send_network_error(
    graph_inout: &Arc<Mutex<GraphInportOutportHolder>>,
    packet: &NetworkErrorResponse,
) {
    //TODO add capabilities check for each client!
    broadcast_to_clients(graph_inout, packet, "network:error");
}

fn send_network_data(
    graph_inout: &Arc<Mutex<GraphInportOutportHolder>>,
    packet: &NetworkDataResponse,
) {
    //TODO add capabilities check for each client!
    //TODO add debug mode check for the graph (network:debug)
    //TODO add check if edge was selected for debugging (network:edges)
    broadcast_to_clients(graph_inout, packet, "network:data");
}

#[derive(Serialize)]
struct TraceEventMessage<T: Serialize> {
    protocol: String,
    command: String,
    payload: T,
}

impl<T: Serialize> TraceEventMessage<T> {
    fn new(command: &str, payload: T) -> Self {
        TraceEventMessage {
            protocol: String::from("trace"),
            command: command.to_owned(),
            payload,
        }
    }
}

fn send_trace_data(
    graph_inout: &Arc<Mutex<GraphInportOutportHolder>>,
    payload: ApiTraceDataEventPayload,
) {
    let message = TraceEventMessage::new("data", payload);
    broadcast_to_clients(graph_inout, &message, "trace:data");
}

fn send_trace_connect(
    graph_inout: &Arc<Mutex<GraphInportOutportHolder>>,
    payload: ApiTraceConnectEventPayload,
) {
    let message = TraceEventMessage::new("connect", payload);
    broadcast_to_clients(graph_inout, &message, "trace:connect");
}

fn send_trace_disconnect(
    graph_inout: &Arc<Mutex<GraphInportOutportHolder>>,
    payload: ApiTraceDisconnectEventPayload,
) {
    let message = TraceEventMessage::new("disconnect", payload);
    broadcast_to_clients(graph_inout, &message, "trace:disconnect");
}

fn create_graph_inout_handle(
    graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
) -> GraphInportOutportHandle {
    let graph_inout_for_output = graph_inout.clone();
    let graph_inout_for_preview = graph_inout;
    (
        Arc::new(move |message: String| {
            let packet = NetworkOutputResponse::new(message);
            send_network_output(&graph_inout_for_output, &packet);
        }),
        Arc::new(move |url: String| {
            let packet = NetworkOutputResponse::new_previewurl(url);
            send_network_output(&graph_inout_for_preview, &packet);
        }),
    )
}
