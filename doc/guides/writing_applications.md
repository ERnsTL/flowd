## Writing Applications

At the current stage, applications in `flowd-rs` are primarily built by composing components into a graph and managing that graph through the FBP WebSocket protocol (typically via noflo-ui / Flowhub).

Recommended workflow:

1. Start the runtime:
   ```sh
   cargo run --release
   ```
   (or bind explicitly: `cargo run --release -- 0.0.0.0:3569`)

2. Open the visual editor and connect to:
   `ws://localhost:3569`

3. Build your application graph:
   * add components
   * connect ports
   * configure components via IIPs / graph configuration
   * start and stop the network for execution

4. Persist the graph definition (stored as `flowd.graph.json`).

5. Add custom components when needed:
   * implement component crates in Rust (`components/*`)
   * register them via `Cargo.toml` + `flowd.build.toml`
   * rebuild and use them in the graph

Current scope and limitations:

* Single-graph runtime behavior is the current default.
* Runtime is alpha; APIs and behavior may still evolve.
* Some advanced lifecycle features (multi-graph, richer online reconfiguration, full tracing/security model) are still in progress.
