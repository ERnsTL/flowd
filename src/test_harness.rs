use crate::bench_api::BenchRuntimeHarness;
use crate::*;
use std::time::Duration;

/// High-level test harness for implementing the testing strategy
/// Tests are external clients of the runtime, not components in the graph
pub struct TestHarness {
    harness: BenchRuntimeHarness,
}

impl TestHarness {
    /// Create a new test harness with a given graph name
    pub fn new(graph_name: &str) -> Self {
        let harness = BenchRuntimeHarness::new(Graph::new(
            graph_name.to_string(),
            "Test harness graph".to_string(),
            "test".to_string(),
        ));

        TestHarness {
            harness,
        }
    }

    /// Get access to the underlying BenchRuntimeHarness
    pub fn harness(&self) -> &BenchRuntimeHarness {
        &self.harness
    }

    /// Get mutable access to the underlying BenchRuntimeHarness
    pub fn harness_mut(&mut self) -> &mut BenchRuntimeHarness {
        &mut self.harness
    }



    /// Add a component under test
    pub fn add_component_under_test(&mut self, component_name: &str, node_id: &str) -> &mut Self {
        let graph_name = self
            .harness
            .graph
            .read()
            .expect("lock poisoned")
            .properties
            .name
            .clone();
        self.harness
            .graph
            .write()
            .expect("lock poisoned")
            .add_node(
                graph_name,
                component_name.to_string(),
                node_id.to_string(),
                GraphNodeMetadata {
                    x: 200,
                    y: 100,
                    width: Some(72),
                    height: Some(72),
                    label: Some(format!("CUT-{}", node_id)),
                    icon: None,
                },
            )
            .expect("Failed to add component under test");

        self
    }

    /// Connect components in the test graph
    pub fn connect(&mut self, from_node: &str, from_port: &str, to_node: &str, to_port: &str) -> &mut Self {
        let graph_name = self
            .harness
            .graph
            .read()
            .expect("lock poisoned")
            .properties
            .name
            .clone();
        self.harness
            .graph
            .write()
            .expect("lock poisoned")
            .add_edge(
                graph_name,
                GraphEdge {
                    source: GraphNodeSpec {
                        process: from_node.to_string(),
                        port: from_port.to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: to_node.to_string(),
                        port: to_port.to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("Failed to connect components");

        self
    }

    /// Add graph inport for external input
    pub fn add_graph_inport(&mut self, public_name: &str, node: &str, port: &str) -> &mut Self {
        self.harness.graph.write().expect("lock poisoned").add_inport(
            public_name.to_string(),
            GraphPort {
                process: node.to_string(),
                port: port.to_string(),
                metadata: GraphPortMetadata { x: 0, y: 0 },
            },
        ).expect("Failed to add graph inport");

        self
    }

    /// Add graph outport for external output
    pub fn add_graph_outport(&mut self, public_name: &str, node: &str, port: &str) -> &mut Self {
        self.harness.graph.write().expect("lock poisoned").add_outport(
            public_name.to_string(),
            GraphPort {
                process: node.to_string(),
                port: port.to_string(),
                metadata: GraphPortMetadata { x: 400, y: 0 },
            },
        ).expect("Failed to add graph outport");

        self
    }

    /// Add initial information packet (IIP) to a component port
    pub fn add_iip(&mut self, node: &str, port: &str, data: &str) -> &mut Self {
        use crate::*;

        let graph_name = self
            .harness
            .graph
            .read()
            .expect("lock poisoned")
            .properties
            .name
            .clone();

        self.harness
            .graph
            .write()
            .expect("lock poisoned")
            .add_initialip(GraphAddinitialRequestPayload {
                graph: graph_name,
                metadata: GraphEdgeMetadata::new(None, None, None),
                src: GraphIIPSpecNetwork {
                    data: data.to_string(),
                },
                tgt: GraphNodeSpecNetwork {
                    node: node.to_string(),
                    port: port.to_string(),
                    index: None,
                },
                secret: None,
            })
            .expect("Failed to add IIP");

        self
    }

    /// Start the test harness
    pub fn start(&self) -> std::result::Result<(), std::io::Error> {
        self.harness.start()
    }

    /// Stop the test harness
    pub fn stop(&self) -> std::result::Result<(), std::io::Error> {
        self.harness.stop()
    }

    /// Send data to a graph inport
    pub fn send_input(&self, inport: &str, data: &[u8]) -> std::result::Result<(), std::io::Error> {
        self.harness.send_data_to_inport(inport, data)
    }

    /// Wait for output on a graph outport
    pub fn wait_for_output(&self, outport: &str, expected_count: usize, timeout: Duration) -> std::result::Result<(), std::io::Error> {
        self.harness.wait_for_outport_data(outport, expected_count, timeout)
    }

    /// Collect outputs from a graph outport
    pub fn collect_outputs(&self, outport: &str) -> Vec<MessageBuf> {
        self.harness.collect_outputs(outport)
    }

    /// Assert set equality for outputs
    pub fn assert_outputs_set_equal(&self, outport: &str, expected: &[&[u8]]) {
        self.harness.assert_outputs_set_equal(outport, expected);
    }

    /// Assert sequence equality for outputs
    pub fn assert_outputs_sequence_equal(&self, outport: &str, expected: &[&[u8]]) {
        self.harness.assert_outputs_sequence_equal(outport, expected);
    }

    /// Assert no message loss
    pub fn assert_no_message_loss(&self, input_count: usize, outport: &str) {
        self.harness.assert_no_message_loss(input_count, outport);
    }

    /// Clear collected outputs
    pub fn clear_outputs(&self) {
        self.harness.clear_outputs();
    }

    /// Assert window-based behavior (event must occur within N scheduler cycles)
    pub fn assert_event_within_window<F>(
        &self,
        outport: &str,
        condition: F,
        max_cycles: usize,
    ) -> std::result::Result<(), std::io::Error>
    where
        F: Fn(&[MessageBuf]) -> bool,
    {
        for cycle in 0..max_cycles {
            // A "cycle" here is one scheduler opportunity where we poll observed
            // runtime outputs and then yield to allow further execution.
            let outputs = self.harness.collect_outputs(outport);
            if condition(&outputs) {
                return Ok(());
            }

            if cycle + 1 < max_cycles {
                let _ = self
                    .harness
                    .wait_for_outport_data(outport, 1, Duration::from_millis(1));
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("Condition not met within {} cycles", max_cycles)
        ))
    }

    /// Assert that no output is observed for a given scheduler/poll window.
    pub fn assert_no_output_within_window(
        &self,
        outport: &str,
        max_cycles: usize,
    ) -> std::result::Result<(), std::io::Error> {
        for cycle in 0..max_cycles {
            let outputs = self.harness.collect_outputs(outport);
            if !outputs.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "Unexpected output observed on port '{}' at cycle {}",
                        outport, cycle
                    ),
                ));
            }

            if cycle + 1 < max_cycles {
                let _ = self
                    .harness
                    .wait_for_outport_data(outport, 1, Duration::from_millis(1));
            }
        }

