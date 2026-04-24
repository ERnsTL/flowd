//! Property-based testing
//! Uses proptest to generate random inputs and verify system properties

use flowd_rs::test_harness::TestHarness;
use proptest::prelude::*;
use std::time::Duration;

const PROP_TIMEOUT: Duration = Duration::from_secs(10);

fn new_repeat_harness(graph_name: &str) -> TestHarness {
    let mut harness = TestHarness::new(graph_name);
    harness
        .add_component_under_test("Repeat", "repeat")
        .add_graph_inport("IN", "repeat", "IN")
        .add_graph_outport("OUT", "repeat", "OUT");
    harness
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("hex string has odd length".to_string());
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let mut i = 0usize;
    while i < hex.len() {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16)
            .map_err(|e| format!("invalid hex at {i}: {e}"))?;
        out.push(byte);
        i += 2;
    }
    Ok(out)
}

// Property: For any valid UTF-8 string input, the output should be identical
proptest! {
    #[test]
    fn prop_message_passthrough_utf8(message in "\\PC*") {
        let harness = new_repeat_harness("prop_passthrough_test");

        let result = harness.run_test_scenario(|h| {
            h.send_input("IN", message.as_bytes())?;
            h.wait_for_output("OUT", 1, PROP_TIMEOUT)?;

            let outputs = h.collect_outputs("OUT");
            assert_eq!(outputs.len(), 1, "Should have exactly one output");

            let output = &outputs[0];
            assert_eq!(output.as_slice(), message.as_bytes(),
                "Output should match input exactly");

            Ok(())
        });

        assert!(result.is_ok(), "Test scenario should complete successfully");
    }
}

// Property: Message integrity is preserved regardless of message size
proptest! {
    #[test]
    fn prop_message_integrity_by_size(
        base_message in "\\PC*",
        multiplier in 1..100usize
    ) {
        let message = base_message.repeat(multiplier);
        let harness = new_repeat_harness("prop_integrity_test");

        let result = harness.run_test_scenario(|h| {
            h.send_input("IN", message.as_bytes())?;
            h.wait_for_output("OUT", 1, PROP_TIMEOUT)?;

            let outputs = h.collect_outputs("OUT");
            assert_eq!(outputs.len(), 1, "Should have exactly one output");

            let output = &outputs[0];
            assert_eq!(output.as_slice(), message.as_bytes(),
                "Large message should be preserved exactly");

            Ok(())
        });

        assert!(result.is_ok(), "Large message test should complete successfully");
    }
}

// Property: System handles arbitrary sequences of messages
proptest! {
    #[test]
    fn prop_arbitrary_message_sequences(
        messages in prop::collection::vec("\\PC*", 1..20)
    ) {
        let harness = new_repeat_harness("prop_sequence_test");

        let result = harness.run_test_scenario(|h| {
            // Send all messages
            for message in &messages {
                h.send_input("IN", message.as_bytes())?;
            }

            // Wait for all outputs
            h.wait_for_output("OUT", messages.len(), PROP_TIMEOUT)?;

            let outputs = h.collect_outputs("OUT");
            assert_eq!(outputs.len(), messages.len(),
                "Should have same number of outputs as inputs");

            // Verify each message is preserved
            for (i, expected) in messages.iter().enumerate() {
                assert_eq!(outputs[i].as_slice(), expected.as_bytes(),
                    "Message {} should be preserved", i);
            }

            Ok(())
        });

        assert!(result.is_ok(), "Message sequence test should complete successfully");
    }
}

// Property: Binary data integrity via reversible transport encoding
// (hex payload is what traverses runtime text channels; decoded bytes must match).
proptest! {
    #[test]
    fn prop_binary_data_integrity(
        data in prop::collection::vec(prop::num::u8::ANY, 1..100)
    ) {
        let harness = new_repeat_harness("prop_binary_test");
        let encoded = hex_encode(&data);

        let result = harness.run_test_scenario(|h| {
            h.send_input("IN", encoded.as_bytes())?;
            h.wait_for_output("OUT", 1, PROP_TIMEOUT)?;

            let outputs = h.collect_outputs("OUT");
            assert_eq!(outputs.len(), 1, "Should have exactly one output");

            let output = &outputs[0];
            assert_eq!(output.as_slice(), encoded.as_bytes(), "Encoded payload should round-trip exactly");
            let decoded = hex_decode(std::str::from_utf8(output).expect("output should be valid utf-8 hex"))
                .expect("output should be valid hex");
            assert_eq!(decoded, data, "Decoded payload should match original bytes");

            Ok(())
        });

        assert!(result.is_ok(), "Binary data test should complete successfully");
    }
}

// Property: Empty messages are handled correctly
proptest! {
    #[test]
    fn prop_empty_message_handling(
        empty_count in 1..10usize
    ) {
        let harness = new_repeat_harness("prop_empty_test");

        let result = harness.run_test_scenario(|h| {
            // Send multiple empty messages
            for _ in 0..empty_count {
                h.send_input("IN", b"")?;
            }

            h.wait_for_output("OUT", empty_count, PROP_TIMEOUT)?;

            let outputs = h.collect_outputs("OUT");
            assert_eq!(outputs.len(), empty_count,
                "Should have same number of outputs as empty inputs");

            // All outputs should be empty
            for output in outputs {
                assert_eq!(output.as_slice(), b"",
                    "Empty message should remain empty");
            }

            Ok(())
        });

        assert!(result.is_ok(), "Empty message test should complete successfully");
    }
}

// Property: Very long messages don't cause system failure
proptest! {
    #[test]
    fn prop_very_long_messages(
        base_size in 1000..10000usize
    ) {
        let long_message = "x".repeat(base_size);
        let harness = new_repeat_harness("prop_long_test");

        let result = harness.run_test_scenario(|h| {
            h.send_input("IN", long_message.as_bytes())?;
            h.wait_for_output("OUT", 1, Duration::from_secs(30))?; // Longer timeout for big messages

            let outputs = h.collect_outputs("OUT");
            assert_eq!(outputs.len(), 1, "Should have exactly one output");

            let output = &outputs[0];
            assert_eq!(output.as_slice(), long_message.as_bytes(),
                "Very long message should be preserved exactly");

            Ok(())
        });

        assert!(result.is_ok(), "Very long message test should complete successfully");
    }
}

// Property: Mixed message sizes work correctly
proptest! {
    #[test]
    fn prop_mixed_message_sizes(
        sizes in prop::collection::vec(1..1000usize, 5..20)
    ) {
        let harness = new_repeat_harness("prop_mixed_sizes_test");

        let result = harness.run_test_scenario(|h| {
            let mut expected_messages = Vec::new();

            // Send messages of different sizes
            for size in &sizes {
                let message = "a".repeat(*size);
                h.send_input("IN", message.as_bytes())?;
                expected_messages.push(message);
            }

            h.wait_for_output("OUT", sizes.len(), PROP_TIMEOUT)?;

            let outputs = h.collect_outputs("OUT");
            assert_eq!(outputs.len(), sizes.len(),
                "Should have same number of outputs as varied inputs");

            // Verify each message size is preserved
            for (i, expected) in expected_messages.iter().enumerate() {
                assert_eq!(outputs[i].as_slice(), expected.as_bytes(),
                    "Message {} size should be preserved", i);
            }

            Ok(())
        });

        assert!(result.is_ok(), "Mixed message sizes test should complete successfully");
    }
}
