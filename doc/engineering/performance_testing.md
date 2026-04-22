# Performance Testing & Regression Tracking

## Status

Accepted

## Date

2026-04-16

---

## 1. Purpose

This document defines the performance benchmarking and regression tracking strategy for flowd.

The goals are:

* establish a reliable performance baseline (pre-ADR-0002)
* track performance across commits
* detect unintended regressions
* understand trade-offs introduced by architectural changes

> Performance must be tracked continuously, not assumed.

---

## 2. Scope

This specification applies to:

* runtime execution
* message passing
* pipeline behavior

It explicitly includes:

* micro-benchmarks (isolated primitives)
* pipeline benchmarks (full runtime execution)

It excludes:

* production monitoring
* deep profiling
* micro-optimizations

---

## 3. Benchmarking Framework

All benchmarks MUST use:

> Criterion.rs

Rationale:

* statistically sound measurements
* reproducibility
* noise reduction

Custom benchmarking implementations are NOT allowed.

---

## 4. Benchmark Architecture

Performance testing is divided into two mandatory layers:

---

### 4.1 Micro Benchmarks (Primitives)

Measure isolated building blocks without involving the runtime.

---

#### Targets

* ringbuffer throughput (push/pop)
* edge/message transfer cost
* cross-thread handover latency

---

#### Purpose

> Understand the cost of fundamental primitives in isolation.

---

#### Requirements

* MUST NOT use the runtime
* MUST isolate the specific primitive under test
* MUST be minimal and deterministic

---

### 4.2 Pipeline Benchmarks (Runtime)

Measure full system behavior using the actual flowd runtime.

---

### ⚠️ Mandatory Requirement

> Pipeline benchmarks MUST execute through the real flowd runtime.

This includes:

* real component execution
* real edges
* real pipeline construction
* actual runtime behavior

---

### ❌ Explicitly Forbidden

* bypassing the runtime
* manually wiring functions instead of using flowd
* simulating pipelines outside the runtime
* replacing runtime execution with shortcuts

---

### Rationale

If the runtime is bypassed:

* results are invalid
* overhead (edges, scheduling) is invisible
* benchmarks become misleading

---

## 5. Component Usage Rules

Pipeline benchmarks MUST reuse existing flowd components whenever possible.

---

### Examples

* Source components
* Demux / fan-out components
* Merge / sink components

---

### Rules

If suitable components exist:

> They MUST be used.

---

If suitable components do NOT exist:

> Minimal local components MAY be implemented inside `/benches`

Constraints:

* strictly minimal logic
* only for benchmarking
* clearly documented as benchmark-only

---

### Rationale

* ensures realistic execution paths
* avoids artificial benchmarks
* keeps tests aligned with real usage

---

## 6. Benchmark Scenarios

---

### 6.1 Linear Pipeline

```text id="lin001"
Source → Node → Node → Sink
```

Purpose:

* baseline throughput
* minimal overhead measurement

---

### 6.2 Fan-Out Pipeline

```text id="fan001"
Source → Node → (Node A, Node B, Node C) → Merge → Sink
```

Purpose:

* message distribution cost
* allocation / duplication overhead

---

### 6.3 Fan-In / Merge Pipeline

```text id="merge001"
Source A →
           → Merge → Node → Sink
Source B →
```

Purpose:

* coordination overhead

---

### 6.4 High-Volume Pipeline

* large message count (e.g. 1M+)

Purpose:

* detect long-run degradation
* observe stability

---

### 6.5 IO-Simulated Pipeline

* artificial delay (sleep)

Purpose:

* baseline for future async/scheduler changes

---

## 7. Metrics

Each benchmark MUST measure:

---

### 7.1 Throughput

```text id="met001"
messages per second
```

---

### 7.2 Latency

```text id="met002"
end-to-end processing time
```

---

### 7.3 Statistical Stability

Handled by Criterion:

* variance
* confidence intervals

---

### Optional

* allocation behavior
* memory trends

---

## 8. Execution Strategy

---

### 8.1 No Static Baselines

The system MUST NOT use:

* `baseline.json`
* hardcoded performance thresholds

---

### 8.2 Commit-Based Tracking

Performance is tracked via:

* benchmark execution per commit
* historical comparison
* CI artifact storage

---

### 8.3 Storage

Results SHOULD be:

* stored as Criterion outputs
* optionally exported (CSV/JSON)
* tracked via CI and external tools

---

## 9. Regression Tracking

Performance is evaluated over time using:

> Bencher

---

### Approach

* track performance across commits
* compare trends
* identify regressions manually (initially)

---

### Important Principle

> Not all performance changes are regressions.

A regression is:

* a significant degradation
* without architectural justification

---

## 10. Workflow

---

### Before Major Changes

(e.g. scheduler, backpressure)

* run full benchmark suite
* document baseline

---

### After Changes

* rerun benchmarks
* compare results
* document impact

---

## 11. Environment Constraints

Benchmarks SHOULD:

* run on stable hardware
* avoid background noise
* be reproducible

---

## 12. Non-Goals

This system does NOT aim to:

* guarantee absolute performance numbers
* replace profiling tools
* simulate full production load

---

## 13. Future Extensions

After ADR-0002 (Scheduler):

* measure fairness vs throughput trade-offs

After ADR-0008 (Memory Model):

* compare Vec vs Arc vs zero-copy

After ADR-0009 (Distribution):

* add network-level benchmarks

---

## 14. Key Principles

> Measure primitives in isolation.
> Measure behavior in composition.

---

## 15. Additional Principle

> Benchmarks must reflect real system behavior — not idealized shortcuts.

---

## 16. Summary

This specification ensures:

* reliable baseline measurement
* continuous performance tracking
* separation of cause (micro) and effect (pipeline)
* realistic, runtime-based benchmarking
