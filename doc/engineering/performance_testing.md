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


## 17. Repository Implementation (Issue #335)

The current implementation in this repository uses:

* Criterion benchmark suites at:
  * `benches/micro_benchmarks.rs`
  * `benches/pipeline_benchmarks.rs`
* benchmark harness configuration in `Cargo.toml`:
  * `[[bench]]`
  * `name = "micro_benchmarks"`
  * `harness = false`
  * `[[bench]]`
  * `name = "pipeline_benchmarks"`
  * `harness = false`
* GitHub Actions workflow:
  * `.github/workflows/performance-benchmarks.yml`

Implemented micro-bench scenarios (primitive layer):

* ringbuffer push/pop throughput
* edge/message transfer cost (payload-size dependent)
* cross-thread handover latency

Implemented pipeline scenarios (runtime layer):

* linear pipeline (`Source → Node → Node → Sink`)
* fan-out pipeline (`Source → Demux → Node A/B/C → Merge → Sink`)
* fan-in / merge pipeline (`Source A/B → Merge → Node → Sink`)
* high-volume linear pipeline
* IO-simulated pipeline (`Source → Delay → Node → Sink`)

Each pipeline scenario has two benchmark sets:

* persistence-loaded graph set:
  * graph is serialized and loaded with the same `read_to_string + serde_json::from_str` mechanism as runtime startup
* direct graph/runtime set:
  * graph and runtime structs are synthesized in-memory and runtime functions are called directly (without websocket graph mutation)

Runtime execution path used by pipeline benchmarks:

* shared internal `run_graph()` in `src/lib.rs`
* `network:start` handler and benchmark harness both call `run_graph()`
* component instantiation, edge wiring, and execution loop are therefore exercised through the same runtime start path
* runtime fix included:
  * source array-port validation now checks repeated use of the same source *port* (not only process), enabling valid multi-outport fan-out graphs

Metrics currently captured:

* throughput via Criterion `Throughput::Elements` (messages/sec) in both micro and pipeline groups
* latency via dedicated Criterion latency groups (including end-to-end runtime scenarios)
* statistical stability via Criterion defaults + configured sample sizes


## 18. Local Usage

Run full benchmark suite:

```sh
cargo bench --bench micro_benchmarks --bench pipeline_benchmarks -- --noplot
```

Outputs are stored in:

```text
target/criterion/
```


## 19. CI Integration

On each `push` and `pull_request`, CI:

* runs both Criterion benchmark targets (`micro_benchmarks` and `pipeline_benchmarks`)
* uploads `target/criterion` as build artifact
* optionally uploads results to Bencher (if configured)

No static baseline file is used.


## 20. Bencher Setup

To enable commit-level history tracking in CI:

1. Create a Bencher project and copy its project slug.
2. Add repository secret `BENCHER_API_TOKEN`.
3. Add repository variable `BENCHER_PROJECT` (project slug).
4. Push a commit or open a PR to trigger the workflow.

CI then publishes each benchmark run with:

* adapter: `rust_criterion`
* branch metadata from GitHub event context
* PR start-point comparison for regression review


## 21. Interpretation Guidance

Use trend comparison between commits, not single-run absolutes:

* throughput drops indicate potential regression in data movement
* latency increases indicate added pipeline overhead
* high-volume drift indicates long-run behavior changes

Manual regression review is the default for now. Soft threshold automation can be added later.
