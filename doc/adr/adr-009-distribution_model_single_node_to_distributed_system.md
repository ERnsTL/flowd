Alles klar — jetzt kommt die **entscheidende Weichenstellung Richtung „Plattform“**.
Hier entscheidet sich, ob flowd lokal bleibt oder sauber in verteilte Systeme wächst — ohne sich in Flink-Komplexität zu verheddern.

Ich halte mich strikt an dein ADR-Template und arbeite alles ein: Subgraph-Schnittstellen, Proxy Nodes, Netzwerk-Edges, Reticulum-Gedanke, HA-Modelle, was bewusst *nicht* gemacht wird etc.

---

# ADR-0009: Distribution Model (Single Node → Distributed System)

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd is designed as a high-performance, deterministic, in-process dataflow runtime.

From previous ADRs:

* ADR-0002: Scheduler-based execution
* ADR-0003: Typed message model
* ADR-0004: Backpressure and edge model
* ADR-0005: State & recovery (local snapshots + replay)
* ADR-0006: Control plane & graph mutation
* ADR-0008: Transport & memory model (ringbuffer, zero-copy)

Initially, flowd operates as:

> A single-process, in-memory execution engine.

However, target use cases require:

* horizontal scaling
* isolation of subsystems
* high availability (HA)
* independent deployment of subsystems
* resilience across process boundaries

Additionally, the architecture already introduced:

* Proxy nodes (ADR-0006)
* Persistent edges (ADR-0004)
* Typed messages (ADR-0003)

These naturally suggest a path toward distribution.

---

## Problem Statement

We must define:

1. How a flowd system can be distributed across processes or machines
2. How subgraphs communicate across boundaries
3. How backpressure behaves across network edges
4. How failure and recovery are handled
5. How to scale without introducing excessive complexity

---

## Constraints

* Maintain deterministic behavior where possible
* Preserve backpressure semantics
* Avoid global coordination (no Flink-style cluster control)
* Keep runtime simple and composable
* Allow independent deployment of subgraphs
* Avoid mandatory cluster manager
* Integrate with existing architecture:

  * proxy nodes
  * edge abstraction
  * message model

---

## Decision

Flowd adopts a **subgraph-based distribution model** with:

1. **Explicit boundaries between subgraphs**
2. **Network edges as first-class edge types**
3. **Proxy nodes as distribution gateways**
4. **No global cluster orchestration**
5. **Composable, loosely coupled distributed topology**

---

## Core Model

---

### 1. Subgraph as Unit of Distribution

A flowd system is composed of:

> Independent subgraphs

Each subgraph:

* runs in its own runtime instance
* has its own scheduler
* has its own memory space

---

### 2. Boundary Definition

Subgraphs are connected via:

> Explicit boundary nodes (proxy nodes)

---

### 3. Network Edges

New edge type:

```text id="tqk2sv"
NetworkEdge
```

Properties:

* serializes `FbpMessage`
* transmits via network (TCP / custom transport)
* integrates with backpressure

---

### 4. Proxy Node Role

Proxy nodes act as:

* ingress/egress points
* protocol translators
* buffering layers
* failover control points

---

### 5. Message Transport Across Boundary

Pipeline:

```text id="qg6l9r"
Node → Ringbuffer → Proxy → NetworkEdge → Proxy → Ringbuffer → Node
```

---

### 6. Backpressure Across Network

Backpressure remains:

> end-to-end

Mechanism:

* remote capacity is exposed via:

  * ACK signals
  * queue depth feedback
* sender throttles accordingly

---

### 7. Deployment Model

Each subgraph can be:

* independently deployed
* independently restarted
* independently scaled

---

### 8. High Availability Model

HA is achieved via:

* multiple instances of subgraph
* load balancing at proxy level
* failover routing

Example:

```text id="9y9bfl"
Proxy → Backend A
      → Backend B
```

---

### 9. Routing Strategy

Proxy nodes may:

* round-robin
* load-based routing
* failover switching

---

### 10. No Global Coordinator

Flowd explicitly avoids:

* cluster master nodes
* distributed consensus
* global scheduling

