# ADR-0002: FBP Runtime Architecture (Scheduler vs. Thread-per-Node)

Status: Accepted
Date: 2026-02-15


## Terminology

For the purpose of this ADR:

- Component: A reusable processing unit definition (code).
- Node: A runtime instance of a component within a graph.

Unless explicitly stated otherwise, the terms “node” and “component instance”
are used interchangeably, with “node” referring to the runtime execution unit.

- Graph: A collection of nodes connected via message-passing edges.
- Edge: A message transport channel between nodes.


## Context

The flowd runtime implements a Flow-Based Programming (FBP) execution engine with the following initial characteristics:

* Each FBP component (node) runs in its own OS thread
* Communication between nodes is implemented via bounded ring buffers per output port
* Nodes directly wake downstream nodes using thread handles
* Backpressure is enforced via bounded queues
* The system is fully synchronous (no async runtime in the core)
* Messages are passed continuously and may fan out (e.g. 1 → 1000 downstream messages)

This model has proven to be:

* Extremely low latency
* Mechanically efficient
* Naturally backpressured

However, several architectural concerns emerged:

1. **Lack of global fairness**

   * Nodes can dominate execution (e.g. fan-out explosions)
   * No centralized control over execution time distribution

2. **Tight coupling between nodes**

   * Nodes directly control downstream scheduling
   * Wakeup logic is distributed and implicit

3. **Difficult observability and debugging**

   * No central authority to reason about execution order
   * Hard to detect starvation or feedback loops

4. **Hot reload and graph reconfiguration complexity**

   * Thread handles become invalid across restarts
   * Dynamic graph changes are difficult to coordinate

5. **Scalability concerns**

   * Thread-per-node does not scale efficiently beyond moderate graph sizes
   * OS scheduler is unaware of FBP semantics

At the same time, the system must preserve:

* Very low latency
* Natural backpressure via ring buffers
* Ability for nodes to perform asynchronous or blocking external IO
* Deterministic and debuggable execution

Additionally, prior architectural constraints apply:

* Compile-time integration of components (ADR-0001)
* No dynamic plugin system
* Strong separation between runtime and component logic
* Transport abstraction (e.g. Reticulum) implemented as FBP components
* Nodes may internally use threads or async runtimes, but must not block the scheduler


## Decision

The runtime adopts a **hybrid scheduler-based architecture** with the following properties:

1. **Central Scheduler per Subgraph**

   * Each subgraph is executed by a scheduler responsible for activation order and fairness

2. **Nodes do NOT directly schedule other nodes**

   * No direct thread handles or wakeup calls between nodes
   * Nodes only signal that work is available

3. **Ring buffers per edge are retained**

   * SPSC ring buffers per output port remain the primary transport mechanism
   * Backpressure remains local and physical

4. **Push-Pull Hybrid Execution Model**

   * Nodes may push messages into downstream buffers
   * Scheduler decides when downstream nodes are executed

5. **Budget-based execution control**

   * Each node receives a processing budget per activation
   * Budget is defined in work units only and MUST be bounded.

6. **Nodes may have internal event loops**

   * Nodes may:
     * Spawn threads
     * Use async runtimes (e.g. Tokio)
     * Nodes MAY wait on external events ONLY outside of process(),e.g. in async tasks or worker threads.
   * But must expose work non-blockingly to the scheduler

7. **Async Runtime Strategy**

   * Components SHOULD use a shared async runtime when supported
   * Components MAY use a component-local runtime when required
   * Scheduler remains synchronous and behavior MUST remain equivalent across both patterns

8. **Scheduler governs fairness, not data transport**

   * Scheduler does not replace ring buffers
   * Scheduler only controls execution order and limits


## Rationale

### Separation of Responsibilities

The system enforces strict separation:

| Responsibility     | Owner        |
| ------------------ | ------------ |
| Data production    | Node         |
| Message transport  | Ringbuffer   |
| Backpressure       | Ringbuffer   |
| Execution ordering | Scheduler    |
| Fairness           | Scheduler    |
| IO handling        | Node / async |

This prevents nodes from making scheduling decisions about other nodes.

### Why not Thread-per-Node

Thread-per-node was rejected as primary model because:

* No global fairness control
* Wakeup cascades and feedback loops
* Difficult reasoning about execution order
* Poor hot-reload behavior
* OS scheduler cannot understand FBP semantics

It optimizes for latency, but not for system behavior.

### Why not Fully Centralized Pull Model

