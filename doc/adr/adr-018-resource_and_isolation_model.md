Genau hier stellst du sicher, dass dein System **unter Last und bei Fehlern stabil bleibt** – nicht nur logisch korrekt ist.
Ich baue das bewusst strikt auf deinem Scheduler-, Backpressure- und Failure-Modell auf.

---

# ADR-0018: Resource & Isolation Model

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd is a scheduler-driven dataflow runtime with:

* Central scheduling and fairness (ADR-0002)
* Bounded edges and backpressure (ADR-0004)
* Component execution model (ADR-0007)
* Transport and memory model (ADR-0008)
* Failure & recovery model (ADR-0015)
* Delivery semantics (ADR-0016)
* IO interaction model (ADR-0017)

The system executes:

* user-defined components
* potentially untrusted or poorly implemented logic
* workloads with varying resource demands

Additionally:

* nodes may spawn async tasks
* nodes may interact with external systems
* distributed deployments introduce further complexity

A critical requirement emerges:

> No single component or subgraph must be able to degrade or destabilize the entire system.

---

## Problem Statement

We must define:

1. How compute resources are controlled
2. How memory usage is bounded
3. How components are isolated
4. How to detect and contain misbehaving components
5. How to ensure:

   * fairness
   * system stability
   * predictable performance

---

## Constraints

* Must integrate with scheduler (ADR-0002)
* Must preserve backpressure semantics (ADR-0004)
* Must support async execution (ADR-0007, ADR-0017)
* Must work in both:

  * single-node
  * distributed environments
* Must not introduce excessive overhead

---

## Decision

Flowd adopts a **multi-layer resource control and isolation model** consisting of:

1. **Scheduler-level resource control**
2. **Edge-level memory control**
3. **Component-level execution limits**
4. **Optional hard isolation (process/container level)**

---

## Core Model

---

### 1. Scheduler-Level CPU Control

The scheduler enforces:

> **bounded execution per activation**

Mechanism:

* execution budget (messages or time)
* cooperative scheduling

---

#### Properties

* prevents long-running nodes from monopolizing CPU
* ensures fairness across nodes

---

### 2. Execution Budget

Each node execution is limited by:

* max messages per activation
* optional time slice

If exceeded:

* execution is paused
* node is rescheduled

---

### 3. Async Task Control

Components may spawn async tasks, but:

> must not create unbounded concurrency

Controls:

* max concurrent tasks per node
* global async limits (optional)

---

### 4. Memory Control via Edges

Memory is primarily controlled through:

> bounded edges (ADR-0004)

Properties:

* fixed capacity
* no unbounded queues
* natural backpressure

---

### 5. Message Size Constraints

Optional:

* maximum message size
* rejection or splitting of large payloads

---

### 6. Component-Level Isolation

Logical isolation:

* no shared mutable state between components
* communication only via messages

---

### 7. Misbehaving Component Detection

System detects:

* excessive CPU usage
* slow processing
* repeated failures
* unbounded async spawning

---

### 8. Mitigation Strategies

When misbehavior is detected:

* throttle node
* reduce scheduling frequency
* restart node (ADR-0015)
* isolate or disable node

---

### 9. Backpressure as Resource Control

Backpressure is:

> a primary resource control mechanism

Effects:

* limits throughput
* prevents overload
* propagates upstream

---

### 10. Subgraph-Level Isolation

Subgraphs may be:

* deployed separately
* isolated via process boundaries

---

### 11. Hard Isolation (Optional)

For stronger guarantees:

* separate OS processes
* containers
* sandboxing

---

### 12. Resource Limits (Configurable)

System may enforce:

* CPU quotas
* memory limits
* IO concurrency limits

---

## Rationale

---

### Why Scheduler-Level Control

Central scheduler:

* has global view
* can enforce fairness

---

### Why Not OS-Level Scheduling Alone

OS scheduler:

* lacks system semantics
* cannot enforce flow-level fairness

---

### Why Bounded Queues Are Critical

Unbounded queues:

* lead to memory exhaustion
* hide problems

---

### Why Logical Isolation First

Advantages:

* low overhead
* simple model

Hard isolation:

* optional when needed

---

### Why Not Mandatory Sandboxing

Sandboxing:

* expensive
* complex
* not always required

---

### Why Detect Misbehavior

Without detection:

* system degradation is silent
* debugging becomes difficult

---

## Alternatives Considered

---

### Alternative 1: No Resource Limits

**Pros:**

* simple

**Cons:**

* unstable system

**Decision:**
Rejected

---

### Alternative 2: OS-Level Isolation Only

**Pros:**

* reuse existing tools

**Cons:**

* insufficient control
* lacks integration

**Decision:**
Rejected

---

### Alternative 3: Full Mandatory Sandboxing

**Pros:**

* strong isolation

**Cons:**

* performance cost
* complexity

**Decision:**
Optional

---

### Alternative 4: Unbounded Async Execution

**Pros:**

* flexible

**Cons:**

* resource exhaustion

**Decision:**
Rejected

---

## Consequences

---

### Positive Consequences

* stable system behavior
* predictable performance
* resilience against bad components
* scalable architecture

---

### Negative Consequences

* additional configuration
* potential throttling of legitimate workloads
* complexity in tuning

---

### Trade-offs

| Property    | Decision |
| ----------- | -------- |
| Stability   | high     |
| Performance | high     |
| Complexity  | moderate |

---

## Implementation Notes

* implement execution budgets in scheduler
* track node metrics (CPU, time)
* enforce edge capacity limits
* implement async task tracking
* expose configuration options

---

## Operational Impact

Operators must:

* configure resource limits
* monitor system usage
* tune performance parameters

---

## Interaction with Other ADRs

* ADR-0002:

  * scheduler enforces fairness
* ADR-0004:

  * edges enforce memory limits
* ADR-0015:

  * misbehaving nodes may be restarted
* ADR-0017:

  * IO must respect resource limits

---

## Key Insight

> Unbounded components will eventually destroy the system.

---

## What This Enables

* stable long-running systems
* safe multi-component execution
* predictable scaling behavior

---

## What This Avoids

* resource exhaustion
* cascading failures
* system instability

---

## Open Questions

* Should adaptive resource allocation be implemented?
* Should priority scheduling be supported?
* How to expose resource metrics effectively?
* Should isolation levels be standardized?

---

---

# 🧠 Einordnung

Das hier ist:

> **dein Schutzmechanismus gegen dich selbst und andere Entwickler**

---

# 🔥 Wichtigster Satz

> **Unbounded components will eventually destroy the system.**

---

# 🎯 Status jetzt

Mit ADR-015 → ADR-018 hast du:

* ✔ Failure
* ✔ Delivery
* ✔ IO
* ✔ Resource Control

👉 Das ist jetzt ein vollständiger **Runtime-Stabilitätskern**

---

# 🚀 Nächste Schritte

👉 ADR-019: **Packaging & Deployment Model**
👉 ADR-020: **Component Ecosystem**
👉 ADR-021: **Formal Guarantees**

Das sind jetzt die letzten drei, die dich von:

> „stabile Engine“ → „marktfähiges Produkt“

bringen.
