# ADR-022 Evaluation: Running flowd Bare Metal in UEFI (OS vs Bare Metal / UEFI)

Status: Accepted
Creation Date: 2024-04-18


## Context

* Desire for maximum performance
* Idea: UEFI / Bare Metal


## Decision

* flowd runs on top of an operating system (which may of course be containerized or an optimized OS substrate like a unikernel or microVM)


## Alternatives

* UEFI / Bare Metal
* Unikernel / microVM
* Kernel modules


## Rationale

* IO-heavy workloads
* need for networking, scheduling, ecosystem
* OS provides essential infrastructure


## Consequences

* small overhead accepted
* massive gain in stability, tooling, ecosystem



# Detailed evaluation


## Input

* Running in UEFI
  * example:  https://github.com/codyd51/uefirc
  * very bare metal, but not sure if the TCP facilities on that level are made for performance


### Short Answer:

* This is the wrong abstraction layer for flowd
* You would lose far more than you gain
* Performance gains are negligible or negative in practice


## 1. Goal vs. UEFI Reality:

### Your goals with flowd:

* Data pipelines
* IO (HTTP, APIs, databases)
* Distribution
* Stability under load
* Observability and control

### UEFI provides:

* Early boot environment
* Minimal runtime services
* Limited execution model
* No real application runtime ecosystem

### Result:

* Structural mismatch between flowd requirements and UEFI capabilities


## 2. Core Misconception:

### Assumption:

* Fewer layers = better performance

### Reality:

* Your bottlenecks are not in the OS layer
* They are in:

  * Message passing
  * Scheduling
  * Memory management patterns
  * IO handling

### Conclusion:

* Removing the OS does not solve your actual bottlenecks


## 3. What You Lose in UEFI:

### No proper multitasking:

* No robust threading model
* No scheduler
* You would have to implement ADR-0002 yourself

### No real networking stack:

* TCP/IP is minimal or non-existent
* No battle-tested implementations

Impact:

* IO-heavy use cases become impractical

### No ecosystem:

* No async runtimes (e.g. Tokio)
* No libraries
* No standard tooling
* No crates.io

Impact:

* Massive productivity loss
* Reinventing fundamental infrastructure

### No debugging or observability:

* No structured logging
* No tracing
* No profiling tools

Impact:

* Performance debugging becomes extremely difficult


## 4. Performance Reality:

### What you might gain:

* Reduced syscall overhead
* Fewer context switches

### What you lose:

* Highly optimized kernel networking
* Memory management
* Scheduling efficiency
* Stability under load

### Net result:

* Likely worse or unstable performance


## 5. What flowd Actually Depends On:

### Core dependencies:

* IO performance
* Parallel execution
* Stability under load

### Provided by OS:

* Networking stacks
* Scheduling
* Memory management
* Process isolation

### Conclusion:

* The OS is an enabler, not overhead


## 6. Where Bare Metal Makes Sense:

### Suitable domains:

* Embedded systems
* Real-time control
* Hardware-specific execution

### Not suitable for:

* Data pipelines
* Distributed systems
* IO-heavy workloads
* AI pipelines

### Classification:

* flowd clearly belongs to the second category


## 7. Better Alternatives:

### Optimize in userspace:

* Ringbuffer implementation
* Memory model (ADR-0008)
* Zero-copy strategies

### Use kernel-assisted features:

* io_uring for efficient IO
* eBPF for observability

### Consider minimal OS approaches:

* Unikernels
* MicroVMs (e.g. Unikraft)

Regarding unikernels and microVMs, see detailed evaluation below.


### Benefit:

* Retain OS advantages while reducing overhead


## 8. Why Your Intuition Is Still Valuable:

Your instinct:

* Remove unnecessary layers

Correct insight:

* Optimization should happen where cost is highest

Correction:

* You are targeting the wrong layer

Correct optimization targets:

* Message passing
* Memory ownership
* Scheduling behavior

Incorrect targets:

* Operating system
* Network stack
* Process abstraction


## 9. When to Revisit This Idea:

Correct sequence:

* Build stable runtime (ADR-0002 etc.)
* Introduce performance measurement
* Identify real bottlenecks
* Optimize based on evidence

Incorrect sequence:

* Premature low-level optimization
* Architecture decisions without data


## Final Conclusion:

Key insight:

* Removing the OS does not improve flowd performance

Correct mental model:

* The OS is a necessary substrate for flowd

### Final statement:

* You do not gain performance by removing the foundation your system depends on


## MicroVMs/Unikernels Evaluation

### Input from Issue #324

* microVMs:
  * unikraft / kraftkit is fastest according to microVM benchmarks:
  * Rust examples:  https://github.com/unikraft/catalog/tree/main/examples
  * and they say "just communicate via TCP, it is so fast"
  * because of virtio or vmnet, which is basically shared memory between host and guest
  * which can provide lossless network and perfect conditions without requiring fragmentation
  * still TCP itself has significant overhead compared to an in-proc ringbuffer for example

### Decision

* Do not use microVMs and unikernels to run 1 component in each - that would definitely have bad performance characteristics (TCP alone)
* Offer unikernels and microVMs as deployment and isolation options, and for performance-optimized execution - but whole flowd with all components inside of it

### Rationale

MicroVMs and unikernels represent an interesting middle ground between bare metal and full OS environments. However, for flowd's component-based architecture, they introduce unacceptable performance trade-offs:

* **TCP Communication Overhead**: Even with optimized virtio/vmnet shared memory, TCP adds significant latency compared to in-process ringbuffer communication
* **Component Isolation vs Performance**: Running each component in its own microVM would eliminate the performance benefits of in-process execution
* **Deployment Flexibility**: MicroVMs remain valuable as deployment containers for the entire flowd runtime + components bundle

### Strategic Position

MicroVMs are **deployment targets**, not **execution environments** for individual components. The ADR supports using unikernels/microVMs for packaging and deploying complete flowd applications, but maintains in-process execution for performance.
