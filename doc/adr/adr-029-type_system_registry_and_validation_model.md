# ADR-029: Type System, Registry and Validation Model

Status: Proposed
Date: 2026-05-01

## Context

ADR-028 defines graph-level typed ports, directed compatibility, lifecycle validation, adapter requirements, and correlation rules.

To implement ADR-028 consistently across runtime, control plane, and tooling, flowd needs a concrete specification for:

* `TypeId` representation and normalization
* schema strictness model
* Rust-level data model
* validator algorithm and error model

Without this, multiple incompatible implementations are likely.

---

## Problem Statement

We must make type validation deterministic and portable across:

* graph load/start
* graph mutations
* tooling (linting, UI, CI)

while preserving ADR-003/008 constraints:

* runtime transport remains `FbpMessage`
* no type-dispatch in hot path
* no implicit conversion

---

## Decision

Flowd introduces a **registry-backed, graph-scoped type contract model** with:

1. canonical `TypeId` grammar
2. two schema validation profiles (`minimal`, `strict`)
3. explicit Rust representations for types, schemas, and compatibility
4. deterministic validator phases with stable error codes

Type validation is performed in control-plane lifecycle hooks, not in data transport.

---

## 1. TypeId Format (Normative)

### 1.1 Canonical form

```text
<namespace>/<type>@<major>
```

Examples:

```text
email/EmailRaw@1
email/ParsedEmail@2
imap/MoveCommand@1
```

### 1.2 Grammar

```text
namespace := [a-z][a-z0-9_\-]{0,63}
type      := [A-Z][A-Za-z0-9]{0,63}
major     := [1-9][0-9]*
TypeId    := namespace "/" type "@" major
```

### 1.3 Normalization rules

* trim surrounding whitespace
* namespace must be lowercase
* type is case-sensitive
* major is integer and must be > 0
* normalized string is the identity key

### 1.4 Versioning rule

`major` is compatibility-significant. Any incompatible semantic change requires a new major.

---

## 2. Schema Structure & Validation Profiles

Schema remains optional at the port level, but behavior depends on validation profile.

### 2.1 `minimal` profile

Goal: low-friction migration.

Requirements:

* valid `TypeId` required
* registry entry required
* compatibility check by type/version rules
* schema presence optional

Guarantee level: type compatibility only.

### 2.2 `strict` profile

Goal: maximal contract safety.

Requirements:

* all `minimal` requirements
* schema required for non-`Any` ports
* schema compatibility checks required
* correlation key annotation required for join-node inputs

Guarantee level: type + structural contract.

### 2.3 Schema baseline (runtime-neutral)

ADR-029 does not mandate one schema language, but requires:

* machine-readable structure
* field optional/required semantics
* object/list/scalar typing
* correlation key annotation support

Recommended initial format: JSON Schema subset.

---

## 3. Rust Representation (Reference Model)

```rust
pub struct TypeId {
    pub namespace: String,
    pub name: String,
    pub major: u32,
}

pub enum SchemaProfile {
    Minimal,
    Strict,
}

pub struct TypeContract {
    pub type_id: TypeId,
    pub schema_ref: Option<String>,
    pub correlation_key_paths: Vec<String>,
}

pub struct RegistryEntry {
    pub type_id: TypeId,
    pub schema: Option<serde_json::Value>,
    pub compatible_from: Vec<TypeId>,
}

pub struct TypeRegistry {
    pub entries: std::collections::HashMap<String, RegistryEntry>,
}

pub enum CompatibilityResult {
    CompatibleExact,
    CompatibleDeclared,
    RequiresAdapter,
    IncompatibleType,
    IncompatibleVersion,
}
```

Notes:

* `ComponentPort.allowed_type` stores canonical `TypeId` string.
* `ComponentPort.schema` stores schema reference or inline schema identifier.
* `Port<Any>` is represented by reserved type ID: `core/Any@1`.
* core/Any@1 is a mandatory built-in registry entry present in every graph scope.

Semantics:

* `compatible_from` defines which producer TypeIds this type version can safely consume (consumer tolerance list).

schema_ref may reference:

- inline schema identifiers
- registry-based schema entries
- external schema resources

Resolution is implementation-defined but must be deterministic.

---

## 4. Validator Algorithm (Normative)

