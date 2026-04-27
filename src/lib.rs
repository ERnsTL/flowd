#![feature(duration_constants)]
#![feature(io_error_more)]
#![feature(map_try_insert)]

// basics and threading
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, Thread};

// network and WebSocket
use std::net::TcpStream;
use tungstenite::Message;

// persistence
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

// arguments
use std::env;

// logging
#[macro_use]
extern crate log;
extern crate simplelog; //TODO check the paris feature flag for tags, useful?

// JSON serialization and deserialization
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use serde_with::skip_serializing_none;

// data structures
use multimap::MultiMap;
use std::collections::HashMap;
//use dashmap::DashMap;

// timekeeping in watchdog thread etc.
use chrono::prelude::*;
use std::time::{Duration, Instant};

// flowd component API crate
pub use flowd_component_api::{
    BudgetClass, Component, ComponentComponentPayload, ComponentPort, FbpMessage,
    GraphInportOutportHandle, MessageBuf, NodeContext, ProcessEdge, ProcessEdgeSink,
    ProcessEdgeSinkConnection, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError, WakeupNotify, PROCESSEDGE_BUFSIZE,
    PROCESSEDGE_IIP_BUFSIZE, PROCESSEDGE_SIGNAL_BUFSIZE,
};
use flowd_component_api::{
    GraphNodeSpecNetwork as TraceGraphNodeSpecNetwork,
    TraceConnectEventPayload as ApiTraceConnectEventPayload,
    TraceDataEventPayload as ApiTraceDataEventPayload,
    TraceDisconnectEventPayload as ApiTraceDisconnectEventPayload,
};

// configuration
const PROCESS_HEALTHCHECK_DUR: core::time::Duration = Duration::from_secs(7); //NOTE: 7 * core::time::Duration::SECOND is not compile-time calculatable (mul const trait not implemented)
const WATCHDOG_POLL_DUR: core::time::Duration = Duration::from_millis(50);
const WATCHDOG_MAX_MISSED_PONGS: u8 = 2;
const CLIENT_BROADCAST_WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(200));
const NODE_WIDTH_DEFAULT: u32 = 72;
const NODE_HEIGHT_DEFAULT: u32 = 72;
const PERSISTENCE_FILE_NAME: &str = "flowd.graph.json";

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
                if ok {
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

#[cfg(test)]
mod tests {
    use super::bench_api::linear_harness_direct;
    use std::any::Any;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn bench_harness_stop_does_not_deadlock_under_repetition() {
        let (done_tx, done_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                for _ in 0..8 {
                    let harness = linear_harness_direct("bench_deadlock_regression");
                    harness
                        .start()
                        .expect("runtime start failed in regression loop");
                    harness
                        .send_data_to_inport("IN", b"x")
                        .expect("runtime packet send failed in regression loop");
                    harness
                        .wait_for_outport_data("OUT", 1, Duration::from_secs(2))
                        .expect("did not observe expected output packet in regression loop");
                    harness
                        .stop()
                        .expect("runtime stop failed in regression loop");
                }
            });

            let send_result = match result {
                Ok(()) => done_tx.send(Ok(())),
                Err(panic_payload) => {
                    let msg = if let Some(s) = (&*panic_payload as &dyn Any).downcast_ref::<&str>()
                    {
                        (*s).to_string()
                    } else if let Some(s) = (&*panic_payload as &dyn Any).downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic payload".to_string()
                    };
                    done_tx.send(Err(msg))
                }
            };
            send_result.expect("failed to signal regression completion");
        });

        match done_rx.recv_timeout(Duration::from_secs(20)) {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => panic!("regression worker panicked: {msg}"),
            Err(_) => {
                panic!("timeout waiting for regression loop completion (possible stop deadlock)")
            }
        }
    }
}

#[derive(Serialize, Debug, Clone, PartialEq)]
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
            payload: RuntimeErrorResponsePayload { message: msg },
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
            payload: GraphErrorResponsePayload { message: err },
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
#[derive(Deserialize, Debug)]
struct RuntimePacketRequest {
    protocol: String,
    command: String,
    payload: RuntimePacketRequestPayload,
}

#[derive(Deserialize, Debug)]
struct RuntimePacketRequestPayload {
    // protocol spec shows it as non-optional, but fbp-protocol says only port, event, graph are required at https://github.com/flowbased/fbp-protocol/blob/555880e1f42680bf45e104b8c25b97deff01f77e/schema/yaml/runtime.yml#L46
    port: String,
    event: RuntimePacketEvent, //TODO spec what does this do? format is string, but with certain allowed values: TODO
    #[serde(rename = "type")]
    typ: Option<String>, // spec: the basic data type send, example "array" -- TODO which values are allowed here? TODO serde rename correct?
    schema: Option<String>, // spec: URL to JSON schema describing the format of the data
    graph: String,
    payload: Option<String>, // spec: payload for the packet. Used only with begingroup (for group names) and data packets. //TODO type "any" allowed
    secret: Option<String>,  // only present on the request payload
}

#[derive(Serialize, Debug)]
struct RuntimePacketResponse {
    protocol: String,
    command: String,
    payload: RuntimePacketResponsePayload,
}

//TODO serde: RuntimePacketRequestPayload is the same as RuntimePacketResponsePayload except the payload -- any possibility to mark this optional for the response?
#[serde_with::skip_serializing_none] // fbp-protocol thus noflo-ui does not like "" or null values for schema, type
#[derive(Serialize, Deserialize, Debug, Clone)]
struct RuntimePacketResponsePayload {
    port: String,
    event: RuntimePacketEvent, //TODO spec what does this do? format? fbp-protocol says: string enum
    #[serde(rename = "type")]
    typ: Option<String>, // spec: the basic data type send, example "array" -- TODO which values are allowed here? TODO serde rename correct?
    schema: Option<String>, // spec: URL to JSON schema describing the format of the data
    graph: String,
    payload: Option<String>, // spec: payload for the packet. Used only with begingroup (for group names) and data packets. //TODO type "any" allowed
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")] // fbp-protocol and noflo-ui expect this in lowercase
enum RuntimePacketEvent {
    Connect,
    BeginGroup,
    Data,
    EndGroup,
    Disconnect,
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
            event: RuntimePacketEvent::Data,
            typ: Some(String::from("string")), //TODO is this correct?
            schema: None,
            graph: String::from("default_graph"),
            payload: Some(String::from("default packet payload")),
        }
    }
}

impl RuntimePacketResponse {
    fn new(payload: RuntimePacketResponsePayload) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: payload,
        }
    }

    fn new_connect(
        graph: String,
        port: String,
        typ: Option<String>,
        schema: Option<String>,
    ) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::Connect,
                typ: typ,
                schema: schema,
                payload: None,
            },
        }
    }

    fn new_begingroup(
        graph: String,
        port: String,
        typ: Option<String>,
        schema: Option<String>,
        group_name: String,
    ) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::BeginGroup,
                typ: typ,
                schema: schema,
                payload: Some(group_name),
            },
        }
    }

    fn new_data(
        graph: String,
        port: String,
        typ: Option<String>,
        schema: Option<String>,
        payload: String,
    ) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::Connect,
                typ: typ,
                schema: schema,
                payload: Some(payload),
            },
        }
    }

    fn new_endgroup(
        graph: String,
        port: String,
        typ: Option<String>,
        schema: Option<String>,
        group_name: String,
    ) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::EndGroup,
                typ: typ,
                schema: schema,
                payload: Some(group_name),
            },
        }
    }

    fn new_disconnect(
        graph: String,
        port: String,
        typ: Option<String>,
        schema: Option<String>,
    ) -> Self {
        RuntimePacketResponse {
            protocol: String::from("runtime"),
            command: String::from("packet"),
            payload: RuntimePacketResponsePayload {
                graph: graph,
                port: port,
                event: RuntimePacketEvent::Disconnect,
                typ: typ,
                schema: schema,
                payload: None,
            },
        }
    }
}

// runtime:packetsent
#[derive(Serialize, Debug)]
struct RuntimePacketsentMessage {
    protocol: String,
    command: String,
    payload: RuntimePacketsentPayload, // clarify spec: echo the full runtime:packet back, with the full payload?! protocol spec looks like runtime needs to echo back oll of runtime:packet except secret, but fbp-protocol schema only requires port, event, graph @ https://github.com/flowbased/fbp-protocol/blob/555880e1f42680bf45e104b8c25b97deff01f77e/schema/yaml/runtime.yml#L194
}

#[serde_with::skip_serializing_none] // fbp-protocol thus noflo-ui does not like "" or null values for schema, type
#[derive(Serialize, Deserialize, Debug)] //TODO Deserialize seems useless, we are not getting that from the client? unless the client is another runtime maybe...?
struct RuntimePacketsentPayload {
    port: String,
    event: RuntimePacketEvent, //TODO spec what does this do? format? fbp-protocol says: string enum
    #[serde(rename = "type")]
    typ: Option<String>, // spec: the basic data type send, example "array" -- TODO which values are allowed here? TODO serde rename correct?
    schema: Option<String>, // spec: URL to JSON schema describing the format of the data
    graph: String,
    payload: Option<String>, // spec: payload for the packet. Used only with begingroup (for group names) and data packets. //TODO type "any" allowed
}

impl RuntimePacketsentMessage {
    //TODO for correctness, we should convert to RuntimePacketsentResponsePayload actually, but they are structurally the same
    fn new(payload: RuntimePacketsentPayload) -> Self {
        RuntimePacketsentMessage {
            protocol: String::from("runtime"),
            command: String::from("packetsent"),
            payload: payload,
        }
    }
}

