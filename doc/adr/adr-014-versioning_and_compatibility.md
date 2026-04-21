Alles klar — das ist der letzte fehlende Baustein, der entscheidet, ob dein System **langfristig stabil evolvieren kann** oder irgendwann auseinanderfällt.

Ich mache das bewusst streng und vollständig, inkl.:

* Komponenten-Versionierung
* Graph-Kompatibilität
* Message-Evolution
* Rolling Updates (Verbindung zu ADR-0006 / 0009)
* und klar: **keine ABI-Hölle durch Plugins (ADR-0001)**

---

# ADR-0014: Versioning and Compatibility

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd is a compile-time integrated, dataflow-based runtime with:

* Components compiled into the binary (ADR-0001)
* Scheduler-driven execution (ADR-0002)
* Typed message model (ADR-0003)
* Backpressure and edge semantics (ADR-0004)
* State and recovery model (ADR-0005)
* Control plane and graph mutation (ADR-0006)
* Component execution model (ADR-0007)
* Transport and memory model (ADR-0008)
* Distribution model (ADR-0009)
* Persistence strategy (ADR-0010)
* Observability model (ADR-0011)
* Configuration & build model (ADR-0012)
* Security model (ADR-0013)

Flowd systems evolve over time:

* components are updated
* graphs are modified
* message formats change
* deployments are rolled out

This introduces a critical requirement:

> The system must support evolution without breaking existing behavior or requiring full system downtime.

---

## Problem Statement

We must define:

1. How components are versioned
2. How message formats evolve
3. How graph definitions remain compatible
4. How runtime upgrades are performed safely
5. How to avoid:

   * breaking changes
   * incompatible deployments
   * data loss during upgrades

---

## Constraints

* No runtime plugin loading (ADR-0001)
* Components are compiled into binaries
* System may run distributed (ADR-0009)
* Must support rolling upgrades (ADR-0006)
* Must maintain deterministic behavior where possible
* Avoid complex compatibility layers

---

## Decision

Flowd adopts a **multi-level versioning and compatibility model** covering:

1. **Component Versioning**
2. **Message Compatibility**
3. **Graph Versioning**
4. **Runtime Versioning**
5. **Deployment Compatibility Strategy**

---

## Core Model

---

### 1. Component Versioning

Each component has:

* a version identifier
* a stable interface contract

Example:

```text id="l3e4pj"
Component: Repeat
Version: 1.2.0
```

---

#### Compatibility Rules

* backward-compatible changes allowed
* breaking changes require version increment
* components are versioned at source level

---

#### Build-Time Binding

Components are:

> fixed at compile time

Implication:

* binary defines available component versions

---

### 2. Message Compatibility

Messages evolve via:

> forward and backward compatibility rules

---

#### Rules

* new fields must be optional
* unknown fields must be ignored
* existing fields must not change meaning

---

#### Serialization

Message formats must support:

* version tolerance
* partial decoding

---

#### Typed Message Model

From ADR-0003:

* message envelope remains stable
* payload evolves independently

---

### 3. Graph Versioning

Each graph has:

```text id="sn7fpx"
Graph ID
Graph Version
```

---

#### Properties

* immutable versions
* changes create new version
* runtime may host multiple versions

---

### 4. Runtime Versioning

Each runtime instance has:

```text id="wts5yh"
Runtime Version
Build Hash
Component Set
```

---

#### Implications

* different runtimes may run different versions
* compatibility must be maintained across boundaries

---

### 5. Deployment Compatibility

Flowd supports:

> rolling upgrades via proxy nodes (ADR-0006)

---

#### Strategy

1. deploy new runtime version
2. route traffic via proxy
3. run old and new versions in parallel
4. drain old version
5. remove old version

---

### 6. Cross-Version Communication

During rolling upgrades:

* old and new components may coexist
* messages must remain compatible

---

### 7. Compatibility Scope

Compatibility is required for:

