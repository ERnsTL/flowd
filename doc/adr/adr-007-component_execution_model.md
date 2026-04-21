Perfekt — das ist jetzt das letzte große Fundament: **wie Komponenten tatsächlich laufen**.
Das ist der Punkt, wo viele Systeme entweder:

* zu simpel werden (nur Threads)
* oder overengineered (alles Tokio/Futures)

Du gehst bewusst den Mittelweg — und genau das halte ich sauber fest.

Ich beziehe mich dabei auch explizit auf deine konsolidierte ADR-Liste .

---

# ADR-0007: Component Execution Model

Status: Accepted
Date: 2026-04-16

---

## Context

Flowd defines a deterministic, scheduler-driven Flow-Based Programming runtime.

From previous ADRs:

* ADR-0002: Scheduler-based execution model (central scheduling, budgets)
* ADR-0003: Typed message model
* ADR-0004: Backpressure & edge semantics
* ADR-0005: State & recovery model
* ADR-0006: Control plane & graph mutation

A key requirement emerges:

> Components must be able to perform computation, IO, and long-lived activities — without breaking scheduler control, backpressure, or system stability.

---

## Problem Statement

We must define:

1. How components (nodes) execute
2. How concurrency is handled
3. How blocking and async IO are integrated
4. How components interact with the scheduler
5. How to avoid:

   * excessive thread usage
   * scheduler starvation
   * runtime complexity explosion

---

## Constraints

* Scheduler controls execution (ADR-0002)
* No direct node-to-node scheduling
* Backpressure must remain valid (ADR-0004)
* Components must support:

  * CPU-bound work
  * IO-bound work
  * long-lived tasks
* No requirement for a global async runtime
* Compile-time integration (ADR-0001)
* Deterministic behavior preferred over implicit concurrency

---

## Decision

Flowd adopts a **hybrid component execution model** with the following properties:

---

### 1. Scheduler-Driven Activation

* Components are executed by the scheduler
* Each activation:

  * processes a bounded number of messages (budget)
* Components do NOT own their execution lifecycle

---

### 2. Components Are Passive by Default

A component:

* reacts to incoming messages
* does not actively schedule itself or others

---

### 3. Internal Concurrency Is Allowed

Components MAY internally:

* spawn threads
* run event loops
* use async runtimes (e.g. Tokio)

BUT:

> This must not block the scheduler or violate backpressure semantics

---

### 4. No Thread-per-Node Model

The runtime does NOT:

* assign one OS thread per component

Reason:

* poor scalability
* lack of global control
* OS scheduler lacks semantic awareness

---

### 5. No Per-Node Tokio Runtime

The runtime does NOT:

* spawn a Tokio runtime per component

Reason:

* thread explosion
* unnecessary overhead
* resource fragmentation

---

### 6. Optional Shared Async Runtime

If async is used:

* a shared runtime MAY exist at process level

Usage:

* IO tasks
* network operations
* timers

---

### 7. Non-Blocking Scheduler Contract

Components must adhere to:

> Scheduler execution must never be blocked

Therefore:

* blocking operations must be offloaded
* long operations must be asynchronous

---

### 8. Event Loop Pattern (Recommended)

Components may implement:

```text id="s9g3fc"
Scheduler → component.process()
           ↓
      enqueue async work
           ↓
   async completes → push message
```

---

### 9. Wake-Up Mechanism

Components signal readiness via:

* message arrival
* internal queue events

Scheduler decides:

* when component is activated

---

### 10. Budget Enforcement

Each activation:

* limited by budget (messages / time)
* prevents runaway execution

---

## Rationale

---

### Why Hybrid Model

Pure models fail:

---

#### Pure Thread Model

Pros:

* simple
* intuitive

Cons:

* no fairness
* poor scaling
* tight coupling

---

#### Pure Async Model

Pros:

* efficient IO
* scalable

Cons:

* complexity explosion
* difficult debugging
* unnecessary for CPU-bound work

---

### Hybrid Model Advantages

* preserves performance
* maintains control
* supports all workloads
* avoids overengineering

---

### Separation of Concerns

| Concern          | Responsibility |
| ---------------- | -------------- |
| Execution order  | Scheduler      |
| Data movement    | Edges          |
| IO / concurrency | Component      |
| Fairness         | Scheduler      |

---

### Why Not Let Nodes Schedule Themselves

Rejected because:

* breaks determinism
* introduces race conditions
* bypasses fairness

---

### Why Not Block in Nodes

Blocking leads to:

* scheduler stalls
* deadlocks
* unpredictable latency

---

### Why Async Is Optional

Not all components need async:

* CPU-bound nodes benefit from sync execution
* forcing async everywhere adds overhead

---

## Alternatives Considered

---

### Alternative 1: Thread-per-Node

**Pros:**

* simple
* isolated

**Cons:**

* resource heavy
* no global control
* hard to debug

**Decision:**
Rejected

---

### Alternative 2: Fully Async Runtime (Tokio Everywhere)

**Pros:**

* unified model
* efficient IO

**Cons:**

* complexity
* poor debuggability
* unnecessary overhead

**Decision:**
Rejected

---

### Alternative 3: Single Global Event Loop

**Pros:**

* simple control

**Cons:**

* bottleneck
* limited parallelism

**Decision:**
Rejected

---

### Alternative 4: Actor Model (Fully Autonomous Nodes)

**Pros:**

* flexible
* scalable

**Cons:**

* difficult backpressure
* implicit scheduling
* hard to reason about

**Decision:**
Rejected

---

## Consequences

---

### Positive Consequences

* controlled execution model
* scalable architecture
* supports both sync and async workloads
* avoids thread explosion
* maintains determinism
* integrates cleanly with scheduler

---

### Negative Consequences

* requires discipline in component design
* developers must understand async vs blocking
* slightly more complex than naive models

---

### Neutral / Trade-offs

* components have partial autonomy
* system is more structured
* requires explicit design patterns

---

## Implementation Notes

* provide helper APIs:

  * spawn async tasks
  * enqueue results
* enforce non-blocking contract
* detect long-running components (optional)
* expose component metrics

---

## Operational Impact

* predictable performance
* stable under load
* easier debugging than fully async systems
* requires monitoring of:

  * task queues
  * scheduler load

---

## Interaction with Other ADRs

* ADR-0002:

  * scheduler controls execution
* ADR-0003:

  * message model affects async boundaries
* ADR-0004:

  * backpressure must be respected
* ADR-0006:

  * graph mutation must not disrupt execution

---

## Key Insight

> Components are not threads.
> They are scheduled execution units with optional internal concurrency.

---

## Open Questions

* Should component-level metrics be standardized?
* Should async helpers be part of core API?
* How to detect and handle misbehaving components?
* Should cooperative yielding be enforced?

---

---

# 🧠 Einordnung (wichtig)

Das ist ein extrem entscheidendes ADR, weil:

* hier entscheidet sich:

  * **Performance**
  * **Komplexität**
  * **Developer Experience**

---

# 🔥 Wichtigster Satz

> **Components are scheduled, not running.**

---

# 🎯 Was du jetzt hast

Mit ADR-0001 bis 0007 hast du:

* Execution ✔
* Messaging ✔
* Backpressure ✔
* State ✔
* Control Plane ✔
* Component Model ✔

👉 Das ist bereits ein vollständiges Runtime-System.

---

Wenn du weitermachst, der nächste logische Schritt:

👉 **ADR-0008: Transport & Memory Model (Ringbuffer, Arc, Zero-Copy)**

Das ist dein letzter „Performance-Kern“.
