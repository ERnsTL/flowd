use flowd_rs::test_harness::{TestHarness, create_linear_pipeline_test, create_mixed_pipeline_test, create_transformer_chain_test, create_filter_test};
use std::time::Duration;

const SHORT_TIMEOUT: Duration = Duration::from_secs(1);
const MEDIUM_TIMEOUT: Duration = Duration::from_secs(2);
const LONG_TIMEOUT: Duration = Duration::from_secs(5);

fn new_repeat_harness(graph_name: &str) -> TestHarness {
    let mut harness = TestHarness::new(graph_name);
    harness
        .add_component_under_test("Repeat", "repeat")
        .add_graph_inport("IN", "repeat", "IN")
        .add_graph_outport("OUT", "repeat", "OUT");
    harness
}

#[test]
fn test_linear_pipeline() {
    let harness = new_repeat_harness("linear_pipeline_test");

    harness
        .run_test_scenario(|h| {
            h.send_input("IN", b"test_data")?;
            h.wait_for_output("OUT", 1, SHORT_TIMEOUT)?;
            h.assert_outputs_sequence_equal("OUT", &[b"test_data"]);
            Ok(())
        })
        .expect("linear pipeline test failed");
}

#[test]
fn test_message_integrity() {
    let harness = new_repeat_harness("message_integrity_test");

    harness
        .run_test_scenario(|h| {
            h.send_input("IN", b"msg1")?;
            h.send_input("IN", b"msg2")?;
            h.send_input("IN", b"msg3")?;

            h.wait_for_output("OUT", 3, MEDIUM_TIMEOUT)?;
            h.assert_no_message_loss(3, "OUT");
            h.assert_outputs_sequence_equal("OUT", &[b"msg1", b"msg2", b"msg3"]);
            Ok(())
        })
        .expect("message integrity test failed");
}

#[test]
fn test_fan_out() {
    let mut harness = TestHarness::new("fan_out_test");
    harness
        .add_component_under_test("Repeat", "stage_a")
        .add_component_under_test("Repeat", "stage_b")
        .add_graph_inport("IN", "stage_a", "IN")
        .connect("stage_a", "OUT", "stage_b", "IN")
        .add_graph_outport("OUT", "stage_b", "OUT");

    harness
        .run_test_scenario(|h| {
            h.send_input("IN", b"chain_data")?;
            h.wait_for_output("OUT", 1, MEDIUM_TIMEOUT)?;
            h.assert_outputs_sequence_equal("OUT", &[b"chain_data"]);
            Ok(())
        })
        .expect("pipeline chain test failed");
}

