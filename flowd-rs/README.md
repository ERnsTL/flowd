Re-implementation of the flowd runtime in Rust.

(More on flowd [in its Github repository](https://github.com/ERnsTL/flowd).)

1. First implement the FBP network protocol.
2. Implement an executor layer as link between FBP network protocol and the processing network. An executor can manage different kinds of components -- use different ways of passing messages between running FBP processes etc. possibly written in a different language itself.
3. Create a first executor layer, maybe a dummy in-memory one in Rust. Add dummy components. Pass the complete FBP test suite and verify FBP network protocol implementation conformance.
4. Add a first real executor with real components, most likely based on GStreamer, which already takes care of loading and unloading of components as well as managing data flow and pipelines. Advantages: Language interoperability, high performance and access to many existing audio, video and networking components.
5. Finally, add support for persistence -- .fbp file format from DrawFBP as well as the JSON graph format from noflo.
6. Transition all Go components.
7. Fade out the Go version (or use it as an "executor" resp. execution layer).
8. More features and real-life applications? Add registration with Flowhub ([source](https://github.com/flowbased/protocol-examples/blob/master/python/flowhub_register.py))? CBOR in addition to JSON? Other FBP UIs for online FBP programming?

Run it with:

```
RUST_LOG=debug cargo run
```

Peculiarities:

* noflo-ui error "connection failed" = runtime down or real network problem
* noflo-ui error "connection timed out" is more than network-level connection timeout; testsuite and noflo-ui does WebSocket upgrade, runtime:getruntime, component:list, network:status, component:getsource ...TODO anything more?
* Firefox seems to automatically use wss:// even if requesting ws:// in connection URL
* If the runtime:runtime response message states that there is a currently running graph, then noflo-ui uses the component:getsource request message to get the source code of the graph. Language can be json (noflo schema) or fbp (FBP network language from J. Paul Morrison). We return a placeholder for the source code and allow the capability component:getsource, otherwise noflo-ui complains that it is not permitted to send component:getsource request messages, see [this issue](https://github.com/noflo/noflo-ui/issues/1019). Using component:getsource to request the graph source seems to make many graph:* network:* and component:list* requests useless.
* The FBP network spec calls them messages, but sometimes the same message name is used for both the request and response direction, just with different fields. (TODO spec: either have distinct names for the requests and responses or call them requests and responses). For a strongly-typed programming language this is difficult to model and also for clarity, in flowd-rs, the messages are called requests and responses, also reflected in their struct names. Also these messages are sometimes send as an information or state update to the client, so the communication pattern is also not consistently request-response type nor event-stream type, meaning "who drives the state update"? (TODO ponder this)
* component:list is not about the running processes nor the list of components used in the current graph, but the list of available components for placement into the graph. Like query the component repository.
* The network:status, network:started, network:stopped responses are pretty similar. The network:status message can be used as an internal status message and constructors for the rest can read part information from the live network status struct.

TODO Further FBP network protocol clarifications needed:

* TODO noflo-ui complains on connect/disconnect button push that the payload of component:componentsready is non-integer, but spec defines no payload thus interpreted to be an empty object. On first connect to the runtime, it does not complain. TODO integer is number of component:component messages before the component:componentsready message?
* for graph:changenode messages (and similarly applicable to the other response messages) the same fields as the request should be present. TODO to be useful, it should send back the same values as confirmation that these values are now set -- but if sending back a different x coordinate for example, noflo-ui does not snap back the node but acts if it was set correctly. -> TODO would probably need graph:error message.
* TODO why does noflo-ui send tens of messages, and if the drag is a long one, a hundred messages or more? (for every mouse-drag+move event -- but all with the same end coordinates
* TODO spec does not define graph:error response message
* TODO spec does not define expected response to network:debug request
* TODO spec what should the response for runtime:packet request be? -- when should a runtime:packetsent be sent? if we receive a runtime:packet response/status message should we also respond with a runtime:packetreceived... or something?
* TODO spec mentions "a few commands do not require any capabilities: [...] and the error responses (runtime:error, graph:error, network:error, component:error)." but does not mention trace:error -> does it also not require any capabilities or does it?
* TODO spec description of trace:dump message is "undefined"
* TODO spec description of trace:dump message attribute "flowtrace" has backticks in description -- wanted?
* TODO spec trace:dump attribute "type": which types are possible?
* TODO spec ordering of trace:dump and trace:clear differs from message list for protocol:trace in capabilities list at top of spec compared to ordering in bottom of spec where listing the actual doc for each message
* TODO spec trace:dump has no field "secret" but is listed as input message resp. request for capability protocol:trace so it should have the attribute "secret" for request usage
* TODO doc for trace protocol gives a link to the Flowtrace protocol but the link is written out in literal Markdown, not converted to an HTML link
* TODO spec network:output and network:error have "'" single quotes at the end of their descriptions.
* TODO spec for network:output: how to transmit binary data?
* TODO spec for network:connect, network:begingroup, network:data, network:endgroup, network:disconnect ... how do they fit together, how are they used together, which ones are mandatory, which are optional?
* TODO spec what is the point resp. use-case of network:edges "the user has selected some edges in the UI"?
* TODO spec network:debug puts the network into debug mode. But how should it behave differently -- only send additional network:processerror messages, anything else? How to get out ot debug mode? What should the response message to network:debug request be?
* TODO spec what is the point resp. use-]aso of network:packet field "event"?
* TODO spec runtime:packet what about port index? or is this not supported on network inports/outports?
* TODO spec should incoming runtime:packet not be confirmed with some kind of packetreceived response like outgoing runtime:packet -> runtime:packetsent?
* TODO spec runtime:packetsent is this meant when local runtime sends runtime:packet to remote runtime that remote confirms using runtime:packetsent or the other way around? why is runtime:packetsent missing the attribute "secret"?
* TODO spec the causal request-response connection between network:start and started and network:stop and stopped should be stated explicitly. Was only indirectly mentioned in a comment in the spec changelog.
* TODO spec is it possible for network:started to have started=false? If it was false it would be an error and should logically return network:error? What is the point use-case for the attribute "started"?
* TODO spec defines several optional arguments on connection:connection.inports and .outports, but in the FBP graph schema, these are not present! Makes it difficult to do serialization based on schema. Should be in sync with the FBP graph schema.
  * a graph's port has different attributes than a component's port.
  * The FBP network protocol has the ports of processes attached to the process (eg. component:component response as an array, side-question why not as a hashmap? TODO) whereas the FBP graph schema has the ports of a process defined in the graph.connections (also as an array).
* TODO component:component.inports and .outports have a field "type" (allowed type). How is this to be interpreted? So if we define it boolean, we can send only booleans? What about struct/object types? How should the runtime verify that?
* TODO component:component.inports and .outports have a field "values" = array of allowed values - how to express "all allowed"? With an empty array, is that correct? Or does empty array mean "no values allowed"?
* TODO runtime:ports response requires the inports and outports to be an array, but in the FBP JSON graph schema, the inports and outports of the graph are an object/hashmap. Creates needless conversion from internal graph representation to external message in the case of requesting the inports of the graph. In case of requesting runtime:ports for a component, the FBP network protocol response and graph.component.ports structure matches also in both cases it is an array.
* TODO runtime:ports can be used for requesting the inports and outports of both a component and a graph. This can lead to name clashes!
* TODO documentation of the component:component message does not say anything about finishing the list with a componentsready message. Should be added. It says so only in the documentation for the componentsready message.
* TODO component:component message: Is it correct that the returned list of components should contain a list of generally available components from a component library, listing all components which are ready to be placed somewhere in the graph? So not just the ones present in the loaded graph? The spec does not clearly say which component should be returned, resp. their criteria.
* TODO component:component message and component:componentsready message: Should the returned components be one per Websocket message or could the list also be returned in one WebSocket message as multiple FBP network protocol messages? Either way, it should be documented.
* TODO general: there are features and changes specified, but no no version of the FBP network protocol released. latest version is 0.7 but there are some features which were added after that, but there is no version 0.8 - formally, they do not belong to any version. How to correctly report support for these changes and new features?
* TODO Possibility to unify network:status, network:started, network:stopped messages?
* TODO NetworkStarted response payload: Format of the time field. Just a String, but what format? ISO8601 or Unixtime? Why is this field neccessary, what is the meaning of it?
* TODO network:debug request the specification does not specify how to respond in success case. Send back an empty network:debug message? Or filled with some values? Would be useful for correlation if the graph name at least was returned. Returning the new state would also make sense, but if the new state != requested state then a network:error would be returned. So, it looks like it is not neccessary to return the new state. Clarify and put it into the spec.
* TODO network:debug from a language pespective, the spec only says enable: "whether to enable debug mode". It does not talk about disabling the debug mode. Improve wording.
* TODO responses for trace:start, trace:stop, trace:clear, trace:dump : Return graph field?
* TODO when is a trace:clear and trace:dump allowed? while running or only while stopped? -> add state diagram or similar information to the trace:* messages.
* TODO trace:dump is the only trace:* message that has the "type" parameter. Looks like the runtime should just capture everything, but then at the final point, the trace:dump request asks just for a subset of the trace and the runtime has to throw most of the trace data away. -> would make more sense to request the type of dump at the trace:start request and trace:dump retrievies that type of dump. Then the runtime would capture only useful data, would be more efficient, throws nothing away.
* TODO trace:dump do we have to return the field "type" on the response message? If the could be multiple requests for trace dumps, then for request-response correlation it would make sense. Otherwise, it would be redundant.
* TODO documentation for trace:dump says "undefined".
* TODO the documentation of graph:clear says "initialize an empty graph". The wording creates the impression of creating a new graph, but the intention of this message seems to be to clear the current graph? OTOH, the description of the field "id" says: "identifier for the graph being created". How to remove a graph? Also the command "clear" is different from "create" usually.
* TODO general: why is there often times a parameter "graph" when there are no messages to create, delete or load another graph?
* TODO inconsistency in graph:clear it is "id" and "name" but described as "human-readable label for the graph being created", but in runtime:runtime it is "id" and "label". Why not call it id+label in both messages? (And additionally, have the "description" field.)
* TODO graph:clear how to interpret the field "main"? not sure at all.
* TODO graph:clear the field "library" how to interpret this? format of this component library id? Where do components come from? Who defines the component library, how to add and remove and manage component libraries? Where is this component library stored? Or is this simply implementation-specific and left open? Or is this field meant as "the identifier of this graph if it is used as a component, under what name it should be listed in the component library"?
* TODO documentation for graph:error is completely missing in the spec
* TODO what is the expected numeric range of metadata x and y that is present on graph node and graph inports and outports? Only positive or also negative? What range in each direction?
* TODO graph:addinport which fields should be sent back to the client?
* TODO in which graph running state should edge changes and adding and removing inports and outports be allowed? The network is wired and running, after all... how to handle?
* TODO FBP network protocol calls the message graph:addnode and the node is backed by a component, which makes sense. Whereas the FBP JSON graph spec calls them processes, but a graph contains nodes or components, but processes are a run-time word, but a graph is not a run-time construct. So, the wording is mixed. Non-runtime: Graph with edges and nodes. Nodes are backed by a component (program). Runtime wording: Network of processes (loaded components) with connections that need to be connected and disconnected.
* TODO graph:changenode detailed behavior: Should only the fields present in the request message be overwritten (but then how to remove previously-set metadata keys?) or should the entire existing metadata block on the node be replaced?
* TODO graph:changenode request: The fields x and y look like mandatory fields, they are not shown as optional. So, is it mandatory to at least change the x and y fields? But what if the client just wants to change some other field(s)?

Clarifications for Graph schema:

* TODO spec allows for process, port, index = indexed ports in (intra-graph) connections, but not for graph inports and outports -- how to connect an inport or outport to an indexed port of a process?
* TODO spec has connection metadata: what is "route" used for? doc says: "Route identifier of a graph edge"
* TODO spec has connection metadata: is "schema" used for validating the passing data against a schema, therefore allowing enforcing well-formed data to pass over this edge? Can it also be used to find matching components, which are known to have compatible data schema on this schema, so that the output and input data is compatible like GStreamer does this component matching with MIME types?
* TODO spec has connection metadata: what is "secure" used for? doc says: "Whether edge data should be treated as secure" what does that mean?
* TODO spec defines graph.inports and graph.outports as objects/hashmaps but graph.groups are suddenly an array of objects (which contain the property "name") - seems inconsistent. Intentional? An array provides an ordered set, does the order of the groups matter?
* TODO spec defines too little information on graph inports and outports. If the FBP network protocol is asking for example for the allowed type in the component:ports message, it is mandatory to send the type field, but in the FBP JSON graph spec, the graph inports and outports do not have that field -> should be added. The field schema is optional on the component:ports message, but would be great to have to validate input and have that field in the FBP Graph spec as well. Problem is: Currently, these extra fields that component ports have cannot be stored in a schema-compliant FBP JSON Graph. Same for description, that would also be useful. In the backward direction, we cannot return the fields that the Graph inports and outports have, which the component:ports message does not know about (process, port, metadata x,y).
  * Generally, why are the graph inports in the FBP JSON Graph spec a special case with less fields? Why not have the same fields (with most of them optional) as for the component ports?