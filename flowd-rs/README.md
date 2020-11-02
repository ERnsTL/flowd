Re-implementation of the flowd runtime in Rust.

(More on flowd [in its Github repository](https://github.com/ERnsTL/flowd).)

1. First implement the FBP network protocol and add dummy components.
2. Then add real components, most likely based on GStreamer which takes care of loading and unloading of components as well as managing data flow and pipelines. Language interoperability, high performance and access to many existing audio, video and networking components.
3. Finally, add support for persistence -- .fbp file format from DrawFBP as well as the JSON graph format from noflo.
4. Transition all Go components.
5. Fade out the Go version (or use it as an "executor" resp. execution layer).
6. More features and real-life applications? CBOR in addition to JSON? Other FBP UIs for online FBP programming?

Run it with:

```
RUST_LOG=debug cargo run
```

Peculiarities:

* noflo-ui error "connection failed" = runtime down or real network problem
* noflo-ui error "connection timed out" is more than network-level connection timeout; testsuite and noflo-ui does WebSocket upgrade, runtime:getruntime, component:list, network:status, component:getsource ...TODO anything more?
* Firefox seems to automatically use wss:// even if requesting ws:// in connection URL
