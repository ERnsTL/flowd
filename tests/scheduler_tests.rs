//! Engine-level tests for the scheduler
//! These tests validate scheduler behavior in isolation

use flowd_rs::scheduler::Scheduler;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Mock component for testing scheduler behavior
struct MockComponent {
    process_count: Mutex<u32>,
    should_work: bool,
}

/// Panics from process() and increments a shared counter.
struct PanicComponent {
    panics: Arc<AtomicUsize>,
}

impl PanicComponent {
    fn new(panics: Arc<AtomicUsize>) -> Self {
        PanicComponent { panics }
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
        PanicComponent::new(Arc::new(AtomicUsize::new(0)))
    }

    fn process(
        &mut self,
        _context: &mut flowd_component_api::NodeContext,
    ) -> flowd_component_api::ProcessResult {
        self.panics.fetch_add(1, Ordering::SeqCst);
        panic!("intentional test panic")
    }

    fn get_metadata() -> flowd_component_api::ComponentComponentPayload {
        flowd_component_api::ComponentComponentPayload::default()
    }
}

impl MockComponent {
    fn new(should_work: bool) -> Self {
        MockComponent {
            process_count: Mutex::new(0),
            should_work,
        }
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

    #[test]
    fn test_scheduler_survives_component_panic() {
        let scheduler = Arc::new(Scheduler::new());
        let node_id = "panic_node".to_string();
        let panic_count = Arc::new(AtomicUsize::new(0));

        scheduler.add_node(node_id.clone(), flowd_component_api::BudgetClass::Normal);
        scheduler.add_component(
            Box::new(PanicComponent::new(Arc::clone(&panic_count))),
            node_id.clone(),
        );
        scheduler.signal_ready(&node_id);

        let runner = Arc::clone(&scheduler);
        let handle = std::thread::spawn(move || runner.run());
        std::thread::sleep(Duration::from_millis(100));

        // Scheduler should still be alive after component panic.
        assert!(!handle.is_finished(), "scheduler thread should keep running");
        assert!(
            panic_count.load(Ordering::SeqCst) >= 1,
            "panic component should have executed at least once"
        );

        scheduler.stop();
        handle.join().expect("scheduler thread should join cleanly");
    }

    #[test]
    fn test_scheduler_restarts_cleanly_after_panic_run() {
        // First run with a panicking component.
        let scheduler1 = Arc::new(Scheduler::new());
        let node_id1 = "panic_restart_node".to_string();
        scheduler1.add_node(node_id1.clone(), flowd_component_api::BudgetClass::Normal);
        scheduler1.add_component(
            Box::new(PanicComponent::new(Arc::new(AtomicUsize::new(0)))),
            node_id1.clone(),
        );
        scheduler1.signal_ready(&node_id1);

        let runner1 = Arc::clone(&scheduler1);
        let handle1 = std::thread::spawn(move || runner1.run());
        std::thread::sleep(Duration::from_millis(100));
        scheduler1.stop();
        handle1
            .join()
            .expect("first scheduler should stop cleanly");

        // Second run with a healthy component should also work.
        let scheduler2 = Arc::new(Scheduler::new());
        let node_id2 = "healthy_restart_node".to_string();
        scheduler2.add_node(node_id2.clone(), flowd_component_api::BudgetClass::Normal);
        scheduler2.add_component(Box::new(MockComponent::new(true)), node_id2.clone());
        scheduler2.signal_ready(&node_id2);

        let runner2 = Arc::clone(&scheduler2);
        let handle2 = std::thread::spawn(move || runner2.run());
        std::thread::sleep(Duration::from_millis(100));
        let metrics = scheduler2.metrics_snapshot();
        assert!(
            metrics
                .executions_per_node
                .get(&node_id2)
                .copied()
                .unwrap_or_default()
                > 0,
            "healthy component should execute after restart"
        );

        scheduler2.stop();
        handle2
            .join()
            .expect("second scheduler should stop cleanly");
    }
}