A purely pull-based scheduler (poll everything, no push) was rejected because:

* Increased latency
* Loss of local optimization (chunking, batching)
* Reduced responsiveness

### Why Hybrid Model

The hybrid model preserves:

* Push efficiency (low latency, local batching)
* Pull control (global fairness and stability)

Key insight:

> Backpressure is local. Fairness is global.

### Budget System Justification

Budgets provide:

* Controlled execution slices
* Protection against runaway nodes
* Explicit prioritization

Example:

* Forbidden: `budget = -1` → realtime control nodes. Unbounded budgets are forbidden. Realtime nodes use higher but bounded budgets.
* `budget = 10–100` → normal processing
* `budget = 1–5` → explosive or expensive nodes

This is superior to OS-level scheduling (`nice`) because:

* OS schedules time, not work
* OS has no visibility into message flows

### Node Autonomy Preserved

Nodes can still:

* Perform async IO
* Wait for events
* Run internal loops

But:

* Scheduler is never blocked
* Nodes expose readiness via queues

This aligns with the “active node, passive scheduler” pattern

### Compatibility with Other Decisions

This model integrates cleanly with:

* Compile-time component integration (ADR-0001)
* Proxy-based subgraph boundaries and restart model
* Transport-as-component architecture


## Alternatives Considered

### Alternative 1: Thread-per-Node (Current Model)

**Description:**
Each node runs in its own thread and directly wakes downstream nodes.

**Pros:**

* Minimal latency
* Simple mental model
* Natural push semantics
* Efficient local backpressure

**Cons:**

* No global fairness
* Tight coupling between nodes
* Difficult debugging
* Hard to evolve (hot reload, graph changes)
* OS scheduler lacks semantic awareness

**Decision:**
Rejected as primary architecture, retained partially (nodes may still spawn threads internally)

### Alternative 2: Fully Centralized Pull Scheduler

**Description:**
Scheduler polls nodes and controls all execution, no push behavior.

**Pros:**

* Maximum control
* Deterministic execution
* Easy observability

**Cons:**

* Increased latency
* Loss of local batching optimizations
* Reduced responsiveness

**Decision:**
Rejected due to performance trade-offs

### Alternative 3: OS Scheduling (nice / priority)

**Description:**
Use OS-level thread priorities for fairness.

**Pros:**

* Simple
* No additional runtime logic

**Cons:**

* No awareness of message flows
* No control over fan-out behavior
* Can amplify pathological behavior

**Decision:**
Rejected as insufficient abstraction level

### Alternative 4: Fully Async Runtime (Tokio Everywhere)

**Description:**
Entire runtime built on async/await and futures.

**Pros:**

* Unified concurrency model
* Good IO handling

**Cons:**

* Increased complexity
* Harder debugging
* Unnecessary overhead for in-proc execution

**Decision:**
Rejected for core runtime; async allowed only at boundaries


## Consequences

### Positive Consequences

* Global fairness and stability
* Clear separation of concerns
* Improved observability and debugging
* Better support for hot reload and graph transitions
* Scalable architecture for larger graphs
* Deterministic execution model

### Negative Consequences

* Increased implementation complexity
* Requires scheduler design and tuning
* Slightly higher latency vs direct thread wakeup
* Requires careful budget configuration

### Neutral / Trade-offs

* Push model is preserved but mediated
* Nodes lose direct control over downstream execution
* System becomes more structured and less "mechanical"


## Implementation Notes

* Replace direct thread wakeups with scheduler notifications
* Introduce `NodeContext` with budget tracking
* Maintain ringbuffer per edge
* Implement readiness signaling (e.g. flags or queues)
* Scheduler loop:
  * select ready nodes
  * assign budget
  * execute until budget exhausted
* Support urgent/fast-path execution for critical nodes


## Operational Impact

* Improved runtime observability (metrics per node)
* Better control under load
* Enables safer rolling updates and graph reconfiguration
* Simplifies reasoning about system behavior


## Related Decisions, Interaction With Other ADRs

* ADR-0001: Compile-Time Component Integration
* ADR-0003: Message Model
* ADR-0004: Backpressure & Budgeting
* ADR-0005: Hot Reload & Graph Lifecycle

### Normative Language

The key words "MUST", "MUST NOT", "SHOULD", "SHOULD NOT", and "MAY"
are to be interpreted as described in RFC 2119 / RFC 8174.

### Mandatory Integration with ADR-0017 (IO Model)

