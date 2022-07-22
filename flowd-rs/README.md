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

Graph spec:

* TODO spec allows for process, port, index = indexed ports in (intra-graph) connections, but not for graph inports and outports -- how to connect an inport or outport to an indexed port of a process?
* TODO spec has connection metadata: what is "route" used for? doc says: "Route identifier of a graph edge"
* TODO spec has connection metadata: is "schema" used for validating the passing data against a schema, therefore allowing enforcing well-formed data to pass over this edge? Can it also be used to find matching components, which are known to have compatible data schema on this schema, so that the output and input data is compatible like GStreamer does this component matching with MIME types?
* TODO spec has connection metadata: what is "secure" used for? doc says: "Whether edge data should be treated as secure" what does that mean?
* TODO spec defines graph.inports and graph.outports as objects/hashmaps but graph.groups are suddenly an array of objects (which contain the property "name") - seems inconsistent. Intentional? An array provides an ordered set, does the order of the groups matter?