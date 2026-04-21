Perfekt — das ist der Punkt, wo sich entscheidet, ob flowd **verwendet wird oder nur bewundert**.
Die Architektur hast du – jetzt definierst du den **Hebel für Adoption**.

Ich mache das bewusst sehr konkret (Kategorien, Qualitätsregeln, Versionierung, Governance).

---

# ADR-0020: Standard Library & Component Ecosystem

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd is a compile-time integrated dataflow runtime with:

* Component-based execution model (ADR-0007)
* Compile-time integration (ADR-0001)
* Typed message model (ADR-0003)
* Backpressure and delivery guarantees (ADR-0004, ADR-0016)
* IO interaction model (ADR-0017)
* Packaging and deployment model (ADR-0019)

Flowd itself provides:

* runtime
* execution model
* infrastructure

However:

> Flowd is only usable if sufficient components exist to build real systems.

Target use cases include:

* automation systems (n8n-like)
* data pipelines (NiFi-like)
* event processing
* AI pipelines (OpenClaw-like)
* distributed systems
* simulation / MUD systems

This introduces a key requirement:

> A structured, consistent, and high-quality component ecosystem must exist.

---

## Problem Statement

We must define:

1. What constitutes a standard component
2. How components are categorized and distributed
3. How quality and compatibility are ensured
4. How the ecosystem evolves
5. How to avoid:

   * fragmentation
   * inconsistent behavior
   * low-quality components

---

## Constraints

* Components are compiled into binaries (ADR-0001)
* Must align with:

  * delivery semantics (ADR-0016)
  * IO model (ADR-0017)
  * resource model (ADR-0018)
* Must support versioning (ADR-0014)
* Must scale from small to large ecosystems

---

## Decision

Flowd adopts a **layered component ecosystem model** consisting of:

1. **Core Standard Library (official components)**
2. **Extended Libraries (official but optional)**
3. **Third-Party Ecosystem**
4. **Application-Specific Components**

All components must adhere to:

> **strict behavioral and interface contracts defined by the runtime**

---

## Core Model

---

### 1. Component Layers

---

#### 1.1 Core Standard Library

Maintained by:

> flowd project

Includes:

* fundamental dataflow primitives
* guaranteed availability
* highest stability requirements

---

#### 1.2 Extended Libraries

Maintained by:

* flowd or trusted contributors

Includes:

* IO integrations
* domain-specific components

---

#### 1.3 Third-Party Components

Developed by:

* external contributors
* organizations

Includes:

* custom integrations
* experimental features

---

#### 1.4 Application-Specific Components

Defined per project:

* business logic
* internal pipelines

---

## Component Categories

---

### 2.1 Flow Primitives

* map
* filter
* merge
* split
* buffer
* delay

---

### 2.2 Control Flow

* conditional routing
* branching
* retries
* loops (controlled)

---

### 2.3 IO Components

* HTTP client/server
* database connectors
* file system access
* message queues

---

### 2.4 Data Transformation

* parsing (JSON, CSV, etc.)
* encoding/decoding
* enrichment

---

### 2.5 AI / ML Components

* LLM calls
* embedding generation
* tool invocation

---

### 2.6 Utility Components

* logging
* metrics
* debugging tools

---

### 2.7 System Components

* proxy nodes
* persistence edges
* network edges

---

## Component Interface Requirements

---

### 3. Deterministic Behavior

Components must:

* behave deterministically where possible
* document non-determinism explicitly

---

### 4. Delivery Semantics Compliance

Components must:

* respect ACK/NACK model (ADR-0016)
* handle retries correctly
* be idempotent where required

---

### 5. Backpressure Compliance

Components must:

* not bypass backpressure
* respect input/output constraints

---

### 6. Resource Compliance

Components must:

* respect execution budgets (ADR-0018)
* avoid unbounded resource usage

---

### 7. IO Compliance

IO components must:

* implement retries
* support timeouts
* respect rate limits

---

## Versioning & Compatibility

---

### 8. Component Versioning

Each component must define:

