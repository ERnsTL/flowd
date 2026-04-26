Perfekt — das ist jetzt der Punkt, wo flowd auf die reale Welt trifft.
Hier entscheidet sich, ob dein System stabil bleibt oder von externen Abhängigkeiten zerlegt wird.

Ich baue dir das bewusst strikt und vollständig: Timeouts, Retries, Idempotenz, Backpressure gegen externe Systeme, Async sauber eingebettet in dein Scheduler-Modell.

---

# ADR-0017: IO & External Systems Interaction Model

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd is a dataflow runtime with:

* Scheduler-driven execution (ADR-0002)
* Typed message model (ADR-0003)
* Backpressure and bounded edges (ADR-0004)
* State and replay model (ADR-0005)
* Control plane and graph mutation (ADR-0006)
* Component execution model (ADR-0007)
* Transport model (ADR-0008)
* Distribution model (ADR-0009)
* Persistence model (ADR-0010)
* Failure & recovery model (ADR-0015)
* Delivery semantics & ACK model (ADR-0016)

Flowd systems must interact with external systems such as:

* HTTP APIs
* databases
* message brokers
* file systems
* AI/LLM services
* hardware / sensors

These systems are:

* unreliable
* slow
* rate-limited
* outside flowd’s control

A critical requirement emerges:

> External system interaction must not break determinism, backpressure, or system stability.

---

## Problem Statement

We must define:

1. How IO is performed within nodes
2. How failures and retries are handled
3. How external latency is managed
4. How to integrate async IO with scheduler model
5. How to prevent:

   * blocking execution
   * overload of external systems
   * cascading failures

---

## Constraints

* Scheduler must not be blocked (ADR-0002)
* Backpressure must remain valid (ADR-0004)
* Must integrate with delivery semantics (ADR-0016)
* Must support both:

  * synchronous operations
  * asynchronous operations
* Must work across:

  * in-process
  * distributed systems

---

## Decision

Flowd adopts an **explicit, non-blocking IO interaction model** with:

1. **IO encapsulated in dedicated components**
2. **Asynchronous execution as default for IO**
3. **Built-in retry, timeout, and backoff semantics**
4. **Backpressure-aware IO**
5. **Optional circuit breaker and rate limiting mechanisms**

---

## Core Model

---

### 1. IO as Component Responsibility

All external interaction is performed by:

> **IO Nodes (Components)**

Examples:

* HttpRequestNode
* DatabaseQueryNode
* FileReadNode
* LLMCallNode

---

### 2. Non-Blocking Requirement

IO must:

> never block scheduler execution

Therefore:

* IO is executed asynchronously
* blocking calls must be offloaded

---

### 3. Async Execution Model

Pattern:

```text id="ioflow1"
Scheduler → Node.process()
          → enqueue async IO task
          → return immediately
          → async completes
          → result pushed as message
```

---

### 4. Timeout Semantics

Every IO operation must define:

* timeout duration in milliseconds
* behavior on timeout

Timeout results in:

* failure (NACK)
* retry (optional)

Defaults:

* Default timeout SHOULD be configured, eg. 5000ms.
* timeout behavior defaults to `NACK` if no retry policy is configured

---

### 5. Retry Model

Retries are:

> explicit and configurable

Parameters:

* retry count
* delay strategy (fixed, exponential)
* jitter (optional)

Defaults:

* `max_retries = 0`
* `backoff = fixed(0ms)`
* retries stop on first permanent error or when `max_retries` is exhausted

---

### 6. Backoff Strategy

Recommended:

* exponential backoff
* capped retries

---

### 7. Circuit Breaker (Optional)

For unstable systems:

* failure threshold
* open/closed state
* cooldown period

---

### 8. Rate Limiting

To protect external systems:

* request rate limits
* concurrency limits

---

### 9. Backpressure Integration

IO must respect:

> system backpressure AND external capacity

Mechanisms:

* limit concurrent IO operations
* queue requests internally
* propagate backpressure upstream

Mandatory constraints:

* internal request queues MUST be bounded
* each IO component MUST declare `max_inflight` and `max_pending`
* overflow behavior MUST be explicit; default is `NACK` on enqueue rejection

