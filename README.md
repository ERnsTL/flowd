# flowd: *the data must flow*<sup>[1](https://en.wikiquote.org/wiki/Dune_(film)#Others)</sup>

> There exist two implementations of ```flowd```:
>   * [flowd-rs](https://github.com/ERnsTL/flowd/) (main variant - this one)
>   * [flowd-go](https://github.com/ERnsTL/flowd-go/)

Wire up components (programs) written in different programming languages, using the best features and available libraries of each.

Make them communicate in a network of components.

Build a *data factory* in which components transform the passed data frames around to produce a useful output.

Components naturally make use of all available processor cores.

A component network can span multiple machines, lending itself for use in distributed systems. Routing is available and a load-balancing component exists.

Use available off-the-shelf components where you can. Grow a collection of specialized components and *reuse* them for the next and next project of yours.

Thus, rather than rewriting code anew for each project, you become more and more efficient with regards to *human time* spent on development.

This is the basic idea of *Flow-based Programming* (FBP), as pioneered by [J. Paul Morrison](http://www.jpaulmorrison.com/fbp/).

The ```flowd``` (for *flow daemon*) is a *runtime environment* for the execution of FBP processing networks, to be defined by a programmer, which then constitutes an *application* or processing system of some kind.

The act of programming is thus shifted from entering strings and lines of tailor-made program source code to a more graphical and visual kind of programming with direct feedback of the changes just made, based on the combination and connection of re-usable *black boxes* working together in a *visually drawable and mappable* processing network resp. application.

You can find out more about this paradigm on J. Paul Morrison's website linked above.

More, humans are terrible at writing, maintaining and understanding code [talk about this](https://www.youtube.com/watch?v=JhCl-GeT4jw). One solution proposed here is to keep programming on a humanly-understandable level by using these re-usable *black boxes*, which are individually all easily understandable, to compose software.

## Installation and Running

Run it with:

```sh
cargo run
```

Next, [open the online editor](https://app.flowhub.io/#runtime/endpoint?protocol%3Dwebsocket%26address%3Dws%3A%2F%2Flocalhost%3A3569). This loads the management application from a central server, but connects to your local runtime.

You should see a predefined test network, can re-arrange them, start/stop the network. You will see output on your terminal.

It should look roughly like this:

![Bildschirmfoto vom 2022-11-19 16-14-57](https://user-images.githubusercontent.com/3127919/202857780-e070ca3f-fffd-41dc-8470-be9e551facc6.png)

For how to use the online editor, see the [manual of noflo-ui](https://github.com/noflo/noflo-ui).

## Examples

TODO

## Features and Current Status

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

Checkpointing:

* Planned, much later.
* Goal:  Never lose your data (packets).

Present in Go version to reach feature parity:

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

TODO connection, disconnection and reconnection (0.4 milestone):

* Load balancing components with high availability, fail-over, reconnection of output ports and programmatic switching of output ports

TODO objects (0.5 milestone):

* brackets using OOB marker
* filtering on object structures in sexp fashion
* "XPath"-like filtering on sexp object structures
* building of header (K/V) and body structure in sexp fashion - then it is on par with Go and JavaFBP implementation
* Modification of frame headers (via sexp objects using brackets)
* Routing based on frame contents or header values (sexp objects using brackets)
* Counter packets matching by header field (on sexp objects)

TODO interaction components (0.7 milestone):

* Example login prompt and command-line interaction component
* Example terminal UI component sending messages into the network


## Next Steps

Check the [milestones on Github](https://github.com/ERnsTL/flowd/milestones).

Basically, implement most functionality using in-memory data structures, then break down the structure into different parts (network backends, component API) and allow the component API to be fulfilled by components from shared objects, scripts etc.

Then add more components, port the Go components or add a wrapper for running them (running components as an external process using STDIN and STDOUT makes sense and will be one of the supported execution models).

Create first applications using these and add features to support these use-cases and evolve in tandem with these.

Finally, become production-ready with management, roles, ACLs, security, hardening overall, monitoring.


## Architecture

TODO


## Writing Components

TODO


## Development aka Hacking on ```flowd```

Running tests:

  ```
  GOPATH=`pwd` go test ./src/github.com/ERnsTL/flowd/...
  ```

Running benchmarks:

  ```
  GOPATH=`pwd` go test -run=BENCHMARKSONLY -bench=. ./src/github.com/ERnsTL/flowd/libflowd/
  ```

Running tests for the JSON FBP network protocol: Follow [the basic instructions](), but initialize with the following

  ```
  fbp-init --name flowd --port 3000 --command "bin/flowd -olc localhost:3000 src/github.com/ERnsTL/flowd/examples/chat-server.fbp" --collection tests
  ```

Use the latest ```node.js``` and ```npm``` from [nodesource](https://www.nodesource.com/), otherwise you may get Websocket errors. The npm package *wscat* is useful for connection testing.


## Documentation

* [Issues with FBP JSON specs and noflo-ui](doc/issues-with-specs-and-noflo-ui.md)


## Goals in General

To make a Flow-Based Programming (FBP) runtime suitable for production operation and reliable applications, it should possess several key characteristics:

* Reliability and Robustness: The FBP runtime should be stable and reliable to ensure uninterrupted execution of critical applications. It should be able to handle errors robustly to avoid or at least minimize failures.

* Scalability: The runtime environment should be capable of handling growing demands and workloads to ensure efficient execution of applications. This may involve scaling vertically (on larger machines) or horizontally (by adding more instances).

* Monitoring and Debugging: There should be mechanisms for monitoring and troubleshooting to analyze performance, identify bottlenecks, and debug issues. This can be achieved through logging, dashboards, tracing, and other tools.

* Security: The runtime environment should provide security mechanisms to ensure data integrity, confidentiality, and availability. This may include authentication, authorization, encryption, and protection against attacks such as injection attacks and denial-of-service attacks.

* Transaction Support: It is important that the FBP runtime supports transactions to ensure data consistency and meet the Atomicity, Consistency, Isolation, and Durability (ACID) properties.

* Integration: The runtime environment should seamlessly integrate with other systems and services to support data flows between different applications, platforms, and external services. This can be done through APIs, protocols such as HTTP, messaging systems, and other mechanisms.

* Documentation and Support: Comprehensive documentation and supportive community can help increase developer productivity and efficiently solve issues. A good FBP runtime should have clear documentation, tutorials, examples, and support channels.

By fulfilling these characteristics, an FBP runtime environment can be made suitable for production and reliable applications to support stable, scalable, and reliable workflows.























## Writing Applications

TODO

Three stages usually:
1. read and packetize
2. filter and transform
3. assemble packets and output

TODO modeling the application in terms of what data is relevant and what structure it has, where the data comes from, how it should be transformed and which results should be produced (see JPM book).

TODO no conceptual dissonance between design and implementation stages.

TODO straight implementation, almost waterfall-like, fewer refactorings.

TODO Linear maintenance cost in relation to program size.


## FBP Runtimes

There exist several FBP *runtimes*, which emphasize different aspects of FBP and realize the underlying concept in different ways.

There are a few categories of FBP runtimes:

1. Single-language systems and libraries. Everything is running inside the same process and is written in the same programming language. Often, the network is defined using this programming language as well. This class has the best performance but is also very specialized.

2. Tighly-integrated systems. These try to pull all components into the same process using dynamic loading of libraries (.so / .dll) and thus into the same address space to save on context switches. To communicate, shared memory is usually used. It is possible to integrate components written in different programming languages, but requires strict conformance and conversion to a common binary message layout and flow of program execution (ABI, application binary interface). Definition of FBP processing networks is done declaratively or using own scripting languages.

3. Loosely-coupled inter-language systems. The different components run as separate processes and communicate using sockets, named pipes, message queueing systems etc. This category requires little effort, tailoring and no special libraries to get started. They can integrate components and even existing non-FBP-aware programs into its processing networks. The chosen data formats, protocols etc. are based on common, widespread formats which are easy to implement. Definition of networks is usually done in a declarative format.

The different ```flowd``` implementations have different approaches and focuses.



* [flowd-rs](flowd-rs/README-Rust.go#FBP Runtimes)
* [flowd-go](README-Go.go#FBP Runtimes)


## Integration with other FBP Runtimes

One feature of FBP is the ability to freely transform data. Thus as a general solution, common IPC mechanisms like TCP, WebSocket or Unix domain sockets can be used to bridge FBP networks running in different FBP runtimes. flowd can also start other runtimes as subprocesses using the ```cmd``` component.

For more optimal and tighter integration, there are gateway components and protocols as follows:

* with JavaFBP: gateway component planned, [Java parser of framing format planned](https://github.com/ERnsTL/flowd/issues/130)
* with NoFlo: implementation of JSON FBP protocol [planned, partway started](https://github.com/ERnsTL/flowd/issues/55)
* with plumber: [gateway component planned](https://github.com/ERnsTL/flowd/issues/124)
* with MsgFlo: runs over message queues; [MQTT component planned](https://github.com/ERnsTL/flowd/issues/71)
* others: Fractalide? ...?


## License

GNU LGPLv3+


## Contributing

1. open an issue or pick an existing one
2. discuss your idea
3. send pull request
4. quality check
5. merged!


## Further documentation

* TODO
* Further historic information in the [flowd-go](https://github.com/ERnsTL/flowd-go/README.md#Further documentation)


## Community

* [Google group](https://groups.google.com/forum/#!forum/flow-based-programming)
* [Reddit sub-reddit](https://www.reddit.com/r/DataflowProgramming/) for FBP and dataflow programming together




















> This is alpha software. It works, is quite optimized, but not all of the planned features are currently present. The API may change unexpectedly.


## Features

Currently present features:

* Parsing of ```.fbp``` network specifications
* Parsing of ```.drw``` network specifications made using [DrawFBP](http://www.jpaulmorrison.com/fbp/software.html#DrawFBP)
* Starting a network of the specified components
* Simple and easy to implement framing format
* Multi-core use resp. parallel processing
* Closing of ports and close detection
* Gracelful shutdown once all data has been processed and all components shut down
* Visualization of the given network in *GraphViz* format
* Display of required components and file dependencies of the given network for deployment
* Ability to use a network bridge or protocol client, which uses the transport protocol and serialization format of your choice - kpc, WebSocket,  GRPC, CapnProto, Protobuf, Flatbuffers, JSON, MsgPack, gob, RON, ...
* Sub-networks resp. composite components
* Fast, direct transfer of IPs between components using named pipes (FIFOs); only shared memory would be faster
* Running a processing network with or without ```flowd``` as the orchestrator
* Can inspect, debug and interact with network components using standard Unix tools
* Can run a terminal UI component - and then bring it to the web using [gotty](https://github.com/yudai/gotty) :-)
* Delivery of *initial information packets* (IIPs)
* Delivery of program parameters to components
* Connections between components in framed or raw way
* Basic array ports
* Broadcasting to multiple output ports, serializing only once

The included example components cover:

* TCP client and server
* Unix domain client and server (abstract and path-based)
* TLS client and server
* SSH client
* Simple HTTP server and client
* Re-use of any existing programs and their output or for transformation of data
* Bridges between different network parts, and thus...
* Distribution of the network across multiple machines
* File reading and writing
* Line splitting
* File tailing resp. following
* Modification of frame headers
* Extraction of data from frame body
* Routing based on frame contents or header values
* Time-based events with cron expressions
* Counter for packets, packet sizes and packets matching by header field
* Example login prompt and command-line interaction component
* Example terminal UI component sending messages into the network
* Compression and decompression in XZ/LZMA2 and Brotli formats
* Load balancing with high availability, fail-over, reconnection of output ports and programmatic switching of output ports
* Zeroconf service publishing and browsing based on mDNS (multicast DNS) resp. Bonjour
* WebSocket server and client with retry on connection establishment

Planned features:

* Runtime protocol for remote control and online network reconfiguration, enabling real-time visual programming
* Parsing of JSON-FBP network specifications [[1]](https://noflojs.org/documentation/graphs/#json) from [NoFlo](https://noflojs.org/)
* Tracing of data packets as they flow through the network
* Integration with other FBP runtimes
* For more, see the issues list!


## Architecture

All components are either normal programs or scripts, which do not have to be specially modified to be used in a ```flowd``` network (wrapped in a ```cmd``` component) or they are programs, which understand the ```flowd``` framing format. Raw binary data streams are also possible, eg. a compressed data stream.

All components are each started by the ```flowd``` program. It parses the network definition, defines the network connections, starts the components with arguments and handles network shutdown. It is also possibe to start a network perfectly fine using a hand-written shell script, but having a declarative network definition and let ```flowd``` manage it is easier.

A component can have multiple input and output ports. Ports are named. Without message framing (wrapped in a ```cmd``` component), input can be passed to an unmodified program and output can be used within the processing framework.

A component communicates with the outside world using named pipes which are handled using standard file operations. Over these, it receives input frames and can send frames to its named ports and thus to other components; the frames are sent directly to the other end of the named pipe, which is the input port of the neighbor component.

The framing format is a simple text-based format very similar to an HTTP/1.x header or a MIME message, which is also used for e-mail. Currently, a subset of [STOMP v1.2](https://stomp.github.io/stomp-specification-1.2.html) is used. It can easily be implemented in any programming language, is easy to extend and can carry a frame body in any currently-trendy format be it textual or binary. A frame contains information on (many fields are optional):

* Over which port did this frame come in? Over which port shall this be sent out to another component?
* Is this a control frame or a data frame?
* If data frame, what is the data type resp. class name resp. message type in the frame body? This is user-defined.
* What is the MIME type resp. content type of the frame body? For example JSON, plain text, XML, Protobuf, Msgpack, any other binary formats or whatever.
* What is the content length resp. frame body length?
* The frame body is a free-form byte array, so you can put in whatever you want.
* Header fields are extensible to convey application-specific meta data and most fields are optional, using no bandwidth.

For more information on the framing format see [the Go implementation source code](https://github.com/ERnsTL/flowd/blob/master/libflowd/framing.go) and the [prose format spec](doc/framing_format.md).

Using several components, a network can be built. It is like a graph of components or like workers in a data factory doing one step in the processing. The application developer connects the output ports to other components' input ports and parameterizes the components. Most of the components will be off-the-shelf ones, though usually a few have to be written for the specific application project. In this fashion, the application is built.


## Installation

Download:

```
GOPATH=`pwd` go get -u github.com/ERnsTL/flowd
```

Compile and install ```flowd``` and all example components:

```
GOPATH=`pwd` go install github.com/ERnsTL/flowd/...
```


## Examples

Several example components and example processing networks are included.

Compile the network orchestrator and runtime ```flowd```, then run examples like this:

```
bin/flowd src/github.com/ERnsTL/flowd/examples/chat-server.fbp
```

This particular example comprises a small chat or console server over TCP. Upon starting ```flowd```, it should show that all components have started up and that the TCP server component is ready for connections.

Then connect to it using, for example:

```
nc -v localhost 4000
```

When you connect, you should see a message that it has accepted your connection. When you send data, you should see it sent to an intermediary copy component and further back to ```tcp-server```'s response port and back out via TCP to your client.

The flag ```-quiet``` removes the frame passing information, in case you do not want to see it.

The data flow is as follows:

```
TCP in -> tcp-server OUT port -> chat IN port -> chat server logic -> chat OUT port -> tcp-server IN port (responses) -> TCP out
```

Also, an *initial information packet* (IIP) is sent to the ```ARGS``` input port of the ```tcp-server``` component, as defined in the network specification:

```
'localhost:4000' -> ARGS tcp-server
```

This is the first packet/frame sent to this component. It usually contains configuration information and is used to *parametrize* this component's behavior. When sending an IIP to the ARGS port, this is converted to program arguments.

A more complete, parser-exercising example is located in ```examples/example.fbp```.

You can find out more about the ```.fbp``` network description grammar here:

* [J. Paul Morrison's FBP book](http://www.jpaulmorrison.com/fbp/notation.shtml)
* [his .fbp parser source](https://github.com/jpaulm/parsefbp)
* [format explanation by NoFlo](https://noflojs.org/documentation/graphs/#fbp)
* [the .fbp variant used by NoFlo](https://github.com/flowbased/fbp#readme).
* [a parser based on the NoFlo definition](https://github.com/oleksandr/fbp) written in Go which ```flowd``` currently re-uses
* [FBP DSL syntax](https://github.com/flowbased/flowbased.org/wiki/FBP-DSL)


## Visualization Example

*flowd* can export the network graph structure into *GraphViz* format for visualization.

The following commands will export a network to STDOUT, convert it to a PNG raster image, view it and clean up:

```
bin/flowd -graph src/github.com/ERnsTL/flowd/examples/example.fbp | dot -O -Kdot -Tpng && eog noname.gv.png ; rm noname.gv.png
```


## Writing Components

Decide if your program shall implement the ```flowd``` framing format or be wrapped in a ```cmd``` component.

If wrapped, you can decide for the program to be called for each incoming frame in order to process it or if your program should process a stream of frame bodies. If one-off, the program will receive data on STDIN, which will be closed after the frame body has been delivered; the program can the output a result, which will be forwarded into the FBP network for further processing. It is then expected to either close STDOUT or exit the program. In the one-instance mode, STDIN and STDOUT will remain open; your program will receive data from incoming frame bodies to be processed and any output will be framed by the ```cmd``` component and again forwarded into the network.

Otherwise, implement the simple ```flowd``` framing format, which can be seen in the files [libflowd/framing.go](libflowd/framing.go) and [libflowd/framing_test.go](libflowd/framing_test.go). It is basically STOMP v1.2 as specified with the modifications mentioned there. This can be done using a small library for the programming language of your choice. Your component is expected to open the named pipes given and will then be connected with the neighbor components. Frames of type *data* and *control* are common. Especially important are the IIPs, denoted by their *body type* IIP, which are usually used for component configuration. Port closing detection is done using regular EOF on the named pipe; this is usually the signal that all data has arrived from the preceding component and that it shut down; it can also be re-opened if that is the use-case. Components should forward existing headers from the incoming frames/IPs, because downstream connections might lead to a loop back to the sender requiring a header field present for correlation, like for example a TCP connection ID, so keep additional header fields intact; packet tracing is also implemented using marker values in the header. Output frames, if any, are then to be sent to the output named pipes. That way, the frames from your component are sent directly to the component which is connected to the other side of the given output port - to be processed, filtered, sorted, stored, transformed and sent out as results to who knows where... That's it - it's up to you!


## FBP Runtimes

As currently implemented, ```flowd-go``` positions itself on the most performant border of the third category, without requiring conformance and internal data conversion to an ABI. Named pipes are the fastest IPC mechanism behind shared memory. The framing format used is easy to implement and parse with modest processing overhead, assumed to be in the range of the data conversion overhead required by the second category for ABI conformance.

What cannot be removed in this third class is the overhead of data copying across process borders which requires context switches and CPU ring switches. On the other hand, the process-level separation buys the capability to be pretty much universal in terms of integration with other FBP runtimes, free choice of programming languages for components as well as re-use of existing non-FBP programs. (And not to worry, ```flowd``` can still transfer several million IPs / messages per second on laptop-class hardware.)

In other words, ```flowd-go``` puts focus on:

1. *Programming language independence.* Being able to easily mix different PLs in one FBP processing network. This enables to combine the strengths of different programming languages and systems into a FBP processing network. To this end, ```flowd``` uses a simple way to communicate, which is common to all programming languages and that is reading and writing to/from files. If it were done in a more complicated way, then it would become neccessary to create a *libflowd* or similar for interfacing with ```flowd``` and to write bindings to that, for each programming language. But not all PLs can import even a C library, let alone a library written in some other calling convention, because it does not fit their way of computing or internal representations, abstractions etc. and you cannot expect every PL being able to import a library/package written in every other PL. And further, all the bindings to some *libflowd* would have to be maintained as well. So, since this path is not desireable, communication using named pipes which are just files was chosen, with STDERR being used for any status output, log messages etc. STDIN and STDOUT can be used for terminal UI components. Also, if any complex data format were mandated (Protobuf, XML, JSON, ZeroMQ, MsgPack, etc.) then this would lock out languages where it is not available. Since all this is not desireable nor feasible, ```flowd``` cannot introduce any new protocols, any new data formats or require importing any of their libraries or bindings to these. Therefore a very simple, text-and-line-based and even *optional* framing format very similar to HTTP/1.x headers and MIME e-mail headers is used, since strings and newlines are available in every programming language and will most likely be available in times to come - well, unless the world decides not to use character strings any more ;-)

2. *Re-use existing programs.* Every program, even a Unix pipe-based processing chain, which can output results either to a file or STDOUT, can be wrapped and re-used by ```flowd``` in an FBP processing network. Therefore, ```flowd``` can be used to extend the Unix pipe processing paradigm to a superset, a directed graph model.

3. *Easy to write components.* Open the input named pipe file, read lines until you hit an empty line, parse the header fields, read the frame/IP body accordingly. Do some processing, write out a few lines of text = header lines, write out the body to the output named pipe. If the component has anything to report, write it to STDERR. No library to import, no complex data formats, no APIs.

4. *Spreads across multiple cores.* The FBP networks of ```flowd``` - like those of other FBP runtimes and systems - intrinsically spread out to multiple CPUs resp. CPU cores. In the case of other systems it is because they are different threads, in ```flowd``` because they are seperate processes. This enables the saturation of all CPU cores and parallel processing to ensue in an easy way - simply by constructing a network of components, which all just read and write to/from files.

5. *Can spread across multiple machines.* It is simple to plug in a network transport component like TCP, TLS, KCP, SSH etc. or even pipe your frames into any external program (using the ```cmd``` component). Either the frame body = data content or the frames themselves can be sent, thus creating a bridge to another part of the FBP processing network. This enables the creation of distributed systems. So, the FBP network concept can spread to and harness the computing power of multiple machines. Using the ```load-balancer``` component, a front-end can forward requests to one of multiple back-end processing networks with the ability to take them offline individually for maintenance or updates.

The downsides of the approach taken by ```flowd```:

1. *More copying.* This is a necessary consequence since many different programming languages are involved, which have different programming models, manage memory differently internally, manage objects and functions and methods totally differently and thus cannot be loaded into each other's memory without requiring a specific ABI, which would lock out several languages and preclude the easy re-use of unmodified existing programs. Therefore they need and require to be the masters of their own address space organization and each component runs as an own process. In order to communicate, data is copied across these process and address space borders. This is done via the reliable central entity in the operating system, the OS *kernel*. Therefore, system calls, CPU ring switches and buffer copying are the required consequence to cross these process borders. For a frame to move from one component to another, it writes into the named pipe, which is moved into a kernel pipe buffer and then read on the other side by the from the other end of the named pipe. Thus the data moves into the kernel and back into the component. Future note: It may be possible to create a modified version, where the inter-process communication is changed from network connections to in-process communication.

If you rather want to do FBP in Go, but prefer an in-process-communicating runtime/library for a single machine, then you might be interested in [goflow](https://github.com/trustmaster/goflow) or [flowbase](https://github.com/flowbase/flowbase). Also check out the FBP runtimes and systems by J. Paul Morrison and *NoFlo* and their compatible runtimes.


## Development aka Hacking on ```flowd```

Running tests:

  ```
  GOPATH=`pwd` go test ./src/github.com/ERnsTL/flowd/...
  ```

Running benchmarks:

  ```
  GOPATH=`pwd` go test -run=BENCHMARKSONLY -bench=. ./src/github.com/ERnsTL/flowd/libflowd/
  ```

Running tests for the JSON FBP network protocol: Follow [the basic instructions](), but initialize with the following

  ```
  fbp-init --name flowd --port 3000 --command "bin/flowd -olc localhost:3000 src/github.com/ERnsTL/flowd/examples/chat-server.fbp" --collection tests
  ```

Use the latest ```node.js``` and ```npm``` from [nodesource](https://www.nodesource.com/), otherwise you may get Websocket errors. The npm package *wscat* is useful for connection testing.
