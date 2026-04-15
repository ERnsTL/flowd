### Compile-Time Integration Model

`flowd` integrates components at compile time from:

1. `flowd.build.toml` (component declarations)
2. `Cargo.toml` (crate dependencies)
3. `build.rs` (validation + code generation)

During build, `build.rs` generates `$OUT_DIR/build_generated.rs`, and `main.rs` includes it via `include!()`.
This generated code provides:

* component imports
* component metadata registration
* component factory wiring
* component log filter registration

So when adding/removing components, no manual edits are needed in runtime registration/factory/logging code paths.

Build-time validation fails for:

* malformed `flowd.build.toml`
* duplicate component names
* unresolved crates
* missing component symbols (compile-time)
* missing or invalid `[package.metadata.flowd].compatible`
* flowd/component compatibility mismatch (major+minor)


* Flowd compatibility of components is declared in each component's Cargo.toml via:
  ```toml
  [package.metadata.flowd]
  compatible = "0.4"
  ```
  Build-time compatibility check matches major and minor version between flowd and this value. This field is required for crate-based components.
  Module-based components (`crate = "components::..."`) are legacy/dev-only: disabled by default and only allowed with `--features allow-module-components`.
* Minimal crate-based component setup example:
  ```toml
  # in flowd Cargo.toml
  [dependencies]
  flowd-bla = { path = "components/bla" }

  # in flowd.build.toml
  [[components.entry]]
  name = "Bla"
  crate = "flowd-bla"
  struct = "BlaComponent"

  # in components/bla/Cargo.toml
  [package.metadata.flowd]
  compatible = "0.4"
  ```
* Create a branch for each flowd version, for example named "0.4" so that users can get the latest component version for their flowd version. This way, improvements can be ported back for an older version of flowd, and porting to new version of flowd can be done independently without disturbing component version for older versions of flowd.
