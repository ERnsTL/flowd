
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

- registry-defined TypeId declarations (primitive messages use reserved Built-In TypeIds instead).
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

Primitive ports use Built-In TypeIds and are validated using fixed built-in compatibility rules rather than registry-based ADR-029 compatibility rules.

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
- TypeId declaration in source graph is optional; after graph normalization, primitive ports SHALL carry the corresponding Built-In TypeId.
- no EncodingId required
- excluded from ADR-029 registry validation

Structured Typed Ports:
- TypeId required
- EncodingId required
- subject to ADR-028 and ADR-029 validation

Mixed connections between primitive and structured ports require
explicit adapter components.

### Primitive Compatibility Rules (Normative)

Primitive port compatibility is evaluated using Built-In TypeIds only.

Compatibility outcomes:

- exact Built-In TypeId match -> compatible
- different Built-In TypeIds -> E_TYPE_ADAPTER_REQUIRED

No implicit primitive conversion is allowed.

Any primitive-to-primitive conversion must be represented by an explicit adapter component.


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

### EncodingId registry

The EncodingId registry is extensible.

Built-in EncodingIds are reserved by flowd.

Additional EncodingIds may be registered by deployments,
graphs or extensions according to future registry ADRs.


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

For structured typed ports, contract identity is:
  (TypeId, EncodingId)

Primitive ports use built-in primitive TypeIds and do not participate in EncodingId validation.

- Encoding compatibility is exact-match only in core validation. If `EncodingId` differs, an explicit adapter node is required.
- Recommended canonical `EncodingId` grammar:
  `encoding := [a-z][a-z0-9_\-]{0,31}`
  Examples: `rkyv`, `flexbuffers`, `capnp`, `json`.
- EncodingIds SHALL be stored in canonical lowercase form.
- Validation order for an edge is: TypeId compatibility first (ADR-029), then encoding compatibility. Type mismatch takes precedence over encoding mismatch.
- Runtime must continue to treat payload as opaque bytes; decode/encode failures occur in components/views/adapters and are reported through component error paths, not transport-layer errors.
- Adapters may transform type, encoding, or both, but each transformation MUST be explicit in graph topology and must not be inserted implicitly by runtime.
- Implementation Note: ComponentPort is extended with:

  encoding: Option<EncodingId>

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

#### Primitive Port:

  allowed_type = core/String@1
  encoding = None
  schema = None

Primitive Ports SHALL use Built-In TypeIds. Primitive ports MAY omit TypeId declarations in graph definitions. During graph normalization, tooling SHALL materialize the corresponding Built-In TypeId before validation.

After normalization, all primitive ports SHALL have a Built-In TypeId.

The following Built-In TypeIds are reserved:

- core/String@1
- core/Bytes@1
- core/Bool@1
- core/Int64@1
- core/Float64@1
- core/OpenBracket@1
- core/CloseBracket@1

#### Structured Typed Port:

  allowed_type = email/ParsedEmail@1
  encoding = rkyv
  schema = ...

### Built-In Primitive TypeId Mapping

Human-facing primitive names map to the following canonical
Built-In TypeIds:

| Primitive Name | Built-In TypeId |
|----------------|-----------------|
| String         | core/String@1   |
| Bytes          | core/Bytes@1    |
| Boolean        | core/Bool@1     |
| Integer        | core/Int64@1    |
| Float          | core/Float64@1  |
| OpenBracket    | core/OpenBracket@1 |
| CloseBracket   | core/CloseBracket@1 |

These mappings are normative and SHALL be used by all tooling,
validators and graph normalization implementations.

### EncodingId Canonicalization Algorithm

1. Trim surrounding whitespace.
2. Convert to lowercase.
3. Validate against EncodingId grammar.
4. Validate against the EncodingId registry available to the graph, deployment or runtime environment.

EncodingIds SHALL be stored in canonical lowercase form.

The resulting normalized string is the canonical EncodingId.

Aliases are not permitted.

Example:

"JSON" -> "json"
"CapNP" -> "capnp"

"capnproto" -> rejected
