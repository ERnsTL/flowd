Sehr gutes Review – der Kollege trifft ziemlich präzise die Punkte, wo aus „gute Richtung“ → „wirklich tragfähige Architektur“ wird.

Ich gehe das erst kurz **kommentierend** durch (klar, direkt), und danach bekommst du eine **überarbeitete ADR-028 v2**, die die Lücken sauber schließt.

---

# 🧠 1. Kommentar zu den Punkten (kurz & ehrlich)

---

## ✅ 1. Validation Lifecycle (HIGH) — absolut richtig

Dein aktuelles ADR denkt implizit „build-time“.

Das ist falsch für dein System.

> **flowd ist kein statisches System → Validierung ist ein Lifecycle-Thema.**

Mit  (ADR-0006) hast du:

* `graph:addedge`
* `graph:removeedge`
* Hot Mutation
* Safe Points

👉 Konsequenz:

> **Validation muss Teil des Control Plane sein, nicht nur Build-Time**

---

## ✅ 2. Type Identity / Compatibility — kritischster Punkt

Der Experte hat hier völlig recht.

Aktuell ist dein ADR:

> „widening, subtype, future…“

➡️ Das ist **nicht implementierbar**.

Mit  (ADR-0014):

* Versionierung ist bereits formalisiert
* aber nicht auf Typen angewendet

👉 Du brauchst:

* **canonical type identifier**
* **version semantics**
* **compatibility matrix**

Sonst:

> Jeder baut sein eigenes Typsystem → Chaos

---

## ✅ 3. Mapping zu bestehendem Modell — sehr wichtig

Du hast schon:

```rust
ComponentPort {
  allowed_type,
  schema
}
```

Wenn ADR-028 das ignoriert:

> baust du **zweites Typsystem parallel** → Designfehler

---

## ⚠️ 4. Performance Claim — korrekt relativieren

Deine Aussage war zu absolut.

Richtig ist:

> Runtime bleibt gleich
> **Graph kann teurer werden (Adapter!)**

Aber:

👉 das ist gewollt und sichtbar → also ok

---

## ⚠️ 5. Correlation — sehr guter Catch

Das ist wichtig:

> **Typisierung löst KEIN Korrelationsthema**

Mit  (ADR-0021):

* kein global ordering
* at-least-once

👉 heißt:

> **UID etc. muss explizit im Payload sein**

Das gehört ins ADR rein, sonst denken Leute:
„Types lösen das“

---

## ✅ 6. Status — klar

Das Ding ist aktuell **Proposed**, nicht Accepted.

---

# 🧱 2. Ergebnis: Was sich ändern muss

Du brauchst:

1. **Validation Lifecycle Definition**
2. **Type System konkretisieren**
3. **Mapping zu existing metadata**
4. **Performance Claim korrigieren**
5. **Correlation klarstellen**
6. Status → Proposed

# ADR-028: Typed Ports, Contracts & Graph Validation

## Status

**Proposed**

## Date

2026-05-01

---

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


## Summary regarding v2 of this ADR

With these changes, ADR-028 changes from "good idea" to "implementable, scalable foundation".

It was important to address the places where systems later implode:

* lifecycle
* type identity
* versioning
* implicit assumption
