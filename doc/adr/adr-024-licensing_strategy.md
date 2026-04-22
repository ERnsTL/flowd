# ADR-0024: Licensing Strategy (Apache 2.0)

Status: Accepted
Date: 2026-04-16


## Context

flowd is a Flow-Based Programming (FBP) runtime system implemented in Rust.

Key characteristics:

* components are executed **in-process**
* applications are composed of interconnected components
* the runtime orchestrates execution, data flow, and lifecycle
* users build systems *on top of* flowd rather than merely linking a library

This raises fundamental licensing questions:

* How does licensing affect adoption?
* How does in-process execution affect legal interpretation (combined work)?
* What licensing model aligns with both technical architecture and ecosystem goals?


## Problem Statement

We must choose a license that:

* allows commercial and enterprise adoption
* aligns with the in-process runtime architecture
* minimizes legal ambiguity for users
* supports ecosystem growth
* does not unintentionally restrict usage patterns


## Decision

> flowd is licensed under the Apache License 2.0.


## Rationale

### 1. System Nature (Runtime, not Library)

flowd is not a traditional library.

* it executes user-defined components
* components run within the same process
* the resulting system can be interpreted as a combined work

Implication:

> Licensing must account for in-process composition, not just linking.

### 2. Copyleft Misalignment

Copyleft licenses (e.g. GPL/LGPL) introduce friction in this architecture.

* in-process execution may be interpreted as a single combined work
* users integrating components could be subject to copyleft obligations
* this affects **users of the system**, not just contributors to flowd

Key insight:

> Copyleft would primarily affect adopters building systems with flowd rather than protecting the core runtime itself.

### 3. Adoption Strategy

The project prioritizes:

* low friction adoption
* ease of integration
* minimal legal overhead

Permissive licensing enables:

* rapid experimentation
* integration into existing systems
* use in both open and closed environments

### 4. Enterprise & Compliance Considerations

In enterprise environments:

* copyleft licenses often trigger legal review
* uncertainty around obligations delays or blocks adoption
* compliance requirements can prevent usage entirely

Apache 2.0 provides:

* legal clarity
* well-understood terms
* compatibility with enterprise procurement processes

### 5. Ecosystem Strategy

The project deliberately chooses:

> growth through adoption and contribution, not restriction.

Instead of enforcing contribution via licensing:

* contributions are encouraged through value creation
* community participation is voluntary
* trust and transparency are prioritized

### 6. Competitive Position

The project does not rely on licensing as a control mechanism.

Instead, it aims to:

* be the best maintained implementation
* provide the most complete feature set
* offer the highest reliability and performance

Key principle:

> The project competes on quality, not on licensing constraints.

### 7. Patent Protection

Apache 2.0 includes:

* explicit patent grants
* protection against patent litigation within the ecosystem

This provides additional safety compared to MIT-style licenses.

### 8. Contribution Model

Contributors:

* retain copyright of their work
* contribute under Apache 2.0 terms

Implications:

* contributions remain usable across ecosystems
* no forced assignment required
* aligns with standard open source expectations

### 9. Commercial Compatibility

Apache 2.0 allows:

* internal usage in proprietary systems
* distribution as part of commercial products
* integration without disclosure obligations

This aligns with:

* consulting-based models
* managed services
* enterprise adoption

### 10. Alignment with Architecture

The chosen license aligns with key architectural decisions:

* in-process component execution
* compile-time integration model
* runtime orchestration of user-defined components

Implication:

> The license avoids creating friction with the system’s core execution model.


## Alternatives Considered

### GPL / LGPL (Copyleft)

Advantages:

* strong protection of source code
* ensures modifications are shared

Disadvantages:

* unclear implications for in-process systems
* high friction for adopters
* reduced enterprise compatibility

Conclusion:

> Rejected due to misalignment with runtime architecture and adoption goals.

### MIT License

Advantages:

* extremely simple
* widely adopted

Disadvantages:

* no explicit patent protection

Conclusion:

> Considered but Apache 2.0 preferred due to patent provisions.


## Consequences

Positive:

* broad adoption potential
* compatibility with commercial use
* reduced legal friction
* strong ecosystem growth potential
* alignment with architectural model

Negative:

* no enforced sharing of improvements
* potential for proprietary forks
* reliance on community goodwill instead of legal enforcement


## Strategic Position

flowd follows:

> permissive licensing + strong technical core + active maintenance

instead of:

> restrictive licensing as a control mechanism


## Design Principle

> Licensing should enable usage, not restrict it.


## Summary

* flowd uses Apache 2.0 to maximize adoption and compatibility
* copyleft is intentionally avoided due to architectural and ecosystem concerns
* the project relies on quality, maintainability, and trust rather than legal enforcement
* licensing is aligned with the runtime’s in-process execution model


## Final Statement

> The project deliberately avoids using licensing as a control mechanism and instead relies on technical quality, maintainability, and community trust.


## Additional Consideration: Commercial Layers and Value Capture

While the core runtime is licensed under Apache 2.0 and remains fully open, the project acknowledges that certain advanced capabilities may represent substantial engineering effort and operational value.

These may include, but are not limited to:

* advanced control plane functionality
* user interfaces and monitoring systems
* enterprise-grade features (e.g. security, governance, compliance tooling)
* large-scale distribution and scaling capabilities
* platform-level integrations built on top of flowd
* customer-specific developments and extensions

### Positioning

Such capabilities are not considered part of the minimal open runtime, but rather:

> higher-level system layers built on top of flowd.

### Commercial Perspective

The project recognizes that:

* these layers often require significant ongoing investment
* they deliver disproportionate value in enterprise contexts
* they represent substantial value in enterprise contexts

### Strategic Principle

> The open runtime enables broad usage and adoption, while higher-level systems built on top may be developed and offered separately where appropriate.

### Important Distinction

* The **core runtime remains open and unrestricted**
* Commercial offerings focus on **added value, not artificial limitations**

### Future Outlook

Such layers may be explored separately from the core runtime if clearly justified, provided that:

* boundaries remain clear and technically justified
* the openness of the core is preserved
* the ecosystem is not fragmented unnecessarily

### Commercial Principle

> Commercial offerings focus on added value, not artificial limitations.

### Final statement

> Value is created through higher-level systems and operational capabilities built on top of the runtime, not by restricting access to the core system.
