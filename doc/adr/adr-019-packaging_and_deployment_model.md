Jetzt kommt der Schritt, der entscheidet, ob jemand flowd **wirklich einsetzen kann** – nicht nur bauen.
Ich verknüpfe sauber: Compile-Time Model (ADR-0001), Distribution (ADR-0009), Versioning (ADR-0014), Control Plane (ADR-0006).

---

# ADR-0019: Packaging & Deployment Model

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd is a compile-time integrated dataflow runtime with:

* Components compiled into the binary (ADR-0001)
* Configuration & build model (ADR-0012)
* Distribution via subgraphs (ADR-0009)
* Control plane for graph mutation (ADR-0006)
* Versioning and compatibility model (ADR-0014)
* Persistence and delivery guarantees (ADR-0010, ADR-0016)

Unlike dynamic systems:

* flowd does not load components at runtime
* capabilities are fixed at build time
* behavior is defined at runtime via graph configuration

This leads to a key requirement:

> The system must provide a clear, reproducible, and operable packaging and deployment model across environments.

---

## Problem Statement

We must define:

1. What constitutes a deployable unit
2. How binaries, graphs, and configuration are packaged
3. How deployments are performed and updated
4. How distributed deployments are structured
5. How to avoid:

   * configuration drift
   * inconsistent deployments
   * unclear ownership of artifacts

---

## Constraints

* Compile-time component model (ADR-0001)
* Runtime graph mutation possible (ADR-0006)
* Distributed subgraphs (ADR-0009)
* Versioning must be respected (ADR-0014)
* Must support:

  * local development
  * production environments
* Must remain simple and explicit

---

## Decision

Flowd adopts a **multi-artifact deployment model** consisting of:

1. **Binary Artifact (runtime + components)**
2. **Graph Artifact (topology + configuration)**
3. **Environment Configuration (externalized runtime parameters)**

These are combined into a **Flowd Application Deployment Unit**.

---

## Core Model

---

### 1. Deployment Artifacts

---

#### 1.1 Binary Artifact

Contains:

* compiled runtime
* all components (ADR-0001)
* build-time configuration

Properties:

* immutable
* versioned
* reproducible

---

#### 1.2 Graph Artifact

Defines:

* nodes
* edges
* configuration
* topology

Formats:

* FBP JSON
* custom formats
* Control Plane definitions

---

#### 1.3 Environment Configuration

Includes:

* credentials
* endpoints
* environment-specific parameters

Sources:

* environment variables
* config files
* secret managers

---

### 2. Flowd Application

A deployable unit consists of:

```text id="appunit1"
(flowd binary) + (graph definition) + (environment config)
```

---

### 3. Separation of Concerns

| Artifact    | Responsibility               |
| ----------- | ---------------------------- |
| Binary      | capabilities                 |
| Graph       | behavior                     |
| Environment | deployment-specific settings |

---

### 4. Deployment Modes

---

#### 4.1 Single-Node Deployment

* one binary
* one or multiple graphs
* local execution

---

#### 4.2 Multi-Node / Distributed Deployment

* multiple binaries
* each runs a subgraph
* connected via network edges

---

### 5. Graph Deployment

Graphs can be:

* loaded at startup
* deployed dynamically via Control Plane
* versioned and updated independently

---

### 6. Versioning

Each deployment includes:

* binary version
* graph version
* configuration version

Compatibility enforced via ADR-0014

---

### 7. Deployment Strategies

---

#### 7.1 Static Deployment

* binary + graph bundled
* deployed together

---

#### 7.2 Dynamic Graph Deployment

* binary deployed once
* graphs updated via Control Plane

---

#### 7.3 Rolling Updates

* deploy new binary version
* route traffic via proxy nodes (ADR-0006)
* run old and new in parallel
* drain old version

---

#### 7.4 Blue/Green Deployment

* two parallel environments
* switch traffic explicitly

---

