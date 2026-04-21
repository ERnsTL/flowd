# flowd: *the data must flow*<sup>[1](https://en.wikiquote.org/wiki/Dune_(film)#Others)</sup>

`flowd` is a deterministic, high-performance execution engine for Flow-Based Programming (FBP).

It allows you to build dataflow systems by composing reusable components into explicit graphs.

`flowd` focuses on:

- explicit data movement
- deterministic execution
- backpressure-aware systems
- high-performance in-process communication

`flowd` is not a low-code tool, not a workflow UI, and not a scripting engine.

It is an execution engine.


## What is Flow-Based Programming?

Flow-Based Programming (FBP) models systems as:

- nodes (components) performing transformations
- edges (connections) transporting data
- messages flowing through the system

Instead of writing control flow, you define how data moves.

`flowd` treats this model as a first-class execution system.


## Philosophy

`flowd` is built around explicitness, determinism and composability.

Read the [flowd Manifesto](./MANIFESTO.md) to understand the design principles and goals of this project.

An essay on [Why flowd exists](doc/explanations/why_flowd.md).



## Quickstart

[Install Rust and Cargo](https://rust-lang.org/tools/install/):
```sh
rustup default nightly
```

Build:
```sh
git clone https://github.com/ERnsTL/flowd
cd flowd
cargo build --release
```

Run:
```sh
cargo run --release
```

By default, the runtime listens on:

```
ws://localhost:3569
```

### The Visual Editor

Connect to your local `flowd` runtime.

[Open the online editor](https://app.flowhub.io/#runtime/endpoint?protocol%3Dwebsocket%26address%3Dws%3A%2F%2Flocalhost%3A3569). This loads the management application from a central server, but connects to your local runtime.

You should see a predefined test network.

It should look roughly like this:

![Bildschirmfoto vom 2022-11-19 16-14-57](https://user-images.githubusercontent.com/3127919/202857780-e070ca3f-fffd-41dc-8470-be9e551facc6.png)

You can now:

* create a processing network (graph)
* connect nodes
* start/stop the graph
* observe execution

For how to use the online editor, see the [manual of noflo-ui](https://github.com/noflo/noflo-ui).

### First Application

1. Add components in the UI
2. Connect them via ports
3. Configure via IIPs
4. Start the network
5. Persist graph (`flowd.graph.json`)

Applications are defined as graphs, not code.

`flowd-rs` currently starts from the persisted graph file `flowd.graph.json` in the repository root (if the file is not found, a default test graph is instantiated).

### Runtime Characteristics

`flowd` is designed to run as a self-contained runtime and does not require container orchestration or external services to run.

- single binary deployment
- no external runtime dependencies required
- in-process execution model
- minimal system assumptions

Primary development and testing target:

- Linux (x86_64)

Other platforms may work but are currently not the primary focus.

It can be deployed as a standalone runtime wherever Rust binaries are supported, including bare metal Linux distributions, virtualized, containerized and optimized Unikernels.

See [detailed examples](doc/tutorials/examples.md).

### The network description format

See [FBP format](doc/reference/fbp_format.md)


## Core Concepts

flowd operates on a small set of explicit primitives:

* **Component (Node)**: processes incoming messages
* **Edge**: bounded connection between components
* **Message (IP)**: unit of data
* **Graph**: system definition

There is no hidden behavior:

* no implicit buffering
* no hidden retries
* no invisible parallelism

Everything is explicit.


## Architecture

`flowd` is a Rust runtime with compile-time component integration.

Components are defined and wired through:

1. `flowd.build.toml`
2. `Cargo.toml`
3. `build.rs` code generation (`$OUT_DIR/build_generated.rs`)

At build time:

* components are validated
* compatibility is checked
* runtime wiring is generated

At runtime:

* components are connected in-memory
* packets are transferred in-proc, in-memory via ring buffers with ownership transfer of message buffers
* control and management happens via WebSocket (FBP protocol)

See `/doc/adr*` files for architectural decisions.


## FBP Runtimes

Runtime variants:

- `flowd-rs` (this repository): https://github.com/ERnsTL/flowd
- `flowd-go` (historical/alternate implementation): https://github.com/ERnsTL/flowd-go


## Component Model

Components are integrated at compile time.

To use components:

1. Add crate dependency (local path recommended)
2. Register in `flowd.build.toml`
3. Build

Example:

```toml
[dependencies]
flowd-bla = { path = "components/bla" }

[[components.entry]]
name = "Bla"
crate = "flowd-bla"
struct = "BlaComponent"
```

Compatibility is enforced via:

```toml
[package.metadata.flowd]
compatible = "0.4"
```


## Writing Applications

Applications are graphs of components.

Workflow:

1. Start runtime
2. Use visual editor
3. Compose graph
4. Persist graph
5. Extend with custom components if needed

`flowd` separates:

* execution (runtime)
* application logic (graph)
* components (code)

See [Writing Applications](doc/guides/writing_applications.md).


## Writing Components

Components are Rust crates using `flowd_component_api`.

Key ideas:

* components react to messages
* can have own threads and async behavior
* no shared mutable state
* explicit input/output ports

See [Writing Components](doc/guides/writing_components.md).


## Features and Current Status

flowd is currently **alpha**: usable and fairly optimized, but not all planned features are present yet. It is not production-ready for critical operations and APIs may change without notice.

Implemented:

* in-memory runtime
* compile-time component integration
* bounded connections (backpressure)
* graph persistence
* WebSocket control (FBP protocol)

Planned:

* security (TLS, auth)
* tracing & observability
* multi-graph runtime
* distributed setups
* checkpointing

See:

* [detailed feature list](doc/reference/features.md)
* [GitHub issues](https://github.com/ERnsTL/flowd/issues)
* [README_WIP](README_WIP.md)
* For a roadmap, see [milestones on Github](https://github.com/ERnsTL/flowd/milestones) and points mentioned in [README_WIP.md](README_WIP.md).

### Included Components

see [Components Reference](doc/reference/components.md)


## Development

Run all tests:
```sh
cargo test --workspace
```

Run tests only for flowd itself (main crate):

```sh
cargo test -p flowd-rs
```

Run benchmarks (if present):

```sh
cargo bench --workspace
```

See [Performance Testing](doc/engineering/performance_testing.md)


## Contributing

1. Open or discuss an issue
2. Propose an ADR, see [the ADR template](doc/adr/adr-000-guidelines.md)
3. Submit PR

See:

* [CONTRIBUTING.md](CONTRIBUTING.md)


## License

GNU LGPLv3+ (see [LICENSE](LICENSE))


## Commercial Support

Professional services around `flowd` are available, including:

* Support & SLA
* Managed runtime
* Solution-as-a-Service
* Consulting

Contact:

[https://www.summitsolutions.at/contactus](https://www.summitsolutions.at/contactus) or book a [strategy meeting](https://www.summitsolutions.at/appointment/1).

For details, see [commercial options](doc/business/commercial.md).


## Further Documentation

* `/doc/`
* [MANIFESTO.md](MANIFESTO.md)
* ADRs in `doc/adr/`
* [Historic background (flowd-go)](https://github.com/ERnsTL/flowd-go/README.md#further-documentation)
* [The FBP paradigm on J. Paul Morrison's website](http://www.jpaulmorrison.com/fbp/)


## Community

* [Discord invitation](https://discord.gg/YBQj6UsD5H) for the "Flow-based Programming" server
* [Reddit sub-reddit](https://www.reddit.com/r/DataflowProgramming/) for FBP and dataflow programming together
* [Google group](https://groups.google.com/forum/#!forum/flow-based-programming)