All components performing external IO MUST comply with ADR-0017.

In particular:

- process() MUST remain non-blocking (ADR-002)
- all IO MUST be asynchronous or offloaded (ADR-0017)
- completion of async work MUST signal the scheduler (ADR-002 + ADR-0017)
- Polling external systems is generally forbidden.

  However, if no callback or wakeup mechanism is available,
components MAY use bounded periodic polling via wake_at().

  Such polling MUST:

  - use scheduler-integrated timing (wake_at)
  - be rate-limited
  - not rely on busy-looping
  - not be the primary mechanism if a callback-based alternative exists

  Polling MUST NOT occur inside process() in a blocking or loop-based manner
  Polling MUST be driven by scheduler wakeups.

ADR-002 defines execution semantics.
ADR-0017 defines external interaction semantics.

Both are required for a correct implementation.

### External Readiness Signaling (MANDATORY)

The scheduler relies on explicit readiness signaling to resume execution.

Any component performing asynchronous or background work MUST:

- enqueue results into its output buffers
- signal the scheduler upon availability of new work

Polling-only designs are forbidden.

The scheduler MUST NOT rely on implicit polling of components
for externally generated events.

External events MUST result in:

- message enqueue
- explicit scheduler wakeup


## Open Questions

* Should budgets be static or adaptive by default?
* Should scheduler support priority classes?
* How to expose scheduling metrics externally?
* Should subgraph-level schedulers be introduced explicitly?


## Resolved Questions

1. Scheduler is single-threaded per subgraph
2. Budget exhaustion yields after completing current work unit
3. Default budgets are static and simple (small set of classes)
4. No separate priority classes beyond budgets
5. Real-time nodes are restricted and must not block the scheduler
6. Each subgraph has its own scheduler

### 1. Scheduler is single-threaded per subgraph

#### Rationale

A single-threaded scheduler ensures:

* deterministic execution order
* no internal locking within the scheduler
* predictable behavior for debugging and observability
* clear ownership of scheduling decisions

The scheduler acts as a coordination loop, not a worker pool.
Parallelism is achieved via:

* multiple subgraphs
* internal concurrency within nodes
* asynchronous IO

> The scheduler must remain simple, predictable, and fully observable.

#### Rejected Alternatives

Thread pool–based scheduler:

* introduces locking and contention
* leads to non-deterministic execution order
* makes debugging and reasoning about fairness difficult
* risks race conditions inside the scheduler itself

Fully parallel scheduler:

* complex coordination required
* unclear execution ordering
* difficult to maintain fairness guarantees

### 2. Budget exhaustion yields after completing current work unit

#### Rationale

Execution is cooperative:

* a node completes its current unit of work (e.g. processing a message)
* then yields control back to the scheduler

This ensures:

* no partial or inconsistent processing
* predictable state transitions
* safe interaction with message passing and buffers

> Work units are atomic from the scheduler’s perspective.

#### Rejected Alternatives

Preemptive interruption (mid-processing):

* risks inconsistent internal state
* requires complex rollback or checkpointing
* significantly increases implementation complexity

Ignoring budget until batch completion:

* allows nodes to dominate execution
* breaks fairness guarantees
* enables runaway behavior (e.g. fan-out explosions)

### 3. Default budgets are static and simple (small set of classes)

#### Rationale

A small set of static defaults provides:

* predictable behavior
* easy reasoning and tuning
* minimal configuration overhead

Typical classes:

* normal processing nodes
* heavy or expensive nodes
* fan-out / burst nodes

> Simplicity is preferred over premature optimization.

#### Rejected Alternatives

Fully dynamic / adaptive budgeting:

* requires runtime feedback loops
* introduces complexity and instability
* hard to debug and reason about

Per-component fine-grained configuration:

* configuration overhead grows quickly
* difficult for users to understand and maintain
* risk of inconsistent tuning across graphs

### 4. No separate priority classes beyond budgets

#### Rationale

Budgets already provide implicit prioritization:

* small budget → less execution time
* large budget → more throughput

Adding priority classes would:

* duplicate functionality
* increase system complexity
* introduce additional scheduling policies

> A single mechanism (budget) is preferred over multiple overlapping mechanisms.

#### Rejected Alternatives

Explicit priority queues (high / medium / low):

* increases scheduler complexity
* creates interaction issues with budgets
* risks starvation of lower-priority nodes

Weighted scheduling with multiple dimensions:

* difficult to reason about
* hard to tune correctly
* unnecessary for current system scope

