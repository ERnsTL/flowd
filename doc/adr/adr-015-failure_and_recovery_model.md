Perfekt — das ist jetzt der Punkt, an dem dein System **von „funktioniert“ zu „überlebt Fehler“** wird.
Ich mache das bewusst kompromisslos vollständig, weil hier entscheidet sich, ob flowd ernst genommen wird.

---

# ADR-0015: Failure & Recovery Model

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd is a deterministic, dataflow-based runtime with:

* Scheduler-driven execution (ADR-0002)
* Typed message model (ADR-0003)
* Backpressure and bounded edges (ADR-0004)
* Node-local state and replay-based recovery (ADR-0005)
* Dynamic graph mutation (ADR-0006)
* Component execution model (ADR-0007)
* Transport and memory model (ADR-0008)
* Distribution model (ADR-0009)
* Persistence model (ADR-0010)
* Observability model (ADR-0011)
* Configuration model (ADR-0012)
* Security model (ADR-0013)
* Versioning model (ADR-0014)

The system operates in environments where:

* failures are inevitable
* external systems are unreliable
* nodes may crash or misbehave
* distributed systems introduce partial failure

A key requirement emerges:

> Failure must be treated as a normal and expected condition, with explicitly defined recovery behavior.

---

## Problem Statement

We must define:

1. What constitutes failure in flowd
2. How failures are detected
3. How the system reacts to failures
4. How recovery is performed
5. How to ensure:

   * system stability
   * predictable behavior
   * no undefined states

---

## Failure Model Scope

Failures are classified into:

---

### 1. Node-Level Failures

* panic / crash
* infinite loop / starvation
* resource exhaustion
* invalid state

---

### 2. Edge-Level Failures

* queue overflow
* persistent storage failure
* network failure (for NetworkEdge)

---

### 3. Runtime-Level Failures

* process crash
* scheduler malfunction

---

### 4. External System Failures

* API failures
* database outages
* network timeouts

---

### 5. Distributed Failures

* node unreachable
* partial partition
* inconsistent state across subgraphs

---

## Decision

Flowd adopts a **supervision-based, restart-oriented failure model with explicit recovery semantics**, characterized by:

1. **Failure detection at component and runtime level**
2. **Supervision hierarchy (logical, not necessarily structural)**
3. **Restart-based recovery**
4. **Replay-based state reconstruction**
5. **Explicit handling of unrecoverable failures**

---

## Core Model

---

### 1. Failure Detection

Failures are detected via:

* panic detection (runtime catches)
* timeout detection (optional)
* health monitoring (scheduler / watchdog)
* edge-level error signals

---

### 2. Node Failure Handling

When a node fails:

```text
Node → failure detected → supervisor decision → restart / stop / escalate
```

---

### 3. Restart Strategy

Nodes are restarted by:

* re-instantiating component
* restoring state (if available)
* resuming message processing

---

### 4. Replay-Based Recovery

Recovery uses:

> message replay (ADR-0004, ADR-0010)

Mechanism:

* unacknowledged messages are replayed
* node must handle duplicates

---

### 5. Restart Policies

Supported strategies:

* immediate restart
* exponential backoff
* limited retries
* permanent failure (stop)

---

### 6. Failure Escalation

If restart fails repeatedly:

* escalate to:

  * subgraph
  * entire graph
  * operator

---

### 7. Dead Letter Handling

Messages that cannot be processed:

* routed to:

  * dead letter queue
  * error edge
  * logging / inspection

---

### 8. Isolation of Failures

Failures are:

> contained to the smallest possible scope

* node failure ≠ graph failure
* subgraph failure ≠ system failure

---

### 9. Interaction with Control Plane

Control Plane may:

* restart nodes
* remove faulty nodes
* replace components
* reconfigure graph

---

### 10. Distributed Failure Handling

For distributed systems:

* network failure treated as:

  * temporary unavailability
* recovery via:

  * reconnection
  * replay

---

## Rationale

---

### Why Restart-Based Recovery

Inspired by:

* Erlang/OTP model

Advantages:

* simple
* robust
* avoids complex state repair

---

### Why Not Try to Fix State

Attempting to repair corrupted state:

* unreliable
* complex

Restart + replay is:

> simpler and more predictable

---

### Why Replay-Based Recovery

Aligns with:

* message-driven architecture
* persistent edges

---

### Why Not Global Failure Handling

Global handling:

* introduces coupling
* reduces resilience

---

### Why Supervision Is Logical (Not Strict Tree)

Unlike Erlang:

* flowd uses graph structure

Therefore:

* supervision is flexible
* not strictly hierarchical

---

## Alternatives Considered

---

### Alternative 1: No Failure Handling

**Pros:**

* simple

**Cons:**

* unusable in production

**Decision:**
Rejected

---

### Alternative 2: Global Restart

**Pros:**

* simple

**Cons:**

* high downtime
* poor resilience

**Decision:**
Rejected

---

### Alternative 3: State Repair

**Pros:**

* avoids restart

**Cons:**

* complex
* error-prone

**Decision:**
Rejected

---

### Alternative 4: Full Erlang Supervision Trees

**Pros:**

* proven

**Cons:**

* does not map cleanly to graph model
* over-constraining

**Decision:**
Adapted (not adopted)

---

## Consequences

---

### Positive Consequences

* robust system behavior
* predictable recovery
* aligns with dataflow model
* supports distributed systems

---

### Negative Consequences

* requires idempotent nodes
* replay overhead
* potential duplicate processing

---

### Trade-offs

| Property    | Decision        |
| ----------- | --------------- |
| Simplicity  | high            |
| Robustness  | high            |
| Performance | moderate impact |
| Guarantees  | at-least-once   |

---

## Implementation Notes

* implement panic catching in runtime
* define restart policies
* implement watchdog / monitoring
* integrate with Control Plane
* implement dead letter handling

---

## Operational Impact

Operators must:

* monitor failures
* configure restart policies
* handle dead letter queues

---

## Interaction with Other ADRs

* ADR-0004:

  * replay via persistent edges
* ADR-0005:

  * state restoration
* ADR-0006:

  * control plane intervention
* ADR-0010:

  * persistence enables recovery

---

## Key Insight

> Failure is normal — recovery must be defined, not improvised.

---

## What This Enables

* resilient systems
* long-running processes
* safe distributed execution

---

## What This Avoids

* undefined behavior
* cascading failures
* fragile systems

---

## Open Questions

* Should supervision hierarchy be formalized further?
* Should automatic escalation policies be configurable?
* How to detect subtle failures (performance degradation)?
* Should circuit breaker patterns be integrated here or in IO model?

---

---

# 🧠 Einordnung (sehr wichtig)

Das hier ist:

> **der Unterschied zwischen „läuft im Demo“ und „läuft 24/7 beim Kunden“**

---

# 🔥 Wichtigster Satz (nochmal klar)

> **Failure is normal — recovery must be defined, not improvised.**

---

# 🎯 Was du jetzt erreicht hast

Mit diesem ADR hast du:

* ✔ Actor-System-Level Robustheit (fast)
* ✔ Flink/NiFi-Level Recovery (leichtgewichtig)
* ✔ MUD-taugliche Stabilität

---

# 🚀 Nächster logischer Schritt

👉 **ADR-016: Delivery Semantics & ACK Model**

Das gehört direkt dazu — sonst bleibt das hier halb.

Wenn du willst, mache ich das genauso sauber weiter.
