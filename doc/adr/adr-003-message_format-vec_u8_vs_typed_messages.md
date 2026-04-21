# ADR-0003: Message Model (Typed vs Byte-Based Messages)

Status: Accepted
Date: 2026-02-16


## Context

The flowd runtime implements a high-performance, deterministic Flow-Based Programming (FBP) execution model with:

* Ringbuffer-based transport between nodes (SPSC per edge)
* Compile-time integrated components (ADR-0001)
* Scheduler-controlled execution (ADR-0002)
* Explicit backpressure handling
* Potential for fan-out-heavy graph topologies

Initially, the message representation was:

```rust
Vec<u8>
```

This choice has the following properties:

* Simple and universal
* Flexible for arbitrary payloads
* Efficient to move (ownership transfer, no copy during transport)
* Compatible with network and serialization boundaries

However, several issues emerged when evaluating real-world usage patterns:

1. **Lack of semantic structure**

   * No distinction between text, binary, control signals, or structured values
   * Requires ad-hoc parsing in nodes

2. **Increased allocation and copying in practice**

   * Not in transport itself, but in usage patterns (fan-out, transformation, reuse)

3. **Inefficient handling of shared data**

   * Multiple consumers require cloning or re-allocation

4. **No support for control-plane or system messages**

   * No standardized representation for events like EOF, stream boundaries, etc.

5. **Difficult observability and debugging**

   * Raw byte payloads lack introspection capabilities

At the same time, constraints and goals include:

* Zero-copy transport where possible
* High throughput and low latency
* Compatibility with fan-out and multi-consumer scenarios
* Support for both structured and opaque data
* Clear separation between data-plane and control-plane semantics
* Minimal overhead in the core runtime


## Decision

The runtime adopts a **typed message envelope model**, replacing `Vec<u8>` as the universal representation.

### Message Definition

```rust
enum FbpMessage {
    Bytes(Arc<[u8]>),
    Text(Arc<str>),
    Value(FbpValue),
    Control(ControlEvent),
}
```

### Key Properties

1. **Arc-based payload sharing**

   * Enables zero-copy fan-out across multiple consumers

2. **Explicit message typing**

   * Differentiates between raw bytes, text, structured values, and control events

3. **Control-plane integration**

   * Control events (e.g. stream boundaries, lifecycle signals) are first-class

4. **Opaque forwarding supported**

   * Messages can still be passed through without interpretation

5. **Serialization only at system boundaries**

   * No mandatory serialization inside the runtime


## Rationale

### Correct Understanding of `Vec<u8>`

It is important to state precisely:

> Moving a `Vec<u8>` is cheap (ownership transfer), and does not imply copying during transport.

The issue is NOT:

> "Vec<u8> causes copying in transport"

The correct statement is:

> While moving `Vec<u8>` itself is cheap, real-world usage patterns
> (fan-out, transformation, serialization) frequently require additional
> allocations and copying.

### Where the Problem Actually Occurs

#### 1. Fan-out Scenarios

```text
Node A → Node B
        → Node C
        → Node D
```

With `Vec<u8>`:

* Each downstream consumer requires:

  * cloning the buffer OR
  * reallocation + copy

With `Arc<[u8]>`:

* All consumers share the same underlying memory
* No additional copies required

#### 2. Data Reuse

If a message needs to be:

* buffered
* cached
* re-emitted

Then `Vec<u8>` forces:

* ownership transfer or cloning

Whereas `Arc` enables:

* shared immutable reuse

#### 3. Transformation Pipelines

Many nodes perform:

* parsing
* serialization
* encoding changes

Typical pattern:

```text
Vec<u8> → parse → struct → serialize → Vec<u8>
```

This introduces:

* repeated allocations
* repeated copies

A typed model avoids unnecessary re-encoding.

#### 4. Multiple Consumers

In multi-consumer graphs:

