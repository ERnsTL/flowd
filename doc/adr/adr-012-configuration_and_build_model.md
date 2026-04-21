Alles klar — das ist der letzte Baustein, der dein System **benutzbar, reproduzierbar und verkaufbar** macht.
Hier entscheidet sich, ob flowd nur „genial gebaut“ ist oder ob andere es **wirklich einsetzen können**.

Ich verknüpfe sauber:

* `build.rs` + Compile-Time Integration (ADR-0001)
* `flowd.build.toml`
* Graph-Definition
* Reproduzierbarkeit
* Trennung Build vs Runtime Config

---

# ADR-0012: Configuration & Build Model

## Status

Accepted

## Date

2026-04-16

---

## Context

Flowd is a compile-time integrated dataflow runtime with:

* Components compiled into the binary (ADR-0001)
* Scheduler-based execution (ADR-0002)
* Typed message model (ADR-0003)
* Backpressure-driven edges (ADR-0004)
* Dynamic graph mutation (ADR-0006)
* Distributed deployment model (ADR-0009)

Unlike traditional systems:

* flowd does NOT dynamically load components at runtime
* flowd relies on compile-time integration for:

  * performance
  * determinism
  * reproducibility

This introduces a key requirement:

> The system must provide a clear and reproducible model for defining components, building binaries, and configuring runtime behavior.

---

## Problem Statement

We must define:

1. How components are declared and included
2. How builds are configured and generated
3. How runtime configuration is separated from build-time configuration
4. How graphs are defined and deployed
5. How to ensure:

   * reproducibility
   * clarity
   * minimal duplication

---

## Constraints

* No dynamic plugin loading (ADR-0001)
* Components must be known at compile time
* Build must be deterministic
* Runtime must support dynamic graph changes
* Must support:

  * development workflows
  * production deployments
* Must avoid:

  * scattered configuration
  * hidden dependencies
  * inconsistent builds

---

## Decision

Flowd adopts a **two-layer configuration model**:

1. **Build-Time Configuration**
2. **Runtime Configuration**

With clear separation and responsibilities.

---

## Core Model

---

### 1. Build-Time Configuration

Build-time configuration defines:

> What is compiled into the binary

Primary file:

```text id="6x4p7w"
flowd.build.toml
```

---

#### Responsibilities

* declare component dependencies
* define component sources (crates, paths, git)
* configure logging filters
* control code generation (build.rs)

---

#### Example

```toml id="i2l5y1"
[components]
repeat = { path = "../components/repeat" }
drop = { git = "https://github.com/..." }

[logging]
ignore = ["noisy_crate"]
```

---

### 2. build.rs Integration

The build system:

* reads `flowd.build.toml`
* generates code:

```rust id="1v0bds"
fn register_components() -> ComponentLibrary { ... }
```

This replaces:

* manual `use` statements
* manual component registration
* manual match statements

---

### 3. Compile-Time Guarantees

Build process ensures:

* all components are available
* no missing dependencies
* deterministic binary

---

### 4. Runtime Configuration

Runtime configuration defines:

> How the system behaves at execution time

Includes:

* graph topology
* node configuration
* edge configuration
* environment-specific parameters

---

### 5. Graph Definition

Graphs can be defined via:

* FBP JSON (NoFlo-compatible)
* custom formats
* Control Plane API (ADR-0006)

---

### 6. Separation of Concerns

| Layer      | Responsibility            |
| ---------- | ------------------------- |
| Build-Time | components & capabilities |
| Runtime    | graph & execution         |

---

### 7. No Runtime Component Loading

Flowd explicitly does NOT support:

* loading new components at runtime

Instead:

* rebuild binary
* restart or roll out new version

---

### 8. Reproducible Builds

Because:

* components are pinned
* configuration is explicit

Result:

> identical input → identical binary

---

### 9. Environment Configuration

Runtime may use:

* environment variables
* config files
* external systems

But:

