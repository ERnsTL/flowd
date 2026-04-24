//! Component-level unit tests
//! These tests validate isolated component logic without using the runtime

use flowd_component_api::*;
use flowd_repeat::RepeatComponent;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use multimap::MultiMap;

/// Specialized test harness for Repeat component
pub struct RepeatTestHarness {
    component: RepeatComponent,
    input_producer: rtrb::Producer<MessageBuf>,
    output_consumer: rtrb::Consumer<MessageBuf>,
    signal_sender: ProcessSignalSink,
    context: NodeContext,
}

impl RepeatTestHarness {
    /// Create a new test harness for the Repeat component
    pub fn new() -> Self {
        // Create ring buffers for component IN and OUT ports
        let (input_producer, input_consumer) = rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);
        let (output_producer, output_consumer) = rtrb::RingBuffer::<MessageBuf>::new(PROCESSEDGE_BUFSIZE);

        // Create signal channels
        let (signal_sender, signal_receiver) = mpsc::sync_channel(PROCESSEDGE_SIGNAL_BUFSIZE);

        // Set up inports and outports for the component
        let mut inports = MultiMap::new();
        inports.insert("IN".to_string(), input_consumer);

        let mut outports = MultiMap::new();
        outports.insert("OUT".to_string(), ProcessEdgeSink {
            sink: output_producer,
            wakeup: None,
            proc_name: None,
            signal_ready: None,
        });

        // Create dummy graph_inout handle (not used in tests)
        let graph_inout: GraphInportOutportHandle = (
            Arc::new(|_| {}), // network_output function
            Arc::new(|_| {}), // network_previewurl function
        );

        // Instantiate the component
        let component = RepeatComponent::new(
            inports,
            outports,
            signal_receiver,
            signal_sender.clone(),
            graph_inout,
        );

        let context = NodeContext {
            node_id: "test_repeat".to_string(),
            budget_class: BudgetClass::Normal,
            remaining_budget: 32,
            ready_signal: Arc::new(AtomicBool::new(false)),
            last_execution: Instant::now(),
            execution_count: 0,
            work_units_processed: 0,
        };