* version
* compatibility guarantees

---

### 9. Compatibility Rules

* backward-compatible changes preferred
* breaking changes require version bump
* must align with ADR-0014

---

## Distribution Model

---

### 10. Component Distribution

Components are distributed via:

* source repositories
* package registries (optional)
* direct inclusion in projects

---

### 11. Build Integration

Components are included via:

* `flowd.build.toml` (ADR-0012)
* compile-time integration

---

## Quality Model

---

### 12. Quality Levels

Components are classified:

| Level        | Description       |
| ------------ | ----------------- |
| Core         | highest stability |
| Stable       | production-ready  |
| Experimental | not guaranteed    |

---

### 13. Testing Requirements

Components must include:

* unit tests
* integration tests
* failure scenario tests

---

### 14. Documentation Requirements

Each component must document:

* inputs/outputs
* behavior
* failure modes
* performance characteristics

---

## Governance Model

---

### 15. Core Governance

Core components:

* reviewed
* maintained centrally

---

### 16. Ecosystem Governance

Third-party components:

* self-managed
* optionally curated

---

### 17. Compatibility Enforcement

Runtime may:

* validate component interfaces
* warn on incompatibility

---

## Rationale

---

### Why Structured Ecosystem

Without structure:

* fragmentation occurs
* adoption suffers

---

### Why Layered Model

Allows:

* stability in core
* flexibility in ecosystem

---

### Why Not Fully Open Without Constraints

Leads to:

* inconsistent behavior
* poor reliability

---

### Why Compile-Time Integration Still Works

Even with ecosystem:

* components remain explicit
* builds remain deterministic

---

## Alternatives Considered

---

### Alternative 1: No Standard Library

**Pros:**

* flexible

**Cons:**

* unusable system

**Decision:**
Rejected

---

### Alternative 2: Fully Centralized Ecosystem

**Pros:**

* consistency

**Cons:**

* slow growth
* bottleneck

**Decision:**
Rejected

---

### Alternative 3: Fully Unregulated Ecosystem

**Pros:**

* fast growth

**Cons:**

* chaos
* poor quality

**Decision:**
Rejected

---

## Consequences

---

### Positive Consequences

* usable system out of the box
* scalable ecosystem
* consistent behavior
* faster adoption

---

### Negative Consequences

* governance overhead
* maintenance effort

---

### Trade-offs

| Property    | Decision |
| ----------- | -------- |
| Usability   | high     |
| Flexibility | high     |
| Complexity  | moderate |

---

## Implementation Notes

* define component templates
* provide starter libraries
* implement validation tools
* define registry (optional)

---

## Operational Impact

Operators benefit from:

* reusable components
* consistent behavior

---

## Interaction with Other ADRs

* ADR-0012:

  * build integration
* ADR-0016:

  * delivery semantics compliance
* ADR-0017:

  * IO behavior
* ADR-0018:

  * resource constraints

---

## Key Insight

> An engine without components is not usable — it is only potential.

---

## What This Enables

* rapid development
* ecosystem growth
* real-world adoption

---

## What This Avoids

* fragmentation
* inconsistent systems
* reinvention of basic components

---

## Open Questions

* Should a central registry be created?
* How to incentivize high-quality components?
* Should certification be introduced?
* How to manage ecosystem scale?

---

---

# 🧠 Einordnung

Das ist:

> **dein größter Hebel für Markt und Umsatz**

---

# 🔥 Wichtigster Satz

> **An engine without components is not usable — it is only potential.**

---

# 🎯 Status jetzt

Du bist jetzt bei:

* Architektur ✔
* Runtime ✔
* Betrieb ✔
* Deployment ✔
* Ecosystem ✔

👉 Es fehlt nur noch:

---

# 🚀 Letzter Schritt

👉 ADR-021: **Formal Guarantees & Execution Semantics**

Das ist der Unterschied zwischen:

* „cooles System“
* und
* **„vertrauenswürdiges System für große Kunden“**

Wenn du willst, mache ich dir den letzten genauso sauber.
