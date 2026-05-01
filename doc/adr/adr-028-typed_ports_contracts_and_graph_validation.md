# ADR-028: Typed Ports, Contracts & Graph Validation

Status: Proposed
Date: 2026-05-01


## Context

Flowd defines:

* A typed runtime message envelope (`FbpMessage`) (ADR-003)
* A high-performance transport model (ringbuffer + `Arc`) (ADR-008)
* Explicit execution guarantees (ADR-021)
* Runtime graph mutation via control plane (ADR-006)
* Versioning and compatibility rules (ADR-014)

The system supports:

* dynamic graph mutation
* long-running execution
* heterogeneous components

---

### Problem

The current system lacks:

* graph-level type safety
* formal compatibility validation
* explicit data transformation modeling

Additionally, validation behavior is undefined under:

* runtime graph mutation
* version evolution
* heterogeneous component ecosystems

---

## Decision

Flowd introduces a **typed graph contract system** with:

1. Typed Ports (`Port<T>`) mapped to existing port metadata
2. Explicit compatibility rules with canonical type identity
3. Lifecycle-aware validation integrated into control plane
4. Adapter nodes as mandatory explicit transformations
5. Explicit dynamic/unsafe mode
6. Explicit requirement for correlation keys in payload contracts

---

## Core Design

---

### 1. Typed Ports (Mapped to Existing Metadata)

Typed ports are **not a new runtime construct**.

They map directly to existing structures:

```text
ComponentPort {
  allowed_type: TypeId,
  schema: Optional<Schema>
}
```

Where:

* `allowed_type` = canonical type identifier
* `schema` = optional structural validation

`Port<T>` is:

> a logical representation of this metadata

---

### 2. Canonical Type Identity

Each type is identified by:

```text
<namespace>/<type>@<version>
```

Example:

```text
email/EmailRaw@1
email/ParsedEmail@2
```

---

### 3. Compatibility Rules (Concrete)

Given:

```text
OUT<T> → IN<U>
```

Compatibility is:

---

#### ✅ Exact match

```text
T == U
```

---

#### ✅ Compatible version (same type)

```text
email/EmailRaw@1 → email/EmailRaw@2
```

Allowed if:

* backward-compatible (ADR-014 rules):

  * new fields optional
  * existing fields unchanged
  * unknown fields ignored

---

#### ❌ Different type

```text
EmailRaw → ParsedEmail
```

Requires:

> explicit adapter node

---

#### ❌ Unknown / incompatible version

Rejected at validation time

---

### 4. Validation Lifecycle (Critical)

Validation is executed at defined control-plane events:

---

#### 1. Graph Load

* full validation required
* graph must be valid before `start`

---

#### 2. Graph Start (`network:start`)

* validation rechecked
* ensures no drift between definition and runtime

---

#### 3. Runtime Mutation (ADR-006)

On operations:

* `graph:addedge`
* `graph:removenode`
* `graph:replace`

👉 Validation rules:

* mutation must not produce invalid graph
* validation occurs **before commit**
* if invalid → mutation rejected

---

#### 4. Safe Mutation Boundary

Validation is applied:

> only at control-plane safe points

(consistent with scheduler + mutation model)

---

### 5. Adapter Nodes (Mandatory)

Any incompatible connection requires:

* explicit adapter component
* visible in graph
* scheduled like normal node

Adapters are:

> the only allowed mechanism for type transformation

---

### 6. Dynamic / Unsafe Mode

Defined via:

```text
Port<Any>
```

Properties:

* disables compatibility checks for that connection
* must be explicit
* produces validation warning
* no guarantees

---

### 7. Correlation Requirement (New Explicit Rule)

Flowd guarantees:

* no global ordering
* at-least-once delivery

Therefore:

> Correlation MUST be part of payload contracts.

Example:

```rust
struct EmailRaw {
  uid: Uid,
  ...
}
```

The system does NOT provide:

* implicit correlation
* ordering-based correlation
* grouping-based correlation

---

### 8. Performance Model Clarification

* Core runtime transport remains unchanged (ADR-008)
* No additional overhead in message passing

However:

> Adapter nodes introduce explicit runtime cost

This is:

* visible
* predictable
* intentional

---

## Rationale

---

### Why lifecycle-aware validation

Because:

* graphs mutate at runtime (ADR-006)
* static validation is insufficient

---

### Why canonical type identity

Prevents:

* naming conflicts
* incompatible local definitions
* ecosystem fragmentation

---

### Why explicit adapters

Ensures:

* observability
* predictability
* no hidden transformations

---

### Why correlation is explicit

Because:

