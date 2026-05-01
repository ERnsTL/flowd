# ADR-028: Typed Ports, Contracts & Graph Validation (v4)

Status: Proposed
Date: 2026-05-01


## Context

Flowd defines:

* Typed message envelope (`FbpMessage`) (ADR-003)
* High-performance transport (ADR-008)
* Formal execution guarantees (ADR-021)
* Runtime graph mutation via control plane (ADR-006)
* Versioning and compatibility rules (ADR-014)

Flowd supports:

* long-running systems
* runtime graph mutation
* heterogeneous component composition

---

## Problem Statement

The current system lacks:

* graph-level type safety
* formal compatibility validation
* enforceable data contracts across components

Additionally:

* validation behavior under graph mutation is undefined
* compatibility semantics are underspecified
* correlation handling is not enforceable
* type identity is not canonicalized

---

## Decision

Flowd introduces a **graph-level type system** based on:

1. Typed Ports mapped to existing metadata
2. Canonical Type Identity with versioning
3. Directed Compatibility Rules
4. Lifecycle-aware Validation integrated into control plane
5. Adapter Nodes as mandatory transformation mechanism
6. Explicit Dynamic/Unsafe Mode
7. Enforceable Correlation Rules
8. Build-time scoped Type Registry

---

## Core Design

---

### 1. Typed Ports (Mapped to Existing Metadata)

Typed ports are represented via existing `ComponentPort`:

```text
ComponentPort {
  allowed_type: TypeId,
  schema: Optional<Schema>
}
```

`Port<T>` is a logical abstraction over this structure.

No new runtime representation is introduced.

---

### 2. Canonical Type Identity

Each type is uniquely identified as:

```text
<namespace>/<type>@<version>
```

Example:

```text
email/EmailRaw@1
email/ParsedEmail@2
```

---

### 3. Type Registry (Scope & Responsibility)

The type registry is:

> **a build-time / graph-definition artifact**

---

#### Properties

* not part of runtime execution path
* not mutable at runtime
* versioned with graph or deployment
* available to validator and tooling

---

#### Responsibilities

* resolve `TypeId`
* provide schema (if defined)
* evaluate compatibility rules

---

#### Explicit Non-Goals

* no global runtime registry
* no dynamic type loading
* no runtime schema resolution

---

### 4. Compatibility Rules (Normative)

Compatibility is **directional**:

```text
OUT<T@vP> → IN<T@vC>
```

---

#### Governing Rule

> **Compatibility is defined by the consumer’s ability to safely interpret the producer output.**

---

#### Allowed

##### Exact match

```text
T@1 → T@1
```

---

##### Forward-compatible

```text
T@1 → T@2
```

Allowed if:

* v2 is backward-compatible with v1 (ADR-014)
* new fields are optional
* existing semantics unchanged
* consumer tolerates missing fields

---

#### Rejected

##### Backward (unsafe by default)

```text
T@2 → T@1
```

Reason:

* consumer cannot safely interpret additional or changed fields

Requires explicit adapter.

---

##### Different types

```text
EmailRaw → ParsedEmail
```

Requires explicit adapter.

---

##### Unknown compatibility

Rejected.

---

### 5. Adapter Nodes

Adapters are:

> **the only valid mechanism for transforming types**

Properties:

* explicit in graph
* scheduled like any component
* observable
* no implicit conversion

---

### 6. Validation Lifecycle

Validation is performed at control-plane boundaries:

---

#### Supported Mutation Operations

Supported mutation operations (aligned with control plane in ADR-006):

- graph:addnode
- graph:removenode
- graph:addedge
- graph:removeedge
- graph:changenode
- graph:changeedge
- graph:changegroup

Note: Configuration changes are performed via existing change operations (e.g. changenode, changeedge, changegroup) and are not represented as a separate mutation command.

#### Validation Points

##### 1. Graph Load

* full validation required

---

##### 2. Graph Start (`network:start`)

* validation rechecked

---

##### 3. Runtime Mutation

For each mutation:

* validation runs **before commit**
* invalid mutation is rejected

---

##### 4. Safe Mutation Boundary

Validation occurs only at control-plane safe points
(consistent with scheduler semantics)

Correlation requirements apply only to data-plane joins.
Nodes with multiple non-data inputs are not subject to correlation validation.

---

### 7. Correlation Rules

---

#### 7.1 Core Principle

Flowd provides:

* no global ordering
* at-least-once delivery

Therefore:

> **Correlation must be explicitly modeled in payload contracts.**

---

#### 7.2 Join Detection (Formalized)

A node is considered a **join node** if:

```text
number_of_connected_data_input_edges > 1
```

Definition:

- Data input edges:
  Edges connected to ports that carry data-plane messages.

- Excluded from join detection:
  - control ports
  - configuration ports
  - IIP-only inputs

Rationale:

Only data-plane inputs participate in correlation semantics.
Control and configuration inputs do not require correlation.

---

#### 7.3 Enforcement Rules

For join nodes:

* input types MUST define a correlation key

Defined via:

* schema annotation OR
* port metadata

---

#### 7.4 Disallowed

```text
Port<Any> → JoinNode
```

unless explicitly marked unsafe.

---

#### 7.5 Adapter Responsibility

If correlation is missing:

* adapter must introduce it

---

### 8. Dynamic / Unsafe Mode

Defined as:

```text
Port<Any>
```

Properties:

* disables type validation
* allowed only if explicitly declared
* produces validation warnings
* disables guarantees

---

### 9. Performance Model

* Core transport remains unchanged (ADR-008)
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

### Why directed compatibility

Because compatibility is asymmetric and defined by the consumer.

---

### Why lifecycle validation

Because graph mutation occurs at runtime (ADR-006).

---

### Why canonical type identity

Prevents incompatible parallel definitions.

---

### Why explicit adapters

Ensures observability and predictability.

---

### Why explicit correlation rules

Because ordering guarantees are limited (ADR-021).

---

## Alternatives Considered

---

### Component<I,O>

Rejected:

* does not support multi-port components
* prevents graph-level validation
* encourages incorrect abstraction

---

### Runtime typed messages

Rejected:

* breaks performance model
* violates separation of concerns

---

### Implicit conversion

Rejected:

* violates explicitness principle
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

* stricter modeling requirements
* need for adapter nodes
* increased upfront design effort

---

### Trade-offs

| Property     | Result                                  |
| ------------ | --------------------------------------- |
| Safety       | high                                    |
| Explicitness | maximal                                 |
| Flexibility  | reduced                                 |
| Performance  | unchanged (core), explicit adapter cost |

---

## Implementation Notes

* extend `ComponentPort.allowed_type`
* define `TypeId` format
* implement validator in control plane
* enforce validation at mutation operations
* define correlation metadata

---

## Related Decisions

* ADR-003: Message Model
* ADR-008: Transport Model
* ADR-006: Control Plane
* ADR-014: Versioning
* ADR-021: Execution Semantics

---

## Key Insight

> Type safety in flowd is enforced at the graph level, validated across the graph lifecycle, and independent of runtime transport.

---

## Open Questions

* Should schema validation be optional or mandatory in strict mode?
* Should adapter insertion be automatable?
* Should compatibility rules be extensible?