### 4.1 Inputs

* graph topology (nodes, edges, ports)
* component metadata (`ComponentPort.allowed_type`, `schema`)
* graph/deployment-scoped `TypeRegistry`
* selected `SchemaProfile`

### 4.2 Output

* `ValidationReport { errors: Vec<ValidationIssue>, warnings: Vec<ValidationIssue> }`
* mutation reject if any error exists

### 4.3 Stable phases

1. **Port Resolution**
* resolve source/target component ports for each edge
* fail if missing port metadata

2. **TypeId Parse & Normalize**
* parse `allowed_type` on both ports
* validate grammar
* resolve entries in registry

3. **Directed Compatibility**
* evaluate producer->consumer compatibility
* accept exact/declared compatible
* otherwise error `E_TYPE_ADAPTER_REQUIRED`

4. **Schema Checks (profile-dependent)**
* `minimal`: skip structural checks
* `strict`: require schemas and compatibility

5. **Join/Correlation Checks**
* identify join nodes: connected data input edges > 1
* for each join input, require correlation metadata (`strict`) or emit warning (`minimal`)
* disallow `core/Any@1` into join unless explicitly marked unsafe

6. **Dynamic/Unsafe Handling**
* if unsafe edge flag present, downgrade selected errors to warnings per policy
* never downgrade parse/registry resolution failures

7. **IIP Validation**

- for each IIP assignment:
  - resolve target port
  - validate TypeId compatibility
  - apply profile rules:
    - minimal → warning
    - strict → error

### 4.4 Pseudocode

```text
for edge in graph.edges:
  src = resolve_out_port(edge.src)
  tgt = resolve_in_port(edge.tgt)

  t_out = parse_type_id(src.allowed_type)
  t_in  = parse_type_id(tgt.allowed_type)

  compat = check_directed_compat(t_out, t_in, registry)
  if compat == Incompatible:
    error(E_TYPE_INCOMPATIBLE, edge)
  if compat == RequiresAdapter:
    error(E_TYPE_ADAPTER_REQUIRED, edge)

  if profile == Strict:
    enforce_schema(src, tgt, registry, edge)

for node in graph.nodes:
  if incoming_data_edges(node) > 1:
    enforce_correlation(node, profile)
```

incoming_data_edges is defined according to port classification rules specified in ADR-028.

Port Classification:

Each port MUST declare one of:

- data
- control
- config

This classification is part of ComponentPort metadata.

---

## 5. Lifecycle Integration

Validator runs at:

* graph load
* `network:start`
* pre-commit of graph mutation commands (`addnode`, `removenode`, `addedge`, `removeedge`, `changenode`, `changeedge`, and equivalent config mutations including all port and IIP mutation operations defined in the control plane
(see ADR-028))

Mutation is rejected on validation errors.


---

## 6. Error Model

Stable codes:

* `E_TYPE_PARSE_INVALID`
* `E_TYPE_UNKNOWN`
* `E_TYPE_INCOMPATIBLE`
* `E_TYPE_ADAPTER_REQUIRED`
* `E_SCHEMA_REQUIRED_STRICT`
* `E_SCHEMA_INCOMPATIBLE`
* `E_CORRELATION_REQUIRED`
* `W_UNSAFE_ANY_EDGE`
* `W_UNSAFE_CORRELATION_BYPASS`

Errors are machine-readable and include node/port/edge references.

---

## 7. Non-Goals

* no runtime hot-path type dispatch
* no automatic adapter insertion in core runtime
* no global mutable runtime type registry
* no implicit coercion between incompatible types

---

## Consequences

### Positive

* deterministic validator behavior
* portable contract checks across runtime/tooling
* clearer migration path from untyped graphs

### Negative

* additional metadata authoring effort
* schema governance overhead in strict mode

---

## Related Decisions

* ADR-003: Message Model
* ADR-006: Control Plane & Graph Mutation
* ADR-008: Transport and Memory Model
* ADR-014: Versioning and Compatibility
* ADR-021: Formal Guarantees
* ADR-028: Typed Ports, Contracts & Graph Validation

---

## Open Questions

* Should schema language be fixed (JSON Schema subset) in a follow-up ADR?
* Should compatibility declarations be per-type only, or also per-port override?
* Should strict mode be default for production builds?
