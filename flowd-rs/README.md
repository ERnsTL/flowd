Re-implementation of the *flowd* FBP runtime in Rust.

More on the idea and concepts of *flowd* [in its Github repository](https://github.com/ERnsTL/flowd).

Once feature parity with the Go version has been achieved (note, the Go version does not support FBP Network Protocol) then flowd-rs will become the main version. (Existing components in Go will still be usable in *flowd-rs* through an adapter component.)


## Running

Run it with:

```
cargo run
```

Next, [open the online editor](https://app.flowhub.io/#runtime/endpoint?protocol%3Dwebsocket%26address%3Dws%3A%2F%2Flocalhost%3A3569). This loads the management application from a central server, but connects to your local runtime.

You should see a predefined test network, can re-arrange them, start/stop the network. You will see output on your terminal.

It should look roughly like this:

![Bildschirmfoto vom 2022-11-19 16-14-57](https://user-images.githubusercontent.com/3127919/202857780-e070ca3f-fffd-41dc-8470-be9e551facc6.png)

For how to use the online editor, see the [manual of noflo-ui](https://github.com/noflo/noflo-ui).


## Current Status

FBP network protocol:
* Full serialization and deserialization of all specified messages in both directions.
* Runtime behaviors are in mock stage, working with in-memory state and actual components.
* Adding components, removing components, changing components and their connections is implemented.
* Starting and stopping the network is implemented (graph traversal, connection setup, component instantiation, thread start/stop, background watchdog, signaling).
* Sending and receiving IPs to/from graph inports and outports is implemented. So, it is possible to send data into/out of the network directly using FBP Network Protocol (besides the possibility to create components which input/output on TCP or any other channel).
* Delivery of IIPs is implemented.
* IP transfer between components and bounded connections between them is implemented.
* Much to clarify with developers of the protocol spec.
* Ability to implement custom buffering and flushing strategies is implemented.
* Bulk transfers and parallel send/receive are implemented.
* No bounds on the size of each information packet (limitation: system memory size, ulimits).

Test suite of the FBP network protocol:
* One of the next milestones (TODO).
* Currently more focusing on practical usability via noflo-ui.
* Several things to clarify with the developers of the test suite, especially error reporting is lacking.

Graph support:
* Full in-memory representation and serialization and deserialization to/from the FBP JSON Graph format is implemented.
* All properties of the FBP JSON Graph data format are defined.
* Loading and saving to/from disk is unimplemented.
* Subgraphs can be (de-)serialized but behavior is unimplemented.
* Some things to clarify with developers of the spec.

Component management:
* Currently all components are compiled-in.
* Currently no support for linking to components in external paths or repositories.
* One C-API-based component exists called *LibComponent* that loads a component from a shared object and calls into it (very basic).

Online editing:
* Supported based on the FBP network protocol.
* Currently used user interface us noflo-ui.
* Currently only 1 graph inside the runtime is implemented, though the data structures are there to support multiple.
* TODO Support for multiple graphs inside the runtime, managed completely separately.
* TODO Support for multiple FBP network protocol clients in parallel (?)
* Much to clarify with developers of noflo-ui, status messages and documentation are terse.

Security:
* Currently unimplemented.
* Basic token-based security and TLS support would be easy to add (TODO).
* User and ACL management as well as ACL checking currently unimplemented (TODO).

Multi-language feature:
* Part of the next milestone (TODO).
* Basic loading and unloading of a dlopen()'ed component is there (LibComponent).

Multiple component APIs, component data formats:
* Currently unimplemented.
* Will likely develop in the direction of having
  1. core components written in Rust working directly with the in-memory data structures and
  2. components which accept the internal data structures but present different API and data formats when talking with the loaded external components (shared library, scripts, components communicating over STDIN/STDOUT)
* Planned: Support for multiple component APIs: (TODO)
  * passive component driven by process(), both stateful and stateless (needs a scratch space somehow)
  * active component that is run inside an own thread (question of 2 intermixed memory allocators?)
  * active component that can do callbacks and feedback into flowd
  * components that can/cannot handle re-connection and state changes
* Planned: Support for multiple network graph backends: Internal Rust, GStreamer-based, MQTT-based etc. (TODO)
* Planned: Support for multiple data formats when communicating with the components: JSON, CBOR, ASN.1, netstrings etc. (TODO)

Online network changes:
* Currently unimplemented, the network has to be stopped and restarted for changes to take effect. (TODO)

Component library:
* Management of in-memory compiled-in Rust components is implemented.
* One of the components, *LibComponent*, can load an external shared object and call a function to process data (very basic).

Debugging, tracing:
* Serialization and deserialization of the accoding messages is fully implemented.
* Currently responds with mock responses but does not send any tracing data.

Logging:
* Runtime logging facilities to STDOUT with multiple levels, mentioning the thread name is implemented.
* TODO logging to logfiles and syslog (-> log rotation)
* Processes can send STDOUT- and STDERR-like information to the runtime logfile and/or to FBP Network Protocol client(s).

Component repository from local files:
* Planned, one of the next milestones (TODO).

Component hub/repository in the internet:
* Planned: Integration/registration with Flowhub ([source](https://github.com/flowbased/protocol-examples/blob/master/python/flowhub_register.py))?
* Planned, much later (TODO).

Deployment and reproducible setups:
* Currently using plain Cargo, no ability to include or compile-in any external/additional components.
* Planned (TODO). Goal is to easily load components from a Github repository, build them and use them in a network. Naming and referencing to such external components by git repository.

Signaling, Monitoring:
* A background caretaker thread with ability to signal to and from all processes is implemented.
* In addition, the main thread can issue one-way signaling to threads, eg. for a stop command.
* The caretaker thread issues ping health check requests at regular intervals to test aliveness and response time of all processes.
* Export of monitoring data, API server or visualization is currently not implemented.

Checkpointing:
* Planned, much later.

Present in Go version to reach feature parity:
* TODO Sub-networks resp. composite components
* TODO Can inspect, debug and interact with network components using standard Unix tools
* TODO Can run a terminal UI component - and then bring it to the web using gotty :-)
* TODO Delivery of program parameters to components (?)
* TODO Connections between components in framed or raw way
* TODO Basic array ports
* TODO Broadcasting to multiple output ports, serializing only once
* TODO Parsing of .fbp network specifications
* TODO Parsing of .drw network specifications made using DrawFBP
* TODO Closing of ports (implemented) and close detection
* TODO Gracelful shutdown once all data has been processed and all components shut down
* TODO Visualization of the given network in GraphViz format
* TODO Display of required components and file dependencies of the given network for deployment
* TODO Ability to use a network bridge or protocol client, which uses the transport protocol and serialization format of your choice

Everything else:
* Maybe I forgot about something important, please post it as a Github issue.


## Included Components

* Repeat
* Drop
* Output
* LibComponent (work in progress - for loading components from C API libraries)
* Unix socket server
* File reader
* Trim
* Split lines
* Count

TODO (copy from Go version):

* TCP client and server
* Unix domain client and server (abstract and path-based)
* TLS client and server
* SSH client
* Simple HTTP server and client
* Re-use of any existing programs and their output or for transformation of data (cmd component)
* Bridges between different network parts, and thus...
* Distribution of the network across multiple machines
* File writing
* File tailing resp. following
* Modification of frame headers (?)
* Extraction of data from frame body
* Routing based on frame contents or header values (?)
* Time-based events with cron expressions
* Counter for packets (done), packet sizes and packets matching by header field (?)
* Example login prompt and command-line interaction component
* Example terminal UI component sending messages into the network
* Compression and decompression in XZ/LZMA2 and Brotli formats
* Load balancing components with high availability, fail-over, reconnection of output ports and programmatic switching of output ports
* Zeroconf service publishing and browsing based on mDNS (multicast DNS) resp. Bonjour
* WebSocket server and client with retry on connection establishment


## Next Steps

Check the [milestones on Github](https://github.com/ERnsTL/flowd/milestones).

Basically, implement most functionality using in-memory data structures, then break down the structure into different parts (network backends, component API) and allow the component API to be fulfilled by components from shared objects, scripts etc.

Then add more components, port the Go components or add a wrapper for running them (running components as an external process using STDIN and STDOUT makes sense and will be one of the supported execution models).

Create first applications using these and add features to support these use-cases and evolve in tandem with these.

Finally, become production-ready with management, roles, ACLs, security, hardening overall, monitoring.
