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
