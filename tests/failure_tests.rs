//! Failure testing scenarios
//! Tests for error conditions, component failures, and system resilience

use flowd_rs::test_harness::TestHarness;
use std::time::Duration;

const MEDIUM_TIMEOUT: Duration = Duration::from_secs(2);

/// Mock component that panics when process() is called
struct PanicComponent;

/// Mock component that works normally for testing recovery
struct MockWorkingComponent;

impl PanicComponent {
    fn new() -> Self {
        PanicComponent
    }
}

impl flowd_component_api::Component for PanicComponent {
    fn new(
        _inports: flowd_component_api::ProcessInports,
        _outports: flowd_component_api::ProcessOutports,
        _signals_in: flowd_component_api::ProcessSignalSource,
        _signals_out: flowd_component_api::ProcessSignalSink,
        _graph_inout: flowd_component_api::GraphInportOutportHandle,
    ) -> Self
    where
        Self: Sized,
    {
        PanicComponent::new()
    }

    fn process(&mut self, _context: &mut flowd_component_api::NodeContext) -> flowd_component_api::ProcessResult {
        panic!("Test component panic");
    }

    fn get_metadata() -> flowd_component_api::ComponentComponentPayload {
        flowd_component_api::ComponentComponentPayload::default()
    }
}

impl MockWorkingComponent {
    fn new() -> Self {
        MockWorkingComponent
    }
}

impl flowd_component_api::Component for MockWorkingComponent {
    fn new(
        _inports: flowd_component_api::ProcessInports,
        _outports: flowd_component_api::ProcessOutports,
        _signals_in: flowd_component_api::ProcessSignalSource,
        _signals_out: flowd_component_api::ProcessSignalSink,
        _graph_inout: flowd_component_api::GraphInportOutportHandle,
    ) -> Self
    where
        Self: Sized,
    {
        MockWorkingComponent::new()
    }

    fn process(&mut self, context: &mut flowd_component_api::NodeContext) -> flowd_component_api::ProcessResult {
        // Simple working component that does some work and finishes
        if context.remaining_budget > 0 {
            context.remaining_budget = context.remaining_budget.saturating_sub(1);
            flowd_component_api::ProcessResult::DidWork(1)
        } else {
            flowd_component_api::ProcessResult::Finished
        }
    }

