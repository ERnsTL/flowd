HTTP server components:

* ```http-parser``` parses HTTP requests, structure similar to and to be used behind a network server, eg. ```tcp-server```
* ```http-router``` routes requests to parts of the FBP network
  TODO maybe not even required - a generic FBP router is probably sufficient

TODO Go ```net.http``` ```Server``` can use any [```Listener```](https://golang.org/pkg/net/#Listener) for network connectivity using [```http.Serve```](https://golang.org/pkg/net/http/#Serve) method; also a possible way for an HTTPd macro-component.
