# ADR-0004: Backpressure, Delivery Semantics and Persistent Edge Model

Status: Accepted
Date: 2026-02-16


## Context

Flowd aims to provide a deterministic, high-performance dataflow runtime capable of covering a wide spectrum of systems:

* Automation pipelines (n8n-like)
* Dataflow systems (NiFi-like)
* Stream processing (partial Flink-like)
* Agent-based systems (OpenClaw-like)
* Dynamic, self-modifying systems (MUD scenario)

A core requirement across all these domains is:

> Controlled, observable, and stable data movement under varying load conditions.

Previous discussions and analysis revealed:

* Backpressure is a **fundamental invariant**, not an optional feature
* Existing systems either:

  * lack proper backpressure (n8n, many actor systems)
  * or implement it implicitly and opaquely (Flink, NiFi)
* Reliable delivery and persistence are required for:

  * robustness
  * recovery
  * decoupling

At the same time, constraints include:

* Maintain simplicity and determinism
* Avoid excessive complexity (e.g. distributed global checkpointing)
* Preserve performance (zero-copy where possible)
* Support both local and distributed execution
* Keep a clean separation between:

  * data plane (execution)
  * control plane (graph mutation)


## Decision

Flowd adopts a **layered transport model** consisting of:

1. **Bounded in-memory edges (default)**
2. **Optional persistent edges**
3. **Explicit delivery semantics (ACK-based)**
4. **Backpressure as a first-class invariant across all edge types**


## Core Model

### 1. Bounded Edges (Default)

All edges are:

* bounded
* FIFO queues
* typically implemented as ringbuffers (SPSC)

Properties:

* constant memory usage
* predictable latency
* immediate backpressure propagation

### 2. Backpressure Semantics

Backpressure is defined as:

> The inability of a downstream node to accept additional messages.

Behavior:

* when an edge is full:

  * upstream node cannot push
  * scheduler will not continue pushing work on that edge
* backpressure propagates upstream through the graph

This applies uniformly to:

* in-memory edges
* persistent edges (with additional mechanisms)

### 3. Persistent Edges

Flowd introduces a pluggable edge abstraction:

```text
EdgeType:
  - LocalRingbufferEdge
  - FileEdge
  - DatabaseEdge
  - (future: NetworkEdge, ReticulumEdge)
```

Persistent edges:

* store messages outside process memory
* enable recovery and decoupling
* allow temporal buffering

### 4. Delivery Semantics

Flowd adopts:

> **At-least-once delivery semantics (explicit ACK-based)**

Mechanism:

* message is considered "processed" only after:

  * downstream node acknowledges processing
* until ACK:

  * message remains in edge (or persistent storage)

### 5. Node Responsibility

Nodes are responsible for:

* idempotent processing (recommended)
* explicit acknowledgement of processed messages

### 6. Optional Snapshot-Based Recovery

Flowd supports lightweight checkpointing via:

* node state snapshots
* optional edge state snapshots

No global coordination required.


## Rationale

### Backpressure as a First-Class Invariant

Many systems fail under load because:

* unbounded queues
* implicit buffering
* lack of flow control

Flowd enforces:

> Every edge is bounded → every system has predictable limits

This ensures:

* stability under burst load
* no unbounded memory growth
* explicit system behavior

### Separation of Concerns

Flowd separates:

| Concern       | Responsibility   |
| ------------- | ---------------- |
| Data movement | Edges            |
| Execution     | Scheduler        |
| Reliability   | Edge + ACK model |
| State         | Nodes            |

This avoids:

* monolithic designs (e.g. Flink-style tightly coupled state + transport)

### Persistent Edges as Abstraction (Key Design Choice)

Instead of embedding persistence in nodes:

> Persistence is implemented at the edge level

Advantages:

* decouples computation from storage
* allows mixing edge types
* aligns with NiFi/Kafka-like architectures

### ACK-Based Delivery (Chosen Model)

Chosen over:

* fire-and-forget
* implicit completion

Because:

* explicit control
* supports recovery
* enables correctness reasoning


## Alternatives Considered

### Alternative 1: No Backpressure (Unbounded Queues)

**Pros:**

* simple
* easy implementation

**Cons:**

* memory explosion under load
* unstable systems
* non-deterministic behavior

**Decision:**
Rejected

### Alternative 2: Implicit Backpressure (Scheduler-Only)

**Pros:**

* simpler model

**Cons:**

* hidden coupling
* hard to reason about
* fragile under complex graphs

**Decision:**
Rejected

### Alternative 3: Flink-Style Global Backpressure + Checkpointing

**Pros:**

* strong guarantees
* exactly-once possible

**Cons:**

* extremely complex
* requires distributed coordination
* high overhead
* overkill for target use cases

**Decision:**
Rejected

### Alternative 4: Exactly-Once Delivery

**Pros:**

* strongest correctness guarantees

**Cons:**

* requires:

  * global coordination
  * consistent snapshots
  * replay mechanisms
* significantly increases complexity

**Decision:**
Explicitly out of scope

### Alternative 5: Stateless Nodes Only

**Pros:**

* simpler reasoning

**Cons:**

* cannot support:

  * aggregation
  * agent systems
  * MUD use case

**Decision:**
Rejected


## Consequences

### Positive Consequences

* predictable system behavior under load
* no unbounded memory growth
* explicit flow control
* modular persistence model
* flexible deployment (local ↔ distributed)
* aligns with minimal feature set required for >90% system coverage

### Negative Consequences

* increased complexity in edge implementation
* need for ACK handling
* potential throughput overhead for persistent edges
* requires careful node design (idempotency)

### Trade-offs

| Feature     | Decision         |
| ----------- | ---------------- |
| Simplicity  | moderate         |
| Performance | high (in-memory) |
| Reliability | medium-high      |
| Complexity  | controlled       |
| Guarantees  | at-least-once    |


## Operational Implications

* system operators must:

  * understand backpressure behavior
  * monitor queue sizes
* persistent edges require:

  * storage management
  * cleanup strategies


## Implementation Notes

* Ringbuffer edges:
  * SPSC, lock-free
* Persistent edges:
  * pluggable backend
* ACK handling:
  * message ID required
* scheduler:
  * must respect edge capacity
* metrics:
  * queue depth
  * throughput
  * ACK latency


## Interaction with Other ADRs

* ADR-0003: Message Model (typed messages)
* ADR-0002: Scheduler (must enforce fairness under backpressure)
* ADR-0005 (planned): Checkpointing & State Model


## Key Insight

> Backpressure is not a feature – it is a system invariant.


## Alignment with Target Systems

This design enables:

* n8n-like systems (simple pipelines)
* NiFi-like systems (persistent flows)
* partial Flink-like capabilities (without global coordination)
* Actor-like systems (via node isolation)
* dynamic systems (MUD)

The MUD use case acts as a stress test for:

* dynamic topology
* sustained load
* interaction complexity


## Open Questions

* Should ACK be per-message or batch-based?
* How to handle backpressure across network edges?
* Should priority queues be supported?
* How to integrate with control-plane mutations safely?

* **ACK-Modell konkret (Sequencing, IDs, batching vs single)**
* **PersistentEdge Design (File vs DB vs Stream)**