impl From<RuntimePacketRequestPayload> for RuntimePacketsentPayload {
    fn from(payload: RuntimePacketRequestPayload) -> Self {
        RuntimePacketsentPayload {
            // we just leave away the field secret; and many fields can be None
            port: payload.port,
            event: payload.event,
            typ: payload.typ,
            schema: payload.schema,
            graph: payload.graph,
            payload: payload.payload,
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
    fn new(runtime: &Runtime, graph: &Graph) -> Self {
        RuntimePortsMessage {
            protocol: String::from("runtime"),
            command: String::from("ports"),
            payload: RuntimePortsPayload {
                graph: runtime.graph.clone(),
                in_ports: graph.ports_as_componentportsarray(&graph.inports),
                out_ports: graph.ports_as_componentportsarray(&graph.outports),
            },
        }
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct NetworkPersistRequest {
    protocol: String,
    command: String,
    payload: NetworkPersistRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkPersistRequestPayload {
    secret: Option<String>,
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct NetworkEdgesRequest {
    protocol: String,
    command: String,
    payload: NetworkEdgesRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkEdgesRequestPayload {
    graph: String,
    edges: Vec<GraphEdgeSpec>,
    secret: Option<String>,
}

//NOTE: Serialize trait needed for FBP graph structs, not for the FBP network protocol
#[derive(Serialize, Deserialize, Debug)]
struct GraphEdgeSpec {
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
}

#[derive(Serialize, Debug)]
struct NetworkEdgesResponse {
    protocol: String,
    command: String,
    payload: NetworkEdgesResponsePayload,
}

#[derive(Serialize, Debug)]
struct NetworkEdgesResponsePayload {
    //TODO spec: is a confirmative response of type network:edges enough or should all values be echoed back? noflo-ui definitely complains about an empty payload on a confirmative network:edges response.
    graph: String,
    edges: Vec<GraphEdgeSpec>,
} //TODO optimize senseless duplication of structs, use serde_partial (or so?) for sending the response

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
        NetworkEdgesResponsePayload {
            graph: String::from("default_graph"),
            edges: vec![],
        }
    }
}

impl NetworkEdgesResponse {
    fn from_request(payload: NetworkEdgesRequestPayload) -> Self {
        NetworkEdgesResponse {
            protocol: String::from("network"),
            command: String::from("edges"),
            payload: NetworkEdgesResponsePayload {
                graph: payload.graph,
                edges: payload.edges,
            },
        }
    }
}

// network:output response
//NOTE spec: like STDOUT output of a Unix process, or a line of console.log in JavaScript. Can also be used for passing images from the runtime to the UI ("peview URL").
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

impl NetworkOutputResponse {
    fn new(msg: String) -> Self {
        NetworkOutputResponse {
            protocol: String::from("network"),
            command: String::from("output"),
            payload: NetworkOutputResponsePayload {
                typ: String::from("message"), //TODO optimize can we &str into a const maybe instead of always generating these String::from()?
                message: Some(msg),
                url: None,
            },
        }
    }

    fn new_previewurl(url: String) -> Self {
        NetworkOutputResponse {
            protocol: String::from("network"),
            command: String::from("output"),
            payload: NetworkOutputResponsePayload {
                typ: String::from("previewurl"),
                url: Some(url),
                message: None,
            },
        }
    }
}

#[derive(Serialize, Debug)]
#[skip_serializing_none]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct NetworkOutputResponsePayload {
    #[serde(rename = "type")]
    typ: String, // spec: either "message" or "previewurl"    //TODO convert to enum
    message: Option<String>, //TODO optimize would &str as parameter be faster to decode into for serde?
    url: Option<String>,     // spec: URL for an image generated by the runtime
                             //TODO serde "rule" that either message or url must be present in case of type="message" or "previewurl", respectively
}

impl Default for NetworkOutputResponsePayload {
    fn default() -> Self {
        NetworkOutputResponsePayload {
            typ: String::from("message"),
            message: Some(String::from("default output message")),
            url: None,
        }
    }
}

// network:connect response
#[derive(Serialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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

#[skip_serializing_none]
#[derive(Serialize, Debug)]
struct NetworkTransmissionPayload {
    //TODO rename to NetworkEventPayload? In FBP network protocol spec the base fields (id, sr, tgt, graph, subgraph) are referenced ad "network/event":  https://github.com/flowbased/fbp-protocol/blob/555880e1f42680bf45e104b8c25b97deff01f77e/schema/yaml/network.yml#L246
    id: String, // spec: textual edge identifier, usually in form of a FBP language line
    // Compatibility: runtime-generated DATA packets in tests don't have src.
    src: Option<GraphNodeSpecNetwork>,
    // Compatibility: some packet types may omit tgt/src depending on event kind.
    tgt: Option<GraphNodeSpecNetwork>,
    graph: String,
    subgraph: Option<Vec<String>>, // spec: Subgraph identifier for the event. An array of node IDs. optional according to schema. TODO what does it mean? why a list of node IDs? - check schema:  https://github.com/flowbased/fbp-protocol/blob/555880e1f42680bf45e104b8c25b97deff01f77e/schema/yaml/shared.yml#L193
    data: Option<String>, //TODO fix do this using type system (composable traits? "inheritance"?) data is only mandatory for network:data response, not for begingroup, endgroup, connect, disconnect
}

impl Default for NetworkTransmissionPayload {
    fn default() -> Self {
        NetworkTransmissionPayload {
            id: String::from("Repeater.OUT -> Display.IN"), //TODO not sure if this is correct
            src: Some(GraphNodeSpecNetwork::default()),
            tgt: Some(GraphNodeSpecNetwork::default()),
            graph: String::from("main_graph"),
            subgraph: Some(vec![String::from("Repeater.OUT -> Display.IN")]), //TODO not sure of this is correct, most likely not
            data: None,
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

impl NetworkDataResponse {
    fn new(payload: NetworkTransmissionPayload) -> Self {
        //TODO solve in a better way using the type system
        if payload.data == None {
            error!("constructing a NetworkDataResponse with empty data field");
        }
        NetworkDataResponse {
            protocol: String::from("network"),
            command: String::from("data"),
            payload: payload,
        }
    }
}

// network:begingroup response
#[derive(Serialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct NetworkStartRequest {
    protocol: String,
    command: String,
    payload: NetworkStartRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkStartRequestPayload {
    graph: String,
    secret: Option<String>,
}

#[derive(Serialize, Debug)]
struct NetworkStartedResponse {
    protocol: String,
    command: String,
    payload: NetworkStartedResponsePayload,
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Debug, Clone)]
struct NetworkSchedulerMetricsPayload {
    #[serde(rename = "executionsPerNode")]
    executions_per_node: HashMap<String, u64>,
    #[serde(rename = "workUnitsPerNode")]
    work_units_per_node: HashMap<String, u64>,
    #[serde(rename = "timeSinceLastExecutionMs")]
    time_since_last_execution_ms: HashMap<String, u64>,
    #[serde(rename = "queueDepth")]
    queue_depth: usize,
    #[serde(rename = "loopIterations")]
    loop_iterations: u64,
}

fn network_scheduler_metrics(
    snapshot: &RuntimeStatusSnapshot,
) -> Option<HashMap<String, NetworkSchedulerMetricsPayload>> {
    if snapshot.scheduler_metrics.is_empty() {
        return None;
    }

    Some(
        snapshot
            .scheduler_metrics
            .iter()
            .map(|(graph_name, metrics)| {
                (
                    graph_name.clone(),
                    NetworkSchedulerMetricsPayload {
                        executions_per_node: metrics.executions_per_node.clone(),
                        work_units_per_node: metrics.work_units_per_node.clone(),
                        time_since_last_execution_ms: metrics.time_since_last_execution_ms.clone(),
                        queue_depth: metrics.queue_depth,
                        loop_iterations: metrics.loop_iterations,
                    },
                )
            })
            .collect(),
    )
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Debug, Clone)]
struct NetworkStartedResponsePayload {
    #[serde(rename = "time")]
    time_started: UtcTime, //TODO clarify spec: defined as just a String. But what time format? meaning of the field anyway?
    graph: String,
    started: bool, // spec: see network:status response for meaning of started and running //TODO spec: shouldn't this always be true?
    running: bool,
    debug: Option<bool>,
    #[serde(rename = "schedulerMetrics")]
    scheduler_metrics: Option<HashMap<String, NetworkSchedulerMetricsPayload>>,
}

//NOTE: this type alias allows us to implement Serialize (a trait from another crate) for DateTime (also from another crate)
#[derive(Debug, Clone)]
struct UtcTime(chrono::DateTime<Utc>);

impl<'de> Deserialize<'de> for UtcTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let dt = chrono::DateTime::parse_from_rfc3339(&s)
            .map_err(serde::de::Error::custom)?
            .with_timezone(&Utc);
        Ok(UtcTime(dt))
    }
}

impl Serialize for UtcTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        return Ok(serializer
            .serialize_str(self.0.format("%+").to_string().as_str())
            .expect("fail serializing datetime"));
    }
}

impl Default for NetworkStartedResponse {
    fn default() -> Self {
        NetworkStartedResponse {
            protocol: String::from("network"),
            command: String::from("started"),
            payload: NetworkStartedResponsePayload::default(),
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
            debug: None,
            scheduler_metrics: None,
        }
    }
}

impl NetworkStartedResponse {
    fn new(status: NetworkStartedResponsePayload) -> Self {
        NetworkStartedResponse {
            protocol: String::from("network"),
            command: String::from("started"),
            payload: status,
        }
    }
}

impl From<RuntimeStatusSnapshot> for NetworkStartedResponsePayload {
    fn from(status: RuntimeStatusSnapshot) -> Self {
        NetworkStartedResponsePayload {
            time_started: UtcTime(status.time_started),
            graph: status.graph,
            started: status.started,
            running: status.running,
            debug: status.debug,
            // Keep protocol responses schema-compatible with strict fbp-protocol tests.
            scheduler_metrics: None,
        }
    }
}

// network:stop -> network:stopped | network:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct NetworkStopRequest {
    protocol: String,
    command: String,
    payload: NetworkStopRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkStopRequestPayload {
    graph: String,
    secret: Option<String>,
}

#[derive(Serialize, Debug)]
struct NetworkStoppedResponse {
    protocol: String,
    command: String,
    payload: NetworkStoppedResponsePayload,
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Debug)]
struct NetworkStoppedResponsePayload {
    #[serde(rename = "time")]
    time_stopped: UtcTime, //TODO spec: string. clarify spec: time format? purpose? datetime where network was stopped?
    uptime: i64, // spec: time the network was running, in seconds //TODO spec: should the time it was stopped be subtracted from this number? //TODO spec: not "time" but "duration"
    graph: String,
    started: bool, // spec: see network:status response for meaning of started and running
    running: bool, // TODO spec: shouldn't this always be false?    //TODO spec: ordering of fields is different between network:started and network:stopped -> fix in spec.
    debug: Option<bool>,
    #[serde(rename = "schedulerMetrics")]
    scheduler_metrics: Option<HashMap<String, NetworkSchedulerMetricsPayload>>,
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
            debug: None,
            scheduler_metrics: None,
        }
    }
}

impl NetworkStoppedResponse {
    fn new(status: &RuntimeStatusSnapshot) -> Self {
        let uptime_duration = chrono::Utc::now() - status.time_started;
        let uptime_seconds = uptime_duration.num_seconds();
        println!(
            "graph '{}' uptime: {}",
            status.graph,
            format_duration(Duration::from_secs(uptime_seconds as u64))
        );
        return NetworkStoppedResponse {
            protocol: String::from("network"),
            command: String::from("stopped"),
            payload: NetworkStoppedResponsePayload {
                time_stopped: UtcTime(chrono::Utc::now()),
                uptime: uptime_seconds,
                graph: status.graph.clone(),
                started: status.started,
                running: status.running,
                debug: status.debug,
                // Keep protocol responses schema-compatible with strict fbp-protocol tests.
                scheduler_metrics: None,
            },
        };
    }
}

// network:getstatus -> network:status | network:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct NetworkGetstatusMessage {
    protocol: String,
    command: String,
    payload: NetworkGetstatusPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkGetstatusPayload {
    graph: String,
    secret: Option<String>,
}

// ----------

#[derive(Serialize, Debug)]
struct NetworkStatusMessage {
    protocol: String,
    command: String,
    payload: NetworkStatusPayload,
}

impl Default for NetworkStatusMessage {
    fn default() -> Self {
        NetworkStatusMessage {
            protocol: String::from("network"),
            command: String::from("status"),
            payload: NetworkStatusPayload::default(),
        }
    }
}

//TODO payload has small size, we could copy it, problem is NetworkStatusPayload cannot automatically derive Copy because of the String does not implement Copy
impl NetworkStatusMessage {
    fn new(payload: NetworkStatusPayload) -> Self {
        NetworkStatusMessage {
            protocol: String::from("network"),
            command: String::from("status"),
            payload: payload,
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Debug, Clone)]
struct NetworkStatusPayload {
    graph: String,
    uptime: Option<i64>, // spec: time the network has been running, in seconds. NOTE: seconds since start of the network. NOTE: i64 because of return type from new() chrono calculations return type, which cannot be converted to u32.
    // NOTE: started+running=is running now. started+not running=network has finished. not started+not running=network was never started. not started+running=undefined (TODO).
    started: bool,       // spec: whether or not network has been started
    running: bool,       // spec: boolean tells whether the network is running at the moment or not
    debug: Option<bool>, // spec: whether or not network is in debug mode
    #[serde(rename = "schedulerMetrics")]
    scheduler_metrics: Option<HashMap<String, NetworkSchedulerMetricsPayload>>,
}

impl Default for NetworkStatusPayload {
    fn default() -> Self {
        NetworkStatusPayload {
            graph: String::from("default_graph"),
            uptime: None,
            started: true,
            running: true,
            debug: None,
            scheduler_metrics: None,
        }
    }
}

impl NetworkStatusPayload {
    fn new(status: &RuntimeStatusSnapshot) -> Self {
        NetworkStatusPayload {
            graph: status.graph.clone(),
            uptime: None,
            started: status.started,
            running: status.running,
            debug: status.debug,
            // Keep protocol responses schema-compatible with strict fbp-protocol tests.
            scheduler_metrics: None,
        }
    }
}

// network:debug -> TODO spec: response not specified | network:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct NetworkDebugRequest {
    protocol: String,
    command: String,
    payload: NetworkDebugRequestPayload,
}

