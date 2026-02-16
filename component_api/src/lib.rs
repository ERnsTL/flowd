use std::sync::{Arc, Mutex};
use std::thread::Thread;

use multimap::MultiMap;
use serde::Serialize;

// ports
pub type ProcessInports = MultiMap<String, ProcessEdgeSource>;
pub type ProcessOutports = MultiMap<String, ProcessEdgeSink>;

// edges
pub type ProcessEdge = rtrb::RingBuffer<MessageBuf>;
pub type ProcessEdgeSource = rtrb::Consumer<MessageBuf>;
pub type ProcessEdgeSinkConnection = rtrb::Producer<MessageBuf>;
pub use rtrb::PushError;    // re-eport for abstraction

#[derive(Debug)]
pub struct ProcessEdgeSink {
    pub sink: ProcessEdgeSinkConnection,
    pub wakeup: Option<WakeupNotify>,
    pub proc_name: Option<String>,
}

pub const PROCESSEDGE_BUFSIZE: usize = 7 * 7 * 7 * 7;
pub const PROCESSEDGE_SIGNAL_BUFSIZE: usize = 2;
pub const PROCESSEDGE_IIP_BUFSIZE: usize = 1;

// port wakeup
pub type WakeupNotify = Thread;

// signals
pub type ProcessSignalSource = std::sync::mpsc::Receiver<MessageBuf>;   // only one allowed (single consumer)
pub type ProcessSignalSink = std::sync::mpsc::SyncSender<MessageBuf>;   // Sender can be cloned (multiple producers) but SyncSender is even more convenient as it implements Sync and no deep clone() on the Sender is neccessary

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

// component
pub trait Component {
    fn new(inports: ProcessInports, outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, graph_inout: GraphInportOutportHandle) -> Self where Self: Sized;
    fn run(self);   //NOTE: consume self because this method is not expected to return, and we can hand over data from self to sub-threads (lifetime of &self issue)
    fn get_metadata() -> ComponentComponentPayload where Self: Sized;

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
pub trait GraphInportOutport: Send {}

pub type GraphInportOutportHandle = Arc<Mutex<dyn GraphInportOutport>>; //TODO optimize - get rid of dyn (just have a struct with the send_runtime_packet method and implement that in the main runtime code, then we can get rid of the Arc<Mutex<>> wrapper and just pass a reference to that struct to the components. But for now, this is easier to implement and does not cause significant overhead, so let's keep it like this for now.)
