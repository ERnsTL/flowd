use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use flowd_component_api::{BudgetClass, Component, NodeContext, ProcessResult};

#[derive(Debug, Clone)]
pub struct SchedulerMetrics {
    pub executions_per_node: HashMap<String, u64>,
    pub work_units_per_node: HashMap<String, u64>,
    pub time_since_last_execution: HashMap<String, std::time::Duration>,
    pub queue_depth: usize,
    pub loop_iterations: u64,
}

#[derive(Debug)]
struct SchedulerState {
    nodes: HashMap<String, NodeContext>,
    components: HashMap<String, ScheduledComponent>,
    ready_flags: HashMap<String, std::sync::Arc<AtomicBool>>,
    ready_queue: VecDeque<String>,
    ready_set: HashSet<String>,
    metrics: SchedulerMetrics,
}

pub struct ScheduledComponent {
    pub instance: Box<dyn Component>,
}

impl std::fmt::Debug for ScheduledComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScheduledComponent")
            .field("instance", &"<Component>")
            .finish()
    }
}

impl ScheduledComponent {
    pub fn new(instance: Box<dyn Component>) -> Self {
        ScheduledComponent { instance }
    }

    pub fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        self.instance.process(context)
    }
}

#[derive(Debug, Default)]
struct ExecutionOutcome {
    did_work: bool,
    finished: bool,
    executions: u64,
    work_units: u64,
}

#[derive(Debug)]
pub struct Scheduler {
    state: Mutex<SchedulerState>,
    condvar: Condvar,
    running: AtomicBool,
}

#[cfg(feature = "enforce-process-non-blocking-contract")]
const PROCESS_CALL_MAX_BLOCKING: Duration = Duration::from_millis(10);