#[derive(Deserialize, Debug)]
struct NetworkDebugRequestPayload {
    enable: bool,
    graph: String,
    secret: Option<String>,
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
            payload: NetworkDebugResponsePayload { graph: graph },
        }
    }
}

// ----------
// protocol:component
// ----------

// component:list -> component:component (multiple possible), then a final component:componentsready | component:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct ComponentListRequest {
    protocol: String,
    command: String,
    payload: ComponentListRequestPayload,
}

#[derive(Deserialize, Debug)]
struct ComponentListRequestPayload {
    // Compatibility: tests send component:list with payload {} and no secret.
    secret: Option<String>,
}

// ----------

#[derive(Serialize, Debug)]
struct ComponentComponentMessage<'a> {
    protocol: String,
    command: String,
    payload: ComponentComponentResponsePayload<'a>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ComponentComponentResponsePayload<'a> {
    // Compatibility: tests look for collection-prefixed names such as "core/Repeat".
    name: String,
    description: &'a String,
    icon: &'a String,
    subgraph: bool,
    in_ports: &'a Vec<ComponentPort>,
    out_ports: &'a Vec<ComponentPort>,
}

impl<'a> ComponentComponentMessage<'a> {
    fn new(payload: &'a ComponentComponentPayload) -> Self {
        // Compatibility: internal component names are often unprefixed, but tests expect "core/<name>".
        let name = if payload.name.contains('/') {
            payload.name.clone()
        } else {
            format!("core/{}", payload.name)
        };
        ComponentComponentMessage {
            protocol: String::from("component"),
            command: String::from("component"),
            payload: ComponentComponentResponsePayload {
                name,
                description: &payload.description,
                icon: &payload.icon,
                subgraph: payload.subgraph,
                in_ports: &payload.in_ports,
                out_ports: &payload.out_ports,
            },
        }
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct ComponentGetsourceMessage {
    protocol: String,
    command: String,
    payload: ComponentGetsourcePayload,
}

#[derive(Deserialize, Debug)]
struct ComponentGetsourcePayload {
    name: String, // spec: Name of the component to for which to get source code. Should contain the library prefix, eg. "my-project/SomeComponent"
    secret: Option<String>,
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphClearRequest {
    protocol: String,
    command: String,
    payload: GraphClearRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphClearRequestPayload {
    #[serde(rename = "id")]
    name: String, // name of the graph
    #[serde(rename = "name")]
    label: Option<String>, // human-readable label of the graph
    // Compatibility: fbp-protocol test fixture sends graph:clear without these keys.
    library: Option<String>, //TODO clarify spec, optional per fbp-tests
    main: bool,              // TODO clarify spec
    icon: Option<String>,
    description: Option<String>,
    secret: Option<String>,
}

#[derive(Serialize, Debug)]
struct GraphClearResponse {
    protocol: String,
    command: String,
    payload: GraphClearResponsePayload,
}

#[serde_with::skip_serializing_none]
#[derive(Serialize, Debug)]
struct GraphClearResponsePayload {
    #[serde(rename = "id")]
    name: String, // name of the graph
    #[serde(rename = "name")]
    label: String, // human-readable label of the graph
    // Compatibility: tests fail if optional keys are echoed as empty strings.
    library: Option<String>, //TODO clarify spec
    main: bool,              // TODO clarify spec
    icon: Option<String>,
    description: Option<String>,
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
            library: None,
            main: true,
            icon: None,
            description: None,
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
                label: payload
                    .label
                    .clone()
                    .unwrap_or_else(|| payload.name.clone()),
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphAddnodeRequest {
    protocol: String,
    command: String,
    payload: GraphAddnodeRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphAddnodeRequestPayload {
    #[serde(rename = "id")]
    name: String, // name of the node/process
    component: String, // component name to be used for this node/process
    // Compatibility: tests send arbitrary metadata object and may omit x/y/width/height.
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>, //TODO spec: key-value pairs (with some well-known values)
    // Compatibility: tests also exercise "graph missing" behavior on purpose.
    graph: Option<String>, // name of the graph
    icon: Option<String>,  // optional icon for the node
    secret: Option<String>,
}

// NOTE: Serialize because used in GraphNode -> Graph which needs to be serialized
#[derive(Serialize, Deserialize, Debug)]
struct GraphNodeMetadata {
    x: i32, // TODO check spec: can x and y be negative? -> i32 or u32? TODO in specs is range not defined, but noflo-ui uses negative coordinates as well
    y: i32,
    width: Option<u32>, // not mentioned in specs, but used by noflo-ui, usually 72
    height: Option<u32>, // not mentioned in specs, but used by noflo-ui, usually 72
    label: Option<String>, // not mentioned in specs, but used by noflo-ui, used for the process name in bigger letters than component name
    icon: Option<String>,  // icon for the node, used by FBP protocol clients like noflo-ui
}

impl Clone for GraphNodeMetadata {
    fn clone(&self) -> Self {
        GraphNodeMetadata {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            label: self.label.clone(),
            icon: self.icon.clone(),
        }
    }
}

impl Default for GraphNodeMetadata {
    fn default() -> Self {
        GraphNodeMetadata {
            x: 0,
            y: 0,
            width: Some(NODE_WIDTH_DEFAULT),
            height: Some(NODE_HEIGHT_DEFAULT),
            label: None,
            icon: None,
        }
    }
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

impl GraphAddnodeResponse {
    fn new(payload: GraphAddnodeResponsePayload) -> Self {
        GraphAddnodeResponse {
            protocol: String::from("graph"),
            command: String::from("addnode"),
            payload: payload,
        }
    }
}

#[derive(Serialize, Debug)]
struct GraphAddnodeResponsePayload {
    // TODO check spec: should the sent values be echoed back as confirmation or is empty graph:addnode vs. a graph:error enough?
    #[serde(rename = "id")]
    name: String,
    component: String,
    metadata: JsonMap<String, JsonValue>,
    graph: String,
}

impl Default for GraphAddnodeResponsePayload {
    fn default() -> Self {
        GraphAddnodeResponsePayload {
            name: "".to_owned(),
            component: "".to_owned(),
            metadata: JsonMap::new(),
            graph: String::from("default_graph"),
        }
    }
}

// graph:removenode -> graph:removenode | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
    secret: Option<String>,
}

#[derive(Serialize, Debug)]
struct GraphRemovenodeResponse {
    protocol: String,
    command: String,
    payload: GraphRemovenodeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemovenodeResponsePayload {
    #[serde(rename = "id")]
    name: String,
    graph: String,
}

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
        GraphRemovenodeResponsePayload {
            name: String::new(),
            graph: String::from("default_graph"),
        }
    }
}

impl GraphRemovenodeResponse {
    fn from_request(payload: GraphRemovenodeRequestPayload) -> Self {
        GraphRemovenodeResponse {
            protocol: String::from("graph"),
            command: String::from("removenode"),
            payload: GraphRemovenodeResponsePayload {
                name: payload.name,
                graph: payload.graph,
            },
        }
    }
}

// graph:renamenode -> graph:renamenode | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
    secret: Option<String>,
}

#[derive(Serialize, Debug)]
struct GraphRenamenodeResponse {
    protocol: String,
    command: String,
    payload: GraphRenamenodeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRenamenodeResponsePayload {
    from: String,
    to: String,
    graph: String,
}

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
        GraphRenamenodeResponsePayload {
            from: String::new(),
            to: String::new(),
            graph: String::from("default_graph"),
        }
    }
}

impl GraphRenamenodeResponse {
    fn from_request(payload: GraphRenamenodeRequestPayload) -> Self {
        GraphRenamenodeResponse {
            protocol: String::from("graph"),
            command: String::from("renamenode"),
            payload: GraphRenamenodeResponsePayload {
                from: payload.from,
                to: payload.to,
                graph: payload.graph,
            },
        }
    }
}

// graph:changenode -> graph:changenode | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphChangenodeRequest {
    protocol: String,
    command: String,
    payload: GraphChangenodeRequestPayload, //TODO spec: key-value pairs (with some well-known values)
}

#[derive(Deserialize, Debug)]
struct GraphChangenodeRequestPayload {
    #[serde(rename = "id")]
    name: String,
    // Compatibility: tests send sparse metadata and use null values to delete keys.
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
    graph: String,
    secret: Option<String>, // if using a single GraphChangenodeMessage struct, this field would be sent in response message
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
    metadata: JsonMap<String, JsonValue>,
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
            metadata: JsonMap::new(),
            graph: String::from("default_graph"),
        }
    }
}

// graph:addedge -> graph:addedge | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphAddedgeRequest {
    protocol: String,
    command: String,
    payload: GraphAddedgeRequestPayload,
}

#[derive(Deserialize, Debug, Clone)]
struct GraphAddedgeRequestPayload {
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    #[serde(default)]
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    graph: String,
    secret: Option<String>, // only present in the request payload
}

#[skip_serializing_none]
//NOTE: PartialEq is for graph.remove_edge() and graph.change_edge()
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
struct GraphNodeSpecNetwork {
    node: String,
    port: String,
    index: Option<String>, // spec: connection index, for addressable ports //TODO spec: string or number -- how to handle in Rust? // NOTE: noflo-ui leaves index away if it is not an indexable port
}

impl Default for GraphNodeSpecNetwork {
    fn default() -> Self {
        GraphNodeSpecNetwork {
            node: String::from("Repeater"),
            port: String::from("IN"),
            index: Some(String::from("1")),
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Deserialize, Serialize, Debug, Clone)]
struct GraphEdgeMetadata {
    route: Option<i32>,     //TODO clarify spec: Route identifier of a graph edge
    schema: Option<String>, //TODO clarify spec: JSON schema associated with a graph edge (TODO check schema)
    secure: Option<bool>,   //TODO clarify spec: Whether edge data should be treated as secure
}

impl Default for GraphEdgeMetadata {
    fn default() -> Self {
        GraphEdgeMetadata {
            //TODO clarify spec: totally unsure what these mean or if these are sensible defaults or if better to leave fields undefined if no value
            route: None,
            schema: None,
            secure: None,
        }
    }
}

impl GraphEdgeMetadata {
    fn new(route: Option<i32>, schema: Option<String>, secure: Option<bool>) -> Self {
        GraphEdgeMetadata {
            route: route,
            schema: schema,
            secure: secure,
        }
    }
}

#[derive(Serialize, Debug)]
struct GraphAddedgeResponse {
    protocol: String,
    command: String,
    payload: GraphAddedgeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphAddedgeResponsePayload {
    //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addedge instead of graph:error enough? sending no response -> timeout and sending graph:addedge with empty payload -> error.
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    graph: String,
} //TODO optimize struct duplication with GraphAddedgeRequestPayload

impl GraphAddedgeResponse {
    fn from_request(payload: GraphAddedgeRequestPayload) -> Self {
        GraphAddedgeResponse {
            protocol: String::from("graph"),
            command: String::from("addedge"),
            payload: GraphAddedgeResponsePayload {
                src: payload.src,
                tgt: payload.tgt,
                metadata: payload.metadata,
                graph: payload.graph,
            },
        }
    }
}

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
        GraphAddedgeResponsePayload {
            src: GraphNodeSpecNetwork::default(),
            tgt: GraphNodeSpecNetwork::default(),
            metadata: GraphEdgeMetadata::default(),
            graph: String::from("default_graph"),
        }
    }
}

