# Engineering: Testing Strategy for flowd

## Status

Accepted

## Purpose

This document defines the testing strategy (what to test) and implementation (how to test it) for the flowd runtime.

The goals are:

* ensure correctness of message flow
* guarantee stability under load and concurrency
* prevent regressions during architectural evolution
* validate system behavior, not just component logic

> Tests are not a feature. They are the foundation of system reliability.

---

## Scope

This strategy applies to:

* runtime execution
* message passing and transport
* scheduling behavior
* pipeline semantics

It explicitly covers:

* component-level behavior
* pipeline-level execution
* system-level guarantees

---

## Core Principle

> Test behavior, not implementation.

---

## Test Layers

flowd uses multiple complementary test layers:

---

### 1. Protocol Conformance Tests

flowd is validated against:

> FBP Protocol test suite

---

#### Purpose

* ensure compatibility with established FBP semantics
* validate message format and protocol behavior
* guarantee interoperability

---

#### Scope

* message handling
* graph execution semantics
* protocol-level correctness

---

## 2. Component Tests

Unit-level tests for individual components.

---

#### Purpose

* validate isolated logic
* ensure deterministic behavior

---

#### Characteristics

* no scheduler involved
* no full pipeline execution
* fast and minimal

---

## 3. Pipeline-Level Tests

Tests for complete graphs and execution flows.

---

#### Purpose

* validate real runtime behavior
* test interaction between components
* verify scheduling + transport integration

---

#### Requirements

* MUST run through the real runtime
* MUST use real components (not mocks, where possible)
* MUST include realistic data flow scenarios

---

#### Examples

* linear pipelines
* fan-out / fan-in graphs
* mixed processing chains

---

## 4. Property / Scenario Tests

Tests that validate system invariants.

---

#### Purpose

* ensure fundamental correctness under all conditions
* detect subtle concurrency and flow bugs

---

#### Key Properties

---

##### Message Integrity

```text id="prop001"
Messages are never lost.
Messages are never duplicated (unless explicitly intended).
```

---

##### Ordering Guarantees

```text id="prop002"
Message order is preserved where required by the graph semantics.
```

---

##### Backpressure Behavior

```text id="prop003"
Backpressure propagates correctly through the system.
Producers are slowed or blocked when downstream is saturated.
```

---

##### Deterministic Behavior (where applicable)

```text id="prop004"
Given identical inputs and configuration, execution produces consistent results.
```

---

#### Notes

* Not all pipelines require strict ordering
* Properties must be defined per scenario

---

## 5. Failure Tests

Tests for error conditions and system resilience.

---

#### Purpose

* ensure predictable behavior under failure
* validate recovery mechanisms

---

#### Scenarios

---

##### Component Failure

* component panics or returns error
* downstream behavior is defined and stable

---

##### Pipeline Interruption

* abrupt stop of execution
* partial processing scenarios

---

##### Restart / Recovery

* graph restart
* re-initialization of components
* state consistency

---

##### Resource Exhaustion

* full buffers
* memory pressure
* slow consumers

---

#### Expected Outcomes

* no undefined behavior
* no deadlocks
* no silent data corruption

---

## 6. Concurrency & Stress Tests

Tests under high load and parallel conditions.

---

#### Purpose

* detect race conditions
* validate scheduler fairness
* ensure stability under pressure

---

#### Examples

* high message throughput
* large fan-out graphs
* mixed IO and CPU workloads

---

## 7. Regression Testing

Continuous validation across commits.

---

#### Strategy

* tests run on every commit
* failures block changes
* no reliance on manual testing

---

#### Integration

* combined with performance benchmarks
* ensures correctness AND performance stability

---

## 8. Test Design Principles

---

### Use Real Execution Paths

> Tests must use the actual runtime, not simplified models.

---

### Avoid Over-Mocking

* mocks only where unavoidable
* prefer real components and real pipelines

---

### Focus on Behavior

* test what the system does
* not how it is implemented internally

---

### Keep Tests Deterministic

* avoid timing-sensitive assertions where possible
* isolate non-deterministic behavior

---

## 9. Non-Goals

This testing strategy does NOT aim to:

* maximize code coverage metrics
* test every internal function
* replace performance benchmarking

---

## 10. Summary

flowd testing ensures:

* correctness of message flow
* stability under load and failure
* predictable and debuggable execution

---

## Final Principle

> A system like flowd is only as good as its behavior under stress, not its behavior in ideal conditions.


## 11. Test Harness & Execution Model

### Purpose

To ensure consistency, maintainability, and correctness of pipeline-level tests, all runtime-based tests MUST be executed through a centralized test harness.

---

### Core Principle

> Tests MUST NOT construct or manage the runtime manually.

---

### Test Harness

A shared test harness MUST be provided that:

* instantiates the runtime
* loads component configurations
* executes graphs
* captures outputs
* validates expected behavior

---

### Responsibilities

The test harness is responsible for:

* creating a runtime instance
* wiring components into a graph
* feeding input data
* executing the scheduler
* collecting outputs
* applying assertions

---

### Execution Model

All pipeline-level tests MUST:

* run through the real runtime
* use the scheduler
* use real components
* avoid direct component invocation

---

> The test harness is the only entry point for runtime-based tests.

---

### Graph Definition

Tests SHOULD define pipelines as:

* FBP graph definitions (preferred)
* or programmatically constructed graphs via the harness

---

### Expected Behavior Definition

Expected outcomes MUST be defined as:

* output message sequences
* state transitions
* invariants (e.g. no message loss)

---

Declarative expectations MAY be used, but MUST be:

* deterministic
* verifiable without timing assumptions

---

---

### Parameterized Testing

The harness MUST support:

* data-driven test scenarios
* multiple graph configurations
* varying input datasets

---

Examples:

* linear pipelines
* fan-out / fan-in
* mixed component chains
* edge cases

---

---

### Property-Based Testing Integration

The harness SHOULD integrate with property-based testing frameworks.

Properties MUST be validated via:

* repeated randomized execution
* invariant checking

---

---

### Component Discovery & Registration

Components SHOULD be registered via a central configuration (e.g. `flowd.build.json`).

The harness MAY:

* dynamically load components
* generate test scenarios based on available components

---

> Test infrastructure must reflect the actual system configuration.

---

---

### Separation of Concerns

Component tests:

* test logic in isolation
* DO NOT use the runtime

Pipeline tests:

* test behavior in context
* MUST use the harness

---

---

### Stability Requirement

Changes to the runtime API MUST:

* NOT require rewriting all tests
* be absorbed by the test harness

---

> The harness acts as a stability layer between tests and runtime evolution.

---

---

### Anti-Patterns

The following are explicitly forbidden:

* tests manually constructing runtime internals
* tests spawning their own scheduler
* tests directly invoking component internals in pipeline scenarios
* duplicating runtime logic inside tests

---

> If tests reimplement the runtime, they are invalid.

---




## 12. Test Implementation Model (Active Testing & Harness-Based Execution)

### Purpose

This section defines how the testing strategy is implemented in practice.

While the previous sections define **what must be tested**, this section defines:

> how tests are structured, executed, and validated in a flowd system.

---

## Core Principle

> Tests are active participants in the system, not passive observers.

---

## Active Test Driver Model

### Concept

Tests are implemented as **active drivers inside the runtime**, not as external assertions over static outputs.

A test:

* injects messages into the system
* observes outputs
* evaluates behavior
* determines completion

---

### Architecture

```text id="active_test_model"
Test Harness
    ↓
Runtime (Scheduler)
    ↓
Graph (Components + Test Driver)
```

---

### Flow

```text id="test_flow"
Test Driver → sends input → graph executes → outputs produced
           → observes outputs → performs assertions → completes test
```

---

## Test Harness Responsibilities

The central test harness:

* creates runtime instances
* loads and executes graphs
* integrates test drivers
* provides utilities for:

  * input injection
  * output capture
  * scheduler execution control

---

> The harness provides execution. Tests provide validation.

---

## Test Driver Responsibilities

A test driver is responsible for:

* generating input messages
* collecting output messages
* evaluating expected behavior
* signaling test completion

---

### Example Responsibilities

* send initial messages into graph
* accumulate outputs from downstream nodes
* assert invariants or expected outputs
* terminate when conditions are met

---

## Output Validation Strategies

### 1. Set-Based Assertions (Preferred)

Used when ordering is not guaranteed.

```text id="set_assertion"
Expected: {A, B, C}
Actual:   {C, A, B} → valid
```

---

#### Rationale

* robust against scheduling variation
* compatible with parallel execution
* avoids brittle tests

---

---

### 2. Sequence Assertions (When Required)

Used only when ordering is semantically required.

```text id="sequence_assertion"
Expected: A → B → C
```

---

#### Constraint

* MUST only be used where ordering is guaranteed by design
* MUST NOT assume incidental ordering

---

---

### 3. Window-Based Assertions

Used for asynchronous or delayed behavior.

```text id="window_assertion"
Event must occur within N scheduler cycles
```

---

#### Purpose

* avoid hard timing dependencies
* ensure eventual correctness

---

---

### 4. Property-Based Assertions

Used for system invariants.

Examples:

* no message loss
* no unintended duplication
* ordering constraints (where applicable)
* backpressure correctness

---

---

## Handling Non-Determinism

flowd tests MUST assume:

* non-deterministic execution order (unless explicitly constrained)
* asynchronous input arrival
* variable scheduling interleavings

---

### Implication

Tests MUST:

* avoid strict ordering assumptions
* prefer invariant-based validation
* use robust comparison strategies

---

---

## Integration with External Event Sources

For components using async IO or external event loops:

* events MUST be injected into the system via message queues
* test drivers MUST observe outputs through the same mechanisms as production

---

> External systems are treated as message sources, not test-controlled flows.

---

---

## Component Testing vs Pipeline Testing

### Component Tests

* test isolated logic
* do NOT use runtime
* fast and deterministic

---

### Pipeline Tests

* test real execution behavior
* MUST use runtime + scheduler
* MUST use test harness

---

---

## Rejected Approach: Declarative Flow Test Formats

The idea of a declarative “flow test format” (e.g. JSON-based expected input/output) was considered.

---

### Rejected Because:

* cannot express non-deterministic behavior reliably
* brittle under concurrency and scheduling variation
* difficult to represent timing and ordering constraints
* leads to fragile, over-specified tests

---

> Static test definitions do not scale to dynamic dataflow systems.

---

---

## Rejected Approach: Direct Output Assertions Outside Runtime

Example:

```text id="bad_test"
run graph → collect outputs → assert externally
```

---

### Rejected Because:

* breaks encapsulation of execution model
* ignores scheduling behavior
* cannot express incremental or temporal assertions

---

---

## Benefits of Active Test Driver Model

* tests run in real execution environment
* no duplication of runtime logic
* flexible assertion strategies
* robust against concurrency effects
* scalable to complex pipelines

---

---

## Design Rule

> Only the scheduler decides when execution happens.
> Tests must adapt to this model, not bypass it.

---

---

## Summary

flowd tests are:

* runtime-driven
* behavior-focused
* resilient to non-determinism
* executed via a centralized harness
* validated through active participation

---

## Final Principle

> A dataflow system is not tested by comparing outputs —
> it is tested by participating in its execution and validating its behavior.

---
