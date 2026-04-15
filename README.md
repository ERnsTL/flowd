# flowd: *the data must flow*<sup>[1](https://en.wikiquote.org/wiki/Dune_(film)#Others)</sup>

> There exist two implementations of ```flowd```:
>
> * [flowd-rs](https://github.com/ERnsTL/flowd/) (main variant - this one)
> * [flowd-go](https://github.com/ERnsTL/flowd-go/)

Wire up components (programs) written in different programming languages, using the best features and available libraries of each.

Make them communicate in a network of components.

Build a *data factory* in which components transform the passed data frames to produce a useful output.

Components naturally make use of all available processor cores.

A component network can span multiple machines, lending itself for use in distributed systems. Routing is available and a load-balancing component exists.

Use available off-the-shelf components where you can. Grow a collection of specialized components and *reuse* them for the next and next project of yours.

Thus, rather than rewriting code anew for each project, you become more and more efficient with regards to *human time* spent on development.

This is the basic idea of *Flow-based Programming* (FBP), as pioneered by [J. Paul Morrison](http://www.jpaulmorrison.com/fbp/).

The ```flowd``` (for *flow daemon*) is a *runtime environment* for the execution of FBP processing networks, to be defined by a programmer, which then constitutes an *application* or processing system of some kind.

The act of programming is thus shifted from entering strings and lines of tailor-made program source code to a more graphical and visual kind of programming with direct feedback of the changes just made, based on the combination and connection of re-usable *black boxes* (components) working together in a *visually drawable and mappable* processing network resp. application.

Such an FBP processing network is not limited to linear pipes, ETL steps, not even a directed acyclic graph (DAG) structure, RPC request-response, client-server, publish-subscribe, event streams etc. Instead, it is a versatile and generic superset allowing processing networks spanning multiple flowd instances and non-FBP processing systems and thus the creation of general processing systems and even interactive applications.

You can find out more about this paradigm on [J. Paul Morrison's website](http://www.jpaulmorrison.com/fbp/).

More, humans are terrible at writing, maintaining and understanding code, refer [a talk about this](https://www.youtube.com/watch?v=JhCl-GeT4jw). The solution proposed is not to fundamentally improve the way software is engineered, but to keep using conventional programming and just add another layer on top, namely to use AI to generate ever more piles of non-reusable custom application code. Unmentioned in the talk: For understanding and navigating it, one will need even more AI. The alternative, which FBP offers, is to go the other direction and keep applications on a humanly-understandable level by using these re-usable *black boxes* (components), which are individually all easily understandable, and connect them into compose software. FBP processing networks are humanly understandable also because they fit the steps, which a design team would use to model and break down the application's functionality, processing steps and data flows.


## Philosophy

Read the [flowd Manifesto](./MANIFESTO.md) to understand the design principles and goals of this project.


## Installation and Running

* [Install Rust and Cargo](https://rust-lang.org/tools/install/) using rustup.
* Clone the repository using git or the Download button:
  ```sh
  git clone https://github.com/ERnsTL/flowd
  ```
* Compile ```flowd``` and all example components:
  ```sh
  cargo build --release
  ```
* Run it with:
  ```sh
  cargo run
  ```

### The Visual Editor

* [Open the online editor](https://app.flowhub.io/#runtime/endpoint?protocol%3Dwebsocket%26address%3Dws%3A%2F%2Flocalhost%3A3569). This loads the management application from a central server, but connects to your local runtime.

You should see a predefined test network, can re-arrange the components, start/stop the network etc. You will see output on your terminal.

It should look roughly like this:

![Bildschirmfoto vom 2022-11-19 16-14-57](https://user-images.githubusercontent.com/3127919/202857780-e070ca3f-fffd-41dc-8470-be9e551facc6.png)

For how to use the online editor, see the [manual of noflo-ui](https://github.com/noflo/noflo-ui).

### Running Anywhere

```flowd``` can be run in most Linux environments including bare metal Linux distributions, virtualized, containerized and optimized Unikernels. Windows and MacOS untested but are generally possible.

Examples included:

* Running in an unikernel micro-VM using [Unikraft](https://unikraft.org/):
  ```sh
  kraft run -M 256M -p 3569:3569
  ```
  * Note for MacOS users: Best run the micro-VM via Qemu network backend "vmnet", which was added by the developer of AxleOS.
* Running containerized in Docker:
  ```sh
  docker build -t "flowd-testing:Dockerfile" .
  docker run "flowd-testing"
  ```


## Examples

`flowd-rs` currently starts from the persisted graph file `flowd.graph.json` in the repository root.

Build and run:
```sh
cargo build --release
cargo run --release
  or
cargo run --release -- 0.0.0.0:3569
```

Then [connect with the visual editor](https://app.flowhub.io/#runtime/endpoint?protocol%3Dwebsocket%26address%3Dws%3A%2F%2Flocalhost%3A3569).


### The network description format

You can find out more about the ```.fbp``` network description grammar here:

* [J. Paul Morrison's FBP book](http://www.jpaulmorrison.com/fbp/notation.shtml)
* [his .fbp parser source](https://github.com/jpaulm/parsefbp)

Conversion of the classic FBP notation to the JSON format:

* [format explanation by NoFlo](https://noflojs.org/documentation/graphs/#fbp)

About the JSON-based FBP graph format:

* [the .fbp variant used by NoFlo](https://github.com/flowbased/fbp#readme).
* [a parser based on the NoFlo definition](https://github.com/oleksandr/fbp) written in Go which ```flowd``` currently re-uses
* [FBP DSL syntax](https://github.com/flowbased/flowbased.org/wiki/FBP-DSL)


## FBP Runtimes

Runtime variants:
- `flowd-rs` (this repository): https://github.com/ERnsTL/flowd
- `flowd-go` (historical/alternate implementation): https://github.com/ERnsTL/flowd-go


## Features and Current Status

> This is alpha software. It works, is quite optimized, but not all of the planned features are currently present and it is not ready for business operations having continuity requiments met. The API may change unexpectedly.

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
* Components can be linked from external repositories via Cargo `git` dependencies and from local paths via Cargo `path` dependencies.
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
* Processes can send copies of received and/or sent IPs out to FBP Network Protocol client(s) for debugging.
* TODO mandatory debugging of all transmitted packets (TODO performance implications? per-process basis or graph-based or whole runtime?)
* Process name is available on each outport and each connection of an array outport.

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

* Planned, there is support in the FBP Network Protocol and in other runtimes for comparison. (TODO)

Persistence:

* Persisting the network graph data structure 1:1 to disk upon network:persist message.
* Goal:  Never lose your network definition.
* (planned) Sending persist message from the GUI designer. (where is the button in noflo-ui to trigger persist message?)
* (planned) Automatic saving of changed network every x minutes.
* (planned) Integrity checker of loaded and saved graphs, showing critical (cannot load, cannot start this graph) and non-critical errors (missing connections, unconnected ports, unavailable components).
* (planned) Keep a previous version of the persisted graph (.bak file)
* Ability to abort the flowd instance, Ctrl+C does not save and overwrite persistence file.
* Exlusion of the default persistence file from VCS.

Checkpointing:

* Planned, much later.
* Goal:  Never lose your data (packets).

Everything else:

* Maybe I forgot about something important, please post it as a Github issue.


## Included Components

* Repeat
* Drop
* Output
* LibComponent (work in progress - for loading components from C API libraries)
* Unix domain socket server, TODO support for abstract address
* File reader
* File tailing resp. following including detection of replaced or truncated file
* File writer
* Trim for removing trailing and heading whitespace
* Split lines
* Count for packets, size of packets and sum of packet values
* Cron for time-based events, with extended 7-parameter form and granularity up to seconds
* Cmd for calling sub-processes or "shelling out", enabling re-use of any existing programs and their output or for transformation of data, sending data out to the sub-process etc. The sub-process commandline with arguments can be configured. TODO add more info about the features of this component
* Hasher for calculating hash value of packets, supports xxHash64
* Equals routing based on matching IP content
* simple HTTP client
* simple HTTP server, supporting multiple HTTP routes
* Muxer to merge multiple connections into one output for components not able to handle array inports
* MQTT publisher
* MQTT subscriber
* Redis Pub/Sub publisher
* Redis Pub/Sub subscriber
* IMAP e-mail append (sender)
* IMAP e-mail fetch+idle (receiver)
* OpenAI Chat component ("ChatGPT component")
* Tera template component, which includes control flow and logic usable for simple scripts
* Regexp data extraction component
* Text replacement
* Compression and decompression in XZ (LZMA2) format
* Compression and decompression in Brotli format
* Unix domain socket client (path-based and abstract socket addresses, support for SEQPACKET)
* strip HTML tags to get the contained content for further processing
* WebSocket client (TODO retry on connection establishment, TODO reconnection)
* TCP client
* TLS client
* TCP server
* TLS server
* WebSocket server
* Zeroconf service publishing and browsing based on mDNS (multicast DNS) resp. Bonjour and DNS-SD
* JSON query component using jaq/jq filter syntax
* XPath filtering on HTML and simple XML data (note that CSS selectors are a subset of XPath query and can be converted into it)
* SSH client (without using OpenSSH client or libssh) TODO streaming capability of remote program output
* Telegram component using Bot API supporting text messages
* Matrix client component


## Roadmap

Check out the [milestones on Github](https://github.com/ERnsTL/flowd/milestones).

These are present in flowd-go and will be re-implemented here:

* TODO Sub-networks resp. composite components
* TODO Can inspect, debug and interact with network components using standard Unix tools
* TODO Can run a terminal UI component - and then bring it to the web using gotty :-)
* TODO Delivery of program parameters to components (?)
* TODO Connections between components in framed or raw way
* TODO Broadcasting to multiple output ports, serializing only once
* TODO Parsing of .fbp network specifications
* TODO Parsing of .drw network specifications made using DrawFBP
* TODO Closing of ports (implemented) and close detection
* TODO Gracelful shutdown once all data has been processed and all components shut down
* TODO Visualization of the given network in GraphViz format
* TODO Display of required components and file dependencies of the given network for deployment

Planned features:

* Runtime protocol for remote control and online network reconfiguration, enabling real-time visual programming
* Parsing of JSON-FBP network specifications [[1]](https://noflojs.org/documentation/graphs/#json) from [NoFlo](https://noflojs.org/)
* Tracing of data packets as they flow through the network
* Integration with other FBP runtimes
* For more, see the issues list!

Milestone 0.x on connection, disconnection and reconnection:

* Load balancing components with high availability, fail-over, reconnection of output ports and programmatic switching of output ports

Milestone 0.x on objects:

* brackets using OOB marker
* filtering on object structures in sexp fashion
* "XPath"-like filtering on sexp object structures
* building of header (K/V) and body structure in sexp fashion - then it is on par with Go and JavaFBP implementation
* Modification of frame headers (via sexp objects using brackets)
* Routing based on frame contents or header values (sexp objects using brackets)
* Counter packets matching by header field (on sexp objects)

Milestone 0.x on interactive components:

* Example login prompt and command-line interaction component
* Example terminal UI component sending messages into the network


## Architecture

`flowd-rs` is a Rust runtime with compile-time component integration.

Components are registered via:

1. `flowd.build.toml`
2. `Cargo.toml` dependencies
3. `build.rs` code generation (`$OUT_DIR/build_generated.rs`)

At runtime, components are instantiated and connected in-memory. Packet are transferred as pointers in-proc, in-memory. Management and graph control are exposed via WebSocket (FBP protocol).

TODO see the ADRs = Architecture Decision Records found in doc/.


## Performance, Bechmarks

TODO add criterion perf tracking


## Writing Applications

TODO rewrite section for flowd-rs

Three stages usually:

1. read and packetize data structures into IPs
2. filter and transform
3. assemble packets and output

TODO difference is that this goes beyond ETL. It also goes beyond the DAGs, which seem fashionable these days.

TODO modeling the application in terms of what data is relevant and what structure it has, where the data comes from, how it should be transformed and which results should be produced (see JPM book).

TODO no conceptual dissonance between design and implementation stages.

TODO straight implementation, almost waterfall-like, fewer refactorings.

TODO Linear maintenance cost in relation to program size.


## Using Components

1. Find the component on a Git service like Github, GitLab, CodeBerg etc. Repositories usually have "flowd-" in their name. Each repository constitutes a Cargo crate; it can contain one or more components.
2. Add the crate repository to your Cargo.toml like so, to make it known to Cargo:
  > flowd-bla = {
  >   git = "https://github.com/org/flowd-bla.git",
  >   rev = "a1b2c3d4e5f6",
  >   branch = "0.4"
  > }
  Alternatively, via submodules which also references a specific commit:
  > git submodule add https://github.com/org/flowd-bla components/bla
  and add into Cargo.toml like so:
  > flowd_bla = { path = "components/bla" }
3. Add all components contained in the crate repository to your flowd.build.toml, to have it built into flowd. Explanation in the included flowd.build.toml file. The component repository probably has a ready block for copy-paste in its README.
4. Build flowd as usual using ```cargo build --release```.
5. For your project, you can also commit Cargo.lock to have it build reproducibly.


## Writing Components

For `flowd-rs`, implement components as Rust crates under `components/*` using `flowd_component_api`.

Register components through:
- `Cargo.toml` dependency
- `flowd.build.toml` component entry

See:
- `doc/writing-components.md`
- compile-time integration model section below


### Compile-Time Integration Model

`flowd` integrates components at compile time from:

1. `flowd.build.toml` (component declarations)
2. `Cargo.toml` (crate dependencies)
3. `build.rs` (validation + code generation)

During build, `build.rs` generates `$OUT_DIR/build_generated.rs`, and `main.rs` includes it via `include!()`.
This generated code provides:

* component imports
* component metadata registration
* component factory wiring
* component log filter registration

So when adding/removing components, no manual edits are needed in runtime registration/factory/logging code paths.

Build-time validation fails for:

* malformed `flowd.build.toml`
* duplicate component names
* unresolved crates
* missing component symbols (compile-time)
* missing or invalid `[package.metadata.flowd].compatible`
* flowd/component compatibility mismatch (major+minor)


* Flowd compatibility of components is declared in each component's Cargo.toml via:
  ```toml
  [package.metadata.flowd]
  compatible = "0.4"
  ```
  Build-time compatibility check matches major and minor version between flowd and this value. This field is required for crate-based components.
  Module-based components (`crate = "components::..."`) are legacy/dev-only: disabled by default and only allowed with `--features allow-module-components`.
* Minimal crate-based component setup example:
  ```toml
  # in flowd Cargo.toml
  [dependencies]
  flowd-bla = { path = "components/bla" }

  # in flowd.build.toml
  [[components.entry]]
  name = "Bla"
  crate = "flowd-bla"
  struct = "BlaComponent"

  # in components/bla/Cargo.toml
  [package.metadata.flowd]
  compatible = "0.4"
  ```
* Create a branch for each flowd version, for example named "0.4" so that users can get the latest component version for their flowd version. This way, improvements can be ported back for an older version of flowd, and porting to new version of flowd can be done independently without disturbing component version for older versions of flowd.


## Development aka Hacking on ```flowd```

TODO rewrite for flowd-rs

Run all tests:
```sh
cargo test --workspace
```

Run tests only for flowd itself (main crate):

```sh
cargo test -p flowd-rs
```

Run benchmarks:

```sh
cargo bench --workspace
```

If no benchmark targets are defined yet, this command is currently a no-op for this repository.


## Goals in General

TODO also see Manifesto
TODO is the following in sync with the Manifesto?

To make a Flow-Based Programming (FBP) runtime suitable for production operation and reliable applications, it should possess several key characteristics:

* Reliability and Robustness: The FBP runtime should be stable and reliable to ensure uninterrupted execution of critical applications. It should be able to handle errors robustly to avoid or at least minimize failures.

* Scalability: The runtime environment should be capable of handling growing demands and workloads to ensure efficient execution of applications. This may involve scaling vertically (on larger machines) or horizontally (by adding more instances).

* Monitoring and Debugging: There should be mechanisms for monitoring and troubleshooting to analyze performance, identify bottlenecks, and debug issues. This can be achieved through logging, dashboards, tracing, and other tools.

* Security: The runtime environment should provide security mechanisms to ensure data integrity, confidentiality, and availability. This may include authentication, authorization, encryption, and protection against attacks such as injection attacks and denial-of-service attacks.

* Transaction Support: It is important that the FBP runtime supports transactions to ensure data consistency and meet the Atomicity, Consistency, Isolation, and Durability (ACID) properties.

* Integration: The runtime environment should seamlessly integrate with other systems and services to support data flows between different applications, platforms, and external services. This can be done through APIs, protocols such as HTTP, messaging systems, and other mechanisms.

* Documentation and Support: Comprehensive documentation and supportive community can help increase developer productivity and efficiently solve issues. A good FBP runtime should have clear documentation, tutorials, examples, and support channels.

By fulfilling these characteristics, an FBP runtime environment can be made suitable for production and reliable applications to support stable, scalable, and reliable workflows.


## License

GNU LGPLv3+

### Commercial Offerings

TODO ...


## Contributing

1. Open an issue or pick an existing one.
2. Discuss your idea. Formulate an ADR according to [the ADR Guidelines](doc/adr-000-guidelines.md).
3. Send pull request.
4. Quality check.
5. Merged!


## Further documentation

* See the doc/ subdirectory.
* [Issues with FBP JSON specs and noflo-ui](doc/issues-with-specs-and-noflo-ui.md)
* [Historic background (flowd-go)](https://github.com/ERnsTL/flowd-go/README.md#further-documentation)


## Community

General FBP ordered by actuality:

* Discord:  Search for "Flow-based Programming"
* [Reddit sub-reddit](https://www.reddit.com/r/DataflowProgramming/) for FBP and dataflow programming together
* [Google group](https://groups.google.com/forum/#!forum/flow-based-programming)