// graph:removeedge -> graph:removeedge | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphRemoveedgeRequest {
    protocol: String,
    command: String,
    payload: GraphRemoveedgeRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemoveedgeRequestPayload {
    graph: String, //TODO spec: for graph:addedge the graph attricbute is after src,tgt but for removeedge it is first
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    secret: Option<String>, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRemoveedgeResponse {
    protocol: String,
    command: String,
    payload: GraphRemoveedgeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemoveedgeResponsePayload {
    //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:addedge instead of graph:error enough? if not sending a response, then comes a timeout. sending empty payload gives an error.
    graph: String, //TODO spec: for graph:addedge the graph attricbute is after src,tgt but for removeedge it is first
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
} //TODO optimize useless duplication

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
        GraphRemoveedgeResponsePayload {
            graph: String::from("default_graph"),
            src: GraphNodeSpecNetwork::default(),
            tgt: GraphNodeSpecNetwork::default(),
        }
    }
}

impl GraphRemoveedgeResponse {
    fn from_request(payload: GraphRemoveedgeRequestPayload) -> Self {
        GraphRemoveedgeResponse {
            protocol: String::from("graph"),
            command: String::from("removeedge"),
            payload: GraphRemoveedgeResponsePayload {
                graph: payload.graph,
                src: payload.src,
                tgt: payload.tgt,
            },
        }
    }
}

// graph:changeedge -> graph:changeedge | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphChangeedgeRequest {
    protocol: String,
    command: String,
    payload: GraphChangeedgeRequestPayload,
}

#[derive(Deserialize, Debug, Clone)]
struct GraphChangeedgeRequestPayload {
    graph: String,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    secret: Option<String>, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphChangeedgeResponse {
    protocol: String,
    command: String,
    payload: GraphChangeedgeResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphChangeedgeResponsePayload {
    //TODO clarify spec: should request values be echoed back as confirmation or is message type graph:changeedge instead of graph:error enough? dont know if a response is expected, but sending one with an empty payload gives an error.
    graph: String,
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    src: GraphNodeSpecNetwork,
    tgt: GraphNodeSpecNetwork,
} //TODO optimize struct duplication from GraphChangeedgeRequestPayload

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
        GraphChangeedgeResponsePayload {
            graph: String::from("default_graph"),
            metadata: GraphEdgeMetadata::default(),
            src: GraphNodeSpecNetwork::default(),
            tgt: GraphNodeSpecNetwork::default(),
        }
    }
}

impl GraphChangeedgeResponse {
    fn from_request(payload: GraphChangeedgeRequestPayload) -> Self {
        GraphChangeedgeResponse {
            protocol: String::from("graph"),
            command: String::from("changeedge"),
            payload: GraphChangeedgeResponsePayload {
                graph: payload.graph,
                src: payload.src,
                tgt: payload.tgt,
                metadata: payload.metadata,
            },
        }
    }
}

// graph:addinitial -> graph:addinitial | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphAddinitialRequest {
    protocol: String,
    command: String,
    payload: GraphAddinitialRequestPayload,
}

#[derive(Deserialize, Debug, Clone)]
struct GraphAddinitialRequestPayload {
    graph: String,
    // Compatibility: tests omit metadata for addinitial.
    #[serde(default)]
    metadata: GraphEdgeMetadata, //TODO spec: key-value pairs (with some well-known values)
    src: GraphIIPSpecNetwork, //TODO spec: object,array,string,number,integer,boolean,null. //NOTE: this is is for the IIP structure from the FBP Network protocol, it is different in the FBP Graph spec schema!
    tgt: GraphNodeSpecNetwork,
    secret: Option<String>, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphAddinitialResponse {
    protocol: String,
    command: String,
    payload: GraphAddinitialResponsePayload,
}

//NOTE: Serialize for graph:addinitial which makes use of the "data" field in graph -> connections -> data according to FBP JSON graph spec.
//NOTE: PartialEq are for graph.remove_initialip()
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct GraphIIPSpecNetwork {
    data: String, // spec: can put JSON object, array, string, number, integer, boolean, null in there TODO how to handle this in Rust / serde?
}

#[derive(Serialize, Debug)]
struct GraphAddinitialResponsePayload {
    src: GraphIIPSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    metadata: GraphEdgeMetadata,
    graph: String,
}

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
        GraphAddinitialResponsePayload {
            src: GraphIIPSpecNetwork {
                data: String::new(),
            },
            tgt: GraphNodeSpecNetwork::default(),
            metadata: GraphEdgeMetadata::default(),
            graph: String::from("default_graph"),
        }
    }
}

impl GraphAddinitialResponse {
    fn from_request(payload: &GraphAddinitialRequestPayload) -> Self {
        GraphAddinitialResponse {
            protocol: String::from("graph"),
            command: String::from("addinitial"),
            payload: GraphAddinitialResponsePayload {
                src: payload.src.clone(),
                tgt: payload.tgt.clone(),
                metadata: payload.metadata.clone(),
                graph: payload.graph.clone(),
            },
        }
    }
}

// graph:removeinitial -> graph:removeinitial | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphRemoveinitialRequest {
    protocol: String,
    command: String,
    payload: GraphRemoveinitialRequestPayload,
}

#[derive(Deserialize, Debug, Clone)]
struct GraphRemoveinitialRequestPayload {
    graph: String,
    // Compatibility: tests remove initial connections with only tgt+graph (src omitted).
    src: Option<GraphIIPSpecNetwork>, //TODO spec: object,array,string,number,integer,boolean,null. //NOTE: this is is for the IIP structure from the FBP Network protocol, it is different in the FBP Graph spec schema!
    tgt: GraphNodeSpecNetwork,
    secret: Option<String>, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRemoveinitialResponse {
    protocol: String,
    command: String,
    payload: GraphRemoveinitialResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemoveinitialResponsePayload {
    src: GraphIIPSpecNetwork,
    tgt: GraphNodeSpecNetwork,
    graph: String,
}

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
        GraphRemoveinitialResponsePayload {
            src: GraphIIPSpecNetwork {
                data: String::new(),
            },
            tgt: GraphNodeSpecNetwork::default(),
            graph: String::from("default_graph"),
        }
    }
}

impl GraphRemoveinitialResponse {
    fn from_removed(graph: String, src: GraphIIPSpecNetwork, tgt: GraphNodeSpecNetwork) -> Self {
        GraphRemoveinitialResponse {
            protocol: String::from("graph"),
            command: String::from("removeinitial"),
            payload: GraphRemoveinitialResponsePayload { src, tgt, graph },
        }
    }
}

// graph:addinport -> graph:addinport | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphAddinportRequest {
    protocol: String,
    command: String,
    payload: GraphAddinportRequestPayload,
}

#[derive(Deserialize, Debug, Clone)]
struct GraphAddinportRequestPayload {
    graph: String,
    public: String, // public name of the exported port
    node: String,
    port: String,
    // Compatibility: tests send addinport without metadata.
    metadata: Option<GraphNodeMetadata>, //TODO spec: key-value pairs (with some well-known values)
    secret: Option<String>,              // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphAddinportResponse {
    protocol: String,
    command: String,
    payload: GraphAddinportResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphAddinportResponsePayload {
    // Compatibility: tests expect these fields to be echoed in addinport ACK.
    graph: String,
    public: String,
    node: String,
    port: String,
}

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
        GraphAddinportResponsePayload {
            graph: String::from("default_graph"),
            public: String::new(),
            node: String::new(),
            port: String::new(),
        }
    }
}

impl GraphAddinportResponse {
    fn from_request(payload: GraphAddinportRequestPayload) -> Self {
        GraphAddinportResponse {
            protocol: String::from("graph"),
            command: String::from("addinport"),
            payload: GraphAddinportResponsePayload {
                graph: payload.graph,
                public: payload.public,
                node: payload.node,
                port: payload.port,
            },
        }
    }
}

// graph:removeinport -> graph:removeinport | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphRemoveinportRequest {
    protocol: String,
    command: String,
    payload: GraphRemoveinportRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemoveinportRequestPayload {
    graph: String,
    public: String,         // public name of the exported port
    secret: Option<String>, // only present in the request payload
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
    secret: Option<String>, // only present in the request payload
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphAddoutportRequest {
    protocol: String,
    command: String,
    payload: GraphAddoutportRequestPayload,
}

#[derive(Deserialize, Debug, Clone)]
struct GraphAddoutportRequestPayload {
    graph: String,
    public: String, // public name of the exported port
    node: String,
    port: String,
    // Compatibility: tests send addoutport without metadata.
    metadata: Option<GraphNodeMetadata>, //TODO spec: key-value pairs (with some well-known values)
    secret: Option<String>,              // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphAddoutportResponse {
    protocol: String,
    command: String,
    payload: GraphAddoutportResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphAddoutportResponsePayload {
    // Compatibility: tests expect these fields to be echoed in addoutport ACK.
    graph: String,
    public: String,
    node: String,
    port: String,
}

impl Default for GraphAddoutportResponsePayload {
    fn default() -> Self {
        GraphAddoutportResponsePayload {
            graph: String::from("default_graph"),
            public: String::new(),
            node: String::new(),
            port: String::new(),
        }
    }
}

impl Default for GraphAddoutportResponse {
    fn default() -> Self {
        GraphAddoutportResponse {
            protocol: String::from("graph"),
            command: String::from("addoutport"),
            payload: GraphAddoutportResponsePayload::default(),
        }
    }
}

impl GraphAddoutportResponse {
    fn from_request(payload: GraphAddoutportRequestPayload) -> Self {
        GraphAddoutportResponse {
            protocol: String::from("graph"),
            command: String::from("addoutport"),
            payload: GraphAddoutportResponsePayload {
                graph: payload.graph,
                public: payload.public,
                node: payload.node,
                port: payload.port,
            },
        }
    }
}

// graph:removeoutport -> graph:removeoutport | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphRemoveoutportRequest {
    protocol: String,
    command: String,
    payload: GraphRemoveoutportRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemoveoutportRequestPayload {
    graph: String,
    public: String,         // public name of the exported port
    secret: Option<String>, // only present in the request payload
}

#[derive(Serialize, Debug)]
struct GraphRemoveoutportResponse {
    protocol: String,
    command: String,
    payload: GraphRemoveoutportResponsePayload,
}

#[derive(Serialize, Debug)]
struct GraphRemoveoutportResponsePayload {
    // Compatibility: tests expect these fields to be echoed in removeoutport ACK.
    graph: String,
    public: String,
}

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
        GraphRemoveoutportResponsePayload {
            graph: String::from("default_graph"),
            public: String::new(),
        }
    }
}

impl GraphRemoveoutportResponse {
    fn from_request(payload: GraphRemoveoutportRequestPayload) -> Self {
        GraphRemoveoutportResponse {
            protocol: String::from("graph"),
            command: String::from("removeoutport"),
            payload: GraphRemoveoutportResponsePayload {
                graph: payload.graph,
                public: payload.public,
            },
        }
    }
}

