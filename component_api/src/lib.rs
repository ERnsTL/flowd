use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use std::thread::Thread;
use std::time::{Duration, Instant};

use multimap::MultiMap;
use serde::Serialize;

// ports
pub type ProcessInports = MultiMap<String, ProcessEdgeSource>;
pub type ProcessOutports = MultiMap<String, ProcessEdgeSink>;

// edges
pub type ProcessEdge = rtrb::RingBuffer<MessageBuf>;
pub type ProcessEdgeSource = rtrb::Consumer<MessageBuf>;
pub type ProcessEdgeSinkConnection = rtrb::Producer<MessageBuf>;
pub use rtrb::PushError; // re-eport for abstraction

pub struct ProcessEdgeSink {
    sink: ProcessEdgeSinkConnection,
    wakeup: Option<WakeupNotify>,
    proc_name: Option<String>,
    signal_ready: Option<SchedulerWaker>,
}

impl ProcessEdgeSink {
    pub fn new(
        sink: ProcessEdgeSinkConnection,
        wakeup: Option<WakeupNotify>,
        proc_name: Option<String>,
        signal_ready: Option<SchedulerWaker>,
    ) -> Self {
        Self {
            sink,
            wakeup,
            proc_name,
            signal_ready,
        }
    }

    /// Push data into the edge and signal readiness if configured
    pub fn push(&mut self, data: MessageBuf) -> Result<(), PushError<MessageBuf>> {
        match self.sink.push(data) {
            Ok(()) => {
                // Signal scheduler that downstream component may be ready
                if let Some(signal) = &self.signal_ready {
                    signal();
                }
                // Wake non-scheduler boundary handlers (for example graph outport bridge).
                if let Some(wakeup) = &self.wakeup {
                    wakeup.unpark();
                }
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn proc_name(&self) -> Option<&str> {
        self.proc_name.as_deref()
    }
}

impl Debug for ProcessEdgeSink {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessEdgeSink")
            .field("sink", &self.sink)
            .field("wakeup", &self.wakeup)
            .field("proc_name", &self.proc_name)
            .finish()
    }
}

pub const PROCESSEDGE_BUFSIZE: usize = 7 * 7 * 7 * 7;
pub const PROCESSEDGE_SIGNAL_BUFSIZE: usize = 2;
pub const PROCESSEDGE_IIP_BUFSIZE: usize = 1;

// port wakeup
pub type WakeupNotify = Thread;

// signals
pub type ProcessSignalSource = std::sync::mpsc::Receiver<MessageBuf>; // only one allowed (single consumer)
pub type ProcessSignalSink = std::sync::mpsc::SyncSender<MessageBuf>; // Sender can be cloned (multiple producers) but SyncSender is even more convenient as it implements Sync and no deep clone() on the Sender is neccessary

// information packets (IPs) = messages
/*
NOTE: Vec<u8> is growable; Box<[u8]> is decidedly not growable, which just brings limitations for forwarding IPs.
Vec is just 1 machine word larger than Box. There is more convenience API and From implementations for Vec.
In the wild, there also seems less use of Box<[u8]>.

NOTE: Changing to [u8] here and then having &[u8] in ProcessEdges creates an avalanche of lifetime problems where something does not live long enough,
maybe there are some possibilities to state "this lifetime is equal to that one" but problem is that we are actually handing over
data from thread A to thread B, so we are not allowing thread B to have a temporary look at the data, but it is actual message passing.
And there is no "master" who owns the data - then we could give threads A and B pointers and borrows into that data, but that is not the case.
*/
pub type MessageBuf = Vec<u8>;

// Process result for scheduler-based execution
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessResult {
    DidWork(u32), // processed N work units
    NoWork,       // no work available
    Finished,     // component completed (for finite sources)
}

// Budget classes for execution control
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BudgetClass {
    Normal = 32,   // 16-64 range, start at 32
    Heavy = 8,     // 4-16 range, start at 8
    Realtime = 96, // 64-128 range, start at 96
}

// Node context for scheduler
#[derive(Clone)]
pub struct NodeContext {
    pub node_id: String,
    pub budget_class: BudgetClass,
    pub remaining_budget: u32,
    pub last_execution: Instant,
    pub execution_count: u64,
    pub work_units_processed: u64,
    ready_signal: std::sync::Arc<std::sync::atomic::AtomicBool>, // level-triggered readiness
    wake_at: Option<Instant>,
    timer_fired_at: Option<Instant>,
}

impl Debug for NodeContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeContext")
            .field("node_id", &self.node_id)
            .field("budget_class", &self.budget_class)
            .field("remaining_budget", &self.remaining_budget)
            .field("last_execution", &self.last_execution)
            .field("execution_count", &self.execution_count)
            .field("work_units_processed", &self.work_units_processed)
            .finish()
    }
}

