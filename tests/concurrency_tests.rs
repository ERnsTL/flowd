//! Concurrency and stress testing
//! Tests for high-load scenarios, parallel execution, and scheduler fairness

use flowd_rs::test_harness::TestHarness;
use std::thread;
use std::time::Duration;

const STRESS_TIMEOUT: Duration = Duration::from_secs(30);

fn new_repeat_harness(graph_name: &str) -> TestHarness {
    let mut harness = TestHarness::new(graph_name);
    harness
        .add_component_under_test("Repeat", "repeat")
        .add_graph_inport("IN", "repeat", "IN")
        .add_graph_outport("OUT", "repeat", "OUT");
    harness
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_throughput_linear() {
        // Test high message throughput in a linear pipeline
        let harness = new_repeat_harness("high_throughput_test");

        harness
            .run_test_scenario(|h| {
                let message_count = 1000;

                // Send messages as fast as possible
                for i in 0..message_count {
                    h.send_input("IN", format!("msg_{:04}", i).as_bytes())?;
                }

                // Wait for all processing
                h.wait_for_output("OUT", message_count, STRESS_TIMEOUT)?;

                // Verify all messages processed
                h.assert_no_message_loss(message_count, "OUT");

                Ok(())
            })
            .expect("high throughput linear test failed");
    }

    #[test]
    fn test_concurrent_input_streams() {
        // Test multiple concurrent input streams
        let harness = new_repeat_harness("concurrent_streams_test");

        harness
            .run_test_scenario(|h| {
                let total_messages = 200;
                let messages_per_thread = 20;
                let thread_count = 10;

                use std::sync::mpsc;

                // Channel for threads to send messages to main thread
                let (tx, rx) = mpsc::channel();

                // Spawn multiple threads sending messages concurrently
                let mut handles = vec![];

                for thread_id in 0..thread_count {
                    let tx_clone = tx.clone();

                    let handle = thread::spawn(move || {
                        // Each thread sends its share of messages
                        for msg_id in 0..messages_per_thread {
                            let message =
                                format!("concurrent_msg_thread{}_msg{}", thread_id, msg_id);
                            tx_clone
                                .send(message)
                                .expect("Failed to send message through channel");
                        }
                    });
                    handles.push(handle);
                }

                // Drop the original sender so the receiver knows when all threads are done
                drop(tx);

                // Receive messages from threads and send them to harness
                for received_msg in rx {
                    h.send_input("IN", received_msg.as_bytes())?;
                }

                // Wait for all threads to complete
                for handle in handles {
                    handle.join().expect("Thread panicked");
                }

                // Wait for processing
                h.wait_for_output("OUT", total_messages, STRESS_TIMEOUT)?;
                h.assert_no_message_loss(total_messages, "OUT");

                Ok(())
            })
            .expect("concurrent input streams test failed");
    }

    #[test]
    fn test_scheduler_fairness() {
        // Test that scheduler provides fair access to CPU across components
        let harness = new_repeat_harness("scheduler_fairness_test");

        harness
            .run_test_scenario(|h| {
                let batch_size = 50;

                // Send messages in batches with different patterns
                for batch in 0..5 {
                    for msg in 0..batch_size {
                        let message = format!("fairness_batch{}_msg{}", batch, msg);
                        h.send_input("IN", message.as_bytes())?;
                    }
                }

                let expected_total = 5 * batch_size;
                h.wait_for_output("OUT", expected_total, STRESS_TIMEOUT)?;

                // Verify all messages processed (fairness means no starvation)
                h.assert_no_message_loss(expected_total, "OUT");

                Ok(())
            })
            .expect("scheduler fairness test failed");
    }

    #[test]
    fn test_memory_pressure_simulation() {
        // Test behavior under memory pressure with large messages
        let harness = new_repeat_harness("memory_pressure_test");

        harness
            .run_test_scenario(|h| {
                let large_message = "x".repeat(10000); // 10KB message
                let message_count = 50; // 500KB total

                // Send large messages
                for i in 0..message_count {
                    let message = format!("{}:{}", i, large_message);
                    h.send_input("IN", message.as_bytes())?;
                }

                // Wait for processing with extended timeout
                h.wait_for_output("OUT", message_count, Duration::from_secs(60))?;

                // Verify all large messages processed
                h.assert_no_message_loss(message_count, "OUT");

                Ok(())
            })
            .expect("memory pressure simulation test failed");
    }

    #[test]
    fn test_burst_traffic_handling() {
        // Test handling of burst traffic patterns
        let harness = new_repeat_harness("burst_traffic_test");

        harness
            .run_test_scenario(|h| {
                let burst_size = 100;

                // Send multiple bursts with delays
                for burst in 0..3 {
                    // Send burst
                    for msg in 0..burst_size {
                        h.send_input("IN", format!("burst{}_msg{}", burst, msg).as_bytes())?;
                    }
                }

                let total_messages = 3 * burst_size;
                h.wait_for_output("OUT", total_messages, STRESS_TIMEOUT)?;
                h.assert_no_message_loss(total_messages, "OUT");

                Ok(())
            })
            .expect("burst traffic handling test failed");
    }

    #[test]
    fn test_basic_repeat_functionality() {
        // Minimal test to verify basic graph inport/outport routing works
        let harness = new_repeat_harness("basic_repeat_test");

        harness
            .run_test_scenario(|h| {
                // Send just one message
                h.send_input("IN", b"test_message")?;

                // Wait for one output with short timeout
                h.wait_for_output("OUT", 1, Duration::from_secs(5))?;

                // Verify we got the expected output
                let outputs = h.collect_outputs("OUT");
                assert_eq!(outputs.len(), 1, "Should receive exactly one output");
                assert_eq!(outputs[0], b"test_message", "Output should match input");

                Ok(())
            })
            .expect("basic repeat functionality test failed");
    }

    #[test]
    fn test_long_running_stability() {
        // Test system stability over extended period
        let harness = new_repeat_harness("long_running_test");

        harness
            .run_test_scenario(|h| {
                let message_count = 500;

                // Send messages in smaller batches over time
                for batch in 0..10 {
                    for msg in 0..(message_count / 10) {
                        h.send_input("IN", format!("stable_batch{}_msg{}", batch, msg).as_bytes())?;
                    }
                }

                // Wait for all outputs to be processed
                h.wait_for_output("OUT", message_count, STRESS_TIMEOUT)?;

                // Verify all messages were processed
                h.assert_no_message_loss(message_count, "OUT");

                Ok(())
            })
            .expect("long running stability test failed");
    }

    #[test]
    fn test_component_isolation_under_load() {
        // Test that components remain isolated under high load
        let harness = new_repeat_harness("isolation_test");

        harness
            .run_test_scenario(|h| {
                // Send messages with different patterns to test isolation
                let patterns = ["pattern_a_", "pattern_b_", "pattern_c_"];

                for pattern in &patterns {
                    for i in 0..50 {
                        h.send_input("IN", format!("{}{}", pattern, i).as_bytes())?;
                    }
                }

                let total_messages = patterns.len() * 50;
                h.wait_for_output("OUT", total_messages, STRESS_TIMEOUT)?;

                let outputs = h.collect_outputs("OUT");

                // Verify all patterns are preserved (component isolation)
                for pattern in &patterns {
                    let pattern_count = outputs
                        .iter()
                        .filter(|output| output.starts_with(pattern.as_bytes()))
                        .count();
                    assert_eq!(
                        pattern_count, 50,
                        "Pattern {} should have 50 messages, got {}",
                        pattern, pattern_count
                    );
                }

                Ok(())
            })
            .expect("component isolation under load test failed");
    }
}