// graph:renameoutport -> graph:renameoutport | graph:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
    secret: Option<String>, // only present in the request payload
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
    secret: Option<String>,       // only present in the request payload
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct GraphRemovegroupRequest {
    protocol: String,
    command: String,
    payload: GraphRemovegroupRequestPayload,
}

#[derive(Deserialize, Debug)]
struct GraphRemovegroupRequestPayload {
    graph: String,
    name: String,
    secret: Option<String>, // only present in the request payload
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
    secret: Option<String>, // only present in the request payload
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
    secret: Option<String>, // only present in the request payload
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
// protocol:runtime
// ----------

#[derive(Deserialize, Debug)]
#[serde(tag = "command", content = "payload")]
enum RuntimeMessage {
    #[serde(rename = "getruntime")]
    Getruntime(RuntimeGetruntimePayload),
    #[serde(rename = "runtime")]
    Runtime,
    #[serde(rename = "ports")]
    Ports,
    #[serde(rename = "packet")]
    Packet(RuntimePacketRequestPayload),
    #[serde(rename = "packetsent")]
    Packetsent(RuntimePacketsentPayload),
}

// ----------
// protocol:network
// ----------

#[derive(Deserialize, Debug)]
#[serde(tag = "command", content = "payload")]
enum NetworkMessage {
    #[serde(rename = "persist")]
    Persist(NetworkPersistRequestPayload),
    #[serde(rename = "getstatus")]
    Getstatus(NetworkGetstatusPayload),
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "edges")]
    Edges(NetworkEdgesRequestPayload),
    #[serde(rename = "start")]
    Start(NetworkStartRequestPayload),
    #[serde(rename = "stop")]
    Stop(NetworkStopRequestPayload),
    #[serde(rename = "debug")]
    Debug(NetworkDebugRequestPayload),
}

// ----------
// protocol:component
// ----------

#[derive(Deserialize, Debug)]
#[serde(tag = "command", content = "payload")]
enum ComponentMessage {
    #[serde(rename = "getsource")]
    Getsource(ComponentGetsourcePayload),
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "list")]
    List(ComponentListRequestPayload),
    #[serde(rename = "component")]
    Component,
    #[serde(rename = "componentsready")]
    Componentsready,
}

// ----------
// protocol:graph
// ----------

#[derive(Deserialize, Debug)]
#[serde(tag = "command", content = "payload")]
enum GraphMessage {
    #[serde(rename = "clear")]
    Clear(GraphClearRequestPayload),
    #[serde(rename = "addnode")]
    Addnode(GraphAddnodeRequestPayload),
    #[serde(rename = "changenode")]
    Changenode(GraphChangenodeRequestPayload),
    #[serde(rename = "renamenode")]
    Renamenode(GraphRenamenodeRequestPayload),
    #[serde(rename = "removenode")]
    Removenode(GraphRemovenodeRequestPayload),
    #[serde(rename = "addedge")]
    Addedge(GraphAddedgeRequestPayload),
    #[serde(rename = "removeedge")]
    Removeedge(GraphRemoveedgeRequestPayload),
    #[serde(rename = "changeedge")]
    Changeedge(GraphChangeedgeRequestPayload),
    #[serde(rename = "addinitial")]
    Addinitial(GraphAddinitialRequestPayload),
    #[serde(rename = "removeinitial")]
    Removeinitial(GraphRemoveinitialRequestPayload),
    #[serde(rename = "addinport")]
    Addinport(GraphAddinportRequestPayload),
    #[serde(rename = "removeinport")]
    Removeinport(GraphRemoveinportRequestPayload),
    #[serde(rename = "renameinport")]
    Renameinport(GraphRenameinportRequestPayload),
    #[serde(rename = "addoutport")]
    Addoutport(GraphAddoutportRequestPayload),
    #[serde(rename = "removeoutport")]
    Removeoutport(GraphRemoveoutportRequestPayload),
    #[serde(rename = "renameoutport")]
    Renameoutport(GraphRenameoutportRequestPayload),
    #[serde(rename = "addgroup")]
    Addgroup(GraphAddgroupRequestPayload),
    #[serde(rename = "removegroup")]
    Removegroup(GraphRemovegroupRequestPayload),
    #[serde(rename = "renamegroup")]
    Renamegroup(GraphRenamegroupRequestPayload),
    #[serde(rename = "changegroup")]
    Changegroup(GraphChangegroupRequestPayload),
}

// ----------
// protocol:trace
// ----------

// spec: This protocol is utilized for triggering and transmitting Flowtraces, see https://github.com/flowbased/flowtrace

#[derive(Deserialize, Debug)]
#[serde(tag = "command", content = "payload")]
enum TraceMessage {
    #[serde(rename = "start")]
    Start(TraceStartRequestPayload),
    #[serde(rename = "stop")]
    Stop(TraceStopRequestPayload),
    #[serde(rename = "clear")]
    Clear(TraceClearRequestPayload),
    #[serde(rename = "dump")]
    Dump(TraceDumpRequestPayload),
    // Trace events (outgoing)
    #[serde(rename = "data")]
    Data(ApiTraceDataEventPayload),
    #[serde(rename = "connect")]
    Connect(ApiTraceConnectEventPayload),
    #[serde(rename = "disconnect")]
    Disconnect(ApiTraceDisconnectEventPayload),
}

// trace:start -> trace:start | trace:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
    secret: Option<String>, // only present in the request payload
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
            graph: String::from("default_graph"),
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
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct TraceStopRequest {
    protocol: String,
    command: String,
    payload: TraceStopRequestPayload,
}

#[derive(Deserialize, Debug)]
struct TraceStopRequestPayload {
    graph: String,
    secret: Option<String>, // only present in the request payload
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
            payload: TraceStopResponsePayload { graph: graph },
        }
    }
}

// trace:clear -> trace:clear | trace:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
struct TraceClearRequest {
    protocol: String,
    command: String,
    payload: TraceClearRequestPayload,
}

#[derive(Deserialize, Debug)]
struct TraceClearRequestPayload {
    graph: String,
    secret: Option<String>, // only present in the request payload
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
            payload: TraceClearResponsePayload { graph: graph },
        }
    }
}

