# Performance Regression Testing Specification

Status: Accepted
Date: 2026-04-16


## 1. Purpose

This document defines the performance regression testing approach for the flowd runtime.

The goal is:

> To continuously measure and track performance characteristics across commits, ensuring that changes do not introduce unintended regressions.

This is especially critical before introducing the scheduler (ADR-0002), in order to:

* establish a reliable baseline
* quantify the cost of architectural changes
* maintain control over performance evolution


## 2. Scope

This specification applies to:

* core runtime execution
* message passing
* pipeline throughput and latency

It does NOT cover:

* micro-optimizations
* system-level benchmarking (OS, network)
* production monitoring


## 3. Benchmarking Framework

### 3.1 Required Tool

All benchmarks MUST use:

> `criterion` (Rust benchmarking library)

Rationale:

* statistically sound measurements
* noise reduction
* reproducibility
* standard ecosystem tool

### 3.2 Configuration

* warm-up phase enabled
* multiple sample runs
* stable measurement time (no ad-hoc timing)

No custom benchmarking frameworks are allowed.


## 4. Benchmark Categories

The following benchmark scenarios MUST be implemented.

### 4.1 Linear Pipeline

#### Description

A simple sequential pipeline:

```text
Source → Node → Node → Sink
```

#### Purpose

* measure raw throughput
* measure minimal overhead of message passing

### 4.2 Fan-Out Pipeline

#### Description

A branching pipeline:

```text
Source → Node → (Node A, Node B, Node C) → Merge → Sink
```

#### Purpose

* measure cost of fan-out
* detect copying / allocation overhead
* validate memory model behavior

### 4.3 Fan-In / Merge Pipeline

#### Description

Multiple sources merging:

```text
Source A →
           → Merge → Node → Sink
Source B →
```

#### Purpose

* measure coordination overhead
* evaluate queue handling

### 4.4 High-Volume Pipeline

#### Description

Large number of messages processed:

* e.g. 1M+ messages

#### Purpose

* detect long-run degradation
* observe memory behavior

### 4.5 IO-Simulated Pipeline

#### Description

Pipeline with artificial delay:

* simulated IO (sleep / async delay)

#### Purpose

* baseline before async integration
* measure scheduling impact later


## 5. Metrics

The following metrics MUST be captured.

### 5.1 Throughput

```text
messages per second
```

Primary metric.

### 5.2 Latency

```text
time from input to output
```

Measured as:

* average
* percentile (p50, p95 if feasible)

### 5.3 CPU Time (Indirect)

Criterion provides timing; no direct CPU profiling required at this stage.

### 5.4 Allocation Behavior (Optional, recommended)

If feasible:

* number of allocations
* memory footprint trends

## 6. Measurement Strategy

### 6.1 No Static Baseline Files

The system MUST NOT rely on:

* `baseline.json`
* hardcoded thresholds in code

### 6.2 Commit-Based Tracking

Each benchmark run MUST:

* be executed per commit (locally or CI)
* produce measurement output
* be comparable across history

### 6.3 Storage

Results SHOULD be:

* stored as Criterion outputs (default)
* optionally exported to structured format (CSV/JSON)

Tracking is done via:

* commit history
* CI artifacts
* optional external visualization tools


## 7. Regression Detection

### 7.1 Philosophy

> Not all performance changes are regressions.

A regression is defined as:

* a significant performance degradation
* without intentional architectural reason

### 7.2 Evaluation

Performance changes must be interpreted:

* in context of changes (e.g. scheduler introduction)
* not blindly rejected

### 7.3 Optional Thresholds

Soft thresholds MAY be defined, e.g.:

* throughput drop > 20%
* latency increase > 30%

These are guidelines, not hard failures.


## 8. Workflow

### 8.1 Before Major Changes

Before implementing:

* scheduler (ADR-0002)
* backpressure (ADR-0004)
* async execution

You MUST:

* run full benchmark suite
* document baseline behavior

### 8.2 After Changes

* rerun benchmarks
* compare results
* document differences

### 8.3 Documentation

Significant changes MUST be documented:

* expected performance impact
* observed impact
* justification


## 9. Constraints

### 9.1 Deterministic Environment

Benchmarks SHOULD:

* run on stable hardware
* avoid background load
* minimize external noise

### 9.2 Reproducibility

Benchmarks MUST be:

* deterministic in setup
* repeatable across runs


## 10. Non-Goals

This system does NOT aim to:

* guarantee absolute performance numbers
* replace profiling tools
* simulate real-world production load fully


## 11. Future Extensions

After ADR-0002 (Scheduler):

* compare pre/post scheduler performance
* analyze fairness vs throughput trade-offs

After ADR-0008 (Memory Model):

* compare Vec vs Arc vs zero-copy

After ADR-0009 (Distribution):

* introduce network benchmarks


## 12. Key Insight

> Performance must be tracked continuously, not assumed.


## 13. Summary

This specification establishes:

* a consistent benchmarking methodology
* commit-based performance tracking
* a foundation for evaluating architectural trade-offs
