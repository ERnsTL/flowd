Following is a clear and reusable ADR (Architecture Decision Record) template.

It is formal, but not overly bureaucratic.

Most importantly, it covers problems -> decisions -> justifications, not discussions.

# 📄 Architecture Decision Record (ADR) Template

```markdown
# ADR-XXXX: <Short Title of the Decision>

## Status
Proposed | Accepted | Rejected | Deprecated | Superseded by ADR-XXXX

## Date
YYYY-MM-DD

---

## Context

Describe the background and problem that led to this decision.

Include:
- Current system state
- Constraints (technical, organizational, performance, etc.)
- Pain points or risks
- Relevant prior decisions (link other ADRs if applicable)

The context should answer:
> Why do we need to make this decision?

---

## Decision

Clearly describe the decision that has been made.

This should be:
- Unambiguous
- Specific
- Actionable

Avoid describing *why* here — only *what* is decided.

Example:
> FBP components will be integrated at compile time using build.rs and a configuration file (`flowd.build.toml`). No dynamic plugin system will be used.

---

## Rationale

Explain *why* this decision was made.

Include:
- Key benefits
- Trade-offs considered acceptable
- Technical reasoning
- Performance, safety, or maintainability arguments

This should answer:
> Why is this the best option given the context?

---

## Alternatives Considered

List realistic alternatives and explain why they were rejected.

For each alternative include:
- Short description
- Pros
- Cons
- Reason for rejection

Example:

### Alternative 1: Dynamic Plugin System (dylib)
- Pros:
  - Runtime extensibility
- Cons:
  - ABI instability
  - Increased complexity
- Decision:
  - Rejected due to lack of type safety and runtime fragility

---

## Consequences

Describe the impact of this decision.

Split into:

### Positive Consequences
- What improves?
- What becomes simpler, faster, safer?

### Negative Consequences
- What becomes harder?
- What limitations are introduced?

### Neutral / Trade-offs
- What changes in behavior or expectations?

---

## Implementation Notes (Optional)

Provide practical guidance for developers.

Examples:
- Required changes to code structure
- Required tools or files (e.g. `build.rs`, config files)
- Migration steps if applicable

---

## Operational Impact (Optional)

If relevant, describe:
- Deployment changes
- Build pipeline implications
- Runtime behavior changes

---

## Related Decisions

List related ADRs:

- ADR-0001: Compile-Time Component Integration
- ADR-0002: Scheduler Model
- ADR-0003: Message Typing

---

## Open Questions (Optional)

List unresolved topics or future considerations.

Example:
- Should component feature flags be supported?
- Should conditional compilation be added later?

---
```

---

# 💡 Optional: Naming & Structure Recommendation

Im Repo:

```
/docs/adr/
  ADR-0001-compile-time-components.md
  ADR-0002-runtime-scheduler.md
  ADR-0003-message-model.md
```

---

# 🔧 Optional: Lightweight Variant (wenn du weniger Formalismus willst)

Wenn du’s schlanker willst:

```markdown
## Context
## Decision
## Rationale
## Alternatives
## Consequences
```

