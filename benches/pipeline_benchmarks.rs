//! Pipeline benchmarks that execute through the real flowd runtime path.
//!
//! Two benchmark sets are provided for runtime behavior comparison:
//! - persistence-loaded graphs (same load mechanism as runtime startup)
//! - direct synthesized graph/runtime (no websocket graph mutation path)
//!
//! Both sets execute through the shared runtime execution path used by
//! `network:start` handling (`run_graph()` in runtime code).

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use flowd_rs::bench_api::{
    fan_in_harness_direct, fan_in_harness_from_persistence, fan_out_harness_direct,
    fan_out_harness_from_persistence, io_sim_harness_direct, io_sim_harness_from_persistence,
    linear_harness_direct, linear_harness_from_persistence,
};
use std::time::Duration;

const PAYLOAD_SIZE_BYTES: usize = 64;
const STANDARD_MESSAGES: usize = 2_000;
const HIGH_VOLUME_MESSAGES: usize = 10_000;
const IO_SIM_MESSAGES: usize = 400;

fn run_single_inport(
    harness: &flowd_rs::bench_api::BenchRuntimeHarness,
    inport: &str,
    message_count: usize,
    payload_size: usize,
) {
    let payload = vec![b'x'; payload_size];
    for _ in 0..message_count {
        harness
            .send_data_to_inport(inport, &payload)
            .expect("runtime packet send failed");
        harness
            .wait_for_outport_data("OUT", 1, Duration::from_secs(5))
            .expect("did not observe expected output packet");
    }
}

fn run_fan_in_inputs(
    harness: &flowd_rs::bench_api::BenchRuntimeHarness,
    message_count: usize,
    payload_size: usize,
) {
    let payload_a = vec![b'a'; payload_size];
    let payload_b = vec![b'b'; payload_size];
    for i in 0..message_count {
        if i % 2 == 0 {
            harness
                .send_data_to_inport("IN_A", &payload_a)
                .expect("runtime packet send failed (IN_A)");
        } else {
            harness
                .send_data_to_inport("IN_B", &payload_b)
                .expect("runtime packet send failed (IN_B)");
        }
        harness
            .wait_for_outport_data("OUT", 1, Duration::from_secs(5))
            .expect("did not observe expected output packet (fan-in)");
    }
}

fn run_linear_direct(message_count: usize, payload_size: usize) {
    let harness = linear_harness_direct("bench_graph_linear_direct");
    harness
        .start()
        .expect("runtime start failed (linear direct)");
    run_single_inport(&harness, "IN", message_count, payload_size);
    harness.stop().expect("runtime stop failed (linear direct)");
}

fn run_linear_persistence(message_count: usize, payload_size: usize) {
    let harness = linear_harness_from_persistence("bench_graph_linear_persistence")
        .expect("failed to build persistence harness (linear)");
    harness
        .start()
        .expect("runtime start failed (linear persistence)");
    run_single_inport(&harness, "IN", message_count, payload_size);
    harness
        .stop()
        .expect("runtime stop failed (linear persistence)");
}

fn run_fan_out_direct(message_count: usize, payload_size: usize) {
    let harness = fan_out_harness_direct("bench_graph_fan_out_direct");
    harness
        .start()
        .expect("runtime start failed (fan-out direct)");
    run_single_inport(&harness, "IN", message_count, payload_size);
    harness
        .stop()
        .expect("runtime stop failed (fan-out direct)");
}

fn run_fan_out_persistence(message_count: usize, payload_size: usize) {
    let harness = fan_out_harness_from_persistence("bench_graph_fan_out_persistence")
        .expect("failed to build persistence harness (fan-out)");
    harness
        .start()
        .expect("runtime start failed (fan-out persistence)");
    run_single_inport(&harness, "IN", message_count, payload_size);
    harness
        .stop()
        .expect("runtime stop failed (fan-out persistence)");
}

fn run_fan_in_direct(message_count: usize, payload_size: usize) {
    let harness = fan_in_harness_direct("bench_graph_fan_in_direct");
    harness
        .start()
        .expect("runtime start failed (fan-in direct)");
    run_fan_in_inputs(&harness, message_count, payload_size);
    harness.stop().expect("runtime stop failed (fan-in direct)");
}

fn run_fan_in_persistence(message_count: usize, payload_size: usize) {
    let harness = fan_in_harness_from_persistence("bench_graph_fan_in_persistence")
        .expect("failed to build persistence harness (fan-in)");
    harness
        .start()
        .expect("runtime start failed (fan-in persistence)");
    run_fan_in_inputs(&harness, message_count, payload_size);
    harness
        .stop()
        .expect("runtime stop failed (fan-in persistence)");
}

fn run_io_sim_direct(message_count: usize, payload_size: usize) {
    let harness = io_sim_harness_direct("bench_graph_io_sim_direct");
    harness
        .start()
        .expect("runtime start failed (io-sim direct)");
    run_single_inport(&harness, "IN", message_count, payload_size);
    harness.stop().expect("runtime stop failed (io-sim direct)");
}

