## Features and Current Status

> This is alpha software: usable and fairly optimized, but not all planned features are present yet. It is not production-ready for continuity-critical operations, and APIs may change without notice.

FBP network protocol:

* Full serialization and deserialization of all specified messages in both directions.
* Runtime behaviors are in mock stage, working with in-memory state and actual components.
* Adding graph nodes, removing nodes, changing nodes and their connections is implemented.
* Renaming nodes (internal ID) and changing the label on nodes is implemented.
* Starting and stopping the network is implemented (graph traversal, connection setup, component instantiation, thread start/stop, background watchdog, signaling).
* Sending and receiving IPs to/from graph inports and outports is implemented. So, it is possible to send data into/out of the network directly using FBP Network Protocol (besides the possibility to create components which input/output on TCP or any other channel).
* Delivery of IIPs is implemented.
* IP transfer between components and bounded connections between them is implemented.
* Much to clarify with developers of the protocol spec.
* Ability to implement custom buffering and flushing strategies is implemented.
* Bulk transfers and parallel send/receive are implemented.
* No bounds on the size of each information packet (limitation: system memory size, ulimits).
* Multiple connections going into a single component is possible, with the consuming process able to control from which it wants to read (array inports).
* Array outports addressable by index with ability to see the target process name for debugging.

Test suite of the FBP network protocol:

* (planned) Fully pass the FBP protocol spec test suite.
* Currently more focusing on practical usability via noflo-ui.
* Several things to clarify with the developers of the test suite, especially error reporting is lacking.

Graph support:

* Full in-memory representation and serialization and deserialization to/from the FBP JSON Graph format is implemented.
* All properties of the FBP JSON Graph data format are defined.
* Subgraphs can be (de-)serialized but behavior is unimplemented.
* Some things to clarify with developers of the spec.

Component management:

* Currently all components are compiled-in.
* Components can be linked from external repositories via Cargo `git` dependencies and from local paths via Cargo `path` dependencies.
* One C-API-based component exists called *LibComponent* that loads a component from a shared object and calls into it (very basic).

Online editing:

* Supported based on the FBP network protocol.
* Currently used user interface is noflo-ui.
* Multi-client behavior is partial; full parallel client support is still being hardened.
* Currently only 1 graph inside the runtime is implemented, though the data structures are there to support multiple.
* (planned) Support for multiple graphs inside the runtime, managed completely separately.
* (planned) Support for multiple FBP network protocol clients in parallel.
* Much to clarify with developers of noflo-ui, status messages and documentation are terse.

Security:

* Currently unimplemented.
* (planned) Basic token-based security and TLS support would be easy to add.
* (planned) User and ACL management as well as ACL checking currently unimplemented.

Multi-language feature:

* (planned) Part of the next milestone.
* Basic loading and unloading of a dlopen()'ed component is there (LibComponent).

Multiple component APIs, component data formats:

* Currently unimplemented.
* Will likely develop in the direction of having
  1. core components written in Rust working directly with the in-memory data structures and
  2. components which accept the internal data structures but present different API and data formats when talking with the loaded external components (shared library, scripts, components communicating over STDIN/STDOUT)
* (planned) Support for multiple component APIs:
  * passive component driven by process(), both stateful and stateless (needs a scratch space somehow)
  * active component that is run inside an own thread (question of 2 intermixed memory allocators?)
  * active component that can do callbacks and feedback into flowd
  * components that can/cannot handle re-connection and state changes
* (planned) Support for multiple network graph backends: Internal Rust, GStreamer-based, MQTT-based etc.
* (planned) Support for multiple data formats when communicating with the components: JSON, CBOR, ASN.1, netstrings etc.

Online network changes:

* (planned) Currently unimplemented, the network has to be stopped and restarted for changes to take effect.

Component library:

* Management of in-memory compiled-in Rust components is implemented.
* One of the components, *LibComponent*, can load an external shared object and call a function to process data (very basic).

Debugging, tracing:

* Serialization and deserialization of the according messages is fully implemented.
* Currently responds with mock responses but does not send any tracing data.
* Processes can send copies of received and/or sent IPs out to FBP Network Protocol client(s) for debugging.
* TODO mandatory debugging of all transmitted packets (TODO performance implications? per-process basis or graph-based or whole runtime?)
* Process name is available on each outport and each connection of an array outport.

Logging:

* Runtime logging facilities to STDOUT with multiple levels, mentioning the thread name is implemented.
* (planned) logging to logfiles and syslog (-> log rotation)
* Processes can send STDOUT- and STDERR-like information to the runtime logfile and/or to FBP Network Protocol client(s).

Component repository from local files:

* (planned) Planned, one of the next milestones - currently, components are Cargo crates placed in components/.

Component hub/repository in the internet:

* (planned) Integration/registration with Flowhub resp. noflojs.org ([source](https://github.com/flowbased/protocol-examples/blob/master/python/flowhub_register.py))?
* (planned) much later.

Deployment and reproducible setups:

* Currently using plain Cargo, no ability to include or compile-in any external/additional components.
* (planned) Goal is to easily load components from a Github repository, build them and use them in a network. Naming and referencing to such external components by git repository.

Signaling, Monitoring:

* A background watchdog thread with ability to signal to and from all processes is implemented.
* In addition, the main thread can issue one-way signaling to threads, eg. for a stop command.
* The watchdog thread issues ping health check requests at regular intervals to test aliveness and response time of all processes.
* Export of monitoring data, API server or visualization is currently not implemented.
* The watchdog thread recognizes network self-shutdown and calls the runtime to stop and signal all FBP protocol clients about the status change.

Maintenance, Operations:

* Ability to use a network bridge or protocol client like TCP, WebSocket etc. which uses the transport protocol and serialization format of your choice, to connect multiple FBP networks, subgraphs or sub-networks.
* Bridges between different network parts, and thus...
* Distribution of the network across multiple machines
* Dynamic discovery of FBP network instances and network parts in order to provide redundancy and ability to shut down parts of the whole system for maintenance.

Testing:

* (planned) There is support in the FBP Network Protocol and in other runtimes for comparison.

Persistence:

* Loading and saving to/from disk is implemented in basic form (`flowd.graph.json`), with further validation and UX improvements planned.
* Persisting the network graph data structure 1:1 to disk as `flowd.graph.json` via `network:persist` message.
* Goal:  Never lose your network definition.
* (planned) Sending persist message from the GUI designer. (where is the button in noflo-ui to trigger persist message?)
* (planned) Automatic saving of changed network every x minutes.
* (planned) Integrity checker of loaded and saved graphs, showing critical (cannot load, cannot start this graph) and non-critical errors (missing connections, unconnected ports, unavailable components).
* (planned) Keep a previous version of the persisted graph (.bak file)
* Ability to abort the flowd instance, Ctrl+C does not save and overwrite persistence file.
* Exclusion of the default persistence file from VCS.

Checkpointing:

* Planned, much later.
* Goal:  Never lose your data (packets).

Everything else:

* Maybe I forgot about something important, please post it as a Github issue.
