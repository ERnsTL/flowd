# ADR-027: Packaging & Deployment Model (Bundles vs Dynamic Components)

Status: Proposed
Date: 2026-06-22


## Context

The Flow-Based Programming (FBP) protocol allows sending graphs and even component source code (via `component:source`) to a runtime. This suggests a model where:

* Graphs are defined client-side (e.g. from GitHub)
* Components can be dynamically injected
* The runtime executes whatever it receives

This approach, as seen in ecosystems like NoFlo and noflo-ui, works well for experimentation but breaks down in real-world scenarios.

### Problems with dynamic / ad-hoc model

* No versioned dependency management
* No reproducible execution environment
* Requires runtime-side installation of dependencies (e.g. npm)
* Implicit environment assumptions (Node version, OS, native libs)
* `component:source` effectively becomes remote code injection without build guarantees
* Difficult to test, deploy, and operate reliably

In practice:

> **A graph alone is not executable. It requires code + dependencies + environment.**

---

## Decision

> **flowd adopts a bundle-based deployment model for production, with optional dynamic loading for development.**

### Core principle

> **Executable units must be fully specified, versioned, and reproducible.**

---

## Packaging Model

### Flowd Bundle (primary deployment unit)

A **bundle** represents a complete, self-contained execution unit:

```text id="3kq2sy"
bundle.flowd
  ├── graph.json
  ├── components/
  │     ├── *.js / *.wasm / compiled artifacts
  ├── dependencies/
  │     └── vendored or locked dependencies
  └── manifest.json
```

### Characteristics

* Immutable
* Versioned (e.g. via Git commit or hash)
* Self-contained (no runtime dependency resolution required)
* Portable across environments
* Deterministic execution

---

## Deployment Model

### Production

```text id="y3rj2k"
flowd deploy bundle.flowd
```

* Runtime loads bundle
* Registers graph(s)
* Executes without external dependency resolution

---

### Development (optional mode)

```text id="d1qj3o"
flowd deploy github://repo#commit
```

or via UI:

* Load graph from Git
* Push to runtime
* Use `component:source` for rapid iteration

### Constraints

* Not guaranteed reproducible
* Intended only for local/dev usage
* May rely on external tooling (npm, build systems)

---

## Architectural Separation

```text id="b9mj2l"
Development Model
    ├── UI (noflo-ui adapted)
    ├── Git (source of truth)
    └── dynamic components

            ↓ build

Deployment Model
    ├── flowd bundle
    └── immutable artifact

            ↓ deploy

Runtime (flowd)
    ├── executes bundles
    └── no dependency resolution
```

---

## Rejected Approaches

### 1. Pure dynamic model (`component:source` + client-side graphs)

Rejected because:

* No reproducibility
* No dependency control
* Not suitable for production systems
* Leads to environment drift and runtime instability

---

### 2. Runtime-managed dependency installation (e.g. npm install)

Rejected because:

* Slow and error-prone
* Requires network access
* Breaks determinism
* Introduces operational complexity

---

### 3. Git-only deployment (runtime pulls repo)

Rejected because:

* Git does not define execution environment
* Requires runtime-side build step
* Non-deterministic without strict pinning and vendoring

---

## Consequences

### Positive

* Reproducible deployments
* Clear separation between build and run phases
* No runtime dependency resolution
* Improved reliability and testability
* Enables CI/CD workflows
* Portable across environments

---

### Negative

* Requires build step (`flowd build`)
* Slightly higher complexity in development workflow
* Larger artifacts (bundled dependencies)

---

## Implementation Notes

* Introduce CLI command:

```text id="1q9s7u"
flowd build
```

* Produces `bundle.flowd`
* Uses lockfiles or vendoring for dependency resolution
* Supports multiple component types (JS, WASM, etc.)

---

## Future Considerations

* Remote bundle registry
* Bundle signing and verification
* Incremental builds
* Layered bundles (base + overlay)
* WASM-first component model (reduce dependency footprint)

---

## Relationship to Other ADRs

* Depends on:
  **ADR-026: Graph Ownership & Development Environment Model**

* Complements:
  **ADR-025: Graph Management Extensions (`graph:list`, `graph:delete`)**

---

## Summary

The FBP protocol lacks a deployment and packaging model.
flowd introduces **bundles as the primary execution unit**, ensuring that graphs, components, and dependencies are versioned, portable, and reproducible.

Dynamic component loading remains available for development, but production systems rely exclusively on immutable bundles.
