Re-implementation of the flowd runtime in Rust.

(More on flowd [in its Github repository](https://github.com/ERnsTL/flowd).)

1. First implement the FBP network protocol and add dummy components.
2. Then add real components, most likely based on GStreamer which takes care of loading and unloading of components as well as managing data flow and pipelines. Language interoperability, high performance and access to many existing audio, video and networking components.
3. Finally, add support for persistence -- .fbp file format from DrawFBP as well as the JSON graph format from noflo.
4. Transition all Go components.
5. Fade out the Go version (or use it as an "executor" resp. execution layer).
6. More features and real-life applications? Add registration with Flowhub ([source](https://github.com/flowbased/protocol-examples/blob/master/python/flowhub_register.py))? CBOR in addition to JSON? Other FBP UIs for online FBP programming?

Run it with:

```
RUST_LOG=debug cargo run
```

Peculiarities:

* noflo-ui error "connection failed" = runtime down or real network problem
* noflo-ui error "connection timed out" is more than network-level connection timeout; testsuite and noflo-ui does WebSocket upgrade, runtime:getruntime, component:list, network:status, component:getsource ...TODO anything more?
* Firefox seems to automatically use wss:// even if requesting ws:// in connection URL
* If the runtime:runtime response message states that there is a currently running graph, then noflo-ui uses the component:getsource request message to get the source code of the graph. Language can be json (noflo schema) or fbp (FBP network language from J. Paul Morrison). We return a placeholder for the source code and allow the capability component:getsource, otherwise noflo-ui complains that it is not permitted to send component:getsource request messages, see [this issue](https://github.com/noflo/noflo-ui/issues/1019). Using component:getsource to request the graph source seems to make many graph:* network:* and component:list* requests useless.
