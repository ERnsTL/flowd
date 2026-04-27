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

