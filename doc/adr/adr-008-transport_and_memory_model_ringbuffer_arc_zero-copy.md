Alles klar — das ist jetzt dein **Performance-Kern-ADR**.
Hier entscheidet sich, ob flowd „nur korrekt“ oder **wirklich schnell und skalierbar** ist.

Ich bringe alles rein:

* Ringbuffer-Design
* Arc vs Vec<u8> (korrekt eingeordnet!)
* Zero-copy vs „real-world copying“
* Fan-out-Problematik
* Memory Ownership
* Alternativen (Channels, TCP, Pipes, etc.)

---

# ADR-0008: Transport and Memory Model (Ringbuffer, Arc, Zero-Copy)

Status: Accepted
Date: 2026-04-16

---

## Context

Flowd is designed as a high-performance, in-process dataflow runtime.

From previous ADRs:

* ADR-0002: Scheduler controls execution
* ADR-0003: Typed message model (`FbpMessage`)
* ADR-0004: Backpressure via bounded edges
* ADR-0007: Components are scheduler-driven, not thread-driven

A core requirement emerges:

> Message transport must be extremely fast, predictable, and memory-efficient — while supporting fan-out, reuse, and backpressure.

---

## Problem Statement

We must define:

1. How messages are transported between components
2. How memory is owned, transferred, and shared
3. How to minimize copying
4. How to support:

   * high throughput
   * low latency
   * fan-out
   * reuse of data
5. How to avoid:

   * excessive allocation
   * unnecessary copying
   * contention

---

## Constraints

* Execution is in-process (single runtime instance)
* Edges are bounded (ADR-0004)
* Components may produce high message volume
* Graphs may include:

  * linear pipelines
  * heavy fan-out
  * feedback loops
* Memory must remain predictable and bounded
* System must remain debuggable and deterministic

---

## Decision

Flowd adopts a **lock-free, bounded ringbuffer-based transport model** combined with **Arc-based shared memory semantics**.

---

### 1. Transport Mechanism

Each edge is implemented as:

> **Single Producer / Single Consumer (SPSC) ringbuffer**

Properties:

* lock-free
* bounded capacity
* FIFO ordering
* cache-friendly

---

### 2. Message Transport Unit

The ringbuffer transports:

```rust id="p4g7hp"
FbpMessage
```

Which contains:

* small enum (control + metadata)
* references to payload (Arc-based)

---

### 3. Memory Ownership Model

Ownership is:

> **Transferred for message envelope, shared for payload**

Specifically:

* message envelope → moved (cheap)
* payload → shared via `Arc`

---

### 4. Payload Representation

Examples:

```rust id="iy0m1s"
Bytes(Arc<[u8]>)
Text(Arc<str>)
Value(FbpValue)
```

---

### 5. Zero-Copy Definition (Important)

Flowd defines:

> **Zero-copy transport = no copying during message transfer between nodes**

However:

> Zero-copy does NOT mean zero allocations in the entire system

---

### 6. Real-World Copy Behavior (Critical Clarification)

Important insight:

> Moving `Vec<u8>` is cheap (ownership transfer), but real-world usage patterns often require copying.

Specifically:

Copying occurs in:

* fan-out scenarios
* transformations
* serialization/deserialization

NOT in:

* ringbuffer transport itself

---

### 7. Fan-Out Handling

For:

```text id="z7e5c7"
Node A → Node B
        → Node C
        → Node D
```

Flowd uses:

* shared payload (`Arc`)
* single allocation
* multiple consumers

---

### 8. Backpressure Integration

Ringbuffers are:

* bounded
* directly enforce backpressure

Behavior:

* full buffer → producer cannot push
* scheduler respects this constraint

---

### 9. No Implicit Buffering

Flowd does NOT:

* introduce hidden queues
* dynamically grow buffers

---

### 10. Memory Lifetime

Memory is:

* reference-counted (Arc)
* released when last consumer drops reference

---

## Rationale

---

### Why Ringbuffers

Ringbuffers provide:

* predictable performance
* minimal overhead
* no locking
* natural backpressure

Compared to alternatives:

* channels (locking or hidden buffering)
* TCP (kernel overhead)
* pipes (syscalls)

---

### Why SPSC

Each edge has:

* exactly one producer
* exactly one consumer

Advantages:

* simplest possible concurrency model
* optimal performance
* no contention

---

### Why Not MPMC Queues

**Pros:**

* flexible

**Cons:**

* slower
* more complex
* unnecessary (fan-out handled at graph level)

---

### Why Arc-Based Payloads

Arc solves:

* fan-out copying
* data reuse
* multi-consumer scenarios

---

