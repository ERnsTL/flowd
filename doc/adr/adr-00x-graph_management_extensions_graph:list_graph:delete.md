# ADR-XXX: Graph Management Extensions (`graph:list`, `graph:delete`)

Status: Proposed
Date: 2026-04-22

## Context

The Flow-Based Programming (FBP) Network Protocol provides a **Graph protocol** for mutating graphs (e.g. `graph:addnode`, `graph:addedge`, `graph:clear`). However, the protocol **does not define any graph lifecycle or discovery operations**, such as:

* Listing available graphs
* Deleting graphs
* Querying graph metadata

Analysis of the official schema definitions (`graph.yml`, `shared.yml`) confirms that:

* No `graph:list` message exists
* No `graph:delete` (or equivalent) exists
* Graphs are addressed purely by `id` and assumed to exist

This leads to several practical issues for a runtime like **flowd**:

* No way to enumerate graphs for UI or tooling
* No clean deletion mechanism (only `graph:clear`, which resets but does not remove)
* Poor test isolation (graphs accumulate over time)
* Clients must maintain their own registry, causing duplication and inconsistency

Therefore, an explicit graph management layer is required.

---

## Decision

We introduce **non-standard extensions** to the Graph protocol:

* `graph:list`
* `graph:delete`

These are implemented as **flowd-specific protocol extensions**, not part of the official FBP specification.

They are gated behind a new capability:

```
graph:management
```

---

## Design

### 1. `graph:list`

Returns all graphs known to the runtime.

#### Request

```json
{
  "protocol": "graph",
  "command": "list",
  "payload": {}
}
```

#### Response (streaming, FBP-style)

```json
{
  "protocol": "graph",
  "command": "graph",
  "payload": {
    "id": "example",
    "name": "Example Graph"
  }
}
```

Final message:

```json
{
  "protocol": "graph",
  "command": "graphsdone",
  "payload": {}
}
```

#### Notes

* Streaming response aligns with existing FBP patterns (`component:list`)
* Allows large graph sets without blocking

---

### 2. `graph:delete`

Removes a graph from the runtime.

#### Request

```json
{
  "protocol": "graph",
  "command": "delete",
  "payload": {
    "id": "example"
  }
}
```

#### Response

```json
{
  "protocol": "graph",
  "command": "deleted",
  "payload": {
    "id": "example"
  }
}
```

#### Notes

* This is a **true deletion**, not equivalent to `graph:clear`
* The graph is removed from the internal registry and persistence layer

---

### 3. Capability

The runtime advertises support via:

```json
{
  "capabilities": [
    "protocol:graph",
    "graph:management"
  ]
}
```

Clients MUST check for `graph:management` before using these commands.

---

## Architecture

The extensions introduce a **Graph Management Layer** on top of the FBP protocol:

```
FBP Protocol (graph:*)
        ↓
Graph Management Extensions (list/delete)
        ↓
GraphStore (runtime)
    - HashMap<GraphId, Graph>
    - list()
    - delete()
    - persist()
```

---

## Consequences

### Positive

* Enables proper graph lifecycle management
* Simplifies client implementations
* Supports UI use cases (graph selection, dashboards)
* Enables clean test setup/teardown
* Aligns with real-world needs of long-running runtimes

### Negative

* Deviates from pure FBP protocol philosophy
* Reduces portability across runtimes (extension-specific)
* Requires capability negotiation handling

---

## Alternatives Considered

### 1. Do nothing (pure FBP)

Rejected:

* No way to list or delete graphs
* Leads to fragmentation across clients

---

### 2. Use `graph:clear` as delete

Rejected:

* Semantically incorrect (clear ≠ delete)
* Leaves empty graph shells
* Confusing for users and tooling

---

### 3. External API (HTTP/CLI)

Rejected:

* Splits responsibility across interfaces
* Breaks protocol consistency
* Adds operational complexity

---

## Rationale

The FBP protocol is designed for **graph mutation**, not **resource management**.

However, a production runtime requires:

* discoverability
* lifecycle control
* persistence management

Adding these as **explicit extensions with capability negotiation** preserves:

* protocol compatibility
* architectural clarity
* future extensibility

---

## Implementation Notes

* Backed by an in-memory or persistent `GraphStore`
* Deletion must remove:

  * graph structure
  * associated metadata
  * persisted state (if applicable)
* `graph:list` should be cheap (no heavy serialization)

---

## Future Work

* `graph:get` (fetch full graph)
* `graph:rename`
* `graph:metadata`
* Namespacing (multi-tenant graph stores)

---

## Summary

The FBP protocol lacks graph lifecycle management.
flowd introduces `graph:list` and `graph:delete` as **explicit, capability-gated extensions** to fill this gap cleanly without polluting the core protocol semantics.