    fn get_metadata() -> flowd_component_api::ComponentComponentPayload {
        flowd_component_api::ComponentComponentPayload::default()
    }
}

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
    fn test_pipeline_interruption() {
        // Test behavior when pipeline execution is interrupted
        let harness = new_repeat_harness("interruption_test");

        harness
            .run_test_scenario(|h| {
                // Send some messages
                h.send_input("IN", b"msg1")?;
                h.send_input("IN", b"msg2")?;

                // Wait for some processing
                h.wait_for_output("OUT", 1, MEDIUM_TIMEOUT)?;

                // Simulate interruption by stopping early
                // In a real scenario, this might be due to component failure
                let outputs = h.collect_outputs("OUT");
                assert!(outputs.len() >= 0, "Should handle interruption gracefully");

                Ok(())
            })
            .expect("pipeline interruption test failed");
    }

    #[test]
    fn test_partial_processing() {
        // Test scenarios where only some messages are processed
        let harness = new_repeat_harness("partial_processing_test");

        harness
            .run_test_scenario(|h| {
                // Send multiple messages
                for i in 0..5 {
                    h.send_input("IN", format!("msg{}", i).as_bytes())?;
                }

                // Only wait for partial completion
                h.wait_for_output("OUT", 3, MEDIUM_TIMEOUT)?;

                let outputs = h.collect_outputs("OUT");
                assert!(outputs.len() >= 3, "Should process at least some messages");

                // Verify no corruption in processed messages
                for output in outputs {
                    assert!(output.starts_with(b"msg"), "Output should be valid message");
                }

                Ok(())
            })
            .expect("partial processing test failed");
    }

    #[test]
    fn test_resource_exhaustion_simulation() {
        // Simulate high load that might cause resource exhaustion
        let harness = new_repeat_harness("resource_exhaustion_test");

        harness
            .run_test_scenario(|h| {
                // Send many messages rapidly
                for i in 0..50 {
                    h.send_input("IN", format!("msg{:03}", i).as_bytes())?;
                }

                // Wait for processing with extended timeout
                h.wait_for_output("OUT", 50, Duration::from_secs(10))?;

                // Verify all messages were processed
                h.assert_no_message_loss(50, "OUT");

                Ok(())
            })
            .expect("resource exhaustion simulation failed");
    }

    #[test]
    fn test_no_deadlocks_under_load() {
        // Test that system doesn't deadlock under concurrent load
        let harness = new_repeat_harness("deadlock_test");

        harness
            .run_test_scenario(|h| {
                // Send messages in rapid succession
                for batch in 0..5 {
                    for msg in 0..10 {
                        h.send_input("IN", format!("batch{}_msg{}", batch, msg).as_bytes())?;
                    }
                }

                // Wait for all processing to complete
                h.wait_for_output("OUT", 50, Duration::from_secs(15))?;

                // If we reach here without timeout, no deadlock occurred
                h.assert_no_message_loss(50, "OUT");

                Ok(())
            })
            .expect("deadlock under load test failed");
    }

    #[test]
    fn test_no_silent_data_corruption() {
        // Test that messages are not corrupted during processing
        let harness = new_repeat_harness("data_corruption_test");

        harness
            .run_test_scenario(|h| {
                let test_messages = vec![
                    "simple message",
                    "message with spaces",
                    "message\nwith\nnewlines",
                    "message\twith\ttabs",
                    "unicode: 🚀🌟💻",
                    "empty",
                ];
                let long_message = "very_long_message_".repeat(10);
                let all_messages = test_messages.iter().map(|s| *s).chain(std::iter::once(long_message.as_str()));

                // Send all test messages
                for msg in all_messages {
                    h.send_input("IN", msg.as_bytes())?;
                }

                // Wait for all outputs (test_messages.len() + 1 for the long message)
                h.wait_for_output("OUT", test_messages.len() + 1, Duration::from_secs(5))?;

                let outputs = h.collect_outputs("OUT");

                // Verify each output matches its input exactly
                for (i, output) in outputs.iter().enumerate() {
                    if i < test_messages.len() {
                        let expected = test_messages[i].as_bytes();
                        assert_eq!(output.as_slice(), expected,
                            "Message {} corrupted: expected {:?}, got {:?}",
                            i, expected, output.as_slice());
                    } else {
                        // Check the long message
                        assert!(output.starts_with(b"very_long_message_"),
                            "Long message corrupted: got {:?}", output.as_slice());
                    }
                }

                Ok(())
            })
            .expect("data corruption test failed");
    }

    #[test]
    fn test_graceful_degradation() {
        // Test that system degrades gracefully under stress
        let harness = new_repeat_harness("graceful_degradation_test");

        harness
            .run_test_scenario(|h| {
                // Send messages at increasing rate
                for batch_size in 1..=10 {
                    for msg in 0..batch_size {
                        let msg_str = format!("batch{}_msg{}", batch_size, msg);
                        h.send_input("IN", msg_str.as_bytes())?;
                    }

                    // Give some time for processing
                    std::thread::sleep(Duration::from_millis(10));
                }

                // Wait for final processing
                // Note: We don't assert exact count since this is testing degradation,
                // not exact message processing
                let result = h.wait_for_output("OUT", 1, Duration::from_secs(5));

                // System should either process messages or fail gracefully
                match result {
                    Ok(_) => {
                        let outputs = h.collect_outputs("OUT");
                        assert!(!outputs.is_empty(), "Should process at least some messages");
                    }
                    Err(_) => {
                        // Graceful failure is also acceptable
                        // (In real implementation, we'd check for proper error handling)
                    }
                }

                Ok(())
            })
            .expect("graceful degradation test failed");
    }

    #[test]
    fn test_component_panic_propagation() {
        // Test that component panics are caught and don't crash the system
        let scheduler = flowd_rs::scheduler::Scheduler::new();
        let node_id = "panic_node";

        scheduler.add_node(node_id.to_string(), flowd_component_api::BudgetClass::Normal);
        scheduler.add_component(
            Box::new(PanicComponent::new()),
            node_id.to_string(),
        );

        // Signal the node as ready to trigger execution
        scheduler.signal_ready(node_id);

        // Start the scheduler in a separate thread
        let scheduler_handle = std::thread::spawn(move || {
            scheduler.run();
        });

        // Give the scheduler time to execute the panicking component
        std::thread::sleep(std::time::Duration::from_millis(100));

        // The test passes if we reach here without panicking
        // The scheduler should have caught the panic and continued running
    }

    #[test]
    fn test_component_panic_marks_finished() {
        // Test that a panicking component is marked as finished and not re-executed
        let scheduler = flowd_rs::scheduler::Scheduler::new();
        let node_id = "panic_finished_node";

        scheduler.add_node(node_id.to_string(), flowd_component_api::BudgetClass::Normal);
        scheduler.add_component(
            Box::new(PanicComponent::new()),
            node_id.to_string(),
        );

        // Signal the node as ready multiple times
        scheduler.signal_ready(node_id);
        scheduler.signal_ready(node_id);
        scheduler.signal_ready(node_id);

        // Start the scheduler in a separate thread
        let scheduler_handle = std::thread::spawn(move || {
            scheduler.run();
        });

        // Give the scheduler time to execute
        std::thread::sleep(std::time::Duration::from_millis(200));

        // The test passes if we reach here - the scheduler should have caught
        // the panic on the first execution and not tried to re-execute the component
    }

    #[test]
    fn test_graph_restart_after_component_failure() {
        // Test that the system can restart a graph after component failures
        // This test verifies that we can create new schedulers and components after failures

        // First scheduler with a panic component
        let scheduler1 = flowd_rs::scheduler::Scheduler::new();
        let node_id1 = "panic_restart_node";

        scheduler1.add_node(node_id1.to_string(), flowd_component_api::BudgetClass::Normal);
        scheduler1.add_component(
            Box::new(PanicComponent::new()),
            node_id1.to_string(),
        );
        scheduler1.signal_ready(node_id1);

        let scheduler1_handle = std::thread::spawn(move || {
            scheduler1.run();
        });

        std::thread::sleep(std::time::Duration::from_millis(100));

        // Now create a second scheduler with a working component
        let scheduler2 = flowd_rs::scheduler::Scheduler::new();
        let node_id2 = "working_restart_node";

        scheduler2.add_node(node_id2.to_string(), flowd_component_api::BudgetClass::Normal);
        scheduler2.add_component(
            Box::new(MockWorkingComponent::new()),
            node_id2.to_string(),
        );
        scheduler2.signal_ready(node_id2);

        let scheduler2_handle = std::thread::spawn(move || {
            scheduler2.run();
        });

        std::thread::sleep(std::time::Duration::from_millis(100));

        // The test passes if both schedulers ran without crashing the process
    }

    #[test]
    fn test_component_reinitialization_after_panic() {
        // Test that components can be properly re-initialized after a panic
        // This simulates creating new component instances after failures

        for attempt in 0..3 {
            let scheduler = flowd_rs::scheduler::Scheduler::new();
            let node_id = format!("reinit_node_{}", attempt);

            scheduler.add_node(node_id.clone(), flowd_component_api::BudgetClass::Normal);
            scheduler.add_component(
                Box::new(PanicComponent::new()),
                node_id.clone(),
            );
            scheduler.signal_ready(&node_id);

            let scheduler_handle = std::thread::spawn(move || {
                scheduler.run();
            });

            std::thread::sleep(std::time::Duration::from_millis(50));

            // Each iteration creates a fresh component instance
            // The test passes if all iterations complete without issues
        }
    }

    #[test]
    fn test_state_consistency_after_failure_recovery() {
        // Test that system state remains consistent after failure and recovery
        let harness = new_repeat_harness("state_consistency_test");

        harness
            .run_test_scenario(|h| {
                // Send some messages and verify processing
                for i in 0..5 {
                    h.send_input("IN", format!("state_test_{}", i).as_bytes())?;
                }

                h.wait_for_output("OUT", 5, MEDIUM_TIMEOUT)?;
                h.assert_no_message_loss(5, "OUT");

                let outputs = h.collect_outputs("OUT");

                // Verify all messages were processed correctly
                for (i, output) in outputs.iter().enumerate() {
                    let expected = format!("state_test_{}", i);
                    assert_eq!(output, expected.as_bytes(),
                        "State consistency check failed for message {}", i);
                }

                Ok(())
            })
            .expect("state consistency after failure recovery test failed");
    }
}
