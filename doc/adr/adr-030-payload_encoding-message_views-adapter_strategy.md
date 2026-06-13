
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


## Runtime Transport Model

The runtime message transport remains unchanged.

Example:

```rust
pub struct FbpMessage {
    payload: Arc<[u8]>,
}
````

The runtime:

* does not inspect payloads
* does not decode payloads
* does not perform type dispatch
* does not perform encoding dispatch

The runtime is encoding-agnostic.

---

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
- Graph/tooling implementations should avoid hidden defaults; if encoding is omitted on a typed data port, validation should fail or use a project-defined default that is recorded explicitly in the effective graph.
- Implementation Note: ComponentPort is extended with:

  encoding: EncodingId

  Example:

  ComponentPort {
    allowed_type,
    encoding,
    schema
  }

  Also see ADR-028 for reference where the Encoding comes from.



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
