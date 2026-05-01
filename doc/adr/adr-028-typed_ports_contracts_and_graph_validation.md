# ADR-028: Typed Ports, Contracts & Graph Validation

**Status:** Accepted
**Date:** 2026-05-01

---

## Context

Flowd currently defines:

* A **typed runtime message envelope** (`FbpMessage`) (ADR-003)
* A **high-performance transport layer** using ringbuffers and `Arc`-based sharing (ADR-008)
* **Explicit execution semantics and guarantees**, including:

  * FIFO per edge
  * no global ordering
  * at-least-once delivery (ADR-021)

Additionally, the system enforces:

* no implicit behavior (Manifesto)
* explicit dataflow
* deterministic reasoning within defined boundaries

---

## Problem Statement

While `FbpMessage` provides a solid **transport-level abstraction**, it does not provide:

* strong typing at the **graph level**
* guarantees about **compatibility between connected components**
* a mechanism to enforce **semantic correctness of dataflow connections**

This leads to several risks:

1. **Implicit coupling via payload structure**

   * Components must interpret payloads (e.g. `Bytes`, `Value`)
   * Type expectations are not enforced

2. **Runtime errors instead of build-time validation**

   * Mismatched expectations are detected late or not at all

3. **Loss of semantic clarity**

   * The graph does not explicitly express data shape transformations

4. **Fragile correlation handling**

   * Identity (e.g. UID) may be lost or inconsistently propagated

At the same time, constraints remain:

* The runtime must stay **untyped and minimal**
* No additional overhead in the transport layer
* No hidden conversions or implicit coercion
* Full compatibility with heterogeneous graphs

---

## Decision

Flowd introduces a **typed graph-level contract system based on ports**, with:

1. **Typed Ports (`Port<T>`)**
2. **Explicit Compatibility Rules**
3. **Adapter Nodes as First-Class Components**
4. **Optional Explicit Dynamic/Unsafe Mode**

This typing exists **only at the graph and validation layer**, not in the runtime transport.

---

## Core Design

---

### 1. Typed Ports

Each component defines its interface via typed ports:

```rust
Port<T>
```

Example:

```text
EmailParser:
  IN  raw:    Port<EmailRaw>
  OUT parsed: Port<ParsedEmail>
```

Properties:

* Ports carry **logical type information**
* Types are **not embedded in `FbpMessage`**
* Types are used **only for validation and composition**

---

### 2. Separation of Concerns

| Layer      | Responsibility            |
| ---------- | ------------------------- |
| Runtime    | transports `FbpMessage`   |
| Graph      | defines typed connections |
| Components | implement behavior        |
| Adapters   | transform data shapes     |

Key principle:

> **Types belong to ports, not to messages.**

---

### 3. Compatibility Rules

Connections between ports must satisfy:

```text
OUT<T> → IN<U>
```

Allowed cases:

1. **Exact match**

```text
T == U
```

2. **Declared compatibility (optional, explicit)**

* widening
* subtype relationships
* version compatibility (future)

3. **Adapter required**

If:

```text
T != U
```

then:

> A conversion must be represented as an explicit adapter node.

---

### 4. Adapter Nodes (First-Class)

All transformations between types must be explicit in the graph.

Example:

```text
IMAPFetch
  → OUT: Bytes
  → [Adapter] Bytes → EmailRaw
  → EmailParser
  → OUT: ParsedEmail
  → [Adapter] ParsedEmail → ClassifiedEmail
```

Properties:

* visible in graph
* schedulable like any component
* observable and debuggable
* no implicit conversion

---

### 5. No Implicit Coercion

The system explicitly forbids:

* automatic casting between types
* implicit decoding (e.g. JSON inside `Bytes`)
* hidden transformations

Rationale:

> Implicit behavior violates core system principles.

---

### 6. Optional Unsafe / Dynamic Mode

A controlled escape hatch is provided:

```text
Port<AnyMessage>
```

or equivalent dynamic type.

Properties:

* must be explicitly declared
* must be visible in the graph
* may emit warnings during validation
* disables type guarantees for that connection

Use cases:

* interoperability
* incremental migration
* external/unknown data sources

---

## Explicit Non-Goals

The following are explicitly NOT part of this design:

---

### ❌ Typed Runtime Transport

The runtime will NOT:

* transport `Message<T>`
* perform type-based dispatch
* depend on generic message types

Reason:

* would break performance guarantees
* would complicate scheduling and transport
* violates separation of concerns

---

### ❌ Global Message Type System

There is no:

* global `enum` of all possible domain types
* centralized schema registry in core runtime

Types are:

> local to graph definition and component contracts

---

### ❌ Implicit Data Structure Encoding (Brackets / Nested IPs)

Brackets (control IPs) are NOT used for:

* representing structured payloads
* encoding key/value data

They remain:

> stream-structure markers, not data containers

---

## Rejected Alternative: `Component<I, O>`

An alternative design using:

```rust
Component<I, O>
```

was considered.

---

### Advantages

* simple mental model
* ergonomic for linear pipelines
* familiar from functional programming

---

### Reasons for Rejection

1. **Does not model multi-port components**

   * FBP components have multiple inports/outports
   * `Component<I, O>` does not scale to:

     ```text
     IN: A, B, C
     OUT: X, Y
     ```

2. **Hides connection semantics**

   * Type compatibility is checked only locally
   * Graph-level validation is not possible

3. **Encourages pipeline thinking instead of graph thinking**

   * FBP is not a function chain
   * Connections must be explicit per port

4. **Becomes complex with real-world components**

   Leads to:

   ```rust
   Component<(A, B, C), (X, Y)>
   ```

   which is:

   * hard to read
   * hard to maintain
   * semantically unclear

---

### Final Position

> `Component<I, O>` is intentionally NOT part of the core architecture.

It may be used:

* outside the core system
* as developer tooling
* as syntactic sugar

But:

> It is not a foundational abstraction in flowd.

---

## Interaction with Existing ADRs

---

### ADR-003 (Message Model)

* remains unchanged
* `FbpMessage` continues as transport envelope
* typed ports do NOT modify message format

---

### ADR-008 (Transport & Memory Model)

* ringbuffer + `Arc` model unaffected
* no additional allocations or copying introduced

---

### ADR-021 (Execution Semantics)

* no reliance on ordering across edges
* typed ports enforce correctness independent of scheduling

---

### ADR-023 (ABI Boundaries)

* typed ports terminate at boundary nodes
* serialization/deserialization happens only at boundaries

---

## Consequences

---

### Positive

* strong graph-level correctness guarantees
* early detection of incompatibilities
* explicit data transformations
* improved observability and debuggability
* no runtime performance penalty
* alignment with system philosophy (explicitness, determinism)

---

### Negative

* additional complexity in graph definition
* requires adapter nodes for transformations
* stricter discipline required for component design

---

### Trade-offs

| Aspect       | Decision                |
| ------------ | ----------------------- |
| Performance  | unchanged               |
| Safety       | significantly improved  |
| Flexibility  | reduced (intentionally) |
| Explicitness | maximized               |

---

## Key Insight

> **Type safety is enforced at the graph boundary, not in the runtime transport.**

---

## Summary

Flowd introduces **typed ports and graph-level contracts** to ensure:

* explicit, verifiable dataflow
* strong compatibility guarantees
* no hidden transformations

While keeping:

* the runtime minimal
* the transport untyped
* performance characteristics unchanged

---

## Final Statement

> A flowd graph is not just a topology of nodes —
> it is a **type-checked dataflow system with explicit transformations and guarantees**.
