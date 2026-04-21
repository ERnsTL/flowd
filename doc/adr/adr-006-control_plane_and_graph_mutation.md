Perfekt – das ist jetzt das **Herzstück deiner gesamten Architektur**.
Wenn ADR-0002 das „Execution Model“ ist, dann ist **ADR-0006 das Betriebssystem-Level**.

Ich halte mich strikt an dein Template  und bringe alles rein, inkl. Proxy Nodes, Hot Reload, Drain, dual backends, etc.

---

# ADR-0006: Control Plane & Graph Mutation

Status: Accepted
Date: 2026-02-16

---

## Context

Flowd is designed as a deterministic Flow-Based Programming runtime that must support:

* Long-running systems
* Dynamic reconfiguration
* Live updates (hot reload)
* External control via UI (e.g. NoFlo / Flowhub protocol)
* Safe evolution of running graphs

From previous ADRs:

* ADR-0001: Compile-time integration of components
* ADR-0002: Scheduler-based execution model
* ADR-0003: Typed message model
* ADR-0004: Backpressure & edge semantics
* ADR-0005: State and recovery model

A key architectural requirement emerges:

> The system must allow **safe, controlled mutation of the execution graph at runtime** without violating consistency, backpressure, or system stability.

---

## Problem Statement

We must define:

1. How a graph is started, stopped, and modified
2. How nodes and edges can be added/removed at runtime
3. How updates interact with:

   * in-flight messages
   * backpressure
   * node state
4. How to support:

   * full runtime restart (coarse-grained update)
   * live graph mutation (fine-grained update)
5. How to coordinate external control (UI, API) with internal execution

---

## Constraints

* No dynamic plugin loading (ADR-0001)
* Nodes are compiled into the binary
* Execution must remain deterministic
* Backpressure must remain valid at all times
* No message loss unless explicitly allowed
* Scheduler must not be blocked
* Graph mutation must be observable and controllable
* System must support:

  * interactive UI control
  * automated orchestration
  * staged rollouts

---

## Decision

Flowd adopts a **dual-layer control model**:

### 1. Control Plane vs Data Plane Separation

* **Data Plane**

  * Executes the graph
  * Processes messages
  * Controlled by scheduler

* **Control Plane**

  * Manages graph lifecycle
  * Handles mutations
  * Exposed via API (e.g. WebSocket / FBP protocol)

---

### 2. Graph Lifecycle Model

A graph has explicit lifecycle states:

```text
Created → Started → Running → Stopping → Stopped
```

Operations:

* `Graph.start()`
* `Graph.stop()`
* `Graph.update()` (controlled mutation)

---

### 3. Mutation Model

Graph mutation is:

> **Explicit, controlled, and staged**

Supported operations:

* Add node
* Remove node
* Add edge
* Remove edge
* Replace node instance
* Update configuration

---

### 4. Safe Mutation Boundary

Mutation must respect:

* message flow
* backpressure
* node execution

Therefore:

> **Mutations are applied only at safe points**

---

### 5. Proxy Node Pattern (Key Mechanism)

To enable safe runtime updates:

> **Proxy Nodes act as boundaries between subgraphs**

Properties:

* buffer messages
* manage multiple downstream targets
* enable switching between graph versions

---

### 6. Dual Backend Model (Rolling Update)

For safe updates:

* two versions of a subgraph may exist simultaneously

```text
Proxy → Backend A (old)
      → Backend B (new)
```

Behavior:

* traffic initially flows to A
* gradually shifted to B
* A is drained
* A is removed

---

### 7. Drain Mechanism

A node or subgraph can be:

> **drained**

Meaning:

* no new messages are accepted
* existing messages are processed
* once empty → safe to stop

Proxy nodes detect:

```text
output drained → ready for shutdown
```

---

### 8. Hot Reload Strategy

Flowd does NOT support:

* dynamic code loading

Instead:

> **Hot reload = process-level restart + graph-level continuity**

Mechanism:

1. new binary compiled
2. new runtime instance started
3. traffic routed via proxy nodes
4. old runtime drained
5. old runtime stopped

---

### 9. In-Process Graph Mutation

For non-code changes:

* graph topology can change live
* configuration updates applied dynamically

Constraints:

* no invalid intermediate states
* scheduler consistency maintained

---

### 10. Control API

