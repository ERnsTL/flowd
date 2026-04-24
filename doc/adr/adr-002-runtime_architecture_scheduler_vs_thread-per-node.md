# ADR-0002: FBP Runtime Architecture (Scheduler vs. Thread-per-Node)

Status: Accepted
Date: 2026-02-15


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
   * Budget may be:

     * Fixed (N messages)
     * Byte-based
     * Time-based
     * Unbounded (-1 for critical nodes)

6. **Nodes may have internal event loops**

   * Nodes may:

     * Spawn threads
     * Use async runtimes (e.g. Tokio)
     * Wait on external events
   * But must expose work non-blockingly to the scheduler

7. **Single async runtime per process (optional)**

   * Async IO is centralized (not per node)
   * Scheduler remains synchronous

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

* `budget = -1` → realtime control nodes
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


## Related Decisions

* ADR-0001: Compile-Time Component Integration
* ADR-0003: Message Model
* ADR-0004: Backpressure & Budgeting
* ADR-0005: Hot Reload & Graph Lifecycle


## Open Questions

* Should budgets be static or adaptive by default?
* Should scheduler support priority classes?
* How to expose scheduling metrics externally?
* Should subgraph-level schedulers be introduced explicitly?


## Resolved Questions

OK die Resolved Decisions ad ADR-002 Scheduler finde ich gut, aber ich brauche auch eine Begründung je Punkt (1 bis 6) zu den Entscheidungen und was die verworfenen gefahrvollen Alternativen gewesen wären.


## Resolved Decisions

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