---

## Rationale

---

### Why Subgraph-Based Distribution

Advantages:

* aligns with FBP model
* natural boundaries
* easy reasoning
* modular scaling

---

### Why Not Distributed Scheduler

Rejected because:

* complexity explosion
* coordination overhead
* reduced determinism

---

### Why Proxy Nodes Are Central

Proxy nodes already exist (ADR-0006):

* extend naturally into distribution
* unify:

  * hot reload
  * scaling
  * routing

---

### Why Not Flink-Style Cluster

Flink requires:

* global checkpointing
* synchronized barriers
* cluster coordination

Flowd avoids:

> global synchronization

---

### Why Network Edge as Abstraction

Consistent with:

* existing edge model (ADR-0004)
* allows uniform handling:

  * in-memory
  * file
  * network

---

### Why Not Transparent Distribution

Systems that hide distribution:

* become unpredictable
* harder to debug

Flowd chooses:

> explicit distribution boundaries

---

## Alternatives Considered

---

### Alternative 1: Single-Process Only

**Pros:**

* simple
* fast

**Cons:**

* no scalability
* no isolation

**Decision:**
Rejected

---

### Alternative 2: Fully Transparent Distribution

**Pros:**

* easy for users

**Cons:**

* hidden complexity
* debugging difficulty

**Decision:**
Rejected

---

### Alternative 3: Global Cluster Manager

**Pros:**

* centralized control

**Cons:**

* single point of failure
* complexity
* unnecessary for most use cases

**Decision:**
Rejected

---

### Alternative 4: Actor-Based Distribution

**Pros:**

* flexible

**Cons:**

* weak backpressure
* implicit behavior

**Decision:**
Rejected

---

### Alternative 5: Kafka-like External Broker

**Pros:**

* scalable

**Cons:**

* external dependency
* latency
* complexity

**Decision:**
Optional integration, not core model

---

## Consequences

---

### Positive Consequences

* scalable architecture
* clear boundaries
* modular deployment
* fault isolation
* flexible topology

---

### Negative Consequences

* network overhead
* serialization cost
* more complex deployment

---

### Neutral / Trade-offs

* explicit configuration required
* less “magic”
* more control

---

## Implementation Notes

* implement `NetworkEdge`
* extend proxy node capabilities
* define serialization format (from ADR-0003)
* implement backpressure signaling across network
* support connection management

---

## Operational Impact

* enables horizontal scaling
* supports HA deployment
* requires network monitoring
* requires routing configuration

---

## Interaction with Other ADRs

* ADR-0006:

  * proxy nodes reused for distribution
* ADR-0004:

  * persistent edges complement network edges
* ADR-0005:

  * recovery via replay still valid

---

## Key Insight

> Distribution is achieved by composing independent runtimes — not by scaling a single global system.

---

## What This Enables

* microservice-like decomposition
* hybrid local + distributed execution
* scalable pipelines
* resilient architectures

---

## What This Avoids

* global synchronization
* cluster complexity
* opaque behavior

---

## Open Questions

* Should network edges support batching?
* Should encryption be built-in?
* How to standardize service discovery?
* How to visualize distributed graphs?

---

---

# 🧠 Einordnung (sehr wichtig)

Das ist jetzt die Entscheidung, die dich trennt von:

* ❌ Flink (heavy distributed system)
* ❌ n8n (single-node toy)

👉 und positioniert dich bei:

> **Composable distributed dataflow runtime**

---

# 🔥 Wichtigster Satz

> **Distribution is composition, not coordination.**

---

# 🎯 Was du jetzt gebaut hast (mental)

Mit ADR-0001 → 0009 hast du:

* Engine ✔
* Runtime ✔
* Control Plane ✔
* Memory Model ✔
* Distribution ✔

👉 Das ist jetzt **eine echte Plattform-Architektur**.

---

Wenn du weitermachen willst:

👉 **ADR-0010: Persistence & Storage Strategy (FileEdge, DBEdge konkretisieren)**
👉 oder
👉 **ADR-0011: Observability (das brauchst du früher als du denkst)**