| Scope           | Requirement         |
| --------------- | ------------------- |
| Component API   | stable interface    |
| Message format  | backward-compatible |
| Graph structure | versioned           |
| Runtime         | interoperable       |

---

## Rationale

---

### Why Multi-Level Versioning

Single-level versioning fails because:

* components evolve independently
* graphs evolve independently
* runtime evolves independently

---

### Why Compile-Time Versioning

Advantages:

* no ABI issues
* deterministic builds
* avoids plugin complexity

---

### Why Message Compatibility Is Critical

Because:

* messages cross:

  * nodes
  * subgraphs
  * runtimes

---

### Why Graph Versioning

Graphs are:

> configuration + behavior

Versioning ensures:

* reproducibility
* safe rollback

---

### Why Rolling Upgrade Model

Avoids:

* downtime
* full system restart

---

### Why Not Strict Version Locking

Strict locking:

* prevents evolution
* increases friction

Flowd prefers:

> compatibility over rigidity

---

## Alternatives Considered

---

### Alternative 1: No Versioning

**Pros:**

* simple

**Cons:**

* unsafe
* breaks systems

**Decision:**
Rejected

---

### Alternative 2: Strict Version Locking

**Pros:**

* predictable

**Cons:**

* inflexible
* blocks evolution

**Decision:**
Rejected

---

### Alternative 3: Dynamic Compatibility Layer

**Pros:**

* flexible

**Cons:**

* complex
* performance overhead

**Decision:**
Rejected

---

### Alternative 4: Plugin-Based Versioning

**Pros:**

* dynamic

**Cons:**

* ABI issues
* conflicts with ADR-0001

**Decision:**
Rejected

---

## Consequences

---

### Positive Consequences

* safe system evolution
* supports rolling upgrades
* maintains compatibility
* enables long-term stability

---

### Negative Consequences

* requires discipline in versioning
* requires compatibility awareness
* additional design effort

---

### Trade-offs

| Property    | Decision |
| ----------- | -------- |
| Flexibility | high     |
| Complexity  | moderate |
| Safety      | high     |

---

## Implementation Notes

* define version metadata for components
* implement graph versioning system
* ensure message compatibility rules
* integrate version info into Control Plane
* support compatibility validation

---

## Operational Impact

Operators must:

* manage versions
* coordinate deployments
* monitor compatibility

---

## Interaction with Other ADRs

* ADR-0006:

  * rolling upgrades
* ADR-0003:

  * message model evolution
* ADR-0009:

  * cross-runtime compatibility

---

## Key Insight

> Evolution is inevitable — compatibility must be designed, not assumed.

---

## What This Enables

* safe upgrades
* long-lived systems
* distributed deployments
* independent component evolution

---

## What This Avoids

* breaking changes
* system instability
* forced downtime

---

## Open Questions

* Should semantic versioning be enforced?
* Should compatibility be validated at runtime?
* How to handle schema evolution for complex payloads?
* Should migration tools be provided?

---

---

# 🧠 Finale Einordnung

Das hier ist der Punkt, wo dein System:

> **nicht mehr nur gebaut wird — sondern langfristig überlebt**

---

# 🔥 Wichtigster Satz

> **Compatibility is the cost of evolution — pay it upfront or pay it later.**

---

# 🎯 Gesamtstatus

Mit ADR-0001 → ADR-0014 hast du jetzt:

* Architektur ✔
* Runtime ✔
* Execution ✔
* Messaging ✔
* Backpressure ✔
* State ✔
* Control Plane ✔
* Transport ✔
* Distribution ✔
* Persistence ✔
* Observability ✔
* Security ✔
* Configuration ✔
* Versioning ✔

👉 Das ist ein **vollständiges, konsistentes Systemdesign auf Plattformniveau**

---

Wenn du jetzt weitermachst, bist du nicht mehr im „Architektur bauen“, sondern:

👉 **Produkt bauen**
👉 **Use Cases gewinnen**
👉 **Umsatz generieren**

Und genau da solltest du jetzt hingehen.