### 5. Real-time nodes are restricted and must not block the scheduler

#### Rationale

Some nodes may require high responsiveness (e.g. control, IO).
These nodes may use:

* higher budgets
* special handling

However:

* they must not block the scheduler
* they must not monopolize execution

> No node is allowed to compromise global system stability.

#### Rejected Alternatives

Unbounded execution (`budget = -1` as true infinite):

* allows a single node to block the system
* breaks fairness guarantees
* can lead to starvation of other nodes

Blocking operations inside scheduler-controlled execution:

* stalls entire system
* violates non-blocking scheduler design

### 6. Each subgraph has its own scheduler

#### Rationale

A scheduler per subgraph provides:

* isolation between independent graph sections
* improved scalability
* simpler lifecycle management (start/stop/restart)
* better fault containment

> Subgraphs act as natural scheduling domains.

#### Rejected Alternatives

Single global scheduler:

* becomes a bottleneck
* increases contention
* complicates reasoning about large graphs

Per-node schedulers:

* excessive overhead
* no global fairness within a graph
* reintroduces problems of thread-per-node model


## Implementation Clarifications

The following clarifications refine the scheduler behavior defined above.
They are binding for all implementations and remove ambiguity in critical areas.

### 1. Budget Values Are Non-Semantic Defaults

#### Clarification

Budget values (e.g. Normal, Heavy, Realtime) are:

* provisional defaults
* implementation-level tuning parameters
* NOT part of the semantic model

In particular:

* “realtime” MUST NOT be interpreted as unbounded execution
* no budget value guarantees uninterrupted execution

#### Rationale

Budgets are a control mechanism, not a semantic contract.

Treating them as semantic would:

* make behavior unpredictable across systems
* encourage misuse (e.g. abusing “realtime” for priority)
* tightly couple components to runtime internals

> Budgets express execution limits, not importance.

#### Rejected Alternatives

Semantic budget classes (e.g. “realtime means always run”):

* breaks fairness guarantees
* introduces hidden priority systems
* leads to starvation and instability

### 2. Scheduler Must Ensure Fair Re-queuing

#### Clarification

The scheduler MUST ensure fair execution across nodes.

Specifically:

* nodes are re-queued after execution if work remains
* execution MUST follow a fair rotation model (e.g. round-robin behavior)
* strict FIFO without rebalancing is NOT sufficient

#### Rationale

Pure FIFO scheduling is vulnerable to:

* starvation
* dominance by high-frequency producers
* instability under fan-out scenarios

Fair re-queuing ensures:

* bounded execution per node
* predictable distribution of processing time

> Fairness is enforced by rotation, not queue order alone.

#### Rejected Alternatives

Strict FIFO scheduling:

* allows hot nodes to dominate
* can starve slower or intermittent nodes
* does not provide fairness guarantees

### 3. Readiness Signaling Is Level-Triggered

#### Clarification

Readiness signaling MUST be level-triggered.

Scheduler MUST coalesce duplicate wakeups for the same node.

This means:

* a node is considered ready as long as work is available
* readiness MUST NOT be lost if a node remains non-empty
* duplicate suppression MUST NOT suppress required re-scheduling

#### Rationale

Edge-triggered signaling is insufficient because:

* it can miss subsequent work
* it depends on precise timing
* it introduces race conditions

Level-triggered signaling ensures:

* no work is lost
* scheduler state reflects actual system state

> Readiness represents state, not events.

#### Rejected Alternatives

Edge-triggered readiness (signal once per event):

* risks lost wakeups
* fragile under concurrency
* leads to silent stalls

### 4. Work Unit Definition Must Be Explicit

#### Clarification

A work unit MUST be explicitly defined.

Default definition:

* one work unit = processing of a single message

Optional:

* bounded batch processing MAY be allowed
* batch size MUST be explicitly defined and limited

#### Rationale

Undefined work units lead to:

* unpredictable execution time
* unfair scheduling
* inability to reason about performance

Explicit units ensure:

* consistent budget application
* predictable execution slices
* fair comparison between nodes

> The scheduler operates on units of work, not arbitrary execution time.

#### Rejected Alternatives

Implicit or variable work units:

* breaks fairness assumptions
* makes performance tuning impossible
* leads to inconsistent behavior

### 5. Scheduler Idle Behavior Must Be Non-Busy

#### Clarification

When no work is available, the scheduler MUST NOT busy-wait.

Instead, it MUST:

* block, sleep, or wait for a signal
* resume immediately when work becomes available

#### Rationale

Busy-waiting:

* wastes CPU resources
* reduces system efficiency
* interferes with other workloads

Proper idle handling ensures:

* efficient resource usage
* predictable latency under load

> Idle systems must not consume active resources.

#### Rejected Alternatives

Busy-loop / spin-wait scheduler:

* high CPU usage
* poor system behavior under low load
* unnecessary contention

### 6. Scheduler Is Isolated from Component Execution Resources

#### Clarification

The scheduler MUST execute in an isolated execution context.

This means:

* it MUST NOT compete with component workloads
* it MUST NOT share execution scheduling with component thread pools
* it MUST remain logically and operationally independent

#### Rationale

If the scheduler competes with components:

* fairness guarantees degrade
* scheduling becomes unpredictable
* system behavior depends on external load

Isolation ensures:

* consistent scheduling decisions
* stable system behavior

> The scheduler must control execution, not compete for it.

#### Rejected Alternatives

Running scheduler inside shared thread pool (e.g. async runtime):

* introduces contention
* breaks control over execution
* reduces determinism

### 7. Scheduler Does Not Interpret Backpressure

#### Clarification

Backpressure remains exclusively managed by ring buffers.

The scheduler:

* MUST NOT override backpressure
* MUST NOT attempt to bypass full queues
* MUST treat buffer state as authoritative

#### Rationale

Backpressure is:

* local
* physical
* deterministic

Scheduler-level interference would:

* break system invariants
* introduce hidden behavior
* complicate reasoning

> Backpressure is enforced by transport, not by scheduling.

#### Rejected Alternatives

Scheduler-driven flow control:

* duplicates responsibility
* introduces hidden coupling
* weakens system guarantees


## Implementation Decisions (Follow-up Clarifications)

The following decisions resolve remaining ambiguities in the scheduler design.

### 1. Work Unit Granularity

#### Decision

Default:

> One work unit = processing of a single message.

Optional:

* bounded batching is allowed
* batching MUST be explicitly limited (e.g. max N messages)
* batching MUST still respect budget limits

#### Rationale

Single-message units:

* maximize fairness
* simplify reasoning
* avoid long execution slices

Bounded batching is allowed for:

* throughput optimization
* reducing scheduling overhead

But:

* batching must not undermine fairness

> Fairness-first, batching as controlled optimization.

#### Constraint

* No unbounded batching
* No implicit batching
* Batch size MUST be small and explicit

### 2. Idle Blocking Mechanism

#### Decision

> Use a blocking mechanism with explicit wakeup signaling.

Preferred options:

* `std::sync::Condvar`
* or equivalent low-level signaling primitive

#### Rationale

The scheduler must:

* NOT busy-wait
* resume immediately when work is available
* avoid unnecessary latency

Condvar-style signaling provides:

* efficient sleep
* low overhead wakeup
* predictable behavior

#### Not Recommended

Channels as primary blocking mechanism:

* introduce unnecessary abstraction
* may hide wakeup semantics
* less explicit control over scheduler loop

> The scheduler should use explicit signaling, not implicit queue semantics.

### 3. Fairness Metrics

#### Decision

The scheduler SHOULD expose basic fairness observability, but not enforce complex policies.

Minimum metrics:

* executions per node
* total work units processed per node
* time since last execution per node

Optional:

* average latency between executions
* queue depth / backlog per node

#### Rationale

Fairness must be:

* observable
* debuggable
* tunable

But:

* not over-engineered
* not dependent on complex runtime feedback loops

> Measure fairness first. Optimize later if needed.

#### Non-Goals

* No strict fairness guarantees (e.g. real-time scheduling)
* No complex priority-based scheduling

### 4. Transition Strategy

#### Decision

> Introduce the scheduler as a complete replacement per subgraph, not partially within a subgraph.

#### Rationale

Mixing models inside a subgraph (thread-per-node + scheduler):

* creates undefined behavior
* breaks scheduling guarantees
* makes debugging extremely difficult

Subgraphs provide a natural boundary:

* clean separation
* controlled rollout
* safe experimentation

#### Rejected Alternatives

Gradual per-node migration within a subgraph:

* inconsistent execution model
* unpredictable scheduling
* high debugging complexity

> The execution model must be consistent within a scheduling domain.

#### Decision

> The scheduler defined in this ADR is the single execution model for flowd.

Specifically:

* there MUST NOT be multiple scheduler implementations in the system
* there MUST NOT be a runtime switch between scheduling models
* all subgraphs MUST use the same scheduler implementation
* no hybrid execution models are allowed within the same runtime

#### Rationale

Allowing multiple scheduling models would:

* introduce undefined behavior at graph boundaries
* make performance and correctness non-deterministic
* significantly increase system complexity
* create long-term maintenance burden (code duplication, feature drift)

The scheduler is a **core runtime primitive**, not a pluggable feature.

> Execution semantics must be globally consistent across the system.

#### On Subgraphs as Scheduling Domains

Subgraphs define **isolation domains**, not alternative execution models.

This means:

* each subgraph has its own scheduler instance
* all scheduler instances follow the same implementation and semantics
* isolation is for scalability and fault containment, not configurability

#### Rejected Alternatives

Multiple scheduler implementations (e.g. legacy + new):

* leads to code bloat
* introduces divergent behavior
* complicates testing and debugging

Runtime switch between scheduling models:

* creates inconsistent system semantics
* hard to reason about correctness
* increases operational complexity

Two models of scheduling within a graph:

* unpredictable execution ordering
* broken fairness guarantees
* extremely difficult to debug

Pluggable scheduler architecture (at this stage):

* premature abstraction
* unclear interface boundaries
* risks fragmentation of execution semantics

#### Future Evolution

If the scheduling model evolves in the future:

* it MUST replace the existing model
* not coexist with it

Migration strategy (if needed) is:

* version-level change (e.g. major version)
* not runtime coexistence

> There is always exactly one scheduler model per runtime.


## Further Clarifications

We need to make a few strict decisions to avoid architectural drift:

1. The Component API must be changed immediately. We do NOT maintain backward compatibility with the thread-per-node model. This is a full execution model replacement.

2. Work signaling must be implemented via ProcessEdgeSink notifying the scheduler. Components must not directly interact with scheduler internals.

3. The scheduler must remain synchronous. Components may use async/IO internally, but process() must be non-blocking and return immediately if no work is available.

4. The transition is all-at-once. There must not be multiple execution models in the system, and no runtime switching.

Additionally:

- ProcessResult MUST be an explicit enum (DidWork / NoWork / Finished)
- The scheduler is the central execution engine, not an optional layer, and MUST NOT be bypassed.
- process() MUST never block.

  External I/O inside process() is restricted to two allowed patterns:

  1. Offloaded I/O (default / recommended)

     - External I/O is executed in background threads or async tasks
     - process() only enqueues work and drains completed results
     - completion MUST signal the scheduler

  2. Bounded non-blocking reactor (allowed under strict constraints)

     - process() MAY perform non-blocking I/O operations
     - operations MUST NOT block under any circumstances
     - execution MUST be bounded by the assigned budget
     - no waiting, sleeping, or blocking syscalls are allowed

  The following are explicitly forbidden inside process():

  - blocking network calls (connect, read with blocking socket, etc.)
  - file I/O that may block
  - sleep / wait / thread join
  - any operation with unbounded latency

  Polling-only designs without scheduler signaling are forbidden.

These constraints are normative requirements.
Conformance MUST be validated by tests and runtime checks.

Interpretation:

"No blocking" means that process() must always return in bounded time, independent of external system behavior.


## 8. Real-Time Components

#### Decision

Real-time components are handled within the existing budget-based scheduling system.

They:

* receive a higher (but bounded) budget
* are scheduled like all other components
* MUST NOT bypass the scheduler

There is no separate execution path or scheduling mechanism for real-time components.

#### Rationale

Introducing a separate mechanism would:

* create a second scheduling model
* break fairness guarantees
* increase system complexity
* make behavior unpredictable

A unified scheduling model ensures:

* consistency
* debuggability
* controlled execution

> Real-time behavior is expressed through controlled prioritization, not special execution paths.

#### Constraints

* budgets for real-time components MUST be bounded
* real-time components MUST NOT block the scheduler
* real-time components MUST follow the same cooperative execution model

#### Rejected Alternatives

Unbounded execution (`budget = -1`):

* allows system domination
* breaks fairness
* can stall the runtime

Separate real-time scheduler or fast-path:

* introduces dual execution semantics
* increases complexity
* difficult to reason about system behavior

Priority-based bypass of scheduler:

* breaks global scheduling guarantees
* leads to starvation of other nodes

#### In Summary

Real-time components are not a separate mechanism.