// trace:dump -> trace:dump | trace:error
#[derive(Deserialize, Debug)]
#[allow(dead_code)] // Never constructed, only used for serde deserialization into enum variants
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
    secret: Option<String>, // only present in the request payload
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
    flowtrace: String, //TODO any better format than String - a data structure?
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
            flowtrace: String::from(""), //TODO clarify spec: how to indicate an empty tracefile? Does it need "[]" or "{}" at least?
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
            payload: TraceErrorResponsePayload { message: err },
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
pub struct Graph {
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
enum AccessLevel {
    ReadOnly,
    ReadWrite,
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
    #[serde(default, skip_serializing)]
    protocol_metadata: JsonMap<String, JsonValue>,
}

#[serde_with::skip_serializing_none]
// noflo-ui interprets even "data": null as "this is an IIP". not good but we can disable serializing None //TODO make issue in noflo-ui
#[derive(Serialize, Deserialize, Debug)]
struct GraphEdge {
    #[serde(rename = "src")]
    source: GraphNodeSpec,
    //TODO enable sending of object/hashmap IIPs also, currently allows only string
    data: Option<String>, // spec: inconsistency between Graph spec schema and Network Protocol spec! Graph: data outside here, but Network protocol says "data" is field inside src and remaining fields are removed.
    #[serde(rename = "tgt")]
    target: GraphNodeSpec,
    metadata: GraphEdgeMetadata,
}

#[serde_with::skip_serializing_none] // do not serialize index if it is None
#[derive(Deserialize, Serialize, Debug)]
struct GraphNodeSpec {
    process: String,
    port: String,
    index: Option<String>, // spec: connection index, for addressable ports //TODO spec: string or number -- how to handle in Rust? // NOTE: noflo-ui leaves index away if it is not an indexable port
}

impl PartialEq<GraphNodeSpecNetwork> for GraphNodeSpec {
    fn eq(&self, other: &GraphNodeSpecNetwork) -> bool {
        self.process == other.node && self.port == other.port && self.index == other.index
    }
}

fn graph_node_metadata_from_payload(metadata: &JsonMap<String, JsonValue>) -> GraphNodeMetadata {
    let mut parsed = GraphNodeMetadata::default();
    if let Some(JsonValue::Number(x)) = metadata.get("x") {
        if let Some(x) = x.as_i64() {
            parsed.x = x as i32;
        }
    }
    if let Some(JsonValue::Number(y)) = metadata.get("y") {
        if let Some(y) = y.as_i64() {
            parsed.y = y as i32;
        }
    }
    if let Some(JsonValue::Number(width)) = metadata.get("width") {
        parsed.width = width.as_u64().map(|w| w as u32);
    }
    if let Some(JsonValue::Number(height)) = metadata.get("height") {
        parsed.height = height.as_u64().map(|h| h as u32);
    }
    if let Some(JsonValue::String(label)) = metadata.get("label") {
        parsed.label = Some(label.clone());
    }
    if let Some(JsonValue::String(icon)) = metadata.get("icon") {
        parsed.icon = Some(icon.clone());
    }
    parsed
}

fn graph_node_metadata_to_payload(metadata: &GraphNodeMetadata) -> JsonMap<String, JsonValue> {
    let mut out = JsonMap::new();
    out.insert(String::from("x"), JsonValue::from(metadata.x));
    out.insert(String::from("y"), JsonValue::from(metadata.y));
    if let Some(width) = metadata.width {
        out.insert(String::from("width"), JsonValue::from(width));
    }
    if let Some(height) = metadata.height {
        out.insert(String::from("height"), JsonValue::from(height));
    }
    if let Some(label) = &metadata.label {
        out.insert(String::from("label"), JsonValue::from(label.clone()));
    }
    if let Some(icon) = &metadata.icon {
        out.insert(String::from("icon"), JsonValue::from(icon.clone()));
    }
    out
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
            groups: vec![],
            nodes: HashMap::new(),
            edges: vec![],
        }
    }

    /// Converts FBP graph ports to component API port definitions.
    ///
    /// This conversion is inherently limited by the FBP protocol specification, which only provides
    /// minimal port information in graph definitions. The resulting ComponentPort structs contain
    /// only the port names, with all other fields defaulted to empty/placeholder values because:
    ///
    /// - FBP graph ports don't specify data types, schemas, or validation rules
    /// - Port requirements (required/optional) aren't defined in the graph format
    /// - Array port indicators and descriptions aren't available
    /// - Default values and allowed value constraints aren't specified
    ///
    /// This design reflects the FBP philosophy of keeping graph definitions simple and delegating
    /// detailed port specifications to the component implementations themselves. The graph serves
    /// as a wiring diagram, while components define their interface contracts.
    ///
    /// Future FBP specification updates might provide richer port metadata, but for now this
    /// conversion serves the practical need of exposing graph ports through the component API
    /// for tooling and introspection purposes.
    fn ports_as_componentportsarray(
        &self,
        inports_or_outports: &HashMap<String, GraphPort>,
    ) -> Vec<ComponentPort> {
        let mut out = Vec::with_capacity(inports_or_outports.len());
        for (name, _info) in inports_or_outports.iter() {
            out.push(ComponentPort {
                name: name.clone(),
                allowed_type: String::from(""), //TODO clarify spec: not available from FBP JSON Graph port TODO what happens if we return a empty allowed type (because we dont know from Graph inport)
                schema: None, //TODO clarify spec: not available from FBP JSON Graph port
                required: true, //TODO clarify spec: not available from FBP JSON Graph port
                is_arrayport: false, //TODO clarify spec: not available from FBP JSON Graph port
                description: String::from(""), //TODO clarify spec: not available from FBP JSON Graph port
                values_allowed: vec![], //TODO clarify spec: not available from FBP JSON Graph port
                value_default: String::from(""), //TODO clarify spec: not available from FBP JSON Graph port
                                                 //TODO clarify spec: cannot return Graph inport fields process, port, metadata (x,y)
            });
        }
        return out;
    }

    fn clear(
        &mut self,
        payload: &GraphClearRequestPayload,
        runtime: &Runtime,
    ) -> Result<(), std::io::Error> {
        if runtime.status.running {
            // not allowed at the moment (TODO), theoretically graph and network could be different and the graph could be modified while the network is still running in the old config, then stop network and immediately start the network again according to the new graph structure, having only short downtime.
            return Err(std::io::Error::new(
                std::io::ErrorKind::ResourceBusy,
                String::from("network still running"),
            ));
            //TODO ^ requires the feature "io_error_more", is this OK or risky, or bloated?
        }
        // update graph properties
        self.properties.name = payload.name.clone();
        self.properties.description = payload.description.clone().unwrap_or_default();
        self.properties.icon = payload.icon.clone().unwrap_or_default();
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
            }
            Err(_) => {
                //TODO we could pass on the std::collections::hash_map::OccupiedError
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    String::from("inport with that name already exists"),
                ));
            }
        }
    }

    fn add_outport(&mut self, name: String, portdef: GraphPort) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should adding an outport be allowed?
        match self.outports.try_insert(name, portdef) {
            Ok(_) => {
                return Ok(());
            }
            Err(_) => {
                //TODO we could pass on the std::collections::hash_map::OccupiedError
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    String::from("outport with that name already exists"),
                ));
            }
        }
    }

    fn remove_inport(&mut self, name: String) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should removing an inport be allowed?
        match self.inports.remove(&name) {
            Some(_) => {
                return Ok(());
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    String::from("inport not found"),
                ));
            }
        }
    }

    fn remove_outport(&mut self, name: String) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in which state should removing an outport be allowed?
        match self.outports.remove(&name) {
            Some(_) => {
                return Ok(());
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    String::from("outport not found"),
                ));
            }
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
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                String::from("inport already exists"),
            ));
        }
        match self.inports.remove(&old) {
            Some(v) => {
                self.inports
                    .try_insert(new, v)
                    .expect("wtf key occupied on insertion"); // should not happen
                return Ok(());
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    String::from("inport not found"),
                ));
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
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                String::from("outport already exists"),
            ));
        }
        match self.outports.remove(&old) {
            Some(v) => {
                self.outports
                    .try_insert(new, v)
                    .expect("wtf key occupied on insertion"); // should not happen
                return Ok(());
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    String::from("outport not found"),
                ));
            }
        }
    }

    fn add_node_from_payload(
        &mut self,
        graph: String,
        component: String,
        name: String,
        metadata: JsonMap<String, JsonValue>,
    ) -> Result<GraphAddnodeResponsePayload, std::io::Error> {
        let metadata_parsed = graph_node_metadata_from_payload(&metadata);
        // Compatibility: tests use collection-prefixed component names (for example "core/Repeat")
        // while internal runtime lookup uses bare names ("Repeat").
        let normalized_component = component
            .rsplit('/')
            .next()
            .unwrap_or(component.as_str())
            .to_owned();
        let mut response =
            self.add_node(graph.clone(), normalized_component, name, metadata_parsed)?;
        if let Some(node) = self.nodes.get_mut(&response.name) {
            // Compatibility: fbp-tests expect changenode responses to contain only user-supplied keys.
            node.protocol_metadata = metadata.clone();
        }
        response.component = component;
        response.metadata = metadata;
        response.graph = graph;
        Ok(response)
    }

    fn add_node(
        &mut self,
        graph: String,
        component: String,
        name: String,
        metadata: GraphNodeMetadata,
    ) -> Result<GraphAddnodeResponsePayload, std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }
        //TODO implement
        //TODO in what state is it allowed do change the nodeset?
        //TODO check graph name and state, multi-graph support

        // TODO check if that node already exists
        // TODO check if that component exists

        let protocol_metadata = graph_node_metadata_to_payload(&metadata);
        let nodedef = GraphNode {
            component,
            metadata,
            protocol_metadata: protocol_metadata.clone(),
        };
        //TODO optimize - constructing here because try_insert() moves the value
        let ret = GraphAddnodeResponsePayload {
            name: name.clone(),
            component: nodedef.component.clone(),
            metadata: protocol_metadata,
            graph: graph.clone(),
        };

        // insert and return
        match self.nodes.try_insert(name, nodedef) {
            Ok(_) => {
                return Ok(ret);
            }
            Err(_) => {
                //TODO we could pass on the std::collections::hash_map::OccupiedError
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    String::from("node with that name already exists"),
                ));
            }
        }
    }

    fn remove_node(
        &mut self,
        graph: String,
        name: String,
    ) -> Result<Vec<GraphRemoveedgeResponsePayload>, std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }
        //TODO implement
        //TODO in which state should removing a node be allowed?
        //TODO check graph name, multi-graph support
        match self.nodes.remove(&name) {
            Some(_) => {
                let mut removed_edges = Vec::new();
                self.edges.retain(|edge| {
                    if edge.source.process == name || edge.target.process == name {
                        removed_edges.push(GraphRemoveedgeResponsePayload {
                            graph: graph.clone(),
                            src: GraphNodeSpecNetwork {
                                node: edge.source.process.clone(),
                                port: edge.source.port.clone(),
                                index: edge.source.index.clone(),
                            },
                            tgt: GraphNodeSpecNetwork {
                                node: edge.target.process.clone(),
                                port: edge.target.port.clone(),
                                index: edge.target.index.clone(),
                            },
                        });
                        false
                    } else {
                        true
                    }
                });
                return Ok(removed_edges);
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    String::from("node not found"),
                ));
            }
        }
    }

    fn rename_node(
        &mut self,
        graph: String,
        old: String,
        new: String,
    ) -> Result<(), std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }
        //TODO in which state should manipulating nodes be allowed?
        //TODO check graph name and state, multi-graph support

        // check if new name already exists
        if self.nodes.contains_key(&new) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                String::from("node with that name already exists"),
            ));
        }
        // remove, then re-insert (there is no rename in HashMap)
        match self.nodes.remove(&old) {
            Some(v) => {
                self.nodes
                    .try_insert(new.clone(), v)
                    .expect("wtf key occupied on insertion"); // should not happen

                // rename in edges
                for edge in self.edges.iter_mut() {
                    if edge.source.process == old {
                        edge.source.process = new.clone();
                    }
                    if edge.target.process == old {
                        edge.target.process = new.clone();
                    }
                }

                return Ok(());
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    String::from("wtf node not found"),
                )); // should not happen either
            }
        }
    }

    fn change_node(
        &mut self,
        graph: String,
        name: String,
        metadata: JsonMap<String, JsonValue>,
    ) -> Result<JsonMap<String, JsonValue>, std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }
        //TODO implement
        //TODO in which state should manipulating nodes be allowed?
        //TODO check graph name and state, multi-graph support

        //TODO currently we discard additional fields! -> issue #188
        //TODO clarify spec: should the whole metadata hashmap be replaced (but then how to delete metadata entries?) or should only the given fields be overwritten?
        if let Some(node) = self.nodes.get_mut(&name) {
            //TODO optimize: borrowing a String here
            for (key, value) in metadata.iter() {
                if value.is_null() {
                    node.protocol_metadata.remove(key);
                } else {
                    node.protocol_metadata.insert(key.clone(), value.clone());
                }
            }
            if let Some(JsonValue::Number(x)) = node.protocol_metadata.get("x") {
                if let Some(x) = x.as_i64() {
                    node.metadata.x = x as i32;
                }
            }
            if let Some(JsonValue::Number(y)) = node.protocol_metadata.get("y") {
                if let Some(y) = y.as_i64() {
                    node.metadata.y = y as i32;
                }
            }
            return Ok(node.protocol_metadata.clone());
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("node by that name not found"),
            ));
        }
    }

    fn add_edge(&mut self, graph: String, edge: GraphEdge) -> Result<(), std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }
        //TODO implement
        //TODO in what state is it allowed do change the edgeset?
        //TODO check graph name and state, multi-graph support

        //TODO check if that edge already exists! There is a dedup(), helpful?
        //TODO check for OOM by extending first?
        self.edges.push(edge);
        //TODO optimize: if it cannot fail, then no need for returning Result
        Ok(())
    }

    fn remove_edge(
        &mut self,
        graph: String,
        source: GraphNodeSpecNetwork,
        target: GraphNodeSpecNetwork,
    ) -> Result<(), std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }

        // find correct index and remove
        for (i, edge) in self.edges.iter().enumerate() {
            if edge.source == source && edge.target == target {
                self.edges.remove(i);
                return Ok(());
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            String::from("edge with that src+tgt not found"),
        ));
    }

    fn change_edge(
        &mut self,
        graph: String,
        source: GraphNodeSpecNetwork,
        target: GraphNodeSpecNetwork,
        metadata: GraphEdgeMetadata,
    ) -> Result<(), std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }

        // find correct index and set metadata
        for (i, edge) in self.edges.iter().enumerate() {
            if edge.source == source && edge.target == target {
                self.edges[i].metadata = metadata;
                return Ok(());
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            String::from("edge with that src+tgt not found"),
        ));
    }

    // spec: According to the FBP JSON graph format spec, initial IPs are declared as a special-case of a graph edge in the "connections" part of the graph.
    fn add_initialip(
        &mut self,
        payload: GraphAddinitialRequestPayload,
    ) -> Result<(), std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed do change initial IPs (which are similar to edges)?
        //TODO check graph name and state, multi-graph support

        //TODO check for OOM by extending first?
        self.edges.push(GraphEdge::from(payload));
        //TODO optimize: if it cannot fail, then no need for returning Result
        Ok(())
    }

    fn remove_initialip(
        &mut self,
        payload: GraphRemoveinitialRequestPayload,
    ) -> Result<GraphIIPSpecNetwork, std::io::Error> {
        //TODO implement
        //TODO in what state is it allowed to change initial IPs (which are similar to edges)?
        //TODO check graph name and state, multi-graph support

        //TODO clarify spec: should we remove first or last match? currently removing first match
        //NOTE: if finding and removing last match first, therefore using self.edges.iter().rev().enumerate(), then rev() reverses the order, but enumerate's index of 0 is the last element!
        for i in 0..self.edges.len() {
            if let Some(iipdata) = self.edges[i].data.clone() {
                if payload.src.is_none()
                    || iipdata.as_bytes()
                        == payload
                            .src
                            .as_ref()
                            .expect("already checked")
                            .data
                            .as_bytes()
                {
                    if self.edges[i].target == payload.tgt {
                        self.edges.remove(i);
                        return Ok(GraphIIPSpecNetwork { data: iipdata });
                    }
                }
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            String::from("edge with that data+tgt not found"),
        ));
    }

    fn add_group(
        &mut self,
        graph: String,
        name: String,
        nodes: Vec<String>,
        metadata: GraphGroupMetadata,
    ) -> Result<(), std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }

        // TODO: check nodes if they actually exist; check for duplicates; and node can only be part of a single group
        self.groups.push(GraphGroup {
            name: name,
            nodes: nodes,
            metadata: metadata,
        });
        Ok(())
    }

    fn remove_group(&mut self, graph: String, name: String) -> Result<(), std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }

        // find correct index and remove
        for (i, group) in self.groups.iter().enumerate() {
            if group.name == name {
                self.groups.remove(i);
                return Ok(());
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            String::from("group with that name not found"),
        ));
    }

    fn rename_group(
        &mut self,
        graph: String,
        old: String,
        new: String,
    ) -> Result<(), std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }

        // find correct index and rename
        for (i, group) in self.groups.iter().enumerate() {
            if group.name == old {
                self.groups[i].name = new;
                return Ok(());
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            String::from("group with that name not found"),
        ));
    }

    fn change_group(
        &mut self,
        graph: String,
        name: String,
        metadata: GraphGroupMetadata,
    ) -> Result<(), std::io::Error> {
        // check graph name
        if graph != self.properties.name {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("wrong graph addressed"),
            ));
        }

        // find correct index and set metadata
        for (i, group) in self.groups.iter().enumerate() {
            if group.name == name {
                self.groups[i].metadata = metadata;
                return Ok(());
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            String::from("group with that name not found"),
        ));
    }

    fn get_source(&self, name: String) -> Result<ComponentSourcePayload, std::io::Error> {
        //TODO optimize: the message handler has already checked the graph name outside
        //###
        if name == self.properties.name {
            return Ok(ComponentSourcePayload {
                name: name,
                language: String::from("json"), //TODO clarify spec: what to set here?
                library: String::from(""),      //TODO clarify spec: what to set here?
                code: serde_json::to_string(self).expect("failed to serialize graph"),
                tests: String::from("// tests for graph here"), //TODO clarify spec: what to set here?
            });
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            String::from("graph with that name not found"),
        ));
    }
}

