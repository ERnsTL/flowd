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
  bin/launch -inframing=false -outframing=false -in udp4://:0#in -out udp4://localhost:4000#out bin/filter-records
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