#[test]
fn test_set_based_validation() {
    let harness = new_repeat_harness("set_validation_test");

    harness
        .run_test_scenario(|h| {
            h.send_input("IN", b"item1")?;
            h.send_input("IN", b"item2")?;
            h.send_input("IN", b"item3")?;

            h.wait_for_output("OUT", 3, MEDIUM_TIMEOUT)?;
            h.assert_outputs_set_equal("OUT", &[b"item1", b"item2", b"item3"]);
            Ok(())
        })
        .expect("set-based validation test failed");
}

    #[test]
    fn test_property_ordering() {
        let harness = new_repeat_harness("ordering_test");

        harness
            .run_test_scenario(|h| {
                for i in 0..10 {
                    let msg = i.to_string();
                    h.send_input("IN", msg.as_bytes())?;
                }

                h.wait_for_output("OUT", 10, MEDIUM_TIMEOUT)?;
                h.assert_outputs_sequence_equal(
                    "OUT",
                    &[
                        b"0", b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9",
                    ],
                );
                Ok(())
            })
            .expect("ordering test failed");
    }

    #[test]
    fn test_mixed_pipeline() {
        let harness = create_mixed_pipeline_test();

        harness
            .run_test_scenario(|h| {
                h.send_input("IN", b"  hello world  ")?;
                h.wait_for_output("OUT", 1, MEDIUM_TIMEOUT)?;
                // Trim removes leading/trailing whitespace, Repeat passes it through
                h.assert_outputs_sequence_equal("OUT", &[b"hello world"]);
                Ok(())
            })
            .expect("mixed pipeline test failed");
    }

    #[test]
    fn test_transformer_chain() {
        let harness = create_transformer_chain_test();

        // Manually control the harness to ensure proper sequencing
        harness.start().expect("failed to start harness");
        harness.send_input("IN", b"line1\nline2\nline3").expect("failed to send input");
        // Give time for processing
        std::thread::sleep(std::time::Duration::from_millis(500));
        harness.stop().expect("failed to stop harness");

        // After harness stopped, check outputs
        let outputs = harness.collect_outputs("OUT");
        assert_eq!(outputs.len(), 1, "Count should output one packet");
        assert_eq!(outputs[0], b"3", "Count should output '3' (number of lines)");
    }

    #[test]
    fn test_filter_pipeline() {
        let harness = create_filter_test();

        harness
            .run_test_scenario(|h| {
                h.send_input("IN", b"  test data  ")?;
                // Drop component should drop all messages, so no output expected
                // Wait a short time to ensure processing completes, then check no output
                std::thread::sleep(std::time::Duration::from_millis(100));
                let outputs = h.collect_outputs("OUT");
                assert_eq!(outputs.len(), 0, "Drop component should filter all messages");
                Ok(())
            })
            .expect("filter pipeline test failed");
    }

    #[test]
    fn test_backpressure_infrastructure() {
        // Test that backpressure assertion infrastructure works correctly
        // This validates the testing framework without requiring actual backpressure implementation
        let harness = new_repeat_harness("backpressure_infrastructure_test");

        harness
            .run_test_scenario(|h| {
                // Send messages and verify the assertion method completes without timing issues
                let test_data: Vec<&[u8]> = vec![b"msg1", b"msg2", b"msg3"];
                h.assert_backpressure_behavior("IN", "OUT", &test_data);
                Ok(())
            })
            .expect("backpressure infrastructure test failed");
    }

    #[test]
    fn test_deterministic_behavior() {
        let harness = new_repeat_harness("deterministic_test");

        harness
            .run_test_scenario(|h| {
                // Test deterministic behavior within the scenario
                h.assert_deterministic_behavior(
                    "IN",
                    "OUT",
                    &[b"input1", b"input2", b"input3"],
                    |_| Ok(()),
                );
                Ok(())
            })
            .expect("deterministic behavior test failed");
    }

    #[test]
    fn test_message_integrity_large_volume() {
        let harness = new_repeat_harness("large_volume_test");

        harness
            .run_test_scenario(|h| {
                // Send 100 messages to test high-volume scenarios
                for i in 0..100 {
                    let msg = format!("message_{}", i);
                    h.send_input("IN", msg.as_bytes())?;
                }

                h.wait_for_output("OUT", 100, LONG_TIMEOUT)?;
                h.assert_no_message_loss(100, "OUT");
                Ok(())
            })
            .expect("large volume message integrity test failed");
    }

    #[test]
    fn test_property_message_integrity() {
        let harness = new_repeat_harness("property_integrity_test");

        harness
            .run_test_scenario(|h| {
                h.assert_property("message_integrity", |harness| {
                    // Send messages and verify no loss
                    harness.send_input("IN", b"test1").unwrap();
                    harness.send_input("IN", b"test2").unwrap();
                    harness.send_input("IN", b"test3").unwrap();

                    harness.wait_for_output("OUT", 3, MEDIUM_TIMEOUT).unwrap();
                    let outputs = harness.collect_outputs("OUT");
                    outputs.len() == 3
                });
                Ok(())
            })
            .expect("property message integrity test failed");
    }

    #[test]
    fn test_window_based_assertion() {
        let harness = new_repeat_harness("window_test");

        harness
            .run_test_scenario(|h| {
                h.send_input("IN", b"test_message")?;

                // Test that output appears within a reasonable window
                h.assert_event_within_window(
                    "OUT",
                    |outputs| outputs.len() >= 1,
                    10, // max cycles
                    MEDIUM_TIMEOUT,
                )?;
                Ok(())
            })
            .expect("window-based assertion test failed");
    }