* `Vec<u8>` leads to structural inefficiency
* `Arc` provides natural multi-reader semantics

### Where `Vec<u8>` Is Still Appropriate

It is critical to note:

> `Vec<u8>` is NOT inherently bad.

It is ideal for:

* producer nodes
* transformation nodes
* short-lived data
* linear pipelines

The problem arises when:

* fan-out is high
* data is reused
* multiple consumers exist

### Typed Messages Enable Semantic Clarity

The new model introduces:

* explicit distinction between:

  * raw bytes
  * text
  * structured values
  * control signals

This improves:

* readability
* debugging
* correctness
* tooling integration

### Control Messages as First-Class Citizens

The system requires explicit representation of:

* stream boundaries (Start/End)
* lifecycle events
* scheduling signals (optional)

Embedding these into raw bytes is error-prone.

### Alignment with Runtime Architecture

The typed message model aligns with:

* Scheduler-driven execution (ADR-0002)
* Backpressure-aware transport
* Future extensions:

  * stateful nodes
  * checkpointing
  * tracing


## Alternatives Considered

### Alternative 1: Keep `Vec<u8>` Only

**Pros:**

* Simple
* Minimal abstraction
* Works for linear pipelines

**Cons:**

* Poor multi-consumer performance
* No structure or typing
* Forces repeated allocations in practice
* Weak debugging support

**Decision:**
Rejected as universal model

### Alternative 2: Fully Serialized Messages (JSON / CBOR)

**Pros:**

* Standardized format
* Easy interoperability

**Cons:**

* Serialization overhead
* No zero-copy
* Not suitable for in-process execution

**Decision:**
Rejected for core runtime

### Alternative 3: Strongly Typed Generic Messages Only

```rust
Message<T>
```

**Pros:**

* Strong type safety

**Cons:**

* Not flexible enough for heterogeneous graphs
* Difficult dynamic routing
* Complex type erasure requirements

**Decision:**
Rejected in favor of enum-based model

### Alternative 4: Arc<Vec<u8>> Only

**Pros:**

* Solves fan-out copying

**Cons:**

* Still lacks semantic structure
* No control message support

**Decision:**
Rejected as incomplete solution


## Consequences

### Positive Consequences

* Reduced copying in fan-out scenarios
* Improved performance in real-world pipelines
* Clear message semantics
* Better observability and debugging
* Support for control-plane integration
* Future-proof for advanced features (state, checkpointing)

### Negative Consequences

* Slightly increased complexity in message handling
* Additional abstraction layer
* Requires conversion at boundaries (e.g. network IO)

### Neutral / Trade-offs

* Some nodes may still use `Vec<u8>` internally
* Developers must choose appropriate message variant
* System becomes more explicit (less implicit flexibility)


## Implementation Notes

* Ringbuffers transport `FbpMessage` (pointer-sized)
* Payloads stored in `Arc` where sharing is beneficial
* Conversion utilities provided:

  * `Vec<u8>` → `Arc<[u8]>`
  * `String` → `Arc<str>`
* Control events defined as enum (`ControlEvent`)
* Avoid unnecessary conversions between variants


## Operational Impact

* Improved runtime performance under load
* Reduced memory pressure in fan-out graphs
* Better debugging capabilities
* Enables future observability tooling


## Related Decisions

* ADR-0001: Compile-Time Component Integration
* ADR-0002: Runtime Scheduler Model
* ADR-0004: Backpressure & Budgeting (planned)


## Open Questions

* Should `FbpValue` support schema validation?
* Should zero-copy deserialization (e.g. via `serde` borrowing) be supported?
* Should message metadata (timestamps, IDs) be embedded in envelope or separate?

* **FbpValue Design (das wird kritisch für Ergonomie & Performance)**
* **ControlEvent Semantik (Streams, Brackets, Lifecycle sauber definieren)**


## Summary note

* Transport (ringbuffer) was never the problem.
* Usage Pattern is the problem.