        RepeatTestHarness {
            component,
            input_producer,
            output_consumer,
            signal_sender,
            context,
        }
    }

    /// Send data to the component's IN port
    pub fn send_input(&mut self, data: &[u8]) -> Result<(), rtrb::PushError<MessageBuf>> {
        self.input_producer.push(data.to_vec())
    }

    /// Run the component for one processing cycle
    pub fn process(&mut self) -> ProcessResult {
        self.component.process(&mut self.context)
    }

    /// Collect all available outputs from the OUT port
    pub fn collect_outputs(&mut self) -> Vec<MessageBuf> {
        let mut outputs = Vec::new();
        while let Ok(data) = self.output_consumer.pop() {
            outputs.push(data);
        }
        outputs
    }

    /// Send a signal to the component
    pub fn send_signal(&self, signal: &[u8]) -> Result<(), mpsc::TrySendError<MessageBuf>> {
        self.signal_sender.try_send(signal.to_vec())
    }

    /// Check if output port has data
    pub fn has_output(&self) -> bool {
        !self.output_consumer.is_empty()
    }

    /// Reset the context for a new test
    pub fn reset_context(&mut self) {
        self.context.remaining_budget = 32;
        self.context.execution_count = 0;
        self.context.work_units_processed = 0;
        self.context.last_execution = Instant::now();
    }

    /// Get remaining budget
    pub fn remaining_budget(&self) -> u32 {
        self.context.remaining_budget
    }

    /// Set remaining budget
    pub fn set_budget(&mut self, budget: u32) {
        self.context.remaining_budget = budget;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repeat_basic_message_passing() {
        let mut harness = RepeatTestHarness::new();

        // Send a message to the input
        harness.send_input(b"hello world").unwrap();

        // Process the component
        let result = harness.process();

        // Should have done work
        assert!(matches!(result, ProcessResult::DidWork(1)));

        // Check output
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], b"hello world");
    }

    #[test]
    fn test_repeat_multiple_messages() {
        let mut harness = RepeatTestHarness::new();

        // Send multiple messages
        harness.send_input(b"msg1").unwrap();
        harness.send_input(b"msg2").unwrap();
        harness.send_input(b"msg3").unwrap();

        // Process until all messages are handled
        let mut total_work = 0u32;
        while harness.remaining_budget() > 0 {
            let result = harness.process();
            match result {
                ProcessResult::DidWork(work) => total_work += work,
                ProcessResult::NoWork => break,
                ProcessResult::Finished => break,
            }
        }

        // Should have processed 3 messages
        assert_eq!(total_work, 3);

        // Check outputs
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 3);
        assert_eq!(outputs[0], b"msg1");
        assert_eq!(outputs[1], b"msg2");
        assert_eq!(outputs[2], b"msg3");
    }

    #[test]
    fn test_repeat_empty_message() {
        let mut harness = RepeatTestHarness::new();

        // Send an empty message
        harness.send_input(b"").unwrap();

        // Process the component
        let result = harness.process();

        // Should have done work
        assert!(matches!(result, ProcessResult::DidWork(1)));

        // Check output
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], b"");
    }

    #[test]
    fn test_repeat_large_message() {
        let mut harness = RepeatTestHarness::new();

        // Create a large message (1KB)
        let large_msg = vec![b'A'; 1024];
        harness.send_input(&large_msg).unwrap();

        // Process the component
        let result = harness.process();

        // Should have done work
        assert!(matches!(result, ProcessResult::DidWork(1)));

        // Check output
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], large_msg);
    }

    #[test]
    fn test_repeat_signal_stop() {
        let mut harness = RepeatTestHarness::new();

        // Send a message to input
        harness.send_input(b"test").unwrap();

        // Send stop signal
        harness.send_signal(b"stop").unwrap();

        // Process the component
        let result = harness.process();

        // Should finish due to stop signal
        assert!(matches!(result, ProcessResult::Finished));

        // Should not have processed the message (stop takes precedence)
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 0);
    }

    #[test]
    fn test_repeat_signal_ping() {
        let mut harness = RepeatTestHarness::new();

        // Send ping signal
        harness.send_signal(b"ping").unwrap();

        // Process the component
        let result = harness.process();

        // Should have no work (no input messages)
        assert!(matches!(result, ProcessResult::NoWork));

        // The ping response would be sent to signals_out, but we can't easily test that
        // with our current harness. In a real scenario, the scheduler would handle this.
    }

    #[test]
    fn test_repeat_budget_exhaustion() {
        let mut harness = RepeatTestHarness::new();

        // Set a small budget
        harness.set_budget(2);

        // Send 3 messages
        harness.send_input(b"msg1").unwrap();
        harness.send_input(b"msg2").unwrap();
        harness.send_input(b"msg3").unwrap();

        // Process with limited budget
        let result1 = harness.process();
        // The component might process multiple messages in one call or return different work amounts
        assert!(matches!(result1, ProcessResult::DidWork(_)));

        let result2 = harness.process();
        // Continue processing until budget is exhausted
        if matches!(result2, ProcessResult::DidWork(_)) {
            let result3 = harness.process();
            assert!(matches!(result3, ProcessResult::NoWork));
        } else {
            assert!(matches!(result2, ProcessResult::NoWork));
        }

        // Should have processed some messages but not all due to budget exhaustion
        let outputs = harness.collect_outputs();
        assert!(outputs.len() >= 1 && outputs.len() < 3, "Should process some but not all messages due to budget");
        assert_eq!(outputs[0], b"msg1");
        if outputs.len() > 1 {
            assert_eq!(outputs[1], b"msg2");
        }
    }

    #[test]
    fn test_repeat_no_input() {
        let mut harness = RepeatTestHarness::new();

        // Process without any input
        let result = harness.process();

        // Should have no work
        assert!(matches!(result, ProcessResult::NoWork));

        // No outputs
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 0);
    }

    #[test]
    fn test_repeat_eof_handling() {
        let mut harness = RepeatTestHarness::new();

        // Send a message
        harness.send_input(b"test").unwrap();

        // Process to consume the message
        let result = harness.process();
        assert!(matches!(result, ProcessResult::DidWork(1)));

        // Now the input should be empty, and since we can't simulate EOF
        // in our test harness (we don't have access to abandon the queue),
        // we just verify normal operation
        let outputs = harness.collect_outputs();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], b"test");
    }

    #[test]
    fn test_repeat_deterministic_behavior() {
        // Test that the component produces consistent results
        let mut harness1 = RepeatTestHarness::new();
        let mut harness2 = RepeatTestHarness::new();

        // Send same inputs to both harnesses
        harness1.send_input(b"input1").unwrap();
        harness1.send_input(b"input2").unwrap();

        harness2.send_input(b"input1").unwrap();
        harness2.send_input(b"input2").unwrap();

        // Process both
        let _ = harness1.process();
        let _ = harness1.process();

        let _ = harness2.process();
        let _ = harness2.process();

        // Outputs should be identical
        let outputs1 = harness1.collect_outputs();
        let outputs2 = harness2.collect_outputs();

        assert_eq!(outputs1, outputs2);
        assert_eq!(outputs1.len(), 2);
        assert_eq!(outputs1[0], b"input1");
        assert_eq!(outputs1[1], b"input2");
    }
}
