# flowd

> This is pre-alpha software. Far from all features currently present.

Wire up components (programs) written in different programming languages, using the best features and libraries of each.

Make them communicate in a network of components.

Build a *data factory* in which components transform the passed data frames around to produce a uselful output.

Components naturally make use of all available processor cores.

A component network can span multiple machines, lending itself for use in distributed systems. Load balancing and routing are planned as well.

## Examples

Components are currently run in reverse order (sink to source).

The data flow for this example as follows:

```data file -> stdin -> UDP -> stdin -> component -> stdout -> UDP -> stdout -> display```

1. Run a sink:

  ```
  bin/udp2stdout localhost:4000
  ```

1. Run a processing component:

  ```
  bin/launch -in udp4://:0 -out udp4://localhost:4000 bin/filter-records
  ```

1. Run a simple data source:

  ```
  port=`avahi-browse -t -r "_flowd._udp"|grep port|uniq|cut -f2 --delimiter="["|cut -f1 --delimiter="]"`
  echo "found on port $port"
  cat src/github.com/ERnsTL/flowd/examples/example.json | bin/stdin2udp localhost:$port
  ```

## Development

Running tests:

  ```
  GOPATH=`pwd` go test ./src/github.com/ERnsTL/flowd/...
  ```

## License

GNU LGPLv3+

## Contributing

1. open an issue
2. discuss the idea
3. send pull request
4. quality check
5. merged!
