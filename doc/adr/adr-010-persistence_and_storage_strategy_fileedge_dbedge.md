Alles klar — das ist jetzt die **letzte große „Infra-Schicht“**, die dein System von „schnell“ zu **„produktionsfähig“** hebt.

Ich baue dir ADR-0010 so, dass es:

* sauber auf ADR-0004 (Edges) aufsetzt
* klar von ADR-0005 (State) getrennt ist
* und bewusst **nicht in Flink-/Kafka-Komplexität abrutscht**

---

# ADR-0010: Persistence & Storage Strategy

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd provides a dataflow runtime with:

* Typed message model (ADR-0003)
* Bounded edge transport (ADR-0004)
* Scheduler-based execution (ADR-0002)
* Node-local state and snapshotting (ADR-0005)
* Distribution via subgraphs and network edges (ADR-0009)

So far, persistence is addressed in two places:

1. **State persistence**

   * Node-local snapshots (ADR-0005)

2. **Transport durability**

   * Persistent edges (ADR-0004, conceptual)

However, persistence has not yet been fully defined as a system-wide strategy.

A key requirement emerges:

> The system must support durable message transport and storage — without sacrificing simplicity, performance, or composability.

---

## Problem Statement

We must define:

1. How messages are persisted beyond memory
2. How persistence integrates with edges
3. What guarantees are provided (durability, ordering, replay)
4. How storage backends are abstracted
5. How to avoid:

   * tight coupling to specific databases
   * excessive overhead
   * global coordination

---

## Constraints

* Persistence must integrate with the edge model (ADR-0004)
* Must support at-least-once delivery
* Must not require global synchronization
* Must work in both:

  * single-node
  * distributed setups
* Must remain optional (not all edges require persistence)
* Must preserve backpressure semantics

---

## Decision

Flowd adopts a **pluggable edge-based persistence model** where:

1. Persistence is implemented at the **edge level**
2. Multiple storage backends are supported
3. Persistence is **explicit and configurable per edge**
4. Replay is driven by the **ACK model**
5. No global storage system is required

---

## Core Model

---

### 1. Persistence as Edge Property

Edges can be:

```text id="m9lqvx"
EdgeType:
  - LocalRingbufferEdge (in-memory)
  - FileEdge (local durable)
  - DatabaseEdge (external storage)
  - NetworkEdge (remote transport)
```

---

### 2. Storage Responsibility

Persistence is:

> owned by the edge, not by the node

Nodes remain:

* stateless or stateful (ADR-0005)
* unaware of storage details

---

### 3. Storage Backends

Supported backend categories:

---

#### File-Based Storage

Examples:

* append-only logs
* segment files

Properties:

* simple
* fast
* local durability

---

#### Database Storage

Examples:

* PostgreSQL
* NoSQL systems

Properties:

* durability
* query capabilities
* scalability

---

#### Future Backends

* object storage (S3-like)
* distributed logs
* custom storage engines

---

### 4. Message Persistence Model

Messages are:

* serialized when entering persistent edge
* stored with:

  * message ID
  * payload
  * metadata

---

### 5. ACK-Based Retention

Messages remain stored until:

> acknowledged by downstream consumer

This aligns with ADR-0004.

---

### 6. Replay Mechanism

On restart or failure:

* unacknowledged messages are replayed
* nodes must handle:

  * duplicates
  * reordered delivery (if backend requires)

---

### 7. Ordering Guarantees

Depends on backend:

| Backend      | Ordering Guarantee |
| ------------ | ------------------ |
| Ringbuffer   | strict FIFO        |
| FileEdge     | append-order FIFO  |
| DatabaseEdge | depends on schema  |

---

### 8. Retention Policy

Edges define:

* retention duration
* cleanup strategy
* max storage size

---

### 9. Backpressure Integration

Persistent edges must:

* expose capacity
* enforce limits

Examples:

* disk space limits
* DB queue limits