#[inline]
fn enforce_non_blocking_process_contract(node_id: &str, elapsed: Duration) {
    #[cfg(feature = "enforce-process-non-blocking-contract")]
    {
        assert!(
            elapsed <= PROCESS_CALL_MAX_BLOCKING,
            "process() call for node '{}' exceeded non-blocking limit: {:?} > {:?}",
            node_id,
            elapsed,
            PROCESS_CALL_MAX_BLOCKING
        );
    }

    #[cfg(not(feature = "enforce-process-non-blocking-contract"))]
    {
        let _ = (node_id, elapsed);
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Scheduler {
            state: Mutex::new(SchedulerState {
                nodes: HashMap::new(),
                components: HashMap::new(),
                ready_flags: HashMap::new(),
                ready_queue: VecDeque::new(),
                ready_set: HashSet::new(),
                metrics: SchedulerMetrics {
                    executions_per_node: HashMap::new(),
                    work_units_per_node: HashMap::new(),
                    time_since_last_execution: HashMap::new(),
                    queue_depth: 0,
                    loop_iterations: 0,
                },
            }),
            condvar: Condvar::new(),
            running: AtomicBool::new(true),
        }
    }

    pub fn add_node(&self, node_id: String, budget_class: BudgetClass) {
        let mut state = self.state.lock().expect("scheduler state lock poisoned");
        let ready_signal = std::sync::Arc::new(AtomicBool::new(false));
        let context = NodeContext {
            node_id: node_id.clone(),
            budget_class,
            remaining_budget: 0,
            ready_signal: ready_signal.clone(),
            last_execution: Instant::now(),
            execution_count: 0,
            work_units_processed: 0,
        };

        state.ready_flags.insert(node_id.clone(), ready_signal);
        state.nodes.insert(node_id.clone(), context);
        state.metrics.executions_per_node.insert(node_id.clone(), 0);
        state.metrics.work_units_per_node.insert(node_id.clone(), 0);
        state
            .metrics
            .time_since_last_execution
            .insert(node_id, std::time::Duration::ZERO);
    }

    pub fn add_component(&self, component: Box<dyn Component>, node_id: String) {
        let mut state = self.state.lock().expect("scheduler state lock poisoned");
        state
            .components
            .insert(node_id, ScheduledComponent::new(component));
    }

    pub fn signal_ready(&self, node_id: &str) -> bool {
        let mut state = self.state.lock().expect("scheduler state lock poisoned");
        let Some(flag) = state.ready_flags.get(node_id) else {
            return false;
        };

        flag.store(true, Ordering::Release);
        if state.ready_set.insert(node_id.to_string()) {
            state.ready_queue.push_back(node_id.to_string());
            state.metrics.queue_depth = state.ready_queue.len();
        }
        self.condvar.notify_one();
        true
    }

    pub fn node_ids(&self) -> Vec<String> {
        let state = self.state.lock().expect("scheduler state lock poisoned");
        state.nodes.keys().cloned().collect()
    }

    pub fn metrics_snapshot(&self) -> SchedulerMetrics {
        let mut state = self.state.lock().expect("scheduler state lock poisoned");
        let now = Instant::now();
        let mut time_since = HashMap::with_capacity(state.nodes.len());
        for (node_id, context) in &state.nodes {
            time_since.insert(node_id.clone(), now.duration_since(context.last_execution));
        }
        state.metrics.time_since_last_execution = time_since;
        state.metrics.clone()
    }

    pub fn run(&self) {
        loop {
            let (node_id, mut component, mut context) = {
                let mut state = self.state.lock().expect("scheduler state lock poisoned");
                while self.running.load(Ordering::Acquire) && state.ready_queue.is_empty() {
                    state = self
                        .condvar
                        .wait(state)
                        .expect("scheduler state lock poisoned while waiting");
                }

                if !self.running.load(Ordering::Acquire) {
                    break;
                }

                let Some(node_id) = state.ready_queue.pop_front() else {
                    continue;
                };
                state.ready_set.remove(&node_id);
                state.metrics.queue_depth = state.ready_queue.len();
                state.metrics.loop_iterations += 1;

                let Some(component) = state.components.remove(&node_id) else {
                    continue;
                };
                let Some(context) = state.nodes.remove(&node_id) else {
                    state.components.insert(node_id, component);
                    continue;
                };

                (node_id, component, context)
            };

            let outcome = Self::execute_component(&mut component, &mut context);
            let mut state = self.state.lock().expect("scheduler state lock poisoned");

            if let Some(executions) = state.metrics.executions_per_node.get_mut(&node_id) {
                *executions += outcome.executions;
            }
            if let Some(work_units) = state.metrics.work_units_per_node.get_mut(&node_id) {
                *work_units += outcome.work_units;
            }

            let ready_flag = context.ready_signal.clone();
            state.components.insert(node_id.clone(), component);
            state.nodes.insert(node_id.clone(), context);

            let should_requeue =
                !outcome.finished && (outcome.did_work || ready_flag.load(Ordering::Acquire));
            if should_requeue && state.ready_set.insert(node_id.clone()) {
                state.ready_queue.push_back(node_id);
                state.metrics.queue_depth = state.ready_queue.len();
                self.condvar.notify_one();
            }
        }
    }

    fn execute_component(
        component: &mut ScheduledComponent,
        context: &mut NodeContext,
    ) -> ExecutionOutcome {
        context.remaining_budget = context.budget_class as u32;
        context.ready_signal.store(false, Ordering::Release);

        let mut outcome = ExecutionOutcome::default();
        loop {
            if context.remaining_budget == 0 {
                break;
            }

            let budget_before = context.remaining_budget;
            let call_started = Instant::now();
            let process_result = component.process(context);
            enforce_non_blocking_process_contract(&context.node_id, call_started.elapsed());

            match process_result {
                ProcessResult::DidWork(units) => {
                    let accounted = units.max(1);
                    // Components are expected to respect the runtime budget, but during migration
                    // some still decrement `remaining_budget` themselves. To avoid double-accounting
                    // while preserving scheduler authority, enforce at least `accounted` budget usage
                    // per reported work unit and never increase remaining budget.
                    let budget_after_component = context.remaining_budget.min(budget_before);
                    let consumed_by_component = budget_before.saturating_sub(budget_after_component);
                    let consumed = consumed_by_component.max(accounted);
                    context.remaining_budget = budget_before.saturating_sub(consumed);
                    context.work_units_processed += accounted as u64;
                    context.execution_count += 1;
                    context.last_execution = Instant::now();

                    outcome.did_work = true;
                    outcome.executions += 1;
                    outcome.work_units += accounted as u64;
                }
                ProcessResult::NoWork => break,
                ProcessResult::Finished => {
                    context.ready_signal.store(false, Ordering::Release);
                    outcome.finished = true;
                    break;
                }
            }
        }

        outcome
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        self.condvar.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "enforce-process-non-blocking-contract")]
    use std::thread;

    struct DecrementingComponent;

    impl Component for DecrementingComponent {
        fn new(
            _inports: flowd_component_api::ProcessInports,
            _outports: flowd_component_api::ProcessOutports,
            _signals_in: flowd_component_api::ProcessSignalSource,
            _signals_out: flowd_component_api::ProcessSignalSink,
            _graph_inout: flowd_component_api::GraphInportOutportHandle,
        ) -> Self {
            Self
        }

        fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
            if context.remaining_budget == 0 {
                return ProcessResult::NoWork;
            }
            context.remaining_budget = context.remaining_budget.saturating_sub(1);
            ProcessResult::DidWork(1)
        }

        fn get_metadata() -> flowd_component_api::ComponentComponentPayload {
            flowd_component_api::ComponentComponentPayload::default()
        }
    }

    struct NonDecrementingComponent;

    impl Component for NonDecrementingComponent {
        fn new(
            _inports: flowd_component_api::ProcessInports,
            _outports: flowd_component_api::ProcessOutports,
            _signals_in: flowd_component_api::ProcessSignalSource,
            _signals_out: flowd_component_api::ProcessSignalSink,
            _graph_inout: flowd_component_api::GraphInportOutportHandle,
        ) -> Self {
            Self
        }

        fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
            if context.remaining_budget == 0 {
                return ProcessResult::NoWork;
            }
            ProcessResult::DidWork(1)
        }

        fn get_metadata() -> flowd_component_api::ComponentComponentPayload {
            flowd_component_api::ComponentComponentPayload::default()
        }
    }

    #[cfg(feature = "enforce-process-non-blocking-contract")]
    struct BlockingComponent;

    #[cfg(feature = "enforce-process-non-blocking-contract")]
    impl Component for BlockingComponent {
        fn new(
            _inports: flowd_component_api::ProcessInports,
            _outports: flowd_component_api::ProcessOutports,
            _signals_in: flowd_component_api::ProcessSignalSource,
            _signals_out: flowd_component_api::ProcessSignalSink,
            _graph_inout: flowd_component_api::GraphInportOutportHandle,
        ) -> Self {
            Self
        }

        fn process(&mut self, _context: &mut NodeContext) -> ProcessResult {
            thread::sleep(Duration::from_millis(20));
            ProcessResult::NoWork
        }

        fn get_metadata() -> flowd_component_api::ComponentComponentPayload {
            flowd_component_api::ComponentComponentPayload::default()
        }
    }

    fn test_context(budget_class: BudgetClass) -> NodeContext {
        NodeContext {
            node_id: "test-node".to_string(),
            budget_class,
            remaining_budget: 0,
            ready_signal: std::sync::Arc::new(AtomicBool::new(false)),
            last_execution: Instant::now(),
            execution_count: 0,
            work_units_processed: 0,
        }
    }

    #[test]
    fn execute_component_does_not_double_decrement_budget() {
        let mut component = ScheduledComponent::new(Box::new(DecrementingComponent));
        let mut context = test_context(BudgetClass::Heavy);
        let outcome = Scheduler::execute_component(&mut component, &mut context);

        assert_eq!(context.remaining_budget, 0);
        assert_eq!(outcome.work_units, BudgetClass::Heavy as u64);
        assert_eq!(context.work_units_processed, BudgetClass::Heavy as u64);
    }

    #[test]
    fn execute_component_accounts_budget_when_component_does_not_decrement() {
        let mut component = ScheduledComponent::new(Box::new(NonDecrementingComponent));
        let mut context = test_context(BudgetClass::Heavy);
        let outcome = Scheduler::execute_component(&mut component, &mut context);

        assert_eq!(context.remaining_budget, 0);
        assert_eq!(outcome.work_units, BudgetClass::Heavy as u64);
        assert_eq!(context.work_units_processed, BudgetClass::Heavy as u64);
    }

    #[cfg(feature = "enforce-process-non-blocking-contract")]
    #[test]
    #[should_panic(expected = "exceeded non-blocking limit")]
    fn execute_component_panics_when_process_blocks_too_long() {
        let mut component = ScheduledComponent::new(Box::new(BlockingComponent));
        let mut context = test_context(BudgetClass::Heavy);
        let _ = Scheduler::execute_component(&mut component, &mut context);
    }
}