        Ok(())
    }

    /// Assert property-based invariants
    pub fn assert_property<F>(&self, property_name: &str, validator: F)
    where
        F: FnOnce(&Self) -> bool,
    {
        assert!(validator(self), "Property '{}' failed", property_name);
    }

    /// Assert backpressure behavior - verify bounded memory usage and eventual processing
    /// This test validates the core backpressure invariant: bounded edges prevent unbounded memory growth
    /// and ensure all messages are eventually processed, without timing assumptions.
    pub fn assert_backpressure_behavior(&self, input_port: &str, output_port: &str, test_data: &[&[u8]]) {
        // Send all messages at once to potentially saturate bounded edges
        for data in test_data {
            self.send_input(input_port, data).expect("Failed to send input");
        }

        // Send additional messages to test that system remains stable under load
        for i in 0..5 {
            let extra_data = format!("backpressure_test_msg_{}", i);
            self.send_input(input_port, extra_data.as_bytes()).expect("Failed to send backpressure test input");
        }

        let total_expected = test_data.len() + 5;

        // Wait for all messages to be processed - this validates that:
        // 1. Bounded edges prevent unbounded memory growth (no OOM)
        // 2. Backpressure ensures eventual processing of all messages
        // 3. System remains stable under load
        self.wait_for_output(output_port, total_expected, Duration::from_secs(60))
            .expect("Backpressure test: system failed to process all messages within timeout");

        // Verify no message loss - this is the core backpressure invariant
        self.assert_no_message_loss(total_expected, output_port);
    }

    /// Assert that producer-side backpressure is observable when downstream is saturated.
    /// This checks that input injection eventually blocks/times out under sustained load.
    pub fn assert_backpressure_propagation(
        &self,
        input_port: &str,
        probe_messages: usize,
        slow_send_threshold: Duration,
        min_slow_or_blocked_sends: usize,
    ) -> std::result::Result<(), std::io::Error> {
        let mut slow_or_blocked = 0usize;

        for i in 0..probe_messages {
            let payload = format!("bp_probe_{i}");
            let send_started = std::time::Instant::now();
            match self.send_input(input_port, payload.as_bytes()) {
                Ok(()) => {
                    if send_started.elapsed() >= slow_send_threshold {
                        slow_or_blocked += 1;
                    }
                }
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::TimedOut {
                        slow_or_blocked += 1;
                        break;
                    }
                    return Err(err);
                }
            }
        }

        assert!(
            slow_or_blocked >= min_slow_or_blocked_sends,
            "Expected backpressure (slow or blocked sends) but only observed {slow_or_blocked} / {min_slow_or_blocked_sends} slow-or-blocked sends"
        );

        Ok(())
    }

    /// Assert deterministic behavior - same inputs should produce same outputs
    pub fn assert_deterministic_behavior<F>(
        &self,
        input_port: &str,
        output_port: &str,
        inputs: &[&[u8]],
        setup_fn: F
    ) where
        F: Fn(&Self) -> std::result::Result<(), std::io::Error>,
    {
        // Run test twice and compare outputs
        let outputs1;
        let outputs2;

        // First run
        self.clear_outputs();
        setup_fn(self).expect("Setup failed for first run");
        for input in inputs {
            self.send_input(input_port, input).expect("Failed to send input in first run");
        }
        self.wait_for_output(output_port, inputs.len(), Duration::from_secs(2))
            .expect("First run timed out");
        outputs1 = self.collect_outputs(output_port);

        // Second run
        self.clear_outputs();
        setup_fn(self).expect("Setup failed for second run");
        for input in inputs {
            self.send_input(input_port, input).expect("Failed to send input in second run");
        }
        self.wait_for_output(output_port, inputs.len(), Duration::from_secs(2))
            .expect("Second run timed out");
        outputs2 = self.collect_outputs(output_port);

        assert_eq!(outputs1, outputs2, "Non-deterministic behavior detected");
    }

    /// Run a complete test scenario
    pub fn run_test_scenario<F>(&self, setup: F) -> std::result::Result<(), std::io::Error>
    where
        F: FnOnce(&Self) -> std::result::Result<(), std::io::Error>,
    {
        self.clear_outputs();
        self.start()?;
        let scenario_result = setup(self);
        let stop_result = self.stop();

        match (scenario_result, stop_result) {
            (Err(test_err), _) => Err(test_err),
            (Ok(()), Err(stop_err)) => Err(stop_err),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

/// Helper function to create a linear pipeline test
/// Tests are external clients - this creates a graph with inport/outport for external interaction
pub fn create_linear_pipeline_test(component_under_test: &str) -> TestHarness {
    let mut harness = TestHarness::new("linear_pipeline_test");

    harness
        .add_component_under_test(component_under_test, "cut")
        .add_graph_inport("IN", "cut", "IN")
        .add_graph_outport("OUT", "cut", "OUT");

    harness
}

/// Helper function to create a fan-out test
/// Tests are external clients - this creates a graph with inport and multiple outports
pub fn create_fan_out_test(component_under_test: &str) -> TestHarness {
    let mut harness = TestHarness::new("fan_out_test");

    harness
        .add_component_under_test(component_under_test, "cut")
        .add_graph_inport("IN", "cut", "IN")
        .add_graph_outport("OUT1", "cut", "OUT1")
        .add_graph_outport("OUT2", "cut", "OUT2");

    harness
}

/// Helper function to create a property validation test
/// Tests are external clients - this creates a graph for property testing
pub fn create_property_test(component_under_test: &str) -> TestHarness {
    let mut harness = TestHarness::new("property_test");

    harness
        .add_component_under_test(component_under_test, "cut")
        .add_graph_inport("IN", "cut", "IN")
        .add_graph_outport("OUT", "cut", "OUT");

    harness
}

/// Helper function to create a fan-in test (multiple inputs to one output)
/// Tests are external clients - this creates a graph with multiple inports and one outport
pub fn create_fan_in_test(component_under_test: &str) -> TestHarness {
    let mut harness = TestHarness::new("fan_in_test");

    harness
        .add_component_under_test(component_under_test, "cut")
        .add_graph_inport("IN1", "cut", "IN1")
        .add_graph_inport("IN2", "cut", "IN2")
        .add_graph_outport("OUT", "cut", "OUT");

    harness
}

/// Helper function to create a mixed pipeline test
/// Tests are external clients - creates a pipeline with multiple components
pub fn create_mixed_pipeline_test() -> TestHarness {
    let mut harness = TestHarness::new("mixed_pipeline_test");

    harness
        .add_component_under_test("Trim", "trim")
        .add_component_under_test("Repeat", "repeat")
        .connect("trim", "OUT", "repeat", "IN")
        .add_graph_inport("IN", "trim", "IN")
        .add_graph_outport("OUT", "repeat", "OUT");

    harness
}

/// Helper function to create a transformer chain test
/// Tests are external clients - creates a chain of transformation components
pub fn create_transformer_chain_test() -> TestHarness {
    let mut harness = TestHarness::new("transformer_chain_test");

    harness
        .add_component_under_test("Trim", "trim1")
        .add_component_under_test("SplitLines", "split")
        .add_component_under_test("Count", "count")
        .connect("trim1", "OUT", "split", "IN")
        .connect("split", "OUT", "count", "IN")
        .add_iip("count", "CONF", "?mode=packets")
        .add_graph_inport("IN", "trim1", "IN")
        .add_graph_outport("OUT", "count", "OUT");

    harness
}

/// Helper function to create a filter test
/// Tests are external clients - creates a graph with filtering logic
pub fn create_filter_test() -> TestHarness {
    let mut harness = TestHarness::new("filter_test");

    harness
        .add_component_under_test("Trim", "trim")
        .add_component_under_test("Drop", "drop")
        .connect("trim", "OUT", "drop", "IN")
        .add_graph_inport("IN", "trim", "IN")
        .add_graph_outport("OUT", "drop", "OUT");

    harness
}
