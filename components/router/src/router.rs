use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, error, info, trace, warn};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct RoutingRule {
    condition: Condition,
    output_port: String,
}

#[derive(Debug, Deserialize)]
struct Condition {
    field: String,
    operator: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct RouterConfig {
    rules: Vec<RoutingRule>,
    default_output: Option<String>,
}

pub struct GenericRouterComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out_ports: HashMap<String, ProcessEdgeSink>,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    config: Option<RouterConfig>,
    compiled_rules: Vec<CompiledRule>,
}

#[derive(Debug)]
struct CompiledRule {
    field: String,
    operator: RuleOperator,
    value: String,
    output_port: String,
}

#[derive(Debug)]
enum RuleOperator {
    Equals,
    Contains,
    Regex(Regex),
    StartsWith,
    EndsWith,
}

impl Component for GenericRouterComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        _scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
    ) -> Self
    where
        Self: Sized,
    {
        // Collect all output ports dynamically
        let mut out_ports = HashMap::new();
        let mut port_names = Vec::new();

        // Collect keys first to avoid borrowing issues
        let port_keys: Vec<String> = outports.keys().cloned().collect();

        for port_name in port_keys {
            if let Some(sink) = outports.remove(&port_name).and_then(|mut sinks| sinks.pop()) {
                out_ports.insert(port_name.clone(), sink);
                port_names.push(port_name);
            }
        }

        debug!("Router initialized with output ports: {:?}", port_names);

        GenericRouterComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            out_ports,
            signals_in,
            signals_out,
            config: None,
            compiled_rules: Vec::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("GenericRouter process() called");

        let mut work_units = 0u32;

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text);
            }
        }

        // Check if we have configuration
        if self.config.is_none() {
            if let Ok(conf_msg) = self.conf.pop() {
                let conf_text = conf_msg.as_text().expect("config must be text");
                debug!("received config: {}", conf_text);
                match serde_json::from_str::<RouterConfig>(conf_text) {
                    Ok(config) => {
                        match compile_rules(&config.rules) {
                            Ok(compiled) => {
                                self.config = Some(config);
                                self.compiled_rules = compiled;
                                work_units += 1;
                            }
                            Err(err) => {
                                error!("failed to compile routing rules: {}", err);
                                return ProcessResult::Finished;
                            }
                        }
                    }
                    Err(err) => {
                        error!("failed to parse config JSON: {}", err);
                        return ProcessResult::Finished;
                    }
                }
            } else {
                return ProcessResult::NoWork;
            }
        }

        // Process input messages
        while context.remaining_budget > 0 && !self.inn.is_empty() {
            if let Ok(input_msg) = self.inn.pop() {
                let input_text = input_msg.as_text().unwrap_or("");
                debug!("routing message: {}", input_text.chars().take(100).collect::<String>());

                let output_port = self.route_message(input_text);
                debug!("routed to port: {}", output_port);

                if let Some(out_sink) = self.out_ports.get_mut(&output_port) {
                    let output_msg = FbpMessage::from_bytes(input_text.as_bytes().to_vec());
                    match out_sink.push(output_msg) {
                        Ok(()) => {
                            work_units += 1;
                            context.remaining_budget -= 1;
                        }
                        Err(PushError::Full(_)) => {
                            warn!("output port {} buffer full, stopping processing", output_port);
                            break;
                        }
                    }
                } else {
                    warn!("no output port found for: {}", output_port);
                }
            } else {
                break;
            }
        }

        // Check if input is abandoned
        if self.inn.is_abandoned() {
            info!("EOF on inport, shutting down");
            return ProcessResult::Finished;
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            ProcessResult::NoWork
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("GenericRouter"),
            description: String::from("Routes messages to different output ports based on configurable rules and conditions."),
            icon: String::from("random"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("JSON configuration with routing rules"),
                    values_allowed: vec![],
                    value_default: String::from("{\"rules\":[],\"default_output\":\"default\"}"),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("input data to route"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
            ],
            out_ports: vec![
                // Dynamic output ports will be added at runtime
            ],
            ..Default::default()
        }
    }
}

impl GenericRouterComponent {
    fn route_message(&self, input_text: &str) -> String {
        // Try to parse as JSON first
        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(input_text) {
            for rule in &self.compiled_rules {
                if self.evaluate_condition(&json_value, rule) {
                    return rule.output_port.clone();
                }
            }
        } else {
            // If not JSON, treat as plain text and check against a "content" field
            for rule in &self.compiled_rules {
                if rule.field == "content" && self.evaluate_text_condition(input_text, rule) {
                    return rule.output_port.clone();
                }
            }
        }

        // No rule matched, use default
        self.config
            .as_ref()
            .and_then(|c| c.default_output.clone())
            .unwrap_or_else(|| "default".to_string())
    }

    fn evaluate_condition(&self, json_value: &serde_json::Value, rule: &CompiledRule) -> bool {
        // Navigate to the field in the JSON
        let field_value = get_nested_field(json_value, &rule.field);
        let field_str = field_value.as_str().unwrap_or("");

        self.evaluate_text_condition(field_str, rule)
    }

    fn evaluate_text_condition(&self, text: &str, rule: &CompiledRule) -> bool {
        match &rule.operator {
            RuleOperator::Equals => text == rule.value,
            RuleOperator::Contains => text.contains(&rule.value),
            RuleOperator::Regex(regex) => regex.is_match(text),
            RuleOperator::StartsWith => text.starts_with(&rule.value),
            RuleOperator::EndsWith => text.ends_with(&rule.value),
        }
    }
}

fn compile_rules(rules: &[RoutingRule]) -> Result<Vec<CompiledRule>, String> {
    let mut compiled = Vec::new();

    for rule in rules {
        let operator = match rule.condition.operator.as_str() {
            "equals" => RuleOperator::Equals,
            "contains" => RuleOperator::Contains,
            "regex" => {
                match Regex::new(&rule.condition.value) {
                    Ok(regex) => RuleOperator::Regex(regex),
                    Err(err) => return Err(format!("invalid regex '{}': {}", rule.condition.value, err)),
                }
            }
            "starts_with" => RuleOperator::StartsWith,
            "ends_with" => RuleOperator::EndsWith,
            unknown => return Err(format!("unknown operator: {}", unknown)),
        };

        compiled.push(CompiledRule {
            field: rule.condition.field.clone(),
            operator,
            value: rule.condition.value.clone(),
            output_port: rule.output_port.clone(),
        });
    }

    Ok(compiled)
}

fn get_nested_field<'a>(value: &'a serde_json::Value, field_path: &str) -> &'a serde_json::Value {
    let mut current = value;

    for part in field_path.split('.') {
        match current {
            serde_json::Value::Object(map) => {
                if let Some(next) = map.get(part) {
                    current = next;
                } else {
                    return &serde_json::Value::Null;
                }
            }
            _ => return &serde_json::Value::Null,
        }
    }

    current
}