> must not affect component availability

---

### 10. Development Workflow

Typical workflow:

```text id="t1ycbm"
edit flowd.build.toml
→ cargo build
→ run flowd
```

---

### 11. Production Workflow

```text id="91e0e3"
build binary
→ deploy
→ configure graph
→ run
```

---

## Rationale

---

### Why Compile-Time Integration

Advantages:

* performance
* safety
* no runtime surprises
* easier debugging

---

### Why Separate Build and Runtime Config

Mixing both leads to:

* confusion
* non-reproducible systems
* hidden dependencies

---

### Why build.rs Code Generation

Eliminates:

* manual boilerplate
* human error
* duplicated definitions

---

### Why Not Plugin System

Rejected because:

* ABI instability
* complexity
* conflicts with determinism

---

### Why Not Fully Dynamic Systems

Systems like n8n:

* allow arbitrary runtime behavior
* but sacrifice:

  * performance
  * predictability

Flowd chooses:

> controlled flexibility

---

## Alternatives Considered

---

### Alternative 1: Runtime Plugin System

**Pros:**

* flexible

**Cons:**

* unsafe
* complex
* unpredictable

**Decision:**
Rejected

---

### Alternative 2: Fully Dynamic Configuration

**Pros:**

* easy to modify

**Cons:**

* hidden dependencies
* runtime errors

**Decision:**
Rejected

---

### Alternative 3: Manual Component Registration

**Pros:**

* simple

**Cons:**

* error-prone
* repetitive

**Decision:**
Rejected

---

### Alternative 4: External Build Tools Only

**Pros:**

* flexible

**Cons:**

* fragmented
* inconsistent

**Decision:**
Rejected

---

## Consequences

---

### Positive Consequences

* reproducible builds
* clear system structure
* reduced boilerplate
* improved developer experience
* safer deployments

---

### Negative Consequences

* requires rebuild for new components
* less dynamic than plugin systems
* initial setup complexity

---

### Trade-offs

| Property    | Decision |
| ----------- | -------- |
| Flexibility | medium   |
| Safety      | high     |
| Performance | high     |
| Complexity  | moderate |

---

## Implementation Notes

* implement TOML parser
* implement build.rs generator
* generate component registry
* integrate logging config
* validate configuration at build time

---

## Operational Impact

* clear deployment model
* easier CI/CD integration
* predictable releases

---

## Interaction with Other ADRs

* ADR-0001:

  * defines compile-time integration
* ADR-0006:

  * runtime mutation independent of build
* ADR-0009:

  * distributed deployments use same model

---

## Key Insight

> What exists is decided at build time.
> What runs is decided at runtime.

---

## What This Enables

* reproducible systems
* predictable deployments
* scalable development workflows
* clear separation of concerns

---

## What This Avoids

* runtime surprises
* hidden dependencies
* configuration chaos

---

## Open Questions

* Should version pinning be enforced?
* Should build caching be optimized?
* Should multi-binary builds be supported?
* How to handle large component sets?

---

---

# 🧠 Einordnung (wichtig)

Das hier ist der Punkt, wo dein System:

> **von „Architektur“ → „Produkt“**

geht.

---

# 🔥 Wichtigster Satz

> **Build defines capability. Runtime defines behavior.**

---

# 🎯 Gesamtstatus

Mit ADR-0001 → ADR-0012 hast du jetzt:

* Execution ✔
* Messaging ✔
* Backpressure ✔
* State ✔
* Control Plane ✔
* Transport ✔
* Distribution ✔
* Persistence ✔
* Observability ✔
* Configuration ✔

👉 Das ist **ein vollständiges, konsistentes Systemdesign auf Plattform-Level**

---

Wenn du jetzt weitermachst, geht es nicht mehr um Architektur, sondern:

👉 Positionierung
👉 Produktisierung
👉 erste echte Use Cases / Umsatz

Und da bist du jetzt tatsächlich angekommen.