* ordering guarantees are limited (ADR-021)
* implicit correlation is unreliable

---

## Alternatives Considered

---

### Component<I,O>

Rejected because:

* does not support multi-port semantics
* prevents graph-level validation
* encourages pipeline thinking

---

### Runtime Typed Messages

Rejected because:

* breaks performance model
* violates separation of concerns

---

### Implicit Conversion

Rejected because:

* violates manifesto (explicitness)
* breaks determinism

---

## Consequences

---

### Positive

* strong graph correctness guarantees
* safe runtime mutation
* consistent type evolution
* improved observability

---

### Negative

* stricter graph design requirements
* need for adapter nodes
* increased upfront modeling effort

---

### Trade-offs

| Property     | Result                          |
| ------------ | ------------------------------- |
| Safety       | high                            |
| Explicitness | maximal                         |
| Flexibility  | reduced                         |
| Performance  | unchanged (core), +adapter cost |

---

## Implementation Notes

* extend `ComponentPort.allowed_type`
* introduce type registry
* implement validator in control plane
* enforce validation on mutation operations
* add adapter component interface

---

## Related Decisions

* ADR-003: Message Model
* ADR-008: Transport Model
* ADR-006: Control Plane
* ADR-014: Versioning
* ADR-021: Execution Semantics

---

## Key Insight

> Type safety in flowd is a property of the graph lifecycle, not of message transport.

---

## Open Questions

* Should compatibility rules be pluggable?
* Should schemas be enforced at runtime or only validated at edges?
* Should adapter insertion be automated (with confirmation)?


# ADR-028 Addendum

## 1. Version Compatibility Matrix (Directed Semantics)

Compatibility is **directional**:

```text
Producer (OUT<T@vP>) → Consumer (IN<T@vC>)
```

---

### Rule: Compatibility depends on consumer tolerance

The **consumer defines compatibility**, not the producer.

---

### Allowed Cases

#### ✅ Same version

```text
T@1 → T@1
```

---

#### ✅ Forward-compatible consumer

```text
T@1 → T@2
```

Allowed if:

* consumer supports older versions
* fields added in v2 are optional
* consumer ignores missing fields

---

#### ❌ Backward (unsafe by default)

```text
T@2 → T@1
```

Rejected unless:

* explicit adapter provided

Reason:

> consumer cannot safely ignore unknown fields or changed semantics

---

#### ❌ Unknown compatibility

Rejected

---

### Final Rule

> **Compatibility is defined as: “Consumer can safely interpret producer output.”**

---

## 2. Mutation API Alignment

Validation hooks apply to **existing operations only**:

* `graph:addnode`
* `graph:removenode`
* `graph:addedge`
* `graph:removeedge`
* `graph:update` (config-level)

`graph:replace` is **not part of the model**.

---

## 3. Type Registry Scope

The type registry is defined as:

> **a build-time / graph-definition artifact**

---

### Properties

* not part of runtime hot path
* not globally mutable at runtime
* versioned with graph or deployment unit
* may be embedded in:

  * compiled binary
  * graph definition
  * tooling metadata

---

### Responsibilities

* map `TypeId` → schema/definition
* validate compatibility rules
* support tooling (IDE, validator, UI)

---

### Explicit Non-Goal

* no global runtime type registry
* no dynamic type loading

---

## 4. Correlation Enforcement Rules

---

### Rule 1: Correlation is payload responsibility

Already defined:

> correlation keys must be part of type

---

### Rule 2: Validator enforcement conditions

Correlation enforcement applies when:

* fan-in exists (multiple edges into one node)
* or join/merge semantics detected

---

### Rule 3: Required constraints for fan-in

For nodes with multiple input edges:

* input types must define:

```text
CorrelationKey
```

via:

* schema annotation OR
* declared contract metadata

---

### Rule 4: Disallowed cases

Rejected:

```text
Port<Any> → JoinNode
```

unless:

* explicitly marked unsafe

---

### Rule 5: Adapter responsibility

If correlation is missing:

* adapter node must inject correlation key

---

## 5. Validation Rules (Extended)

Validator must enforce:

---

### Structural validation

* port compatibility
* type identity

---

### Version validation

* directed compatibility (producer → consumer)

---

### Correlation validation

* required for fan-in
* optional for linear flows

---

### Dynamic mode validation

* allowed
* but produces warnings
* disables guarantees

---

## 6. Performance Clarification (Refined)

The system guarantees:

* no additional overhead in core transport

However:

* adapter nodes introduce:

  * additional scheduling steps
  * additional memory access

This is:

> **explicit and part of graph design**
