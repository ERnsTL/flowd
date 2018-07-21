# flowd

> This is pre-alpha software. It basically works, but not all of the planned features are currently present. The API is not stable.

Wire up components (programs) written in different programming languages, using the best features and available libraries of each.

Make them communicate in a network of components.

Build a *data factory* in which components transform the passed data frames around to produce a uselful output.

Components naturally make use of all available processor cores.

A component network can span multiple machines, lending itself for use in distributed systems. Routing is available and load balancing is a planned feature.

Use available off-the-shelf components where you can. Grow a collection of specialized components and *reuse* them for the next and next project of yours.

Thus, rather than rewriting code anew for each project, you become more and more efficient with regards to *human time* spent on development.

This is the basic idea of *Flow-based Programming* (FBP), as pioneered by [J. Paul Morrison](http://www.jpaulmorrison.com/fbp/).

The ```flowd``` (for *flow daemon*) is a *runtime environment* for the execution of FBP processing networks, to be defined by a programmer, which then constitutes an *application* or processing system of some kind.

The act of programming is thus shifted from entering strings and lines of tailor-made program source code to a more graphical and potentially visual kind of programming, based on the combination and connection of re-usable *black boxes* working together in a *mappable* processing network resp. application.

You can find out more about this paradigm on J. Paul Morrison's website linked above.


## Features

* Parsing of ```.fbp``` network specifications
* Starting a network of the specified components
* Simple and easy to implement framing format
* Multi-core use resp. parallel processing
* Closing of ports and close detection
* Gracelful shutdown once all data has been processed and all components shut down
* Visualization of the given network in *GraphViz* format
* Display of required components and file dependencies of the given network
* Ability to use a network bridge or protocol client, which uses the transport protocol and serialization format of your choice - kpc, WebSocket,  GRPC, CapnProto, Protobuf, Flatbuffers, JSON, MsgPack, gob, RON, ...
* Sub-networks resp. composite components
* Fast, direct transfer of IPs between components using named pipes (FIFOs); only shared memory would be faster
* Running a processing network with or without ```flowd``` as the orchestrator
* Ability to run a terminal UI component - and then bring it to the web using [gotty](https://github.com/yudai/gotty) :-)

The included example components cover:

* TCP client and server
* Unix domain client and server (abstract and path-based)
* TLS client and server
* SSH client
* simple HTTP server and client
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
* Counter for packets and packet sizes
* Example login prompt and command-line interaction component
* Example terminal UI component sending messages into the network

Planned features:

* Runtime protocol for remote control and online network reconfiguration, enabling real-time visual programming
* Parsing of ```.drw``` network specifications made using [DrawFBP](https://github.com/jpaulm/drawfbp)
* Network discovery of services using Zeroconf / Bonjour
* Load balancing
* Tracing of data packets as they flow through the network
* For more, see the issues list!


## Architecture

> This describes the pre-alpha state.

All components are either normal programs or scripts, which do not have to be specially modified to be used in a ```flowd``` network (wrapped in a ```cmd``` component) or they are programs, which understand the ```flowd``` framing format.

All components are each started by the ```flowd``` program. It manages the message framing, message routing and handles all the network connections with other components.

A component can have multiple input and output ports. Ports are named. Without message framing (wrapped in a ```cmd``` component), input can be passed to an unmodified program and output can be used within the processing framework.

A component communicates with the outside world simply using standard input and output. Over these, it receives multiplexed input frames and can send frames to its named ports and thus to other components; the demultiplexing resp. routing is done by ```flowd``` as the managing parent process.

The framing format is a simple text-based format very similar to an HTTP/1.x header or a MIME message, which is also used for e-mail. Currently, a subset of [STOMP v1.2](https://stomp.github.io/stomp-specification-1.2.html) is used. It can easily be implemented in any programming language, is easy to extend and can carry a frame body in any currently-trendy format be it textual or binary. A frame contains information on:

* Over which port did this frame come in? Over which port shall this be sent out to another component?
* Is this a control frame or a data frame?
* If data frame, what is the data type resp. class name resp. message type in the frame body? This is user-defined.
* What is the MIME type resp. content type of the frame body? For example JSON, plain text, XML, Protobuf, Msgpack, any other binary formats or whatever.
* What is the content length resp. frame body length?
* The frame body is a free-form byte array, so you can put in whatever you want.
* Header fields are extensible to convey application-specific meta data.

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

Also, an *initial information packet* (IIP) is sent to the ```CONF``` input port of the ```tcp-server``` component, as defined in the network specification:

```
'localhost:4000' -> CONF tcp-server
```

This is the first packet/frame sent to this component. It usually contains configuration information and is used to *parametrize* this component's behavior.

You can find out more about the ```.fbp``` network description grammar here:

* [J. Paul Morrison's FBP book](http://www.jpaulmorrison.com/fbp/notation.shtml)
* [his .fbp parser source](https://github.com/jpaulm/parsefbp)
* [the .fbp variant used by NoFlo](https://github.com/flowbased/fbp#readme).
* [a parser based on the NoFlo definition](https://github.com/oleksandr/fbp) written in Go which ```flowd``` currently re-uses

A more complete, parser-exercising example is located in ```examples/example.fbp```.


## Visualization Example

*flowd* can export the network graph structure into *GraphViz* format for visualization.

The following commands will export a network to STDOUT, convert it to a PNG raster image, view it and clean up:

```
bin/flowd -graph src/github.com/ERnsTL/flowd/examples/example.fbp | dot -O -Kdot -Tpng && eog noname.gv.png ; rm noname.gv.png
```


## Writing Components

Decide if your program shall implement the ```flowd``` framing format or be wrapped in a ```cmd``` component.

If wrapped, you can decide for the program to be called for each incoming frame in order to process it or if your program should process a stream of frame bodies. If one-off, the program will receive data on STDIN, which will be closed after the frame body has been delivered; the program can the output a result, which will be forwarded into the FBP network for further processing. It is then expected to either close STDOUT or exit the program. In the one-instance mode, STDIN and STDOUT will remain open; your program will receive data from incoming frame bodies to be processed and any output will be framed by the ```cmd``` component and again forwarded into the network.

Otherwise, implement the simple ```flowd``` framing format, which can be seen in the files [libflowd/framing.go](libflowd/framing.go) and [libflowd/framing_test.go](libflowd/framing_test.go). It is basically STOMP v1.2 as specified with the modifications mentioned there. This can be done using a small library for the programming language of your choice. Your component is expected to open STDIN and will receive a stream of frames to be processed according to purpose. Checking out the ```port``` header field will tell on which input port it came in. Frames of type *data* and *control* are common. Especially important are the IIPs, denoted by their *body type* IIP, which are usually used for component configuration, and the *port closed notifications* of body type *PortClose* as notification that one of your input ports got closed; this is usually the signal that all data has arrived from the preceding component and that it shut down. Components should forward existing headers from the incoming frames/IPs, because downstream connections might lead to a loop back to the sender requiring a header field present for correlation, like for example a TCP connection ID, so keep additional header fields intact. Output frames, if any, are then to be sent to STDOUT. According to the ```port``` header field, the frames from your component will be forwarded to the component which is connected to the other side of the given output port - to be processed, filtered, sorted, stored, transformed and sent out as results to who knows where... That's it - it's up to you!


## Writing Applications

TODO

Three stages usually:
1. read and packetize
2. filter and transform
3. assemble packets and output

TODO modeling the application in terms of what data is relevant and what structure it has, where the data comes from, how it should be transformed and which resuts should be produced (see JPM book).

TODO no conceptual dissonance between design and implementation stages.

TODO straight implementation, almost waterfall-like, less refactorings.

TODO Linear maintenance cost in relation to program size.


## FBP Runtimes

There exist several FBP *runtimes*, which emphasize different aspects of FBP and realize the underlying concept in different ways.

In general, ```flowd``` puts focus on:

1. *Programming language independence.* Being able to easily mix different PLs in one FBP processing network. This enables to combine the strengths of different programming languages and systems into a FBP processing network.

  To this end, ```flowd``` uses the simplest way to communicate, which is common to all programming languages and that is reading and writing to/from standard input and standard output. If it were done differently, then it would become neccessary to create a *libflowd* or similar for interfacing with ```flowd``` and to write bindings to that, for each programming language. But not all PLs can import even a C library, let alone a library written in some other calling convention, because it does not fit their way of computing or internal representations, abstractions etc. and you cannot expect every PL being able to import a library/package written in every other PL. And further, all the bindings to some *libflowd* would have to be maintained as well. So, since this path is not desireable, communication using STDIN and STDOUT was chosen, with STDERR being used for any status output, log messages etc.

  Also, if any complex data format were mandated (Protobuf, XML, JSON, ZeroMQ, MsgPack, etc.) then this would lock out languages where it is not available.

  Since all this is not desireable nor feasible, ```flowd``` cannot introduce any new protocols, any new data formats or require importing any of their libraries or bindings to these. Therefore a very simple, text-and-line-based and even *optional* framing format very similar to HTTP/1.x headers and MIME e-mail heders is used, since strings and newlines are available in every programming language and will most likely be available in times to come - well, unless the world decides not to use character strings any more ;-)

2. *Re-use existing programs.* Every program, even a Unix pipe-based processing chain, which can output results either to a file or STDOUT, can be wrapped and re-used by ```flowd``` in an FBP processing network. Therefore, ```flowd``` can be used to extend the Unix pipe processing paradigm.

3. *Easy to write components.* Open STDIN, read lines until you hit an empty line, parse the header fields, read the frame/IP body accordingly. Do some processing, write out a few lines of text = header lines, write out the body to STDOUT. If the component has anything to report, write it to STDERR. No library to import, no complex data formats, no APIs.

4. *Spreads across multiple cores.* The FBP networks of ```flowd``` - like those of other FBP runtimes and systems - intrinsically spread out to multiple CPUs resp. CPU cores. In the case of other systems it is because they are different threads, in ```flowd``` because they are seperate processes. This enables the saturation of all CPU cores and parallel processing to ensue in an easy way - simply by constructing a network of components, which all just read and write to/from STDIN and STDOUT.

5. *Can spread across multiple machines.* Component ports are forwarded by the ```launch``` program to network connections resp. sockets, not in-memory pipes (though that is of course possible). This enables the creation of distributed systems using the built-in included functionality. So, the FBP network concept can spread to and harness the computing power of multiple machines, all managed from a central ```flowd``` process.

Downsides of the approach taken by ```flowd```:

1. *More copying.* This is a neccessary consequence since different programming languages are involved, which have different programming models, manage memory differently, manage objects and functions and methods totally differently and thus cannot be loaded into each other's memory. Therefore they need and require to be the masters of their own address space organization and each component runs as an own process. In order to communicate, data copying across these process and address space borders is neccessary. This is done via the reliable central entity in the operating system, the OS *kernel*. Therefore, system calls, CPU ring switches and buffer copying are the required consequence to cross these process borders.

  Future note: It may be possible to create a production-mode version, where the inter-process communication is changed from network connections to in-process communication, which would reduce the amount of data copying.

If you rather want to do FBP in Go, but prefer an in-process-communicating runtime/library for a single machine, then you might be interested in [goflow](https://github.com/trustmaster/goflow). Also check out the FBP runtimes and systems by J. Paul Morrison and *NoFlo* and their compatible runtimes.


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


## License

GNU LGPLv3+


## Contributing

1. open an issue or pick an existing one
2. discuss your idea
3. send pull request
4. quality check
5. merged!


## Further documentation

* [design history and rationale](doc/design_history.md)
