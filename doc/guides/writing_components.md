## Writing Components

For `flowd-rs`, implement components as Rust crates under `components/*` using `flowd_component_api`.

Register components through:
- `Cargo.toml` dependency
- `flowd.build.toml` component entry

See:
- `doc/writing-components.md`
- compile-time integration model section below

For details, see [the compile-time integration model](../reference/compile-time_integration.md).


## Inbox

* TODO categories of components:
  * input - a data source from outside the FBP network; ETL converting source data into FBP object structures
  * processing - in FBP object structures or other FBP-native formats
  * output - output or flowing back to the input components' inports; converting
  * above is a fractal / recursive concept on the level of components, component groups, subnetworks, FBP networks etc. where components have inports, logic and outports.
* TODO common component structures, what to use as template
  * in-FBP processing
  * event handler sub-thread; usually just one needed
  * signalling that thread
  * reading list of inputs
  * TODO using sub-threads
  * TODO how to handle shutdown
  * having to start something in a tokio worker thread - telegram, matrix components as template
