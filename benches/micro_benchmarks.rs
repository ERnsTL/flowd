//! Micro benchmark suite for issue #335.
//!
//! These benchmarks intentionally avoid the runtime and isolate primitive costs:
//! - ringbuffer push/pop throughput
//! - edge message transfer cost (payload-size dependent)
//! - cross-thread handover latency

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use flowd_component_api::{MessageBuf, PushError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

const RINGBUFFER_OPS: usize = 200_000;
const EDGE_TRANSFER_MSGS: usize = 100_000;
const CROSS_THREAD_MSGS: usize = 50_000;

fn push_blocking(producer: &mut rtrb::Producer<MessageBuf>, mut packet: MessageBuf) {
    loop {
        match producer.push(packet) {
            Ok(()) => return,
            Err(PushError::Full(returned)) => {
                packet = returned;
                thread::yield_now();
            }
        }
    }
}

fn benchmark_ringbuffer_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("micro_ringbuffer_push_pop");
    group.sample_size(20);
    group.throughput(Throughput::Elements(RINGBUFFER_OPS as u64));

    group.bench_function(BenchmarkId::new("ops", RINGBUFFER_OPS), |b| {
        b.iter(|| {
            let (mut producer, mut consumer) = rtrb::RingBuffer::<MessageBuf>::new(1024);
            let payload = vec![0_u8; 8];

            for _ in 0..RINGBUFFER_OPS {
                push_blocking(&mut producer, payload.clone());
                let msg = consumer
                    .pop()
                    .expect("consumer pop failed in ringbuffer throughput benchmark");
                black_box(msg);
            }
        });
    });

    group.finish();
}

fn benchmark_edge_transfer_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("micro_edge_transfer_cost");
    group.sample_size(20);

    for payload_size in [64_usize, 1024, 4096] {
        group.throughput(Throughput::Elements(EDGE_TRANSFER_MSGS as u64));
        group.bench_function(BenchmarkId::new("payload_bytes", payload_size), |b| {
            b.iter(|| {
                let (mut producer, mut consumer) = rtrb::RingBuffer::<MessageBuf>::new(1024);
                let payload = vec![7_u8; payload_size];

                for _ in 0..EDGE_TRANSFER_MSGS {
                    push_blocking(&mut producer, payload.clone());
                    let msg = consumer
                        .pop()
                        .expect("consumer pop failed in edge transfer benchmark");
                    black_box(msg);
                }
            });
        });
    }

    group.finish();
}

fn benchmark_cross_thread_handover(c: &mut Criterion) {
    let mut group = c.benchmark_group("micro_cross_thread_handover_latency");
    group.sample_size(30);
    group.throughput(Throughput::Elements(CROSS_THREAD_MSGS as u64));

    group.bench_function(BenchmarkId::new("messages", CROSS_THREAD_MSGS), |b| {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<MessageBuf>::new(1024);
        let (ack_tx, ack_rx) = mpsc::sync_channel::<()>(1024);
        let running = Arc::new(AtomicBool::new(true));
        let running_consumer = Arc::clone(&running);

        let consumer_thread = thread::spawn(move || {
            while running_consumer.load(Ordering::Relaxed) {
                if consumer.pop().is_ok() {
                    let _ = ack_tx.send(());
                } else {
                    thread::yield_now();
                }
            }
        });

        let payload = vec![1_u8; 64];

        b.iter(|| {
            for _ in 0..CROSS_THREAD_MSGS {
                push_blocking(&mut producer, payload.clone());
                ack_rx
                    .recv()
                    .expect("ack receive failed in cross-thread handover benchmark");
            }
        });

        running.store(false, Ordering::Relaxed);
        let _ = producer.push(Vec::new());
        consumer_thread
            .join()
            .expect("consumer thread panicked in cross-thread handover benchmark");
    });

    group.finish();
}

fn benchmark_micro_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("micro_latency_end_to_end");
    group.sample_size(40);

    group.bench_function("ringbuffer_one_message", |b| {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<MessageBuf>::new(8);
        let payload = vec![3_u8; 64];
        b.iter(|| {
            push_blocking(&mut producer, payload.clone());
            let msg = consumer
                .pop()
                .expect("consumer pop failed in ringbuffer one-message latency benchmark");
            black_box(msg);
        });
    });

    group.bench_function("cross_thread_one_message", |b| {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<MessageBuf>::new(64);
        let (ack_tx, ack_rx) = mpsc::sync_channel::<()>(64);
        let running = Arc::new(AtomicBool::new(true));
        let running_consumer = Arc::clone(&running);

        let consumer_thread = thread::spawn(move || {
            while running_consumer.load(Ordering::Relaxed) {
                if consumer.pop().is_ok() {
                    let _ = ack_tx.send(());
                } else {
                    thread::yield_now();
                }
            }
        });

        let payload = vec![4_u8; 64];
        b.iter(|| {
            push_blocking(&mut producer, payload.clone());
            ack_rx
                .recv()
                .expect("ack receive failed in cross-thread one-message latency benchmark");
        });

        running.store(false, Ordering::Relaxed);
        let _ = producer.push(Vec::new());
        consumer_thread
            .join()
            .expect("consumer thread panicked in cross-thread one-message latency benchmark");
    });

    group.finish();
}

criterion_group!(
    micro_benches,
    benchmark_ringbuffer_throughput,
    benchmark_edge_transfer_cost,
    benchmark_cross_thread_handover,
    benchmark_micro_latency
);
criterion_main!(micro_benches);