Control Plane is exposed via:

* WebSocket API
* FBP protocol

Supports:

* graph creation
* node wiring
* runtime commands
* inspection

---

## Rationale

---

### Separation of Control and Data Plane

Prevents:

* control logic interfering with execution
* unstable mutation behavior

Aligns with:

* distributed systems design
* networking architecture

---

### Proxy Node Pattern

Key insight:

> Safe mutation requires buffering and decoupling

Without proxy:

* direct edges → unsafe to rewire
* message loss or duplication risk

Proxy provides:

* isolation
* controlled switching
* drain detection

---

### Dual Backend Approach

Inspired by:

* blue/green deployment
* rolling updates

Advantages:

* zero downtime
* safe migration
* observable transition

---

### Why Not Direct Mutation

Naive approach:

```text
Remove node → Add new node → reconnect edges
```

Problems:

* messages in-flight lost
* inconsistent state
* race conditions

---

### Why Not Dynamic Plugins

Rejected because:

* ABI instability
* unsafe runtime behavior
* conflicts with compile-time integration (ADR-0001)

---

### Why Process Restart Is Acceptable

Because:

* flowd is designed for fast startup
* graph can be reconstructed
* proxies enable seamless transition

---

### Why Mutation Must Be Explicit

Implicit mutation leads to:

* unpredictable systems
* debugging nightmares

Explicit control ensures:

* observability
* reproducibility
* safety

---

## Alternatives Considered

---

### Alternative 1: No Runtime Mutation

**Pros:**

* simple
* deterministic

**Cons:**

* no live updates
* poor UX
* unsuitable for interactive systems

**Decision:**
Rejected

---

### Alternative 2: Direct Graph Mutation (Unsafe)

**Pros:**

* simple API

**Cons:**

* race conditions
* message loss
* undefined states

**Decision:**
Rejected

---

### Alternative 3: Dynamic Plugin System

**Pros:**

* no restart required

**Cons:**

* unsafe
* complex
* conflicts with compile-time model

**Decision:**
Rejected

---

### Alternative 4: Full Global Pause

Stop entire graph → mutate → restart

**Pros:**

* safe

**Cons:**

* downtime
* poor UX

**Decision:**
Rejected as default (still possible fallback)

---

### Alternative 5: Distributed Control Plane (Early)

**Pros:**

* scalable

**Cons:**

* premature complexity

**Decision:**
Deferred

---

## Consequences

---

### Positive Consequences

* safe runtime mutation
* zero-downtime updates
* clear separation of concerns
* strong control over system evolution
* supports UI-driven workflows
* enables MUD-style dynamic systems

---

### Negative Consequences

* increased complexity
* requires proxy nodes
* requires careful orchestration
* introduces additional latency at boundaries

---

### Neutral / Trade-offs

* system becomes more structured
* mutation is slower but safer
* requires explicit lifecycle management

---

## Implementation Notes

* introduce `GraphController`
* proxy node implementation required
* support dual backend routing
* implement drain detection
* ensure scheduler compatibility
* maintain mutation queue

---

## Operational Impact

* supports rolling upgrades
* enables zero-downtime deployments
* improves debugging (clear lifecycle)
* requires monitoring of:

  * drain status
  * active backends

---

## Related Decisions

* ADR-0001: Compile-Time Integration
* ADR-0002: Scheduler Model
* ADR-0003: Message Model
* ADR-0004: Backpressure
* ADR-0005: State Model

---

## Key Insight

> The graph is not static.
> It is a living system that must evolve safely.

---

## Open Questions

* Should mutation be transactional?
* Should versioning of graphs be explicit?
* How to visualize mutation in UI?
* How to coordinate state migration between versions?

---

---

# 🧠 Einordnung (wichtig)

Das ist jetzt:

> **der Unterschied zwischen „Runtime“ und „Plattform“**

Ohne dieses ADR bist du:

* nur Engine

Mit diesem ADR bist du:

* **steuerbares System**

---

# 🔥 Wichtigster Satz

> **Safe mutation requires isolation, staging, and explicit control.**

---

Wenn du als nächstes weitermachst, dann:

👉 **ADR-0007: Execution Model (Tokio / Threads / IO Integration)**
oder
👉 **ADR-0009: Distribution Model**

Das sind jetzt die nächsten Hebel.
