# ADR-0005: State Model, Snapshotting, and Checkpointing Strategy

Status: Accepted
Date: 2026-02-16


## Context

Flowd is designed as a deterministic, modular dataflow runtime that must support:

* Long-lived, stateful nodes
* Dynamic graph mutation (Control Plane)
* Reliable execution under failure
* High performance (low latency, minimal overhead)

From previous ADRs:

* ADR-0002 defines execution and scheduling
* ADR-0003 defines the message model
* ADR-0004 defines backpressure, delivery semantics, and persistent edges

A key requirement emerges:

> **State must be manageable, recoverable, and consistent — without introducing excessive complexity.**

---

## Problem Statement

We must define:

1. How node-local state is represented
2. How state can be persisted
3. How recovery works after failure
4. How to balance:

   * correctness
   * performance
   * complexity

---

## Key Constraints

* Avoid global coordination wherever possible
* Avoid Flink-style distributed checkpoints
* Maintain deterministic behavior where feasible
* Support:

  * linear pipelines
  * fan-out graphs
  * dynamic graphs (MUD scenario)
* Integrate cleanly with:

  * ACK model (ADR-0004)
  * Edge abstraction (persistent vs in-memory)

---

## Decision

Flowd adopts a **node-centric, opt-in snapshot model** with:

1. **Node-local state ownership**
2. **Explicit snapshot()/restore() API**
3. **Optional checkpoint orchestration (local, not global)**
4. **Loose coupling between state and transport**
5. **At-least-once recovery semantics**

---

## Core Model

---

### 1. Node-Local State

Each node owns its state.

Example:

```rust
struct NodeState {
    counter: u64,
    cache: HashMap<Key, Value>,
}
```

Properties:

* isolated (no shared mutable state)
* not directly accessible by other nodes
* lifecycle-bound to node instance

---

### 2. State Characteristics

State may be:

| Type      | Description                 |
| --------- | --------------------------- |
| ephemeral | lost on restart             |
| durable   | persisted via snapshots     |
| derived   | reconstructible from inputs |

---

### 3. Snapshot Interface

Nodes implement:

```rust
trait StatefulNode {
    fn snapshot(&self) -> StateSnapshot;
    fn restore(snapshot: StateSnapshot) -> Self;
}
```

Properties:

* explicit
* controlled by node author
* format is opaque to runtime

---

### 4. Snapshot Granularity

Snapshots are:

* node-local
* independent
* not globally synchronized

---

### 5. Checkpoint Model

Flowd uses:

> **Local, asynchronous checkpointing**

Mechanism:

* runtime may trigger snapshot()
* node decides when/how to produce consistent snapshot
* snapshots stored externally (disk, DB, etc.)

---

### 6. Recovery Model

On restart:

1. Node instance is recreated
2. restore(snapshot) is called
3. edges replay messages (via ACK model)

---

### 7. Interaction with Delivery Semantics

Because:

* delivery is at-least-once
* messages may be replayed

Nodes must be:

> **idempotent OR replay-safe**

---

## Important Clarification (Vec<u8> Insight)

A key insight from previous discussions:

> Moving Vec<u8> is cheap — but real-world usage patterns often force copies.

Specifically:

* fan-out
* transformation
* reuse across multiple consumers

Therefore:

> **State + message model must support reuse without forcing copies**

Implication:

* snapshotting should not rely on copying large buffers
* prefer:

  * Arc-based sharing
  * reference-based designs (where applicable)

---

## Rationale

---

### Why Node-Centric State?

Advantages:

* aligns with Actor model
* avoids shared memory complexity
* enables modular reasoning
* supports dynamic graphs (MUD)

---

### Why Explicit Snapshot API?

Avoids:

* hidden serialization costs
* runtime guessing state structure
* unnecessary copying

Gives control to:

* component authors
* domain-specific optimizations

---

### Why No Global Checkpointing?

Global checkpointing (Flink-style) requires:

* coordination barriers
* distributed synchronization
* consistent snapshot across nodes

Problems:

* high complexity
* performance overhead
* brittle under dynamic graphs

Decision:

> **Not required for >90% of use cases**

---

### Why Local Checkpoints?

Benefits:

* simple
* composable
* works with dynamic topology
* sufficient for:

  * crash recovery
  * most real-world pipelines

---

## Alternatives Considered

---

### Alternative 1: No State (Stateless Only)

**Pros:**

* simple
* easy reasoning

**Cons:**

* cannot support:

  * aggregation
  * sessions
  * agents
  * MUD

**Decision:**
Rejected

---

### Alternative 2: Global Distributed Checkpointing (Flink)

**Pros:**

* strong guarantees
* exactly-once possible

**Cons:**

* complex coordination
* performance overhead
* difficult with dynamic graphs

**Decision:**
Rejected

---

### Alternative 3: External State Store (Centralized)

Example:

* Redis
* RocksDB cluster

**Pros:**

* durability
* shared state

**Cons:**

* network overhead
* coupling
* scalability bottlenecks
* breaks isolation

**Decision:**
Rejected (as default)

---

### Alternative 4: Automatic State Capture

Runtime inspects memory and snapshots automatically.

**Pros:**

* easy for developers

**Cons:**

* unpredictable
* inefficient
* language/runtime dependent
* hard to debug

**Decision:**
Rejected

---

### Alternative 5: Immutable Event Sourcing Only

**Pros:**

* full replay
* no explicit snapshots

**Cons:**

* replay cost grows over time
* slow recovery
* complex for users

**Decision:**
Rejected as sole model (can be combined)

---

## Consequences

---

### Positive

* simple mental model
* supports wide range of systems
* works with dynamic topology
* avoids global coordination
* high performance

---

### Negative

* weaker guarantees than Flink
* requires careful node design
* replay may duplicate effects

---

### Trade-offs

| Property    | Decision      |
| ----------- | ------------- |
| Consistency | medium        |
| Performance | high          |
| Complexity  | low–medium    |
| Flexibility | very high     |
| Guarantees  | at-least-once |

---

## Operational Implications

Operators must understand:

* snapshot frequency impacts recovery time
* large state → slower snapshot
* replay may cause duplicates

---

## Interaction with Other ADRs

* ADR-0004:

  * persistent edges provide replay capability
* ADR-0003:

  * message model affects replay behavior
* ADR-0002:

  * scheduler must handle restored nodes correctly

---

## Special Consideration: MUD / Dynamic Systems

Dynamic systems require:

* nodes appearing/disappearing
* state tied to entity lifecycle

This model supports:

* per-node lifecycle state
* independent recovery

This is critical because:

> MUD combines state, interaction, and dynamic topology simultaneously

---

## Key Insight

> **State is local. Consistency is eventual. Recovery is replay-driven.**

---

## What This Enables

* Actor-like systems
* stream processing (without full Flink complexity)
* automation pipelines
* AI agent systems
* dynamic simulations

---

## What This Explicitly Avoids

* global synchronization
* exactly-once semantics
* centralized state coordination

---

## Open Questions

* Should snapshot frequency be runtime-controlled or node-controlled?
* Should incremental snapshots be supported?
* How to optimize large state serialization?
* Should edge snapshots be coordinated with node snapshots?

---

---

## Klare Einordnung (wichtig)

Das hier ist ein **sehr sauberer Cut gegenüber Flink**:

* Du hast:

  * ✔ State
  * ✔ Recovery
  * ✔ Replay
* Aber bewusst NICHT:

  * ❌ global checkpoint barriers
  * ❌ exactly-once

Und genau das ist richtig für dein Ziel:

> **>90% Use Cases abdecken ohne 10x Komplexität**

---

## Wichtigster Satz aus dem ADR

> **State is local. Consistency is eventual. Recovery is replay-driven.**

Wenn du das konsequent durchziehst, bekommst du:

* NiFi-ähnliche Robustheit
* Teile von Flink, die 90% der Use Cases abdecken - leichtgewichtig statt exactly-once auf Biegen und Brechen
* Actor-System-Fähigkeiten
* UND dein MUD

---

Wenn du weitermachen willst, die nächsten logischen ADRs wären:

👉 ADR-0006: **Control Plane & Graph Mutation (das MUD-Herzstück)**
👉 ADR-0007: **Distribution Model (Single Node → Cluster)**

Das sind die Punkte, wo dein System entweder **gewöhnlich** oder **einzigartig** wird.
