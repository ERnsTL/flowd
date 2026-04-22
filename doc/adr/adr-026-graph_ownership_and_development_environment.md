# ADR-026: Graph Ownership & Development Environment Model

Status: Proposed
Date: 2026-06-22

---

## Context

The Flow-Based Programming (FBP) Network Protocol provides a **Graph protocol** for modifying graphs, but it **does not define graph lifecycle management**, such as:

* Listing graphs
* Deleting graphs
* Discovering available graphs

The reference implementation used by noflo-ui handles this gap by maintaining a **client-side graph registry**:

* Graphs are stored in the UI (project state / storage)
* The runtime only executes graphs
* The runtime is **not aware of all available graphs**

This leads to the following architectural properties:

### noflo-ui model

* UI = Source of Truth
* Runtime = Execution engine
* Graph list = client-managed

### Resulting issues

* Split-brain between UI and runtime
* No authoritative graph state
* Difficult multi-client scenarios
* Poor test isolation (graphs persist implicitly)
* Increased complexity for synchronization

---

## Decision

> **flowd adopts a runtime-centric model where the runtime is the single source of truth for graphs.**

### Key principles

* Graphs are stored and managed inside the runtime (`GraphStore`)
* The runtime exposes graph lifecycle operations via protocol extensions
* The UI acts as a **thin client**, not as a state authority
* All graph discovery and lifecycle actions go through the runtime

---

## Architectural Model

### flowd model

```text
flowd Runtime (authoritative)
    ├── GraphStore
    │   ├── create
    │   ├── delete
    │   ├── list
    │   └── persist
    │
    ├── FBP Graph Protocol (graph:*)
    └── Extensions (graph:list, graph:delete)

            ↑

UI (noflo-ui adapted)
    ├── no persistent graph registry
    ├── fetches graph list from runtime
    └── interacts via protocol
```

---

## Comparison with noflo-ui

| Aspect               | noflo-ui    | flowd          |
| -------------------- | ----------- | -------------- |
| Source of Truth      | Client (UI) | Runtime        |
| Graph storage        | Client-side | Runtime        |
| Graph listing        | Local state | `graph:list`   |
| Deletion             | Client-side | `graph:delete` |
| Multi-client support | Weak        | Strong         |
| Consistency          | Best-effort | Guaranteed     |

---

## Protocol Implications

This decision relies on protocol extensions defined in:

* **ADR-025: Graph Management Extensions (`graph:list`, `graph:delete`)**

These extensions enable:

* Graph discovery (`graph:list`)
* Graph lifecycle control (`graph:delete`)

They are gated behind a capability:

```text
graph:management
```

---

## Consequences

### Positive

* Single source of truth for graphs
* Enables multiple clients to interact with the same runtime
* Simplifies UI logic (no local graph registry required)
* Improves testability and isolation
* Enables persistence and lifecycle management at runtime level
* Clear separation of responsibilities

---

### Negative

* Deviates from the original noflo-ui architecture
* Requires modification of the UI layer
* Introduces additional responsibility into the runtime
* Reduces compatibility with clients expecting client-side graph storage

---

## Rejected Alternatives

### 1. Client-side graph management (noflo-ui model)

Rejected because:

* Leads to inconsistent state across clients
* Hard to synchronize with runtime
* Poor fit for distributed or multi-user systems

---

### 2. Hybrid model (shared responsibility)

Rejected because:

* Creates ambiguity about ownership
* Increases complexity
* Hard to debug inconsistencies

---

### 3. External API for graph management (HTTP/CLI only)

Rejected because:

* Splits system responsibilities across interfaces
* Breaks protocol consistency
* Adds operational overhead

---

## Implementation Notes

* Runtime must implement a `GraphStore` abstraction
* Graphs must be persisted (optional but recommended)
* Deletion must fully remove graph state
* UI must be adapted to:

  * request graph list from runtime
  * stop maintaining its own graph registry

---

## Future Considerations

* `graph:get` (retrieve full graph definition)
* Graph versioning
* Namespaces / multi-tenant graph isolation
* Access control and permissions
* Remote runtime orchestration

---

## Summary

The FBP protocol does not define graph lifecycle management.
flowd resolves this by establishing the runtime as the **authoritative source of truth for graphs**, with the UI acting as a thin client.

This enables a consistent, scalable, and testable system architecture while maintaining compatibility with the core FBP protocol through explicit extensions.