impl Default for GraphPropertiesEnvironment {
    fn default() -> Self {
        GraphPropertiesEnvironment {
            typ: String::from("flowd"), //TODO constant value - optimize
            content: String::from(""),  //TODO always empty for flowd - optimize
        }
    }
}

//TODO optimize, graph:addinport request payload is very similar to GraphPort -> possible re-use without conversion?
impl From<GraphAddinportRequestPayload> for GraphPort {
    fn from(payload: GraphAddinportRequestPayload) -> Self {
        let metadata = payload.metadata.unwrap_or_default();
        GraphPort {
            //TODO optimize structure very much the same -> use one for both?
            process: payload.node,
            port: payload.port,
            metadata: GraphPortMetadata {
                //TODO optimize: GraphPortMetadata and GraphNodeMetadata are structurally the same
                x: metadata.x,
                y: metadata.y,
            },
        }
    }
}

//TODO optimize, graph:addoutport is also very similar
impl From<GraphAddoutportRequestPayload> for GraphPort {
    fn from(payload: GraphAddoutportRequestPayload) -> Self {
        let metadata = payload.metadata.unwrap_or_default();
        GraphPort {
            //TODO optimize structure very much the same -> use one for both?
            process: payload.node,
            port: payload.port,
            metadata: GraphPortMetadata {
                //TODO optimize: GraphPortMetadata and GraphNodeMetadata are structurally the same
                x: metadata.x,
                y: metadata.y,
            },
        }
    }
}

//TODO optimize, make the useless -- actually only the graph field is too much -> filter that out by serde?
impl From<GraphAddedgeRequestPayload> for GraphEdge {
    fn from(payload: GraphAddedgeRequestPayload) -> Self {
        GraphEdge {
            source: GraphNodeSpec::from(payload.src),
            target: GraphNodeSpec::from(payload.tgt),
            data: None,
            metadata: payload.metadata,
        }
    }
}

// spec: IIPs are special cases of a graph connection/edge
impl<'a> From<GraphAddinitialRequestPayload> for GraphEdge {
    fn from(payload: GraphAddinitialRequestPayload) -> Self {
        GraphEdge {
            source: GraphNodeSpec {
                //TODO clarify spec: what to set as "src" if it is an IIP?
                process: String::from(""),
                port: String::from(""),
                index: Some(String::from("")), //TODO clarify spec: what to save here when noflo-ui does not send this field?
            },
            data: if payload.src.data.len() > 0 {
                Some(payload.src.data)
            } else {
                None
            }, //NOTE: there is an inconsistency between FBP network protocol and FBP graph schema
            target: GraphNodeSpec::from(payload.tgt),
            metadata: payload.metadata, //TODO defaults may be unsensible -> clarify spec
        }
    }
}

impl From<GraphNodeSpecNetwork> for GraphNodeSpec {
    fn from(nodespec_network: GraphNodeSpecNetwork) -> Self {
        GraphNodeSpec {
            process: nodespec_network.node,
            port: nodespec_network.port,
            index: nodespec_network.index,
        }
    }
}

// ----------
// processes
// ----------

struct BoundaryThread {
    signal: ProcessSignalSink, // signalling channel, uses mpsc channel which is lower performance but shareable and ok for signalling
    joinhandle: std::thread::JoinHandle<()>, // for waiting for thread exit TODO what is its generic parameter?
                                             //TODO detect process exit
}

type BoundaryThreadManager = HashMap<String, BoundaryThread>;

struct ThreadExitFlag {
    exited: Arc<AtomicBool>,
}

impl ThreadExitFlag {
    fn new(exited: Arc<AtomicBool>) -> Self {
        ThreadExitFlag { exited }
    }
}

impl Drop for ThreadExitFlag {
    fn drop(&mut self) {
        self.exited.store(true, Ordering::Release);
    }
}

impl std::fmt::Debug for BoundaryThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundaryThread")
            .field("signal", &self.signal)
            .field("joinhandle", &self.joinhandle)
            .finish()
    }
}

// ----------
// component library
// ----------

//TODO cannot think of a better name ATM, see https://stackoverflow.com/questions/1866794/naming-classes-how-to-avoid-calling-everything-a-whatevermanager
//TODO implement some functionality
#[derive(Default)]
pub struct ComponentLibrary {
    available: Vec<ComponentComponentPayload>,
}

impl ComponentLibrary {
    fn new(available: Vec<ComponentComponentPayload>) -> Self {
        ComponentLibrary {
            available: available,
        }
    }

    fn get_source(&self, name: String) -> Result<ComponentSourcePayload, std::io::Error> {
        // spec: Name of the component to for which to get source code.
        // spec: Should contain the library prefix, example: "my-project/SomeComponent"
        //TODO how to re-compile Rust components? how to meaningfully debug from the web? would need compiler output.
        //NOTE: components used as nodes in the current graph may be a different set than those that the component libary has available!
        //TODO optimize: access HashMap by &String or name.as_str()?
        //TODO optimize: for component:getsource we need to return an array, but for internal purpose a HashMap would be much more efficient
        //if let Some(node) = self.available.get(&name) {
        //TODO there is vec.binary_search() and vec.sort_by_key() - maybe as fast as hashmap?
        for component in self.available.iter() {
            if component.name == name {
                return Ok(ComponentSourcePayload {
                    //TODO implement
                    name: name,
                    language: String::from(""), //TODO implement - get real info
                    library: String::from(""),  //TODO implement - get real info
                    code: String::from("// code for component here"), //TODO implement - get real info
                    tests: String::from("// tests for component here"), //TODO where to get the tests from? in what format?
                });
            }
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            String::from("component not found"),
        ));
    }

    fn new_component(
        name: String,
        inports: ProcessInports,
        outports: ProcessOutports,
        signalsource: ProcessSignalSource,
        watchdog_signalsink: ProcessSignalSink,
        graph_inout: GraphInportOutportHandle,
    ) -> Result<(), std::io::Error> {
        if let Some(_component) = instantiate_component(
            name.as_str(),
            inports,
            outports,
            signalsource,
            watchdog_signalsink,
            graph_inout,
            None, // TODO: pass actual scheduler waker
        ) {
            // Component will be managed by scheduler, not run directly
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                String::from("component not found"),
            ))
        }
    }
}

