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

WIP NOTE: Some places in the code and examples currently use ```unixpacket```, which is only available on Linux. If you run on OSX, you may have to change this in the code and network from ```unixpacket://``` to ```unix://```. For Windows, you may have to change to ```tcp``` or ```udp```.

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

## Features

* TCP endpoints
* Unix domain endpoints (abstract and path-based)
* TCP output endpoints try again to dial connection, later turn into warnings, later into an error. This makes it possible to start components resp. their ```launch``` instances in any order.
* TCP input endpoints with fixed port number will listen again for another connection so that a new component can submit input data or, if connection is lost, it can be resumed or that the source component can be re-launched.

## Development

Running tests:

  ```
  GOPATH=`pwd` go test ./src/github.com/ERnsTL/flowd/...
  ```

## License

GNU LGPLv3+

## Contributing

1. open an issue or pick an existing one
2. discuss your idea
3. send pull request
4. quality check
5. merged!
