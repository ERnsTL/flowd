
# ADR-030: Payload Encoding, Message Views & Adapter Strategy

Status: Accepted

Date: 2026-06-13

## Context

ADR-003 and ADR-008 define flowd's runtime transport model.

Messages are transported as opaque payloads through ring buffers
without runtime knowledge of application-level types.

ADR-028 and ADR-029 introduce:

- TypeId-based contracts
- Registry-defined compatibility
- Graph validation
- Explicit adapter nodes

However, payload encoding remains undefined.

Questions include:

- How should payload encodings be represented?
- Should the runtime know about encodings?
- How are rkyv, Flexbuffers, JSON, Cap'n Proto, or future formats handled?
- How can zero-copy access be achieved without coupling the runtime to specific serialization technologies?

A consistent model is required.


## Decision

Payload encoding SHALL be treated as a property of a port contract.

Payload encoding SHALL NOT be treated as a property of the runtime transport layer.

The runtime transports opaque payload bytes only.

Encoding-specific interpretation is delegated to:

- components
- message views
- explicit adapter components

### Classic FBP Compatibility

flowd SHALL remain capable of classic Flow-Based Programming operation.

Built-in primitive message types and control IPs:

- String
- Bytes
- Integer
- Float
- Boolean
- OpenBracket
- CloseBracket

do not require:

- TypeId declarations
- EncodingId declarations
- Schema definitions
- Registry entries

Typed contracts extend classic FBP but do not replace it.

EncodingId is not part of FbpMessage.

EncodingId is metadata associated with port contracts and validation.

Runtime transport remains encoding-agnostic.


## Runtime Transport Model

The runtime message transport remains unchanged.

Example:

```rust
pub struct FbpMessage {
    payload: Arc<[u8]>,
}
```

The runtime:

* does not inspect payloads
* does not decode payloads
* does not perform type dispatch
* does not perform encoding dispatch

The runtime is encoding-agnostic.


## Port Contracts

Port contracts are extended with an encoding identifier.

Example:

```rust
PortContract {
    type_id: "email/ParsedEmail@1",
    encoding: "rkyv"
}
```

Examples:

```text
email/ParsedEmail@1 + rkyv
email/ParsedEmail@1 + flexbuffers
email/ParsedEmail@1 + capnp
email/ParsedEmail@1 + json
```

Encoding becomes part of compatibility validation.


## Primitive and Structured Payloads

Graph/tooling implementations should avoid hidden defaults. EncodingId is mandatory for structured typed payloads. EncodingId is not required for primitive built-in message types. If a structured typed port omits EncodingId, validation SHALL fail.

### Rationale

Classic Flow-Based Programming predates schema-driven and
encoding-aware message contracts.

Primitive message types such as:

- String
- Bytes
- Boolean
- Integer
- Float

have an intrinsic runtime representation and therefore do not
require an explicit EncodingId.

Requiring EncodingId declarations for primitive message types
would add configuration overhead without providing additional
semantic information.

Structured typed payloads are fundamentally different.

A TypeId alone identifies the semantic meaning of a payload,
but does not define its binary representation.

For example:

    email/ParsedEmail@1

may be represented using:

- rkyv
- flexbuffers
- capnp
- json

or future encodings.

Without an explicit EncodingId, two components could agree on
TypeId compatibility while remaining unable to interpret the
payload representation.

Therefore:

- primitive built-in message types do not require EncodingId
- structured typed payloads require EncodingId
- encoding mismatches require explicit adapter components

This preserves compatibility with classic FBP while enabling
multiple serialization technologies for structured payloads.


## Compatibility Rules

Two ports are compatible if:

1. TypeId compatibility succeeds according to ADR-029.
2. Encoding compatibility succeeds.

Examples:

Allowed:

```text
Email@1+rkyv
 ->
Email@1+rkyv
```

Adapter required:

```text
Email@1+rkyv
 ->
Email@1+flexbuffers
```

Rejected:

```text
Email@1+rkyv
 ->
Order@1+rkyv
```

### Encoding Compatibility Error Mapping

Encoding compatibility is evaluated after successful TypeId compatibility validation.

Results:

- exact EncodingId match -> compatible
- different EncodingId -> E_TYPE_ADAPTER_REQUIRED

Encoding mismatches are not treated as E_TYPE_INCOMPATIBLE because
the semantic type contract remains compatible.

The required transformation must be represented by an explicit
adapter component.

### Validation Scope

Ports are classified into:

1. Primitive Ports
2. Structured Typed Ports

The compatibility rules in this section apply only to
structured typed ports.

Primitive ports are validated according to the classic FBP model and are excluded from TypeId and EncodingId compatibility checks.

Primitive Ports:

- String
- Bytes
- Boolean
- Integer
- Float
- OpenBracket
- CloseBracket

Structured Typed Ports:

- any port declaring a TypeId other than built-in primitive types

Validation Rules:

Primitive Ports:
- no TypeId required
- no EncodingId required
- excluded from ADR-029 registry validation

Structured Typed Ports:
- TypeId required
- EncodingId required
- subject to ADR-028 and ADR-029 validation

Mixed connections between primitive and structured ports require
explicit adapter components.


## Canonical Built-In EncodingIds

The following identifiers are reserved:

- rkyv
- flexbuffers
- capnp
- json

Aliases are not permitted.

Examples of invalid aliases:

- capnproto
- capn_proto
- flat-json
- JSON

Tooling MUST normalize user input to canonical identifiers
or reject invalid aliases.


## Message Views

Components SHALL access payload data through encoding-specific views.

Examples:

```rust
RkyvView<T>
FlexbuffersView
CapnpView<T>
JsonView
```

Views provide:

* decoding
* validation
* zero-copy access where supported

Views are component-level abstractions.

Views are not part of the runtime core.


## Adapter Components

Encoding transformations SHALL be represented by explicit adapter components.

Examples:

```text
RkyvToFlexbuffersAdapter
FlexbuffersToJsonAdapter
JsonToCapnpAdapter
```

Implicit runtime conversion is prohibited.

All payload transformations must be visible in the graph.

This preserves:

* observability
* predictability
* deterministic validation


## Supported Encodings

This ADR does not mandate any specific encoding.

Implementations may provide:

* rkyv
* Flexbuffers
* Cap'n Proto
* JSON
* future encodings

through adapter components and view implementations.


## Recommended Usage

### rkyv

Recommended for:

* Rust-only execution paths
* high-performance internal graph segments
* stable internal contracts

Advantages:

* true zero-copy access
* minimal runtime overhead

### Flexbuffers

Recommended for:

* dynamic payloads
* external integrations
* cross-language communication

Advantages:

* schema-less
* portable
* available across multiple languages

### JSON

Recommended for:

* debugging
* diagnostics
* human-facing interfaces

Not recommended for performance-sensitive paths.


## Rationale

This approach preserves the separation established by ADR-003 and ADR-008.

Transport remains:

* simple
* fast
* encoding-agnostic

Encoding becomes a graph-level concern rather than a runtime concern.

This avoids:

* runtime type dispatch
* runtime encoding dispatch
* serialization technology lock-in

while enabling:

* zero-copy execution
* multiple encoding technologies
* explicit graph-level transformations


## Consequences

Positive:

* Runtime remains minimal.
* New encodings can be added without modifying the runtime.
* Adapter nodes remain first-class.
* Zero-copy technologies can be used where appropriate.
* External systems can communicate using their native formats.

Negative:

* Additional adapter components may be required.
* Encoding compatibility becomes part of graph validation.
* Graphs may become more explicit.

These costs are considered acceptable because they improve
observability and maintain architectural consistency.


## Future Work

Potential future ADRs may define:

* Encoding registry model
* Standard encoding identifiers
* Adapter discovery mechanisms
* Encoding capability negotiation

