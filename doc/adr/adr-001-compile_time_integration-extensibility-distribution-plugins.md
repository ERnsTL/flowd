# ADR-001: Compile-Time Integration of FBP Components via `build.rs`, `flowd.build.toml` and Generated Code

Status: Accepted
Date: 2026-04-15
Supersedes: none
Superseded by: none


## 1. Purpose

This document specifies the architecture and implementation requirements for integrating Flow-Based Programming (FBP) components into the runtime **at compile time**, using:

* `flowd.build.toml` (build configuration file)
* `build.rs` (Cargo build script)
* Generated Rust source (e.g., `build_generated.rs`)
* Static linking via Cargo path dependencies or Git submodules

The goal is to eliminate dynamic plugin systems and ensure:

* Deterministic builds
* Maximum runtime performance
* Compile-time type safety
* No runtime component registration logic duplication
* No manual edits required in multiple source locations when adding/removing components


## 2. Context / Problem Statement

Currently, components must be referenced in multiple places:

1. `use` imports (for component structs)
2. Logging setup (`.add_filter_ignore_str(...)`)
3. `ComponentLibrary::new(vec![ ... ])` metadata registration
4. `graph.start()` component instantiation match statement

This creates duplication, fragility, and human error risk.

We require:

* A **single source of truth** (`flowd.build.toml`)
* Automatic code generation during build
* No manual source edits when components change
* Support for per-component log ignore configuration

## 3. Decision / High-Level Design

### 3.1 Design Principles

* Components are statically compiled.
* `build.rs` reads `flowd.build.toml`.
* A generated Rust file (`build_generated.rs`) contains:

  * All necessary `use` statements
  * Logging filter setup
  * Component metadata registration
  * Component factory mapping
* `main.rs` includes this file using `include!()`.

No string-based source code patching is allowed.


## 4. Build Configuration File

### 4.1 File Name

```
flowd.build.toml
```

### 4.2 Structure

Example:

```toml
[components]

[[components.entry]]
name = "Repeat"
crate = "repeat_component"
struct = "RepeatComponent"
log_ignore = ["hyper::proto::h1", "tower::buffer"]

[[components.entry]]
name = "Drop"
crate = "drop_component"
struct = "DropComponent"
log_ignore = []
```

### 4.3 Fields

| Field      | Type          | Required | Description                            |
| ---------- | ------------- | -------- | -------------------------------------- |
| name       | string        | yes      | Graph-visible component name           |
| crate      | string        | yes      | Rust crate name                        |
| struct     | string        | yes      | Rust struct implementing the component |
| log_ignore | array[string] | no       | List of log prefixes to ignore         |


## 5. build.rs Responsibilities

`build.rs` must:

1. Read `flowd.build.toml`
2. Parse all component entries
3. Generate a file:

```
$OUT_DIR/build_generated.rs
```

4. The generated file must contain:

   * All required `use` statements
   * A function to register logging filters
   * A function to build the component metadata library
   * A factory function mapping component names to constructors

5. Fail compilation if configuration is invalid.


## 6. Generated File Contract

### 6.1 Generated File Name

```
build_generated.rs
```

### 6.2 Must Contain

#### 6.2.1 Imports

```rust
use repeat_component::RepeatComponent;
use drop_component::DropComponent;
```

#### 6.2.2 Logging Setup Function

```rust
pub fn register_component_log_filters(logger: &mut Logger) {
    logger.add_filter_ignore_str("hyper::proto::h1");
    logger.add_filter_ignore_str("tower::buffer");
}
```

Only include entries defined in config.


#### 6.2.3 Component Library Builder

```rust
pub fn build_component_library() -> Arc<RwLock<ComponentLibrary>> {
    Arc::new(RwLock::new(ComponentLibrary::new(vec![
        RepeatComponent::get_metadata(),
        DropComponent::get_metadata(),
    ])))
}
```


#### 6.2.4 Component Factory

```rust
pub fn instantiate_component(name: &str, args: ComponentArgs) -> Option<Box<dyn FbpComponent>> {
    match name {
        "Repeat" => Some(Box::new(RepeatComponent::new(args))),
        "Drop" => Some(Box::new(DropComponent::new(args))),
        _ => None,
    }
}
```

This replaces manual `match component_name.as_str()` logic.


## 7. main.rs Integration

`main.rs` must include:

```rust
include!(concat!(env!("OUT_DIR"), "/build_generated.rs"));
```

And replace manual component logic with:

```rust
register_component_log_filters(&mut logger);

let componentlib = build_component_library();

if let Some(component) = instantiate_component(component_name, args) {
    component.run();
}
```

No manual match statements allowed.


## 8. Why Generated Functions Instead of Inline Code Injection

### Alternative Considered:

* Injecting lines into `main.rs` after markers

### Rejected Because:

* Fragile
* Breaks formatting
* Hard to maintain
* Not Rust-idiomatic
* Violates build determinism

### Decision:

Use generated functions in a generated module.


## 9. Cargo & Component Source Management

Components must be available via:

* Git submodules (preferred)
* Or Cargo path dependencies

Example:

```toml
[dependencies]
repeat_component = { path = "components/repeat_component" }
drop_component = { path = "components/drop_component" }
```

Build script does NOT clone repositories.


## 10. Error Handling Requirements

The build must fail if:

* A component name is duplicated
* A struct name is missing
* The config is malformed
* A crate cannot be resolved

Use `panic!()` in `build.rs` to abort build.


## 11. Non-Goals

* Runtime dynamic loading
* Partial reload of components
* Plugin ABI systems
* Source patching


## 12. Consequences / Resulting Guarantees

After implementation:

* Adding a component requires:

  1. Add Git submodule
  2. Add Cargo dependency
  3. Add entry in `flowd.build.toml`
* No source code edits required
* Build is deterministic
* Component registry and factory are generated
* Logging suppression is declarative


## 13. Architectural Rationale

Compile-time integration ensures:

* Zero runtime indirection
* No plugin ABI instability
* Full type safety
* Maximum optimization by Rust compiler
* Static graph of available components

Dynamic systems were rejected to preserve performance and simplicity.


## 14. Future Extension Points

Optional future enhancements may include:

* Feature flags per component
* Conditional compilation targets
* Component capability validation
* Version pinning metadata



## Implementation plan

* See sub-issues.

### Resulting Architecture After All Tasks

* Components defined only in:
  * `flowd.build.toml`
  * `Cargo.toml`
* All runtime wiring auto-generated
* Deterministic, compile-time integrated system
* No plugin system
* No hard-coded registration logic