fn run_io_sim_persistence(message_count: usize, payload_size: usize) {
    let harness = io_sim_harness_from_persistence("bench_graph_io_sim_persistence")
        .expect("failed to build persistence harness (io-sim)");
    harness
        .start()
        .expect("runtime start failed (io-sim persistence)");
    run_single_inport(&harness, "IN", message_count, payload_size);
    harness
        .stop()
        .expect("runtime stop failed (io-sim persistence)");
}

fn benchmark_runtime_pipeline_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline_runtime_throughput_messages_per_sec");
    group.sample_size(20);

    group.throughput(Throughput::Elements(STANDARD_MESSAGES as u64));
    group.bench_function(
        BenchmarkId::new("linear_persistence", STANDARD_MESSAGES),
        |b| {
            b.iter(|| run_linear_persistence(STANDARD_MESSAGES, PAYLOAD_SIZE_BYTES));
        },
    );

    group.throughput(Throughput::Elements(STANDARD_MESSAGES as u64));
    group.bench_function(BenchmarkId::new("linear_direct", STANDARD_MESSAGES), |b| {
        b.iter(|| run_linear_direct(STANDARD_MESSAGES, PAYLOAD_SIZE_BYTES));
    });

    group.throughput(Throughput::Elements(STANDARD_MESSAGES as u64));
    group.bench_function(
        BenchmarkId::new("fan_out_persistence", STANDARD_MESSAGES),
        |b| {
            b.iter(|| run_fan_out_persistence(STANDARD_MESSAGES, PAYLOAD_SIZE_BYTES));
        },
    );

    group.throughput(Throughput::Elements(STANDARD_MESSAGES as u64));
    group.bench_function(BenchmarkId::new("fan_out_direct", STANDARD_MESSAGES), |b| {
        b.iter(|| run_fan_out_direct(STANDARD_MESSAGES, PAYLOAD_SIZE_BYTES));
    });

    group.throughput(Throughput::Elements(STANDARD_MESSAGES as u64));
    group.bench_function(
        BenchmarkId::new("fan_in_persistence", STANDARD_MESSAGES),
        |b| {
            b.iter(|| run_fan_in_persistence(STANDARD_MESSAGES, PAYLOAD_SIZE_BYTES));
        },
    );

    group.throughput(Throughput::Elements(STANDARD_MESSAGES as u64));
    group.bench_function(BenchmarkId::new("fan_in_direct", STANDARD_MESSAGES), |b| {
        b.iter(|| run_fan_in_direct(STANDARD_MESSAGES, PAYLOAD_SIZE_BYTES));
    });

    group.throughput(Throughput::Elements(IO_SIM_MESSAGES as u64));
    group.bench_function(
        BenchmarkId::new("io_sim_persistence", IO_SIM_MESSAGES),
        |b| {
            b.iter(|| run_io_sim_persistence(IO_SIM_MESSAGES, PAYLOAD_SIZE_BYTES));
        },
    );

    group.throughput(Throughput::Elements(IO_SIM_MESSAGES as u64));
    group.bench_function(BenchmarkId::new("io_sim_direct", IO_SIM_MESSAGES), |b| {
        b.iter(|| run_io_sim_direct(IO_SIM_MESSAGES, PAYLOAD_SIZE_BYTES));
    });

    group.throughput(Throughput::Elements(HIGH_VOLUME_MESSAGES as u64));
    group.bench_function(
        BenchmarkId::new("linear_high_volume_direct", HIGH_VOLUME_MESSAGES),
        |b| {
            b.iter(|| run_linear_direct(HIGH_VOLUME_MESSAGES, 16));
        },
    );

    group.finish();
}

fn benchmark_runtime_pipeline_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline_runtime_latency_end_to_end");
    group.sample_size(30);

    group.bench_function("linear_persistence_one_message", |b| {
        b.iter(|| run_linear_persistence(1, PAYLOAD_SIZE_BYTES));
    });
    group.bench_function("linear_direct_one_message", |b| {
        b.iter(|| run_linear_direct(1, PAYLOAD_SIZE_BYTES));
    });

    group.bench_function("fan_out_persistence_one_message", |b| {
        b.iter(|| run_fan_out_persistence(1, PAYLOAD_SIZE_BYTES));
    });
    group.bench_function("fan_out_direct_one_message", |b| {
        b.iter(|| run_fan_out_direct(1, PAYLOAD_SIZE_BYTES));
    });

    group.bench_function("fan_in_persistence_one_message", |b| {
        b.iter(|| run_fan_in_persistence(2, PAYLOAD_SIZE_BYTES));
    });
    group.bench_function("fan_in_direct_one_message", |b| {
        b.iter(|| run_fan_in_direct(2, PAYLOAD_SIZE_BYTES));
    });

    group.bench_function("io_sim_persistence_one_message", |b| {
        b.iter(|| run_io_sim_persistence(1, PAYLOAD_SIZE_BYTES));
    });
    group.bench_function("io_sim_direct_one_message", |b| {
        b.iter(|| run_io_sim_direct(1, PAYLOAD_SIZE_BYTES));
    });

    group.finish();
}

criterion_group!(
    pipeline_benches,
    benchmark_runtime_pipeline_throughput,
    benchmark_runtime_pipeline_latency
);
criterion_main!(pipeline_benches);