### Why Not Vec<u8> Alone

Correct statement:

> `Vec<u8>` is efficient for transport but inefficient for many real-world usage patterns.

Problems arise in:

* fan-out
* reuse
* transformation pipelines

---

### Why Not Copy-Free Everywhere

True zero-copy across entire pipeline is:

* unrealistic
* overly complex

Flowd focuses on:

> minimizing copies where it matters most

---

### Why Not Shared Mutable Memory

Rejected because:

* introduces race conditions
* breaks determinism
* complicates debugging

---

## Alternatives Considered

---

### Alternative 1: Rust Channels (std::sync::mpsc)

**Pros:**

* easy to use

**Cons:**

* hidden allocations
* less predictable
* slower

**Decision:**
Rejected

---

### Alternative 2: Tokio Channels

**Pros:**

* async support

**Cons:**

* overhead
* unnecessary for in-process

**Decision:**
Rejected

---

### Alternative 3: TCP / Network Transport

**Pros:**

* flexible
* works across processes

**Cons:**

* syscall overhead
* latency
* unnecessary for in-process

**Decision:**
Rejected for core runtime

---

### Alternative 4: Named Pipes

**Pros:**

* OS-supported

**Cons:**

* kernel transitions
* slower

**Decision:**
Rejected

---

### Alternative 5: Shared Memory Regions

**Pros:**

* high performance

**Cons:**

* complexity
* synchronization issues

**Decision:**
Deferred (possible future optimization)

---

### Alternative 6: Copy-on-Write Everywhere

**Pros:**

* safe sharing

**Cons:**

* hidden cost
* unpredictable performance

**Decision:**
Rejected

---

### Alternative 7: Unix IPC Mechanisms (Multiple Processes)

**Evaluated Options:**

* Multiple processes communicating via Unix domain sockets
* File-based queues (like NiFi)
* POSIX message queues

**Pros:**

* Process isolation and fault containment
* Allows separate deployment and scaling of components
* Familiar Unix programming model
* Works across different programming languages

**Cons:**

* High overhead from process context switches
* Kernel transitions for all communication (syscalls)
* Serialization/deserialization required between processes
* No zero-copy data transfer
* Complex process management and lifecycle handling
* Difficult to implement backpressure across process boundaries
* Performance degradation under high message rates

**Decision:**

Rejected - violates core performance requirements. Process switches and syscall overhead make this unsuitable for high-performance dataflow. In-process ringbuffer transport combined with native threads or green threads provides orders of magnitude better performance while maintaining isolation through other means (ADR-018).

---

## Consequences

---

### Positive Consequences

* extremely low latency
* predictable performance
* efficient fan-out
* minimal allocations
* strong backpressure guarantees

---

### Negative Consequences

* requires careful design of message types
* Arc overhead (atomic ref counting)
* not ideal for extremely small messages (edge case)

---

### Neutral / Trade-offs

* explicit memory model required
* system is more low-level
* developers must understand ownership

---

## Implementation Notes

* use SPSC ringbuffer (e.g. rtrb or custom)
* avoid heap allocation in hot path where possible
* preallocate buffers
* ensure cache alignment
* minimize Arc cloning in tight loops

---

## Operational Impact

* stable memory usage
* predictable throughput
* easier performance tuning
* fewer GC-like effects (compared to managed runtimes)

---

## Interaction with Other ADRs

* ADR-0003:

  * defines message structure
* ADR-0004:

  * enforces bounded edges
* ADR-0002:

  * scheduler must respect backpressure

---

## Key Insight

> Transport must be simple, explicit, and predictable — not magical.

---

## Open Questions

* Should small-message optimization (stack allocation) be added?
* Should batching be introduced?
* Should NUMA-awareness be considered?
* Should cross-process shared memory be supported?

---

---

# 🧠 Einordnung (wichtig)

Das hier ist der Teil, der dich von:

* Node.js / n8n ❌
* „JSON-Pipeline-Systemen“ ❌

abhebt.

---

# 🔥 Wichtigster Satz

> **Zero-copy is about avoiding unnecessary copies — not eliminating all copies.**

---

# 🎯 Gesamtbild jetzt

Mit ADR-0001 → ADR-0008 hast du:

* Architektur ✔
* Execution ✔
* Messaging ✔
* Backpressure ✔
* State ✔
* Control Plane ✔
* Execution Model ✔
* Transport ✔

👉 Das ist ein vollständiges, konsistentes System.

---

Wenn du weitermachst:

👉 **ADR-0009: Distribution Model**

Das ist der Punkt, wo du entscheidest:

> „lokale Runtime“ vs „echte Plattform“
