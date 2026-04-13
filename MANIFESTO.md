# Flowd Manifesto

## Purpose

Flowd is a high-performance, deterministic Flow-Based Programming (FBP) runtime designed for building robust dataflow systems.

It is not a scripting engine, not a low-code tool, and not an automation platform.

Flowd is an execution engine.


## Flowd Philosophy

Many modern systems implicitly implement dataflow concepts:

- nodes performing transformations
- data moving along edges
- pipelines of processing steps

However, these systems are typically designed around specific use cases such as automation, stream processing, or integration workflows.

As a result, dataflow emerges as an implementation detail rather than a first-class concept.

### Explicit Dataflow

Flowd treats dataflow as a primary abstraction, not an incidental pattern.

- Nodes are defined by their behavior on incoming data
- Edges are explicit communication channels
- Messages are first-class entities
- Backpressure and flow control are integral to the system

Nothing is implicit.

### General-Purpose Execution Model

Instead of optimizing for a single domain, Flowd provides a general execution model for dataflow systems.

This allows building:

- automation systems
- streaming pipelines
- AI workflows
- distributed or local processing systems

These are not separate modes, but different applications of the same underlying model.

### Separation of Engine and Application

Flowd intentionally separates:

- the execution engine (runtime, scheduling, flow control)
- from the applications built on top (automation tools, UIs, integrations)

This separation enables:

- consistent execution semantics
- reusable components
- flexible system composition

Flowd does not define how systems are used, only how they execute.

### No Implicit Behavior

Many systems introduce hidden mechanisms:

- automatic retries
- implicit buffering
- hidden parallelism
- uncontrolled resource usage

Flowd avoids this.

All behavior must be explicit and observable.

### Deterministic Systems Thinking

Flowd is built on the assumption that:

> A system should be understandable from its structure.

Given a graph and its configuration, the behavior of the system should be:

- predictable
- explainable
- reproducible

This is prioritized over convenience or abstraction.

### Composability Over Specialization

Instead of building specialized features for specific use cases, Flowd focuses on composability:

- small, well-defined components
- clear interfaces
- explicit dataflow between parts

Complex systems emerge from composition, not from feature accumulation.

### A Foundation, Not a Product

Flowd is not an end-user product.

It is a foundation for building systems.

Other tools may provide:

- visual editors
- automation interfaces
- domain-specific workflows

Flowd provides the execution model underneath.

### Guiding Principle

When designing or extending Flowd, the guiding question is:

> Does this make dataflow more explicit, more predictable, and more composable?

If not, it likely does not belong in the core system.


## Capabilities

Flowd provides a general-purpose execution model for FBP dataflow systems, enabling the construction of:

- low-code automation systems (e.g. n8n, NiFi)
- visual workflow editors (e.g. via NoFlo tooling)
- AI pipelines and integration systems (akin to OpenClaw)
- event-driven and stream processing systems (conceptually similar to Flink)

These are considered higher-level systems built on top of the runtime.


## Architectural Boundary

Flowd separates execution from presentation.

- The runtime is responsible for execution, scheduling, and dataflow semantics.
- User interfaces, automation layers, and visual editors are external systems.

Flowd provides the foundation; it does not define the user experience.


## Core Principles

### 1. Performance First

- In-process communication must avoid unnecessary allocations and copying.
- Serialization is only allowed at system boundaries.
- Zero-copy and typed data structures are preferred over generic byte buffers.

### 2. Determinism Over Convenience

- Execution behavior must be predictable and reproducible.
- Hidden scheduling or implicit side effects are not allowed.
- Runtime behavior must be explainable.

### 3. Explicit Backpressure

- Backpressure is a first-class concept.
- All edges must have bounded capacity.
- Systems must fail or slow down explicitly, never silently.

### 4. Compile-Time Composition

- Components are integrated at compile time.
- No dynamic plugin loading in the core system.
- The system must be statically verifiable.

### 5. Clear Separation of Concerns

- Nodes define behavior.
- The runtime defines execution.
- The scheduler defines fairness.
- Transport is external to core execution.

### 6. No Hidden Work

- Every cost must be visible.
- No implicit buffering, spawning, or retries.
- No hidden background processing.

### 7. Dataflow, Not Control Flow

- Systems are built as graphs, not scripts.
- Messages flow, they are not "called".
- Nodes react to data, not commands.

### 8. Explicit Boundaries

- Subsystems are separated via well-defined boundaries (e.g. proxy nodes).
- Cross-boundary communication must be explicit and controlled.

### 9. Simplicity Over Abstraction

- Avoid unnecessary abstractions.
- Prefer simple, explicit mechanisms over flexible but complex systems.
- Complexity must be justified.


## Design Philosophy

Flowd prioritizes:

- correctness over convenience
- performance over flexibility
- explicitness over magic


## Guiding Question

When making design decisions, always ask:

> Does this make the system more predictable, more explicit, and more efficient?

If not, it is likely the wrong direction.


## Positioning

Flowd is an execution engine for deterministic, high-performance dataflow systems.

It is not primarily designed as:

- a low-code automation tool
- a UI-first workflow system
- a general-purpose scripting environment
- a distributed system (by default)

Flowd focuses on providing a robust and deterministic foundation rather than end-user tooling.