### 8. Packaging Formats

Possible formats:

* native binaries
* container images (Docker)
* system packages

---

### 9. Deployment Targets

Supported environments:

* local machines
* servers
* containers
* orchestrators (e.g. Kubernetes)

---

### 10. Distributed Topology

Each subgraph:

* deployed independently
* connected via network edges

---

### 11. Configuration Management

Configuration must be:

* externalized
* versioned
* environment-specific

---

### 12. Observability Integration

Deployment must include:

* metrics endpoints
* logging configuration
* tracing setup

---

### 13. Security Considerations

Deployment must handle:

* credentials securely
* network access control
* authentication for Control Plane

---

## Rationale

---

### Why Separate Binary and Graph

Advantages:

* flexibility
* reproducibility
* independent updates

---

### Why Not Fully Dynamic Systems

Dynamic systems:

* harder to reason about
* introduce runtime risk

Flowd prefers:

> controlled flexibility

---

### Why Multi-Artifact Model

Separating concerns:

* improves clarity
* reduces coupling
* supports different lifecycles

---

### Why Control Plane Deployment

Allows:

* dynamic updates
* runtime reconfiguration
* operational flexibility

---

### Why Not Single Monolithic Artifact

Monolith:

* inflexible
* harder to update

---

## Alternatives Considered

---

### Alternative 1: Single Monolithic Deployment

**Pros:**

* simple

**Cons:**

* inflexible
* hard to update

**Decision:**
Rejected

---

### Alternative 2: Fully Dynamic Runtime

**Pros:**

* flexible

**Cons:**

* unpredictable
* unsafe

**Decision:**
Rejected

---

### Alternative 3: External Orchestration Only

**Pros:**

* reuse tools

**Cons:**

* lacks system semantics

**Decision:**
Rejected as sole model

---

## Consequences

---

### Positive Consequences

* clear deployment model
* reproducible systems
* flexible updates
* scalable architecture

---

### Negative Consequences

* multiple artifacts to manage
* requires coordination between artifacts

---

### Trade-offs

| Property        | Decision |
| --------------- | -------- |
| Flexibility     | high     |
| Complexity      | moderate |
| Reproducibility | high     |

---

## Implementation Notes

* define application packaging format
* integrate build outputs with deployment
* provide CLI tools for deployment
* support containerization

---

## Operational Impact

Operators must:

* manage artifacts
* coordinate versions
* monitor deployments

---

## Interaction with Other ADRs

* ADR-0012:

  * build-time configuration
* ADR-0006:

  * dynamic graph updates
* ADR-0009:

  * distributed deployments
* ADR-0014:

  * version compatibility

---

## Key Insight

> A system that cannot be deployed predictably is not a system.

---

## What This Enables

* production deployments
* scalable systems
* controlled updates
* enterprise usage

---

## What This Avoids

* configuration chaos
* inconsistent environments
* deployment uncertainty

---

## Open Questions

* Should packaging format be standardized further?
* Should deployment automation tools be provided?
* How to handle large-scale deployments?
* Should rollback mechanisms be automated?

---

---

# 🧠 Einordnung

Das hier ist:

> **der Schritt von „Engine“ → „Produkt das jemand betreiben kann“**

---

# 🔥 Wichtigster Satz

> **A system that cannot be deployed predictably is not a system.**

---

# 🎯 Status jetzt

Mit ADR-015 → ADR-019 hast du:

* ✔ Failure
* ✔ Delivery
* ✔ IO
* ✔ Resource Control
* ✔ Deployment

👉 Das ist jetzt ein **vollständig betreibbares System**

---

# 🚀 Nächste zwei (letzte)

👉 ADR-020: **Component Ecosystem (extrem wichtig für Markt!)**
👉 ADR-021: **Formal Guarantees (extrem wichtig für Vertrauen!)**

Das sind die letzten zwei, die über Erfolg entscheiden.
