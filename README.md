# flowd

> This is pre-alpha software. Far from all features currently present.

Wire up components (programs) written in different programming languages, using the best features and available libraries of each.

Make them communicate in a network of components.

Build a *data factory* in which components transform the passed data frames around to produce a uselful output.

Components naturally make use of all available processor cores.

A component network can span multiple machines, lending itself for use in distributed systems. Load balancing and routing are planned features.

Use available off-the-shelf components where you can. Grow a collection of specialized components and *reuse* them for the next and next project of yours.

Rather than rewriting code anew for each project, you become more and more efficient with regards to *human time* spent on development.

## Installation

```
GOPATH=`pwd` go get -u github.com/ERnsTL/flowd
```

## Examples

WIP NOTE: Some places in the code and examples currently use ```unixpacket```, which is only available on Linux. If you run on OSX, you may have to change this in the code and network from ```unixpacket://``` to ```unix://```. For Windows, you may have to change to ```tcp``` or ```udp```. There is an [information page](https://groups.google.com/forum/?_escaped_fragment_=topic/golang-nuts/Rmxxba5rb8Y#!topic/golang-nuts/Rmxxba5rb8Y), which explains the differences a bit and how Go handles the different network types.

WIP NOTE: Some places in the code and examples currently use *abstract* Unix domain sockets starting with ```@```. These are not path-bound and available only on Linux. If you run on OSX, you may have to change this in the code and network to a path-bound name without the ```@```. For Windows, you may have to change ot ```tcp``` or ```udp```.

### Example 1

In this example, a simple processing pipeline is run using the network orchestrator / runtime, ```flowd```.

1. Run some receiver of the pipeline output:

  ```
  bin/unix2stdout @flowd/print1/in
  ```

  This should listen for a connection.

1. Run the processing network, a simple pipeline in this case. You can give the name as argument or pipe it in.

  ```
  bin/flowd -debug -launch bin/launch -in unixpacket://@flowd/network1/in#NETIN -out unixpacket://@flowd/print1/in#NETOUT src/github.com/ERnsTL/flowd/examples/example-network.fbp
  ```

  or

  ```
  bin/flowd -debug -launch bin/launch -in unixpacket://@flowd/network1/in#NETIN -out unixpacket://@flowd/print1/in#NETOUT < src/github.com/ERnsTL/flowd/examples/example-network.fbp
  ```

  This should start up the network and display that all ports are connected and the network *inport* listening.

  You can find out more about the ```.fbp``` network description grammar here:

  * [J. Paul Morrison's FBP book](http://www.jpaulmorrison.com/fbp/notation.shtml)
  * [his .fbp parser source](https://github.com/jpaulm/parsefbp)
  * [the .fbp variant used by NoFlo](https://github.com/flowbased/fbp#readme).
  * [a parser based on the NoFlo definition](https://github.com/oleksandr/fbp) written in Go which ```flowd``` currently re-uses

1. Send in some data for processing:

  ```
  cat src/github.com/ERnsTL/flowd/examples/example-list.json | bin/stdin2frame -bodytype FilterMessage -content-type application/json | bin/stdin2unix @flowd/network1/in
  ```

  This connects to the network and sends it some *framed* JSON test data to filter.

  Switching back to the data sink, you should now see the input data filtered on the Name attribute.


### Example 2

The same can also be done manually using the ```launch``` program. The orchestrator / runtime also calls this binary behind the scenes.

You have to manually set up the components in reverse order (sink to source). This example is the same as example 1. It also uses *message / IP framing*. This allows the use of multiple and named *ports* on the input and output side and the creation of complex, non-linear processing networks. But it is also possible to set ```launch``` into unframed mode and work with streams instead of *messages / IPs*.

The data flow for this example as follows:

```
data file -> stdin -> frame -> stdout/in -> Unix/TCP/UDP -> stdin -> component -> stdout -> Unix/TCP/UDP -> stdout -> display
```

1. Run a sink:

  ```
  bin/unix2stdout @flowd/print1/in
  ```

  This should listen for the results.

1. Run an example copy component:

  ```
  bin/launch -in unixpacket://@flowd/copy/in#in -out unixpacket://@flowd/print1/in#out1 bin/copy out1
  ```

1. Run a processing component:

  ```
  bin/launch -in unixpacket://@flowd/network1/in#in -out unixpacket://@flowd/copy/in#OUT bin/filter-records-framed
  ```

  This should run a simple filtering component, waiting for connection on its *input endpoint*.


1. Run a simple data source:

  ```
  cat src/github.com/ERnsTL/flowd/examples/example-list.json | bin/stdin2frame -bodytype FilterMessage -content-type application/json | bin/stdin2unix @flowd/network1/in
  ```

  This connects to the network and sends it some *framed* JSON test data to filter.

  Switching back to the data sink, you should now see the input data filtered on the Name attribute.

### Example 3

You could also extend the above example 2 to copy to multiple *output endpoints*:

```
bin/launch -in unix://@flowd/copy/in#in -out unix://@flowd/print1/in#out1 -out unix://@flowd/print2/in#out2 bin/copy out1 out2
```

... and also start a second printer component accordingly. This would result in a Y-split in the network.

### Example 4

Start a simple TCP echo server using the following command:

```
bin/flowd -debug -launch bin/launch src/github.com/ERnsTL/flowd/examples/example-echo-server.fbp
```

This should parse the network definition file, start up all ```launch``` instances, all components, connect all ports between components, deliver configuration information (*IIPs*) to the components and finally, start the listen socket.

Then connect to it using, for example:

```
nc -v localhost 4000
```

When you connect, you should see a message that it has accepted your connection. When you send data, you should see it sent to an intermediary copy component and further back to ```tcp-server```'s response port and back out via TCP to your client.

The data flow is as follows:

```
TCP in -> tcp-server OUT port -> copy IN port -> copy OUT port -> tcp-server IN port (responses) -> TCP out
```

Also, an *initial information packet*/frame is sent to the ```CONF``` input port of the ```tcp-server``` component:

```
'localhost:4000' -> CONF tcp-server
```

This is the first packet/frame sent to this component. It usually contains configuration information and is used to *parametrize* this component's behavior.

## Visualization Example

*flowd* can export the network graph structure into *GraphViz* format for visualization.

The following commands will export a network to STDOUT, convert it to a PNG raster image, view it and clean up:

```
bin/flowd -graph src/github.com/ERnsTL/flowd/examples/example.fbp | dot -O -Kdot -Tpng && eog noname.gv.png ; rm noname.gv.png
```

## Architecture

> This describes the current pre-alpha state. Far from all of the target architecture is currently present.

At this stage, all components are basically normal programs or scripts, which do not have to be specially modified or little modified to be used in a flowd network.

All components are each started by an instance of the ```launch``` program. It manages the message framing, if requested, message routing and handles all the network connections with other components.

A component can have multiple input and output ports. If message framing is used, then multiple ports are possible and they can be named. Without message framing, currently only one input and output port is possible, but on the other hand, the program does not have to be specially modified.

A component communicates with the outside world simply using standard input and output. Over these, it receives multiplexed input frames and can send frames to its named ports and thus to other components; the demultiplexing resp. routing is done by its ```launch``` parent process.

The framing format is a simple text-based format very similar to an HTTP header. It can can easily be implemented in any programming language and is easy to extend without being bound to any currently-trendy data format. It contains basic information on:

* Over which port did this frame come in? Over which port shall this be sent out to another component?
* Is this a control frame or a data frame?
* If data frame, what is the data type resp. class name resp. message type in the frame body? This is user-defined.
* What is the MIME type resp. content type of the frame body? For example JSON, plain text, XML, Protobuf, Msgpack, any other binary formats or whatever.
* What is the content length resp. frame body length?
* The frame body is a free-form byte array, so you can put in whatever you want.
* Header fields are extensible to convey application-specific meta data.

Using several components, a network can be built. It is like a graph of components or workers in a data factory doing one step in the processing. The application developer connects the output ports to other components' input ports and parameterizes the components. Most of the components will be off-the-shelf ones, though usually a few have to be written for the specific application project. In this fashion, the application is built.

## Writing Components

TODO

## Writing Applications

TODO

Three stages usually:
1. read and packetize
2. filter and transform
3. assemble packets and output

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

1. *More copying.* This is a neccessary consequence since different programming languages are involved, which have different programming models, manage memory differently, manage objects and functions and methods totally differently. Therefore they need and require to be the masters of their own address space organization and each component runs as an own process. In order to communicate, data copying across these process and address space borders is neccessary. This is done via the reliable central entity in the operating system, the OS *kernel*. Therefore, system calls, CPU ring switches and buffer copying are the required consequence to cross these process borders.

  Future note: It may be possible to create a production-mode version, where the inter-process communication is changed from network connections to in-process communication, which would reduce the amount of data copying.

If you rather want to do FBP in Go, but prefer an in-process-communicating runtime/library for a single machine, then you might be interested in [goflow](https://github.com/trustmaster/goflow). Also check out the FBP runtimes and systems by J. Paul Morrison and *NoFlo* and their compatible runtimes.

## Features

* TCP endpoints
* Unix domain endpoints (abstract and path-based)
* TCP output endpoints try again to dial connection, later turn into warnings, later into an error. This makes it possible to start components resp. their ```launch``` instances in any order.
* TCP input endpoints with fixed port number will listen again for another connection so that a new component can submit input data or, if connection is lost, it can be resumed or that the source component can be re-launched.
* Optional, but usually used framing format between component and launch and in between launch instances.
* *Initial information packets* (IIPs) for component *parametrization*.
* A few included example components (filter, copy, TCP server).

## Development

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
