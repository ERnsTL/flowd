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

Components are currently run in reverse order (sink to source).

### Example 1

The data flow for this example as follows:

```
data file -> stdin -> UDP -> stdin -> component -> stdout -> UDP -> stdout -> display
```

1. Run a sink:

  ```
  bin/udp2stdout localhost:4000
  ```

  This should listen for the results.

1. Run a processing component:

  ```
  bin/launch -in udp4://:0#in -out udp4://localhost:4000#out -inframing=false -outframing=false bin/filter-records
  ```

  This should run a simple filtering component, publish it on the network using mDNS and output the UDP port it listens on.

1. Run a simple data source:

  ```
  port=`avahi-browse -t -r "_flowd._udp"|grep port|uniq|cut -f2 --delimiter="["|cut -f1 --delimiter="]"`
  echo "found on port $port"
  cat src/github.com/ERnsTL/flowd/examples/example-stream.json | \
  bin/stdin2udp localhost:$port
  ```

  This looks up the component using mDNS, connects to it and sends it some JSON test data to filter.

  Switching back to the data sink, you should now see the input data filtered on the Name attribute.

### Example 2

This example is the same as example 1, but uses message framing. This allows the use of multiple and named ports on the input and output side and the creation of complex, non-linear processing networks.

The data flow for this example as follows:

```
data file -> stdin -> frame -> stdout/in -> UDP -> stdin -> component -> stdout -> UDP -> stdout -> display
```

1. Run a sink:

  ```
  bin/udp2stdout localhost:4000
  ```

  This should listen for the results.

1. Run a processing component:

  ```
  bin/launch -in udp4://:0#in -out udp4://localhost:4000#out bin/filter-records-framed
  ```

  This should run the same filtering component as in example 1, but a version which uses message framing. Again, the component's ```in``` port is published on the network using mDNS and outputs the UDP port it listens on.

1. Run a simple data source:

  ```
  port=`avahi-browse -t -r "_flowd._udp"|grep port|uniq|cut -f2 --delimiter="["|cut -f1 --delimiter="]"`
  echo "found on port $port"
  cat src/github.com/ERnsTL/flowd/examples/example-list.json | \
  bin/stdin2frame -debug -bodytype FilterMessage -content-type "application/json" | \
  bin/stdin2udp localhost:$port
  ```

  This looks up the component using mDNS, connects to it and sends it some framed JSON test data to filter.

  Switching back to the data sink, you should now see the input data filtered on the Name attribute.

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

* TCP output endpoints try again to dial connection, later turn into warnings, later into an error. This makes it possible to start components resp. their ```launch``` instances in any order.
* TCP input endpoints with fixed port number will listen again for another connection so that a new component can submit data or, if connection is lost, it can be resumed.

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