They are handled via the existing budget system using higher but bounded budgets.

There is no special execution path, no bypass, and no second scheduler.


## Final Implementation Decisions

### 1. Budget Defaults

#### Decision

Start with **simple static defaults**, not user-configurable.

Example:

* Normal: 16–64 work units
* Heavy: 4–16 work units
* Realtime: 64–128 work units (bounded)

Configuration MAY be introduced later, but is NOT part of the initial implementation.

#### Rationale

Static defaults:

* reduce complexity
* avoid premature tuning
* ensure consistent system behavior

Early configurability would:

* increase cognitive load
* create inconsistent deployments
* complicate debugging

> The system must behave predictably before it becomes configurable.

#### Rejected Alternatives

Fully configurable budgets from the start:

* premature optimization
* difficult to reason about
* leads to misconfiguration

### 2. Work Unit Definition (Batching)

#### Decision

Work unit is defined as:

> processing of a single message (default)

Batching:

* MAY be implemented by components
* MUST be bounded
* MUST respect budget limits

Batch size is:

> component-controlled, but constrained by the runtime budget

#### Rationale

The runtime:

* controls fairness via budgets

The component:

* controls internal efficiency (batching)

This separation ensures:

* scheduler simplicity
* component flexibility

#### Constraint

* no unbounded loops inside `process()`
* component MUST yield when budget is exhausted

#### Rejected Alternatives

Runtime-controlled batching:

* requires deep knowledge of component internals
* increases coupling

Unbounded batching:

* breaks fairness
* leads to starvation

### 3. Async Components Integration

#### Decision

Async components:

* use internal async runtimes or threads (e.g. Tokio)
* MUST NOT block the scheduler
* MUST signal readiness back to the scheduler

#### Pattern

1. `process()` is called
2. if no data → return `NoWork`
3. if IO required → spawn async task
4. async task produces data → pushes to edge
5. edge signals scheduler → node becomes ready

#### Rationale

This ensures:

* scheduler remains synchronous
* IO runs concurrently
* system remains deterministic

#### Constraint

* `process()` MUST be non-blocking
* async work MUST signal readiness explicitly

#### Rejected Alternatives

Blocking inside process():

* stalls scheduler
* breaks entire runtime

Making scheduler async:

* increases complexity
* reduces debuggability

### 4. Transition Strategy

#### Decision

This change is a **breaking architectural change**.

* it MUST be implemented as a **new major version**
* the old model is removed
* there is NO coexistence of models

#### Rationale

Maintaining both models would:

* double complexity
* introduce inconsistent behavior
* create long-term maintenance issues

> The execution model is a fundamental property, not a feature toggle.

#### Rejected Alternatives

Temporary dual-model support:

* leads to code bloat
* encourages incomplete migration
* complicates debugging

### 5. Observability

#### Decision

The scheduler MUST expose minimal but sufficient metrics.

Required:

* executions per node
* processed work units per node
* time since last execution per node

Recommended:

* queue length per node
* scheduler loop iteration time
* number of ready nodes

#### Rationale

Observability must:

* support debugging
* reveal fairness issues
* enable performance tuning

But:

* must not introduce heavy overhead
* must not require complex instrumentation

#### Non-Goals

* no full tracing system in initial implementation
* no complex fairness scoring algorithms

#### Rejected Alternatives

Heavy observability / tracing from start:

* high overhead
* unnecessary complexity
* distracts from core runtime behavior


## External Execution, Async Integration and Time-Based Scheduling

### Status

Binding clarification

---

## 1. Scope

This section defines mandatory rules for:

* asynchronous execution
* external IO integration
* readiness signaling
* time-based scheduling

It complements:

* ADR-0002 (scheduler model)
* ADR-0017 (external IO model)

---

## 2. Non-Blocking Execution Contract

### Decision

`process()` MUST be strictly non-blocking.
`process()` MUST complete within a bounded time.

The execution time of `process()` MUST be short enough to guarantee
scheduler fairness and responsiveness across all nodes.

Implementations MUST define and enforce a maximum execution time
(e.g. via runtime checks or CI validation).

The following operations are FORBIDDEN inside `process()`:

* network IO
* file IO
* blocking synchronization (mutex wait, join, sleep)
* waiting on async results
* long-running loops

All such work MUST be executed in:

* background threads, OR
* async tasks

Async or threaded execution MUST only be used for IO-bound or externally-blocking operations. Pure components SHOULD execute entirely within process() without background threads or async mechanisms.