---

### 10. Idempotency Requirement

Because of retries:

> Non-idempotent operations MUST define a deduplication or idempotency strategy.

Examples:

* GET → safe
* POST → must be designed carefully

---

### 11. Error Handling

Errors are classified:

* transient (retryable)
* permanent (fail fast)

Each IO component MUST define a deterministic error mapping table
from external/library errors to `{transient, permanent}`.


* ACK after successful external operation
* NACK after retries exhausted or permanent failure

Each message MUST result in exactly one terminal outcome: ACK or NACK.


---

### 12. Integration with Delivery Semantics

* successful IO → ACK
* failed IO → NACK
* retry controlled by node logic

Emission point:

* ACK MUST be emitted only after external success is confirmed
* NACK MUST be emitted on permanent failure or retry exhaustion

---

### 13. Streaming IO

For streaming systems:

* chunked processing
* backpressure-aware streaming

---

### 14. External System Isolation

Failures in external systems must:

> not cascade into full system failure

---

## Rationale

---

### Why IO Must Be Explicit

Implicit IO:

* hides latency
* breaks reasoning

Explicit IO:

* observable
* controllable

---

### Why Async by Default

Blocking IO:

* stalls scheduler
* breaks performance guarantees

---

### Why Retry Is Built-In

External systems fail frequently:

* network issues
* rate limits
* temporary outages

---

### Why Circuit Breaker Is Optional

Not all systems need it:

* adds complexity
* but critical for unstable integrations

---

### Why Idempotency Is Required

Because:

* retries are unavoidable
* duplicates are possible

---

### Why Backpressure Must Extend to IO

Without it:

* system overloads external services
* cascading failures occur

---

## Alternatives Considered

---

### Alternative 1: Blocking IO

**Pros:**

* simple

**Cons:**

* stalls scheduler
* poor performance

**Decision:**
Rejected

---

### Alternative 2: No Retry Handling

**Pros:**

* simple

**Cons:**

* unreliable

**Decision:**
Rejected

---

### Alternative 3: Fully External IO Management

**Pros:**

* offload complexity

**Cons:**

* inconsistent behavior
* poor integration

**Decision:**
Rejected

---

### Alternative 4: Mandatory Circuit Breakers Everywhere

**Pros:**

* safe

**Cons:**

* overengineering

**Decision:**
Optional

---

## Consequences

---

### Positive Consequences

* robust external interaction
* controlled system behavior
* prevents cascading failures
* integrates cleanly with flow model

---

### Negative Consequences

* increased complexity
* requires careful node design
* async complexity for developers

---

### Trade-offs

| Property    | Decision |
| ----------- | -------- |
| Reliability | high     |
| Complexity  | moderate |
| Performance | high     |

---

## Implementation Notes

* provide IO helper APIs
* integrate async runtime (shared or component-local per ADR-0002 rules)
* implement retry utilities
* implement timeout handling
* expose configuration options

---

## Operational Impact

Operators must:

* configure retry policies
* monitor IO performance
* manage rate limits

---

## Interaction with Other ADRs

* ADR-0016:

  * ACK/NACK integration
* ADR-0015:

  * failure handling
* ADR-0002:

  * scheduler must not be blocked

### Dependency on ADR-0002 (Scheduler Model)

All IO components operate under the execution constraints defined in ADR-0002.

Specifically:

- IO must never block scheduler execution
- process() MUST comply with the non-blocking execution contract defined in ADR-0002. In particular, it MUST NOT perform blocking operations and MUST return control to the scheduler without waiting for external IO or asynchronous completion.
- async completion MUST use ADR-0002 external readiness signaling (explicit scheduler wakeup), and polling-only completion detection is forbidden.
- scheduler controls execution timing


## Key Insight

> External systems are unreliable — your runtime must absorb that uncertainty.

---

## What This Enables

* stable integration with external systems
* robust pipelines
* safe interaction with APIs and services

---

## What This Avoids

* system instability due to IO
* cascading failures
* blocking execution

---

## Open Questions

* Should IO patterns be standardized further?
* Should connection pooling be managed centrally?
* Should adaptive rate limiting be supported?
* Should IO metrics be built-in or external?
