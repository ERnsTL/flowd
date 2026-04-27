Perfekt — das ist der Punkt, an dem sich entscheidet, ob dein System **beherrschbar** ist oder nicht.
Viele Systeme scheitern genau hier: sie bauen Runtime + Features, aber **keine echte Sichtbarkeit**.

Ich baue dir ADR-0011 so, dass es:

* zu deinem Scheduler + Backpressure Modell passt
* Control Plane integriert
* NICHT in „Prometheus + Logs fertig“ abrutscht
* sondern wirklich **FBP-native Observability** definiert

---

# ADR-0011: Observability Model

Status: Accepted
Date: 2026-04-16


## Context

Flowd is a deterministic dataflow runtime composed of:

* Scheduler-driven execution (ADR-0002)
* Typed message model (ADR-0003)
* Backpressure and bounded edges (ADR-0004)
* Node-local state and recovery (ADR-0005)
* Dynamic graph mutation (ADR-0006)
* Hybrid component execution model (ADR-0007)
* High-performance transport (ADR-0008)
* Distributed subgraphs (ADR-0009)
* Edge-based persistence (ADR-0010)

The system is:

* concurrent
* dynamic
* distributed (optionally)
* backpressure-driven

This creates a critical requirement:

> The system must be observable in terms of flow, behavior, and state — not just logs and metrics.

---

## Problem Statement

We must define:

1. How to observe system behavior at runtime
2. How to inspect:

   * nodes
   * edges
   * messages
   * scheduler activity
3. How to debug:

   * backpressure
   * stalls
   * bottlenecks
4. How to provide insight into:

   * dynamic graph changes
   * distributed execution
5. How to avoid:

   * opaque systems
   * fragmented observability
   * excessive overhead

---

## Constraints

* Observability must not significantly impact performance
* Must work for:

  * in-memory execution
  * persistent edges
  * distributed setups
* Must integrate with Control Plane (ADR-0006)
* Must respect backpressure model
* Must support both:

  * real-time inspection
  * offline analysis

---

## Decision

Flowd adopts a **multi-layer, flow-native observability model** consisting of:

1. **Metrics (quantitative)**
2. **Tracing (message flow)**
3. **Introspection (graph state)**
4. **Event streams (lifecycle and control events)**


## Relations to Other ADRs

* ADR-002 Scheduler
* ADR-003 Messages
* ADR-008 Transport


## Core Model

---

### 1. Metrics Layer

Metrics provide:

> aggregated numerical insights

Examples:

* node throughput (messages/sec)
* edge queue depth
* scheduler utilization
* ACK latency
* processing time per node

---

### 2. Tracing Layer

Tracing provides:

> visibility into message flow

Each message may carry:

* trace ID
* path through nodes
* timestamps

Enables:

* end-to-end tracing
* bottleneck detection
* latency analysis

---

### 3. Introspection Layer

Introspection provides:

> real-time system structure and state

Includes:

* current graph topology
* node states (running, idle, draining)
* edge status (capacity, fullness)
* scheduler queues

---

### 4. Event Stream Layer

Event streams capture:

> discrete system events

Examples:

* node started/stopped
* edge created/removed
* graph mutation events
* errors
* backpressure signals

---

## Key Design Principle

> Observability must follow the dataflow — not just the runtime.

---

## Rationale

---

### Why Metrics Alone Are Not Enough

Metrics provide:

* averages
* aggregates

But cannot explain:

* why a message is delayed
* where flow is blocked

---

### Why Tracing Is Required

Flowd is:

> a message-driven system

Therefore:

* tracing message paths is essential
* equivalent to request tracing in web systems

---

### Why Introspection Is Required

Operators must see:

* actual graph state
* not just metrics

Example:

* which node is stalled?
* which edge is full?

---

### Why Event Streams Are Required

System is dynamic:

* nodes added/removed
* edges rewired

Events provide:

* temporal understanding
* audit trail

---

## Observability Dimensions

---

### 1. Node-Level Observability

Each node exposes:

* input rate
* output rate
* processing time
* error rate
* state (idle, active, blocked)

---

### 2. Edge-Level Observability

Each edge exposes:

* queue depth
* capacity
* throughput
* backpressure status

---

### 3. Scheduler-Level Observability

Scheduler exposes:

* active nodes
* scheduling decisions
* budget usage
* fairness metrics

---

### 4. Message-Level Observability

Messages may expose:

* trace ID
* timestamps
* path history

---

### 5. Graph-Level Observability

Graph exposes:

* topology
* version
* mutation history

---

## Integration with Control Plane

Observability is:

> exposed via Control Plane APIs

Examples:

* query graph state
* subscribe to events
* inspect node metrics

---

## Real-Time vs Offline

---

### Real-Time

* live dashboards
* streaming metrics
* UI integration (e.g. NoFlo)

---

### Offline

* logs
* trace storage
* historical analysis

---

## Performance Considerations

Observability must be:

* low overhead
* optionally enabled
* configurable

Strategies:

* sampling
* selective tracing
* aggregation

---

## Alternatives Considered

---

### Alternative 1: Logs Only

**Pros:**

* simple

**Cons:**

* unstructured
* insufficient for flow systems

**Decision:**
Rejected

---

### Alternative 2: Metrics Only

**Pros:**

* lightweight

**Cons:**

* lacks causality
* no flow visibility

**Decision:**
Rejected

---

### Alternative 3: External Observability Only (Prometheus, etc.)

**Pros:**

* reuse existing tools

**Cons:**

* lacks system semantics
* incomplete visibility

