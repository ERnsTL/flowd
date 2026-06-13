## ADR Overview

* ADR-0001: Compile-Time Integration of Components: **What exists is decided at build time, not at runtime.**
* ADR-0002: Runtime Execution Model (Scheduler & Budgets): **Backpressure is local, fairness is global.**
* ADR-0003: Message Model (Typed vs Byte-Based): **While transport can be zero-copy, real-world usage patterns determine where copying actually happens.**
* ADR-0004: Backpressure, Delivery Semantics, and Edge Model: **Backpressure is not a feature — it is a system invariant.**
* ADR-0005: State Model, Snapshotting, and Checkpointing: **State is local, consistency is eventual, recovery is replay-driven.**
* ADR-0006: Control Plane & Graph Mutation: **Safe mutation requires isolation, staging, and explicit control.**
* ADR-0007: Component Execution Model: **Components are scheduled, not running.**
* ADR-0008: Transport and Memory Model (Ringbuffer, Arc, Zero-Copy): **Zero-copy is about avoiding unnecessary copies — not eliminating all copies.**
* ADR-0009: Distribution Model: **Distribution is composition, not coordination.**
* ADR-0010: Persistence & Storage Strategy: **Persistence belongs to the data path, not the computation.**
* ADR-0011: Observability Model: **You cannot operate what you cannot see.**
* ADR-0012: Configuration & Build Model: **Build defines capability, runtime defines behavior.**
* ADR-0013: Security Model: **Security must be explicit, layered, and composable — never assumed.**
* ADR-0014: Versioning and Compatibility: **Compatibility is the cost of evolution — pay it upfront or pay it later.**

Addons 2026-04-16:

* ADR-015: Failure & Recovery Model: **Failure is normal — recovery must be defined, not improvised.**
* ADR-016: Delivery Semantics & Acknowledgement Model: **Delivery guarantees must be explicit — otherwise they are undefined.**
* ADR-017: IO & External Systems Interaction Model: **External systems are unreliable — your runtime must absorb that uncertainty.**
* ADR-015,016,017 = reliability stack: Failure, Delivery, IO.
* ADR-018: Resource & Isolation Model: **Unbounded components will eventually destroy the system.**
* ADR-019: Packaging & Deployment Model: **A system that cannot be deployed predictably is not a system.**
* ADR-020: Standard Library & Component Ecosystem: **An engine without components is not usable — it is only potential.**
* ADR-021: Formal Guarantees & Execution Semantics: **If guarantees are not written down, they do not exist.**

This is now not a feature set anymore, this is a coherent philosophy and system model.
And this is an actual competitive advantage.

ADR-Prompt:

Ok dann Bitte erstelle ein Specdokument auf englisch für "ADR-021: Formal Guarantees & Execution Semantics" im ADR-Format. Arbeite alles ein was relevant ist und was ich dir vorher angehängt habe, alles was wir besprochen haben. Bitte alles vollständig und detailliert rein - verlustfrei, auch zwischenüberlegungen die wir besprochen haben und verworfen wurden, die genauen vorteile und nachteile usw. aber in STruktur der ADR-Template. Länge spielt keine Rolle, lieber vollständig. Erinnere dich an die Liste der geplanten ADRs + den Ergänzungen bis ADR-021.




## Commentary on ADR-030

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
