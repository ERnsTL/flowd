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
* TODO noflo-ui complains on connect/disconnect button push that the payload of component:componentsready is non-integer, but spec defines no payload thus interpreted to be an empty object. On first connect to the runtime, it does not complain. TODO integer is number of component:component messages before the component:componentsready message?
* for graph:changenode messages (and similarly applicable to the other response messages) the same fields as the request should be present. TODO to be useful, it should send back the same values as confirmation that these values are now set -- but if sending back a different x coordinate for example, noflo-ui does not snap back the node but acts if it was set correctly. -> TODO would probably need graph:error message.
* TODO why does noflo-ui send tens of messages, and if the drag is a long one, a hundred messages or more? (for every mouse-drag+move event -- but all with the same end coordinates
* TODO spec does not define graph:error response message