**Decision:**
Rejected as sole model

---

### Alternative 4: Full Distributed Tracing System

**Pros:**

* powerful

**Cons:**

* heavy
* complex

**Decision:**
Deferred / optional integration

---

## Consequences

---

### Positive Consequences

* deep system visibility
* easier debugging
* better performance tuning
* supports complex use cases
* enables UI integration

---

### Negative Consequences

* additional implementation complexity
* potential overhead if misused

---

### Trade-offs

| Property   | Decision     |
| ---------- | ------------ |
| Visibility | very high    |
| Complexity | moderate     |
| Overhead   | controllable |

---

## Implementation Notes

* define metrics interface
* implement tracing hooks
* expose introspection API
* implement event stream system
* integrate with Control Plane

---

## Operational Impact

Operators gain:

* visibility into bottlenecks
* insight into system behavior
* ability to debug live systems

Requires:

* monitoring setup
* possible storage for traces/events

---

## Interaction with Other ADRs

* ADR-0002:

  * scheduler metrics
* ADR-0004:

  * backpressure visibility
* ADR-0006:

  * mutation events
* ADR-0009:

  * distributed observability

---

## Key Insight

> A dataflow system must be observable as a flow — not as a collection of logs.

---

## What This Enables

* production-grade debugging
* performance optimization
* UI-driven system control
* operational confidence

---

## What This Avoids

* blind operation
* guess-based debugging
* hidden bottlenecks

---

## Open Questions

* Should tracing be always-on or sampled?
* How to store traces efficiently?
* Should UI be built-in or external?
* How to correlate distributed traces?


## 7. Debug and Trace Model (FBP Integration)

### Purpose

The system MUST provide visibility into:

* runtime state (debugging)
* dataflow execution (tracing)

These serve different purposes and MUST NOT be conflated.

---

## 7.1 Terminology

### Debug

Debug refers to runtime-level state and control information:

* lifecycle events (start/stop)
* errors
* component state
* scheduler state

---

### Trace

Trace refers to the flow of information packets (IPs) through the system:

* messages on edges
* connection events
* grouping (begingroup/endgroup)

---

> Trace is observational and MUST NOT affect execution.

---

## 7.2 FBP Protocol Mapping

flowd MUST expose trace and debug information compatible with the FBP protocol.

---

### Trace Events (REQUIRED for UI integration)

* `trace:data`
* `trace:connect`
* `trace:disconnect`
* `trace:begingroup`
* `trace:endgroup`

---

### Debug Events

* `runtime:status`
* `runtime:error`
* `runtime:started`
* `runtime:stopped`

---

---

## 7.3 Emission Rules

Trace events MUST be emitted at the following points:

* when a message is transferred across an edge
* when connections are opened/closed
* when group boundaries are entered/exited

---

Debug events MUST be emitted for:

* runtime lifecycle changes
* errors and failures
* control plane operations

---

---

## 7.4 Integration Points (Implementation Guidance)

Trace emission SHOULD occur at:

* edge push operations
* edge pop operations (optional)
* component output boundaries

---

Recommended minimal model:

```text
Component → Edge → Runtime
                 ↓
             Trace Event
```

---

---

## 7.5 Performance Model

Trace MUST be:

* disabled by default OR configurable
* zero or near-zero overhead when disabled

Optional:

* sampling
* filtering by component / edge

---

---

## 7.6 UI Compatibility

Trace output MUST be compatible with tools such as NoFlo UI.

This requires:

* correct node/port identification
* correct ordering of events
* consistent edge mapping

---

---

## 7.7 Guarantees

Trace MUST:

* reflect actual message flow
* preserve ordering (best effort)
* not mutate or delay messages

---

Trace is best-effort and not guaranteed to be complete under load.

---

---

## 7.8 Anti-Patterns

The following are forbidden:

* mixing debug and trace events
* emitting trace inside component logic instead of runtime layer
* modifying execution behavior based on trace state

---

---

## 7.9 Design Decision

Trace is implemented at the runtime level, not within components.

Components MUST remain unaware of tracing unless explicitly instrumented.


## 7.10 Implementation Considerations

Pros:
- enables visual debugging
- aligns with FBP ecosystem
- improves developer productivity

Cons:
- runtime overhead if misused
- increased complexity in runtime
- potential large data volume

Mitigation:
- configurable tracing
- sampling
- selective enabling


## Implementation Phases

Phase 1:
- Trace (message flow visualization)

Phase 2:
- Debug (runtime state)

Phase 3:
- Metrics

Phase 4:
- Logging and export








---

---

# 🧠 Einordnung (sehr wichtig)

Das ist einer der unterschätztesten Teile.

Ohne das bist du:

> ❌ technisch korrekt, aber operativ blind

Mit dem:

> ✅ kontrollierbares System

---

# 🔥 Wichtigster Satz

> **You cannot operate what you cannot see.**

---

# 🎯 Was du jetzt hast

Mit ADR-0001 → ADR-0011:

* Architektur ✔
* Execution ✔
* Messaging ✔
* Backpressure ✔
* State ✔
* Control Plane ✔
* Transport ✔
* Distribution ✔
* Persistence ✔
* Observability ✔

👉 Das ist jetzt ein **vollständiges, konsistentes Plattform-Design**

---

# 🚀 Wenn du weitergehst

Jetzt kommt die letzte Ebene:

👉 ADR-0012: **Configuration & Build Model**

Das ist der Punkt, wo dein System:

> **für andere benutzbar wird**

Sag Bescheid 👍