### Enforcement

Violations MUST be treated as:

> critical runtime errors

---

---

## 3. Async Execution Model

### Decision

Components MAY perform background work using:

* threads
* async runtimes (e.g. Tokio)

### Constraints

Components MUST:

* fully encapsulate async execution
* never block `process()`
* communicate results via output queues only

---

### Explicit Rule

> The scheduler MUST never depend on component-internal execution state.

---

---

## 4. External Readiness Signaling (MANDATORY)

### Decision

Components performing background or async work MUST signal the scheduler.

### Required Behavior

When async work completes:

1. result MUST be enqueued into a bounded component-owned completion queue OR directly into an output buffer. Component-owned completion queues MUST be bounded and MUST be drained within process() execution.
2. scheduler MUST be explicitly woken via waker

Async completion MUST follow this order:

1. enqueue output
2. signal scheduler

Reversing this order is forbidden.

Failure to signal the scheduler after async completion MUST be treated as a critical correctness violation.

Components that do not signal readiness after producing output are considered invalid, as they can cause permanent execution stalls.

### Forbidden

The following patterns are FORBIDDEN:

* polling external systems or waiting for external completion inside `process()`
* relying on repeated scheduler invocation
* passive waiting for external events

Bounded draining of local in-memory completion queues is allowed only as regular
work processing and MUST NOT be the sole wakeup mechanism.

---

### Required API

The runtime MUST provide:

```text id="waker_api"
scheduler_waker()  // node-bound, thread-safe, callable from async contexts
```

This MUST:

* mark the node as ready
* wake the scheduler if it is idle

---

---

## 5. Time-Based Scheduling (MANDATORY)

### Decision

The scheduler MUST implement a time-based wakeup primitive:

```text id="wake_at_api"
wake_at(node_id, timestamp)
```

`wake_at` MUST be:

- scheduler-owned
- cancelable when node finishes
- idempotent across repeated registrations

---

### Semantics

* `timestamp` is an absolute time
* node MUST NOT be scheduled before timestamp
* node MUST be scheduled as soon as possible after timestamp
* scheduling delay MUST be minimal and bounded

---

### Scheduler Requirements

The scheduler MUST:

* maintain a timer queue (min-heap or equivalent)
* integrate timer events with readiness queue
* block efficiently when idle
* wake up on:

  * new readiness signals
  * earliest timer expiration

---

### Forbidden

Components MUST NOT:

* implement time delays via loops
* repeatedly signal readiness for waiting
* check time inside `process()` to simulate timers

---

### Required Behavior

Timer-driven components MUST:

* register wakeup via `wake_at`
* return `NoWork` until scheduled again

---

---

## 6. Idle Behavior

### Decision

The scheduler MUST NOT busy-wait.

---

### Required Behavior

When idle, scheduler MUST block until:

* a node is woken via `scheduler_waker`
* a timer expires

---

---

## 7. Backpressure Compliance

### Decision

Backpressure MUST be preserved in all execution paths.

---

### Required Behavior

If an output buffer is full:

Components MUST:

* retry later, OR
* buffer locally

If backpressure prevents progress:

- process() MUST return NoWork
- node MUST be re-scheduled via readiness signaling

---

### Forbidden

The following are FORBIDDEN:

* panicking on full buffers
* dropping messages silently

---

### Conformance Checklist (MANDATORY)

A component is ADR-002 conformant only if all conditions are true:

* `process()` is non-blocking and returns in bounded minimal time
* no network/file IO is performed inside `process()`
* async/background completion triggers explicit scheduler wakeup
* timer-driven behavior uses `wake_at` (no time-loop simulation in `process()`)
* all outputs are emitted through scheduler-controlled execution paths

---

---

## 8. Optional Shared Async Runtime

### Decision

The runtime MAY provide a shared async runtime.

---

### Rules

Components SHOULD use shared runtime when:

* supported by external libraries

Components MAY create their own runtime when:

* required by external libraries
* isolation is needed

---

### Explicit Rule

> The system MUST support both patterns without behavioral differences.

---

---

## 9. Deterministic Scheduling Boundary

### Decision

All execution MUST re-enter the scheduler before producing observable effects.

---

### Meaning

* async tasks MUST NOT directly execute downstream logic
* all outputs MUST pass through scheduler-controlled execution

---

---

## 10. Final Principle

> The scheduler is the only authority for execution.
>
> External work may happen anywhere,
> but execution always returns to the scheduler.

---
