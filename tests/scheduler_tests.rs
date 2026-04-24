//! Engine-level tests for the scheduler
//! These tests validate scheduler behavior in isolation

use flowd_rs::scheduler::Scheduler;
use std::sync::Mutex;

/// Mock component for testing scheduler behavior
struct MockComponent {
    process_count: Mutex<u32>,
    should_work: bool,
}

impl MockComponent {
    fn new(should_work: bool) -> Self {
        MockComponent {
            process_count: Mutex::new(0),
            should_work,
        }
    }

    fn process_count(&self) -> u32 {
        *self.process_count.lock().unwrap()
    }
}

impl flowd_component_api::Component for MockComponent {
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
        MockComponent::new(true)
    }

    fn process(&mut self, _context: &mut flowd_component_api::NodeContext) -> flowd_component_api::ProcessResult {
        let mut count = self.process_count.lock().unwrap();
        *count += 1;

        if self.should_work {
            flowd_component_api::ProcessResult::DidWork(1)
        } else {
            flowd_component_api::ProcessResult::NoWork
        }
    }

    fn get_metadata() -> flowd_component_api::ComponentComponentPayload {
        flowd_component_api::ComponentComponentPayload {
            name: "MockComponent".to_string(),
            description: "Mock component for testing".to_string(),
            icon: "fa-cog".to_string(),
            subgraph: false,
            in_ports: vec![],
            out_ports: vec![],
            support_health: false,
            support_perfdata: false,
            support_reconnect: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_creation() {
        let scheduler = Scheduler::new();

        let metrics = scheduler.metrics_snapshot();
        assert_eq!(metrics.loop_iterations, 0);
        assert_eq!(metrics.queue_depth, 0);

        let node_ids = scheduler.node_ids();
        assert_eq!(node_ids.len(), 0);
    }

    #[test]
    fn test_scheduler_add_node() {
        let scheduler = Scheduler::new();

        let node_id = "test_node".to_string();
        scheduler.add_node(node_id.clone(), flowd_component_api::BudgetClass::Normal);

        let node_ids = scheduler.node_ids();
        assert_eq!(node_ids.len(), 1);
        assert_eq!(node_ids[0], node_id);
    }

    #[test]
    fn test_scheduler_add_component() {
        let scheduler = Scheduler::new();
        let node_id = "test_node".to_string();

        scheduler.add_node(node_id.clone(), flowd_component_api::BudgetClass::Normal);

        let component = Box::new(MockComponent::new(true));
        scheduler.add_component(component, node_id.clone());

        // Component should be stored - verify by checking node exists
        let node_ids = scheduler.node_ids();
        assert!(node_ids.contains(&node_id));
    }

    #[test]
    fn test_scheduler_signal_ready() {
        let scheduler = Scheduler::new();
        let node_id = "test_node".to_string();

        scheduler.add_node(node_id.clone(), flowd_component_api::BudgetClass::Normal);
        let component = Box::new(MockComponent::new(true));
        scheduler.add_component(component, node_id.clone());

        // Signal node as ready
        let was_signaled = scheduler.signal_ready(&node_id);
        assert!(was_signaled, "Node should be signaled as ready");
    }

    #[test]
    fn test_scheduler_metrics() {
        let scheduler = Scheduler::new();

        let initial_metrics = scheduler.metrics_snapshot();
        assert_eq!(initial_metrics.loop_iterations, 0);
        assert_eq!(initial_metrics.queue_depth, 0);

        // Add a node
        let node_id = "test_node".to_string();
        scheduler.add_node(node_id.clone(), flowd_component_api::BudgetClass::Normal);

        let metrics_after_add = scheduler.metrics_snapshot();
        assert_eq!(metrics_after_add.executions_per_node.len(), 1);
        assert!(metrics_after_add.executions_per_node.contains_key(&node_id));
    }

    #[test]
    fn test_scheduler_budget_classes() {
        let scheduler = Scheduler::new();

        // Test different budget classes
        let normal_node = "normal_node".to_string();
        let heavy_node = "heavy_node".to_string();
        let realtime_node = "realtime_node".to_string();

        scheduler.add_node(normal_node.clone(), flowd_component_api::BudgetClass::Normal);
        scheduler.add_node(heavy_node.clone(), flowd_component_api::BudgetClass::Heavy);
        scheduler.add_node(realtime_node.clone(), flowd_component_api::BudgetClass::Realtime);

        let node_ids = scheduler.node_ids();
        assert_eq!(node_ids.len(), 3);
        assert!(node_ids.contains(&normal_node));
        assert!(node_ids.contains(&heavy_node));
        assert!(node_ids.contains(&realtime_node));
    }



    #[test]
    fn test_scheduler_invalid_node_signal() {
        let scheduler = Scheduler::new();

        // Try to signal a non-existent node
        let was_signaled = scheduler.signal_ready("nonexistent");
        assert!(!was_signaled, "Non-existent node should not be signaled");
    }
}