These topics are explicitly out of scope for this ADR.

## Implementation Clarifications

- Encoding is part of the port contract identity for validation purposes: `(TypeId, EncodingId)`.
- Encoding compatibility is exact-match only in core validation. If `EncodingId` differs, an explicit adapter node is required.
- Recommended canonical `EncodingId` grammar:
  `encoding := [a-z][a-z0-9_\-]{0,31}`
  Examples: `rkyv`, `flexbuffers`, `capnp`, `json`.
- Encoding identifiers are case-sensitive in storage but SHOULD be normalized to lowercase by tooling before persistence.
- Validation order for an edge is: TypeId compatibility first (ADR-029), then encoding compatibility. Type mismatch takes precedence over encoding mismatch.
- Runtime must continue to treat payload as opaque bytes; decode/encode failures occur in components/views/adapters and are reported through component error paths, not transport-layer errors.
- Adapters may transform type, encoding, or both, but each transformation MUST be explicit in graph topology and must not be inserted implicitly by runtime.
- Implementation Note: ComponentPort is extended with:

  encoding: EncodingId

  Example:

  ComponentPort {
    allowed_type,
    encoding,
    schema
  }

  Also see ADR-028 for reference where the Encoding comes from.

### ComponentPort Encoding Cardinality

ComponentPort.encoding SHALL be optional.

Reference model:

ComponentPort {
    allowed_type: Option<TypeId>,
    encoding: Option<EncodingId>,
    schema: Option<SchemaRef>
}

Primitive Port:

  allowed_type = None
  encoding = None
  schema = None

  or (if Primitive Types are modelled via Builtin-TypeIds):

  allowed_type = core/String@1
  encoding = None
  schema = None

Structured Typed Port:

  allowed_type = email/ParsedEmail@1
  encoding = rkyv
  schema = ...

### EncodingId Canonicalization Algorithm

1. Trim surrounding whitespace.
2. Convert to lowercase.
3. Validate against EncodingId grammar.
4. Validate against reserved EncodingId registry.

The resulting normalized string is the canonical EncodingId.

Aliases are not permitted.

Example:

"JSON" -> "json"
"CapNP" -> "capnp"

"capnproto" -> rejected


## Commentary

Ich würde ADR-030 tatsächlich schreiben, aber mit einer wichtigen Nuance:

> **ADR-030 sollte nicht "rkyv vs Flexbuffers" entscheiden.**
>
> ADR-030 sollte entscheiden:
>
> **"Encoding is a contract property, not a transport property."**

Das ist die eigentliche Architekturentscheidung.

Ob später:

* rkyv
* Flexbuffers
* Cap'n Proto
* JSON

existieren, ist dann nur eine Registry-/Adapter-Frage.

Das verhindert, dass du in 2 Jahren ADR-030 wieder ändern musst.


---

Meine Einschätzung:

**ADR-030 ist deutlich wichtiger als die Wahl zwischen rkyv und Flexbuffers.**

Die eigentliche Architekturentscheidung lautet:

> **Encoding gehört in den Contract, nicht in `FbpMessage`.**

Wenn du das sauber festschreibst, kannst du in 5 Jahren problemlos `rkyv`, `Flexbuffers`, `Arrow`, `FlatBuffers` oder etwas völlig Neues hinzufügen, ohne ADR-003, ADR-008, ADR-028 oder ADR-029 anfassen zu müssen. Das ist normalerweise ein Zeichen für eine gute ADR.

### 7. Die eigentliche Stärke des aktuellen Stands

Wenn ich ADR-028, 029 und 030 zusammen betrachte:

Dann habt ihr jetzt sauber getrennt:

| Ebene      | Verantwortlich |
| ---------- | -------------- |
| FbpMessage | Transport      |
| TypeId     | Semantik       |
| EncodingId | Repräsentation |
| Schema     | Struktur       |
| Registry   | Kompatibilität |
| Adapter    | Transformation |

Das ist architektonisch ziemlich elegant.
