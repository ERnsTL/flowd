# ADR-0023: ABI Boundaries and External Component Integration

Status: Proposed
Creation date: 2024-04-18
Date: 2026-04-16


## Context

flowd is implemented in Rust and relies heavily on:

* strong ownership semantics
* zero-cost abstractions
* compile-time guarantees
* efficient message passing

During the design of extensibility mechanisms (plugins, external components), the question arises:

> Can Rust-native objects be passed across ABI boundaries (e.g. C ABI)?


## Problem Statement

We need a way to:

* extend flowd with external components
* integrate non-Rust libraries (e.g. C, C++)
* maintain performance and safety
* avoid unnecessary reimplementation of existing libraries

At the same time, we must respect the limitations of ABI boundaries.


## Constraints

### Rust ABI Characteristics:

* Rust does NOT provide a stable ABI
* Layout of Rust types is NOT guaranteed
* Internal representations may change across compiler versions

### C ABI Characteristics:

* Supports only:

  * primitive types
  * pointers
  * simple structs with defined layout

### Incompatibilities:

The following Rust types are NOT ABI-safe:

* `Vec<T>`
* `String`
* `Arc<T>`
* `Box<T>` (across boundaries)
* trait objects (`dyn Trait`)


## Key Insight

> Crossing an ABI boundary means giving up Rust’s guarantees.


## Observed Problems

### Layout instability:

* Rust structs may change layout
* No compatibility guarantees across builds

### Ownership ambiguity:

* unclear who owns memory
* unclear who is responsible for `drop()`

### Memory safety risks:

* double free
* use-after-free
* invalid pointer dereferencing


## Decision

flowd will:

> Avoid passing Rust-native objects across ABI boundaries.

Instead, the system will:

* keep all core execution and data structures in Rust
* introduce controlled ABI boundaries only where necessary
* enforce safe patterns for external integration


## Architectural Approach

### 1. Pure Rust Core (Preferred Path)

The default and preferred approach is:

* all components written in Rust
* full integration into flowd runtime
* no ABI boundary

Rationale:

* maximum performance
* full safety guarantees
* no impedance mismatch


## 2. Controlled ABI Boundary via LibComponent

A dedicated component type will be introduced:

> **LibComponent**

Purpose:

* enable integration of external (non-Rust) code
* isolate ABI complexity
* keep the rest of the system purely Rust

Use Cases:

* C++ libraries
* C libraries
* platform-specific native code
* functionality not available in Rust


## LibComponent Design

Communication Model:

* C ABI
* opaque pointers
* explicit lifecycle management

Core Pattern:

```c
void* handle;
```

API Shape:

```c
void* create();
void process(void* handle, Message* input, Message* output);
void destroy(void* handle);
```

Characteristics:

* Rust retains control over orchestration
* external code operates behind opaque handles
* no Rust-native types cross the boundary


## Data Exchange Strategy

### Option A: FFI-Safe Structs

```c
struct Message {
    uint8_t* data;
    size_t len;
};
```

### Option B: Serialized Messages

* binary format
* JSON (optional)
* custom protocol

### Rationale:

* avoids layout issues
* avoids ownership ambiguity
* ensures compatibility

## Explicitly Forbidden

The following patterns are NOT allowed:

* passing `Vec<T>` across ABI
* passing `String` across ABI
* sharing `Arc<T>` across ABI
* passing trait objects
* relying on Rust internal layout


## Alternative Considered

### Direct Rust-to-Rust dynamic linking (`cdylib`)

Advantages:

* seemingly natural for Rust

Problems:

* no stable ABI
* requires identical compiler versions
* requires identical build flags
* fragile and non-portable

Conclusion:

> Not suitable for long-term stability


### abi_stable crate

Advantages:

* provides stable Rust ABI layer

Disadvantages:

* adds complexity
* restricts design
* introduces abstraction overhead

Conclusion:

> Considered overkill for current system


## Consequences

Positive:

* clear separation of concerns
* safe integration of external code
* full Rust guarantees preserved internally
* extensibility without compromising core design

Negative:

* additional complexity at ABI boundary
* manual lifecycle management required
* potential performance overhead (serialization)


## Strategic Insight

flowd follows:

> Rust-first architecture with controlled escape hatches.

Interpretation:

* Rust is the default implementation language
* ABI boundaries are exceptional, not standard
* external integration is explicitly isolated

## Design Principle

> Keep the core safe and fast. Isolate unsafe boundaries.


## Summary

* Rust-native objects cannot be safely passed across C ABI
* ABI boundaries introduce loss of guarantees
* flowd avoids ABI crossings by default
* LibComponent provides a controlled integration mechanism
* external code is isolated using opaque pointers and message passing


## Final Statement

> Extensibility is achieved without compromising safety by isolating ABI boundaries rather than embracing them.


## Inbox

Plugin system:
* dlopen c abi rust rust object as parameter - is that possible?
* https://www.reddit.com/r/rust/comments/vvutxu/how_does_rust_handle_sharing_code_as_a_dllso/
* Live-reloading rust, plugin-architecture:  https://fasterthanli.me/articles/so-you-want-to-live-reload-rust#and-now-some-rust
* lazy_static, useful:  https://lib.rs/crates/lazy_static
* libloading:  https://lib.rs/crates/libloading
* abi_stable, the other option instead of dlopen:  https://docs.rs/abi_stable/latest/abi_stable/
  * might work but why not compile directly into single binary
* use cdylib instead of dylib
* Issue for this:  https://github.com/rust-lang/rust/issues/73295
* Calling from plugin into main app:  https://www.reddit.com/r/rust/comments/zaggaj/help_with_having_cdylib_that_gets_loaded_in_c_via/