---

## Rationale

---

### Why Edge-Based Persistence

Separating persistence from nodes:

* simplifies component design
* improves composability
* allows mixing:

  * transient
  * durable flows

---

### Why Not Node-Level Persistence

Embedding persistence in nodes:

* duplicates logic
* reduces flexibility
* increases coupling

---

### Why Not Global Storage System

Systems like Kafka:

* centralize persistence
* introduce dependency

Flowd chooses:

> decentralized persistence via edges

---

### Why Not Mandatory Persistence

Not all pipelines require durability:

* in-memory pipelines are faster
* persistence adds overhead

---

### Why ACK-Based Retention

Advantages:

* simple
* aligns with delivery semantics
* enables replay

---

### Why Not Exactly-Once Storage

Requires:

* coordination
* deduplication
* transactional guarantees

Decision:

> out of scope

---

## Alternatives Considered

---

### Alternative 1: Central Message Broker (Kafka)

**Pros:**

* scalable
* durable

**Cons:**

* external dependency
* complexity
* latency

**Decision:**
Rejected as core model

---

### Alternative 2: Always-Persistent Edges

**Pros:**

* strong durability

**Cons:**

* performance penalty
* unnecessary overhead

**Decision:**
Rejected

---

### Alternative 3: Node-Based Storage

**Pros:**

* flexible

**Cons:**

* inconsistent behavior
* duplicated logic

**Decision:**
Rejected

---

### Alternative 4: In-Memory Only

**Pros:**

* fast

**Cons:**

* no recovery

**Decision:**
Rejected as sole model

---

## Consequences

---

### Positive Consequences

* flexible persistence model
* composable architecture
* supports wide range of use cases
* avoids central bottlenecks
* integrates naturally with existing design

---

### Negative Consequences

* requires backend-specific implementation
* replay handling required in nodes
* storage management complexity

---

### Trade-offs

| Property    | Decision        |
| ----------- | --------------- |
| Flexibility | high            |
| Complexity  | moderate        |
| Performance | high (optional) |
| Durability  | configurable    |

---

## Implementation Notes

* define storage abstraction interface
* implement:

  * FileEdge
  * DatabaseEdge
* define serialization format (ADR-0003)
* implement message ID tracking
* implement cleanup policies

---

## Operational Impact

Operators must manage:

* disk usage
* database capacity
* retention policies

Monitoring required:

* queue sizes
* replay rates
* storage growth

---

## Interaction with Other ADRs

* ADR-0004:

  * defines edge abstraction
* ADR-0005:

  * defines state persistence
* ADR-0009:

  * defines distributed usage

---

## Key Insight

> Persistence belongs to the dataflow edges, not the computation nodes.

---

## What This Enables

* durable pipelines
* replay-based recovery
* hybrid memory + storage systems
* scalable distributed architectures

---

## What This Avoids

* central bottlenecks
* global coordination
* unnecessary complexity

---

## Open Questions

* Should compaction be supported?
* Should batching be implemented?
* Should retention policies be standardized?
* Should tiered storage be supported?

---

---

# 🧠 Einordnung (wichtig)

Das hier ist der Punkt, wo dein System:

> **von „Runtime“ → „Produktionssystem“**

geht.

---

# 🔥 Wichtigster Satz

> **Persistence is a property of the data path, not the computation.**

---

# 🎯 Was du jetzt hast

Mit ADR-0001 → ADR-0010 hast du:

* Runtime ✔
* Control Plane ✔
* Execution ✔
* Memory ✔
* Distribution ✔
* Persistence ✔

👉 Das ist jetzt **ein vollständiges Systemdesign auf Infra-Level**.

---

# 🚀 Nächster sinnvoller Schritt

👉 ADR-0011: **Observability Model**

Ganz ehrlich:

> Ohne das fliegst du blind.

Wenn du willst, bau ich dir das genauso sauber wie die anderen.