impl NodeContext {
    pub fn new(
        node_id: String,
        budget_class: BudgetClass,
        ready_signal: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            node_id,
            budget_class,
            remaining_budget: 0,
            last_execution: Instant::now(),
            execution_count: 0,
            work_units_processed: 0,
            ready_signal,
            wake_at: None,
            timer_fired_at: None,
        }
    }

    pub fn signal_ready(&self) {
        self.ready_signal
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn clear_ready(&self) {
        self.ready_signal
            .store(false, std::sync::atomic::Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.ready_signal.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn wake_at(&mut self, at: Instant) {
        self.wake_at = Some(at);
    }

    pub fn take_wake_at(&mut self) -> Option<Instant> {
        self.wake_at.take()
    }

    pub fn mark_timer_fired(&mut self, at: Instant) {
        self.timer_fired_at = Some(at);
    }

    pub fn take_timer_fired(&mut self) -> Option<Instant> {
        self.timer_fired_at.take()
    }
}

// component
pub trait Component: Send {
    fn new(
        inports: ProcessInports,
        outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<SchedulerWaker>,
    ) -> Self
    where
        Self: Sized;

    // Scheduler-based execution: process one work unit and return result
    fn process(&mut self, _context: &mut NodeContext) -> ProcessResult {
        // Default implementation for backward compatibility
        // Components should override this with proper scheduler-aware logic
        ProcessResult::NoWork
    }

    // Legacy method for backward compatibility during transition
    fn run(self)
    where
        Self: Sized,
    {
        //NOTE: consume self because this method is not expected to return, and we can hand over data from self to sub-threads (lifetime of &self issue)
        // Components will be migrated to implement process() instead
        // For now, this is a placeholder that will be removed
        unimplemented!("Components should implement process() for scheduler-based execution");
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized;

    /*// to support reconnecting of inports and outports
    fn reconfigure_connection(
        &mut self,
        source_port: &str,
        target_component: &mut dyn Component,   //TODO optimize - is dyn fast?
        target_port: &str,
    ) -> std::result::Result<(), std::io::Error> {
        // remove the current connection from the source process
        self.get_outports_mut().retain(|port, _| port != source_port);

        // remove the current connection from the target process
        target_component.get_inports_mut().retain(|port, _| port != target_port);

        // add the new connection
        self.get_outports_mut().insert(source_port.to_string(), target_port.to_string());
        target_component.get_inports_mut().insert(target_port.to_string(), source_port.to_string());

        Ok(())
    }
    fn get_inports_mut(&mut self) -> &mut ProcessInports;
    fn get_outports_mut(&mut self) -> &mut ProcessOutports;*/
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ComponentComponentPayload {
    pub name: String, // spec: component name in format that can be used in graphs. Should contain the component library prefix.
    pub description: String,
    pub icon: String, // spec: visual icon for the component, matching icon names in Font Awesome
    pub subgraph: bool, // spec: is the component a subgraph?
    pub in_ports: Vec<ComponentPort>, // spec: array. TODO could be modelled as a hashmap/object
    pub out_ports: Vec<ComponentPort>, // spec: array. TODO clould be modelled as a hashmap/object ... OTOH, tere are usually not so many ports, can just as well iterate over 1/2/3/4 ports.
    #[serde(skip)]
    pub support_health: bool,
    #[serde(skip)]
    pub support_perfdata: bool,
    #[serde(skip)]
    pub support_reconnect: bool, //TODO should this belong to the ports (in_ports, out_ports fields)?
}

impl Default for ComponentComponentPayload {
    fn default() -> Self {
        ComponentComponentPayload {
            name: String::from("main/Repeat"), //TODO Repeat, Drop, Output required for tests
            description: String::from("description of the Repeat component"),
            icon: String::from("fa-usd"), //TODO with fa- prefix? FontAwesome should not so much be our concern in a new()-like method
            subgraph: false,
            in_ports: vec![],
            out_ports: vec![],
            support_health: false,
            support_perfdata: false,
            support_reconnect: false,
        }
    }
}

#[serde_with::skip_serializing_none] // fbp-protocol thus noflo-ui does not like "" or null values for schema
#[derive(Serialize, Debug)]
pub struct ComponentPort {
    #[serde(rename = "id")]
    pub name: String,
    #[serde(rename = "type")]
    pub allowed_type: String, //TODO clarify spec: so if we define a boolean, we can send only booleans? What about struct/object types? How should the runtime verify that? //TODO map JSON types <-> Rust types
    #[serde(default)]
    pub schema: Option<String>, // spec: optional
    #[serde(default)]
    pub required: bool, // spec: optional, whether the port needs to be connected for the component to work (TODO add checks for that and notify user (how?) that a vital port is unconnected if required=true)
    #[serde(default, rename = "addressable")]
    pub is_arrayport: bool, // spec: optional
    #[serde(default)]
    pub description: String, // spec: optional
    #[serde(default, rename = "values")]
    pub values_allowed: Vec<String>, // spec: optional, can probably be any type, but TODO how to map JSON "any values" to Rust?
    #[serde(default, rename = "default")]
    pub value_default: String, // spec: optional, datatype any TODO how to map JSON any values in Rust?
}

impl Default for ComponentPort {
    fn default() -> Self {
        ComponentPort {
            name: String::from("out"),
            allowed_type: String::from("string"),
            schema: None,
            required: true,
            is_arrayport: false,
            description: String::from("a default output port"),
            values_allowed: vec![], //TODO clarify spec: does empty array mean "no values allowed" or "all values allowed"?
            value_default: String::from(""),
        }
    }
}

impl ComponentPort {
    pub fn default_in() -> Self {
        ComponentPort {
            name: String::from("in"),
            allowed_type: String::from("string"),
            schema: None,
            required: true,
            is_arrayport: false,
            description: String::from("a default input port"),
            values_allowed: vec![], //TODO clarify spec: does empty array mean "no values allowed" or "all values allowed"?
            value_default: String::from(""),
        }
    }

    pub fn default_out() -> Self {
        ComponentPort::default()
    }
}

// graph
//NOTE: this is just an alias for convenience to components so that they dont have to write out the full Arc<Mutex<InportOutportHolder>>
pub type GraphInportOutportHandle = (
    Arc<dyn Fn(String) + Send + Sync>,
    Arc<dyn Fn(String) + Send + Sync>,
);

/// Convenience function for sending network output messages.
/// This is a free function that components can call directly on their GraphInportOutportHandle.
pub fn send_network_output_comfortable(graph_inout: &GraphInportOutportHandle, message: String) {
    (graph_inout.0)(message);
}

/// Convenience function for sending network preview URL messages.
/// This is a free function that components can call directly on their GraphInportOutportHandle.
pub fn send_network_previewurl_comfortable(graph_inout: &GraphInportOutportHandle, url: String) {
    (graph_inout.1)(url);
}

// Scheduler waker type for components to signal readiness from background threads
pub type SchedulerWaker = Arc<dyn Fn() + Send + Sync>;

/// Default bounded periodic polling interval for components that must poll
/// external systems because no callback-based wakeup is available.
pub const DEFAULT_IO_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Wake scheduler from background/async contexts.
#[inline]
pub fn wake_scheduler(waker: &Option<SchedulerWaker>) {
    if let Some(waker) = waker {
        waker();
    }
}