pub mod bench_api {
    use super::*;
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant};

    pub struct BenchRuntimeHarness {
        runtime: Arc<RwLock<Runtime>>,
        pub graph: Arc<RwLock<Graph>>,
        components: Arc<RwLock<ComponentLibrary>>,
        graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
        packet_rx: Receiver<RuntimePacketResponsePayload>,
        collected_outputs: Arc<Mutex<HashMap<String, Vec<MessageBuf>>>>,
    }

    impl BenchRuntimeHarness {
        fn message_data_bytes(payload: &MessageBuf) -> Option<&[u8]> {
            match payload {
                FbpMessage::Bytes(bytes) => Some(bytes),
                FbpMessage::Text(text) => Some(text.as_bytes()),
                _ => None,
            }
        }

        fn drain_runtime_packets(&self) {
            while let Ok(pkt) = self.packet_rx.try_recv() {
                if matches!(pkt.event, RuntimePacketEvent::Data) {
                    if let Some(payload) = &pkt.payload {
                        let mut outputs = self.collected_outputs.lock().expect("lock poisoned");
                        outputs
                            .entry(pkt.port.clone())
                            .or_insert_with(Vec::new)
                            .push(FbpMessage::from_text(payload.clone()));
                    }
                }
            }
        }

        pub fn new(graph: Graph) -> Self {
            let runtime: Arc<RwLock<Runtime>> =
                Arc::new(RwLock::new(Runtime::new(graph.properties.name.clone())));
            let components: Arc<RwLock<ComponentLibrary>> = build_component_library();
            let (packet_tx, packet_rx) = mpsc::sync_channel(65_536);
            let graph_inout = Arc::new(Mutex::new(GraphInportOutportHolder {
                inports: None,
                outports: None,
                websockets: HashMap::new(),
                packet_tap: Some(packet_tx),
            }));
            let graph = Arc::new(RwLock::new(graph));

            BenchRuntimeHarness {
                runtime,
                graph,
                components,
                graph_inout,
                packet_rx,
                collected_outputs: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        pub fn start(&self) -> std::result::Result<(), std::io::Error> {
            run_graph(
                self.runtime.clone(),
                self.graph.clone(),
                self.components.clone(),
                self.graph_inout.clone(),
            )
        }

        pub fn stop(&self) -> std::result::Result<(), std::io::Error> {
            self.runtime
                .write()
                .expect("lock poisoned")
                .stop(self.graph_inout.clone(), false)?;
            Ok(())
        }

        pub fn send_data_to_inport(
            &self,
            inport: &str,
            payload: &[u8],
        ) -> std::result::Result<(), std::io::Error> {
            let graph_name = self.runtime.read().expect("lock poisoned").graph.clone();
            let payload_text = std::str::from_utf8(payload).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("runtime packet payload must be valid UTF-8: {err}"),
                )
            })?;
            let packet = RuntimePacketRequestPayload {
                port: inport.to_string(),
                event: RuntimePacketEvent::Data,
                typ: None,
                schema: None,
                graph: graph_name,
                payload: Some(payload_text.to_string()),
                secret: None,
            };
            Runtime::packet(&packet, self.graph_inout.clone(), self.runtime.clone())?;
            Ok(())
        }

        pub fn wait_for_outport_data(
            &self,
            outport: &str,
            expected_count: usize,
            timeout: Duration,
        ) -> std::result::Result<(), std::io::Error> {
            let started = Instant::now();
            let mut count = 0usize;
            while count < expected_count {
                let elapsed = started.elapsed();
                if elapsed >= timeout {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("timed out waiting for outport data: {count}/{expected_count}"),
                    ));
                }
                let remaining = timeout - elapsed;
                let pkt = self.packet_rx.recv_timeout(remaining).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timed out waiting for runtime packet",
                    )
                })?;
                if pkt.port == outport && matches!(pkt.event, RuntimePacketEvent::Data) {
                    // Collect output for testing
                    if let Some(payload) = &pkt.payload {
                        let mut outputs = self.collected_outputs.lock().expect("lock poisoned");
                        outputs
                            .entry(outport.to_string())
                            .or_insert_with(Vec::new)
                            .push(FbpMessage::from_text(payload.clone()));
                    }
                    count += 1;
                }
            }
            Ok(())
        }

        /// Collect all outputs from a specific outport
        pub fn collect_outputs(&self, outport: &str) -> Vec<MessageBuf> {
            self.drain_runtime_packets();
            let outputs = self.collected_outputs.lock().expect("lock poisoned");
            outputs.get(outport).cloned().unwrap_or_default()
        }

        /// Assert that outputs match expected values (set-based comparison)
        pub fn assert_outputs_set_equal(&self, outport: &str, expected: &[&[u8]]) {
            let outputs = self.collect_outputs(outport);
            let actual_set: std::collections::HashSet<Vec<u8>> = outputs
                .iter()
                .map(|payload| {
                    Self::message_data_bytes(payload)
                        .expect("expected byte/text payload while comparing output set")
                        .to_vec()
                })
                .collect();
            let expected_set: std::collections::HashSet<Vec<u8>> =
                expected.iter().map(|payload| payload.to_vec()).collect();

            assert_eq!(
                actual_set, expected_set,
                "Output sets do not match for port {}",
                outport
            );
        }

        /// Assert that outputs match expected values in sequence
        pub fn assert_outputs_sequence_equal(&self, outport: &str, expected: &[&[u8]]) {
            let outputs = self.collect_outputs(outport);
            assert_eq!(
                outputs.len(),
                expected.len(),
                "Output count mismatch for port {}",
                outport
            );

            for (i, (actual, &expected)) in outputs.iter().zip(expected.iter()).enumerate() {
                let actual_bytes = Self::message_data_bytes(actual)
                    .expect("expected byte/text payload while comparing output sequence");
                assert_eq!(
                    actual_bytes, expected,
                    "Output {} does not match for port {}",
                    i, outport
                );
            }
        }

        /// Assert no message loss (all inputs should produce outputs)
        pub fn assert_no_message_loss(&self, input_count: usize, outport: &str) {
            let outputs = self.collect_outputs(outport);
            assert_eq!(
                outputs.len(),
                input_count,
                "Message loss detected: expected {} outputs, got {} for port {}",
                input_count,
                outputs.len(),
                outport
            );
        }

        /// Clear collected outputs
        pub fn clear_outputs(&self) {
            let mut outputs = self.collected_outputs.lock().expect("lock poisoned");
            outputs.clear();
        }
    }

    fn load_graph_from_persistence_path(
        path: &std::path::Path,
    ) -> std::result::Result<Graph, std::io::Error> {
        let json_data = std::fs::read_to_string(path)?;
        let graph: Graph = serde_json::from_str(&json_data).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("failed to parse graph from persistence file: {e}"),
            )
        })?;
        Ok(graph)
    }

    fn graph_to_persistence_json(graph: &Graph) -> std::result::Result<String, std::io::Error> {
        Ok(graph.get_source(graph.properties.name.clone())?.code)
    }

    fn default_node_metadata(x: i32, y: i32, label: &str) -> GraphNodeMetadata {
        GraphNodeMetadata {
            x,
            y,
            height: Some(NODE_HEIGHT_DEFAULT),
            width: Some(NODE_WIDTH_DEFAULT),
            label: Some(label.to_string()),
            icon: None,
        }
    }

    fn build_linear_graph(name: &str) -> Graph {
        let mut graph = Graph::new(
            name.to_string(),
            "benchmark".to_string(),
            "random".to_string(),
        );

        graph.inports.insert(
            "IN".to_string(),
            GraphPort {
                process: "RepeatA".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 0 },
            },
        );
        graph.outports.insert(
            "OUT".to_string(),
            GraphPort {
                process: "RepeatB".to_string(),
                port: "OUT".to_string(),
                metadata: GraphPortMetadata { x: 300, y: 0 },
            },
        );

        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "RepeatA".to_string(),
                default_node_metadata(80, 80, "RepeatA"),
            )
            .expect("failed to add RepeatA node");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "RepeatB".to_string(),
                default_node_metadata(180, 80, "RepeatB"),
            )
            .expect("failed to add RepeatB node");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "RepeatA".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "RepeatB".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add edge RepeatA.OUT -> RepeatB.IN");

        graph
    }

    fn build_fan_out_graph(name: &str) -> Graph {
        let mut graph = Graph::new(
            name.to_string(),
            "benchmark".to_string(),
            "random".to_string(),
        );

        graph.inports.insert(
            "IN".to_string(),
            GraphPort {
                process: "Demux".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 0 },
            },
        );
        graph.outports.insert(
            "OUT".to_string(),
            GraphPort {
                process: "Merge".to_string(),
                port: "OUT".to_string(),
                metadata: GraphPortMetadata { x: 600, y: 0 },
            },
        );

        graph
            .add_node(
                name.to_string(),
                "Demux3".to_string(),
                "Demux".to_string(),
                default_node_metadata(100, 80, "Demux"),
            )
            .expect("failed to add Demux");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "A".to_string(),
                default_node_metadata(240, 40, "A"),
            )
            .expect("failed to add A");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "B".to_string(),
                default_node_metadata(240, 80, "B"),
            )
            .expect("failed to add B");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "C".to_string(),
                default_node_metadata(240, 120, "C"),
            )
            .expect("failed to add C");
        graph
            .add_node(
                name.to_string(),
                "Muxer".to_string(),
                "Merge".to_string(),
                default_node_metadata(420, 80, "Merge"),
            )
            .expect("failed to add Merge");

        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Demux".to_string(),
                        port: "A".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "A".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Demux.A -> A.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Demux".to_string(),
                        port: "B".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "B".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Demux.B -> B.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Demux".to_string(),
                        port: "C".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "C".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Demux.C -> C.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "A".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Merge".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add A.OUT -> Merge.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "B".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Merge".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add B.OUT -> Merge.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "C".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Merge".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add C.OUT -> Merge.IN");

        graph
    }

    fn build_fan_in_graph(name: &str) -> Graph {
        let mut graph = Graph::new(
            name.to_string(),
            "benchmark".to_string(),
            "random".to_string(),
        );

        graph.inports.insert(
            "IN_A".to_string(),
            GraphPort {
                process: "Merge".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 0 },
            },
        );
        graph.inports.insert(
            "IN_B".to_string(),
            GraphPort {
                process: "Merge".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 40 },
            },
        );
        graph.outports.insert(
            "OUT".to_string(),
            GraphPort {
                process: "Node".to_string(),
                port: "OUT".to_string(),
                metadata: GraphPortMetadata { x: 400, y: 0 },
            },
        );

        graph
            .add_node(
                name.to_string(),
                "Muxer".to_string(),
                "Merge".to_string(),
                default_node_metadata(120, 80, "Merge"),
            )
            .expect("failed to add Merge");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "Node".to_string(),
                default_node_metadata(260, 80, "Node"),
            )
            .expect("failed to add Node");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Merge".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Node".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Merge.OUT -> Node.IN");

        graph
    }

    fn build_io_sim_graph(name: &str) -> Graph {
        let mut graph = Graph::new(
            name.to_string(),
            "benchmark".to_string(),
            "random".to_string(),
        );

        graph.inports.insert(
            "IN".to_string(),
            GraphPort {
                process: "Delay".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 0 },
            },
        );
        graph.outports.insert(
            "OUT".to_string(),
            GraphPort {
                process: "Node".to_string(),
                port: "OUT".to_string(),
                metadata: GraphPortMetadata { x: 400, y: 0 },
            },
        );

        graph
            .add_node(
                name.to_string(),
                "Delay".to_string(),
                "Delay".to_string(),
                default_node_metadata(100, 80, "Delay"),
            )
            .expect("failed to add Delay");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "Node".to_string(),
                default_node_metadata(260, 80, "Node"),
            )
            .expect("failed to add Node");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Delay".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Node".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Delay.OUT -> Node.IN");
        graph
            .add_initialip(GraphAddinitialRequestPayload {
                graph: name.to_string(),
                metadata: GraphEdgeMetadata::new(None, None, None),
                src: GraphIIPSpecNetwork {
                    data: "?delay=50us".to_string(),
                },
                tgt: GraphNodeSpecNetwork {
                    node: "Delay".to_string(),
                    port: "CONF".to_string(),
                    index: None,
                },
                secret: None,
            })
            .expect("failed to add IIP to Delay.CONF");

        graph
    }

    pub fn linear_harness_direct(name: &str) -> BenchRuntimeHarness {
        BenchRuntimeHarness::new(build_linear_graph(name))
    }

    pub fn linear_harness_from_persistence(
        name: &str,
    ) -> std::result::Result<BenchRuntimeHarness, std::io::Error> {
        let graph = build_linear_graph(name);
        let graph_json = graph_to_persistence_json(&graph)?;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "flowd-bench-graph-load-{}-{}.json",
            std::process::id(),
            name
        ));
        std::fs::write(&path, graph_json)?;
        let loaded = load_graph_from_persistence_path(&path)?;
        let _ = std::fs::remove_file(&path);
        Ok(BenchRuntimeHarness::new(loaded))
    }

    pub fn fan_out_harness_direct(name: &str) -> BenchRuntimeHarness {
        BenchRuntimeHarness::new(build_fan_out_graph(name))
    }

    pub fn fan_out_harness_from_persistence(
        name: &str,
    ) -> std::result::Result<BenchRuntimeHarness, std::io::Error> {
        let graph = build_fan_out_graph(name);
        let graph_json = graph_to_persistence_json(&graph)?;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "flowd-bench-graph-load-{}-{}.json",
            std::process::id(),
            name
        ));
        std::fs::write(&path, graph_json)?;
        let loaded = load_graph_from_persistence_path(&path)?;
        let _ = std::fs::remove_file(&path);
        Ok(BenchRuntimeHarness::new(loaded))
    }

    pub fn fan_in_harness_direct(name: &str) -> BenchRuntimeHarness {
        BenchRuntimeHarness::new(build_fan_in_graph(name))
    }

    pub fn fan_in_harness_from_persistence(
        name: &str,
    ) -> std::result::Result<BenchRuntimeHarness, std::io::Error> {
        let graph = build_fan_in_graph(name);
        let graph_json = graph_to_persistence_json(&graph)?;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "flowd-bench-graph-load-{}-{}.json",
            std::process::id(),
            name
        ));
        std::fs::write(&path, graph_json)?;
        let loaded = load_graph_from_persistence_path(&path)?;
        let _ = std::fs::remove_file(&path);
        Ok(BenchRuntimeHarness::new(loaded))
    }

    pub fn io_sim_harness_direct(name: &str) -> BenchRuntimeHarness {
        BenchRuntimeHarness::new(build_io_sim_graph(name))
    }

    pub fn io_sim_harness_from_persistence(
        name: &str,
    ) -> std::result::Result<BenchRuntimeHarness, std::io::Error> {
        let graph = build_io_sim_graph(name);
        let graph_json = graph_to_persistence_json(&graph)?;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "flowd-bench-graph-load-{}-{}.json",
            std::process::id(),
            name
        ));
        std::fs::write(&path, graph_json)?;
        let loaded = load_graph_from_persistence_path(&path)?;
        let _ = std::fs::remove_file(&path);
        Ok(BenchRuntimeHarness::new(loaded))
    }
}
