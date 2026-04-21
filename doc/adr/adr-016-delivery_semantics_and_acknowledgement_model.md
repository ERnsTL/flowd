Perfekt — das ist die direkte Ergänzung zu ADR-015.
Hier definierst du jetzt **die Wahrheit über Datenbewegung im System**. Wenn das unscharf ist, ist alles unscharf.

Ich mache das bewusst präzise und technisch sauber (inkl. Lifecycle, ACK/NACK, Retry, Edge-Interaktion).

---

# ADR-0016: Delivery Semantics & Acknowledgement Model

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd is a dataflow runtime with:

* Typed message model (ADR-0003)
* Bounded edges with backpressure (ADR-0004)
* State & replay-based recovery (ADR-0005)
* Control plane with dynamic mutation (ADR-0006)
* Transport model (ADR-0008)
* Persistence model (ADR-0010)
* Failure & recovery model (ADR-0015)

The system is message-driven:

* nodes consume and produce messages
* edges transport messages
* persistent edges store messages

A critical requirement emerges:

> Message delivery semantics must be explicit, consistent, and enforceable across all edge types and execution contexts.

---

## Problem Statement

We must define:

1. What delivery guarantees flowd provides
2. How message acknowledgement works
3. How retries and failures are handled
4. How delivery interacts with persistence and replay
5. How to avoid:

   * message loss
   * duplicate ambiguity
   * undefined behavior

---

## Constraints

* Must integrate with edge model (ADR-0004)
* Must align with failure model (ADR-0015)
* Must work for:

  * in-memory edges
  * persistent edges
  * network edges
* Must not require global coordination
* Must remain performant and predictable

---

## Decision

Flowd adopts a **default at-least-once delivery model with explicit acknowledgement semantics**, characterized by:

1. **Message lifecycle tracking**
2. **Explicit ACK / NACK handling**
3. **Replay-based recovery**
4. **Optional deduplication (not enforced by runtime)**
5. **Uniform semantics across all edge types**

---

## Core Model

---

### 1. Delivery Guarantee

Flowd provides:

> **At-least-once delivery**

Meaning:

* a message will be delivered one or more times
* duplicates are possible
* loss is avoided unless explicitly configured

---

### 2. Message Lifecycle

Each message follows:

```text id="lifecycle1"
Created → Enqueued → In-Flight → Processed → Acknowledged → Released
```

---

### 3. Acknowledgement Model

A message is:

> considered successfully processed only after explicit ACK

---

#### ACK

* signals successful processing
* removes message from edge (or marks as completed)

---

#### NACK

* signals failure
* triggers retry or escalation

---

### 4. ACK Ownership

ACK responsibility lies with:

> the consuming node

---

### 5. Retry Behavior

On NACK or failure:

* message is retried according to policy

Policies include:

* immediate retry
* delayed retry (backoff)
* limited retries
* infinite retry (optional)

---

### 6. Retry Scope

Retries occur at:

* edge level (persistent edges)
* runtime level (in-memory edges)

---

### 7. Interaction with Persistent Edges

Persistent edges:

* store messages until ACK
* replay unacknowledged messages on restart

---

### 8. Duplicate Handling

Because of at-least-once:

* duplicates may occur

Therefore:

> nodes must be idempotent or duplicate-tolerant

---

### 9. Deduplication

Flowd does NOT enforce deduplication globally.

Optional:

* component-level deduplication
* application-level logic

---

### 10. Ordering Guarantees

Ordering is:

| Context        | Guarantee      |
| -------------- | -------------- |
| Single edge    | FIFO           |
| Multiple edges | not guaranteed |
| Distributed    | best-effort    |

---

### 11. In-Memory Edges

* fast
* ACK may be implicit (configurable)
* no persistence

---

### 12. Network Edges

* require explicit ACK propagation
* must handle:

  * retransmission
  * connection loss

---

### 13. Failure Interaction

If node fails:

* unacknowledged messages are replayed

---

### 14. Timeout Handling

If ACK not received:

* message considered failed
* retry triggered

---

### 15. Dead Letter Handling

After retry exhaustion:

* message routed to:

  * dead letter queue
  * error output

---

## Rationale

---

### Why At-Least-Once

Advantages:

* simple
* robust
* avoids data loss
* aligns with replay model

---

### Why Not Exactly-Once

Requires:

* global coordination
* distributed transactions
* checkpointing

Rejected because:

* high complexity
* performance overhead

---

### Why Explicit ACK

Implicit completion:

* ambiguous
* unreliable

Explicit ACK:

* clear semantics
* observable behavior

---

### Why Not At-Most-Once

At-most-once:

* allows silent data loss

Rejected for most use cases

---

### Why Deduplication Is Not Built-In

Deduplication:

* domain-specific
* costly
* not always needed

---

## Alternatives Considered

---

### Alternative 1: At-Most-Once Delivery

**Pros:**

* simple
* fast

**Cons:**

* data loss

**Decision:**
Rejected

---

### Alternative 2: Exactly-Once Delivery

**Pros:**

* strongest guarantee

**Cons:**

* complex
* heavy

**Decision:**
Rejected

---

### Alternative 3: Implicit ACK

**Pros:**

* simpler API

**Cons:**

* unclear semantics

**Decision:**
Rejected

---

### Alternative 4: Global Deduplication

**Pros:**

* avoids duplicates

**Cons:**

* expensive
* complex

**Decision:**
Rejected

---

## Consequences

---

### Positive Consequences

* predictable delivery semantics
* robust failure handling
* aligns with persistence model
* scalable without coordination

---

### Negative Consequences

* duplicate processing possible
* requires idempotent components
* retry overhead

---

### Trade-offs

| Property    | Decision      |
| ----------- | ------------- |
| Reliability | high          |
| Complexity  | moderate      |
| Performance | high          |
| Guarantees  | at-least-once |

---

## Implementation Notes

* assign message IDs
* track ACK state
* implement retry scheduler
* integrate with persistent edges
* support timeout detection

---

## Operational Impact

Operators must:

* configure retry policies
* monitor failure rates
* handle dead letter queues

---

## Interaction with Other ADRs

* ADR-0015:

  * failure triggers retry
* ADR-0010:

  * persistence enables replay
* ADR-0004:

  * edge model enforces flow control

---

## Key Insight

> Delivery guarantees must be explicit — otherwise they are undefined.

---

## What This Enables

* reliable data processing
* safe retries
* consistent recovery behavior

---

## What This Avoids

* silent data loss
* ambiguous processing state
* inconsistent behavior

---

## Open Questions

* Should batch ACK be supported?
* Should per-edge delivery modes be configurable?
* Should deduplication helpers be provided?
* How to optimize high-throughput ACK handling?

---

---

# 🧠 Einordnung (wichtig)

Jetzt hast du:

* ADR-015 → **was passiert wenn etwas kaputt geht**
* ADR-016 → **was passiert mit den Daten dabei**

👉 Zusammen ist das dein **Reliability Core**

---

# 🔥 Wichtigster Satz

> **Delivery guarantees must be explicit — otherwise they are undefined.**

---

# 🎯 Was du jetzt hast

Jetzt bist du nicht mehr bei:

> „wir schicken Nachrichten rum“

sondern:

> **„wir garantieren Verhalten unter Fehlerbedingungen“**

---

# 🚀 Nächster sinnvoller Schritt

👉 ADR-017: **IO & External Systems Interaction Model**

Das ist jetzt der letzte große „Realitätskontakt“.

Wenn du willst, bau ich den genauso sauber.
