const TYPE_CORE_ANY: &str = "core/Any@1";
const TYPE_CORE_STRING: &str = "core/String@1";
const TYPE_CORE_BYTES: &str = "core/Bytes@1";
const TYPE_CORE_BOOL: &str = "core/Bool@1";
const TYPE_CORE_INT64: &str = "core/Int64@1";
const TYPE_CORE_FLOAT64: &str = "core/Float64@1";
const TYPE_CORE_OPEN_BRACKET: &str = "core/OpenBracket@1";
const TYPE_CORE_CLOSE_BRACKET: &str = "core/CloseBracket@1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IssueSeverity {
    Error,
    Warning,
}

impl IssueSeverity {
    fn rank(&self) -> u8 {
        match self {
            IssueSeverity::Error => 0,
            IssueSeverity::Warning => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemaProfile {
    Minimal,
    Strict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidationIssue {
    severity: IssueSeverity,
    code: &'static str,
    message: String,
    node_id: Option<String>,
    port_id: Option<String>,
    edge_id: Option<String>,
    iip_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ValidationReport {
    errors: Vec<ValidationIssue>,
    warnings: Vec<ValidationIssue>,
}

impl ValidationReport {
    fn push(&mut self, issue: ValidationIssue) {
        match issue.severity {
            IssueSeverity::Error => self.errors.push(issue),
            IssueSeverity::Warning => self.warnings.push(issue),
        }
    }

    fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    fn sorted(mut self) -> Self {
        self.errors.sort_by(compare_issues);
        self.warnings.sort_by(compare_issues);
        self
    }

    fn into_io_error(self) -> std::io::Error {
        let mut lines: Vec<String> = Vec::new();
        for issue in self.errors.iter().chain(self.warnings.iter()) {
            lines.push(format!(
                "[{}] {}: {}",
                match issue.severity {
                    IssueSeverity::Error => "error",
                    IssueSeverity::Warning => "warning",
                },
                issue.code,
                issue.message
            ));
        }
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("graph contract validation failed: {}", lines.join(" | ")),
        )
    }
}

fn compare_issues(a: &ValidationIssue, b: &ValidationIssue) -> std::cmp::Ordering {
    (
        a.severity.rank(),
        a.code,
        a.node_id.as_deref().unwrap_or(""),
        a.port_id.as_deref().unwrap_or(""),
        a.edge_id.as_deref().unwrap_or(""),
        a.iip_id.as_deref().unwrap_or(""),
    )
        .cmp(&(
            b.severity.rank(),
            b.code,
            b.node_id.as_deref().unwrap_or(""),
            b.port_id.as_deref().unwrap_or(""),
            b.edge_id.as_deref().unwrap_or(""),
            b.iip_id.as_deref().unwrap_or(""),
        ))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TypeId {
    namespace: String,
    name: String,
    major: u32,
}

impl TypeId {
    fn canonical(&self) -> String {
        format!("{}/{}@{}", self.namespace, self.name, self.major)
    }

    fn is_any(&self) -> bool {
        self.canonical() == TYPE_CORE_ANY
    }

    fn is_builtin_primitive(&self) -> bool {
        matches!(
            self.canonical().as_str(),
            TYPE_CORE_STRING
                | TYPE_CORE_BYTES
                | TYPE_CORE_BOOL
                | TYPE_CORE_INT64
                | TYPE_CORE_FLOAT64
                | TYPE_CORE_OPEN_BRACKET
                | TYPE_CORE_CLOSE_BRACKET
        )
    }
}

#[derive(Debug, Clone)]
struct RegistryEntry {
    type_id: TypeId,
    schema: Option<String>,
    compatible_from: Vec<TypeId>,
}

#[derive(Debug, Clone, Default)]
struct TypeRegistry {
    entries: std::collections::HashMap<String, RegistryEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompatibilityResult {
    CompatibleExact,
    CompatibleDeclared,
    RequiresAdapter,
    IncompatibleType,
    IncompatibleVersion,
}

pub(crate) fn schema_profile_from_env() -> SchemaProfile {
    match std::env::var("FLOWD_SCHEMA_PROFILE") {
        Ok(value) if value.eq_ignore_ascii_case("strict") => SchemaProfile::Strict,
        _ => SchemaProfile::Minimal,
    }
}

fn primitive_alias_to_builtin(input: &str) -> Option<&'static str> {
    match input {
        "string" => Some(TYPE_CORE_STRING),
        "bytes" | "byte" => Some(TYPE_CORE_BYTES),
        "boolean" | "bool" => Some(TYPE_CORE_BOOL),
        "integer" | "int" | "int64" => Some(TYPE_CORE_INT64),
        "float" | "float64" | "double" => Some(TYPE_CORE_FLOAT64),
        "openbracket" | "open_bracket" => Some(TYPE_CORE_OPEN_BRACKET),
        "closebracket" | "close_bracket" => Some(TYPE_CORE_CLOSE_BRACKET),
        "any" => Some(TYPE_CORE_ANY),
        _ => None,
    }
}

fn parse_type_id(raw: &str) -> Result<TypeId, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty");
    }
    let lowered = trimmed.to_ascii_lowercase();
    if let Some(mapped) = primitive_alias_to_builtin(lowered.as_str()) {
        return parse_type_id(mapped);
    }

    let (namespace, rest) = trimmed.split_once('/').ok_or("missing slash")?;
    let (name, major_raw) = rest.split_once('@').ok_or("missing at sign")?;
    if namespace.is_empty() || name.is_empty() || major_raw.is_empty() {
        return Err("missing parts");
    }
    if !is_valid_namespace(namespace) {
        return Err("invalid namespace");
    }
    if !is_valid_typename(name) {
        return Err("invalid type");
    }
    if !major_raw.chars().all(|c| c.is_ascii_digit()) {
        return Err("invalid major");
    }
    let major = major_raw.parse::<u32>().map_err(|_| "invalid major")?;
    if major == 0 {
        return Err("invalid major");
    }
    Ok(TypeId {
        namespace: namespace.to_string(),
        name: name.to_string(),
        major,
    })
}

fn is_valid_namespace(namespace: &str) -> bool {
    let bytes = namespace.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes.iter().all(|b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-'
    })
}

fn is_valid_typename(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    if !bytes[0].is_ascii_uppercase() {
        return false;
    }
    bytes.iter().all(|b| b.is_ascii_alphanumeric())
}

fn normalize_encoding_id(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if !is_valid_encoding_id(&normalized) {
        return None;
    }
    Some(normalized)
}

fn is_valid_encoding_id(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.is_empty() || bytes.len() > 32 {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes.iter().all(|b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-'
    })
}

impl TypeRegistry {
    fn build_from_components(components: &ComponentLibrary) -> Result<Self, std::io::Error> {
        let mut registry = TypeRegistry::default();
        for builtin in [
            TYPE_CORE_ANY,
            TYPE_CORE_STRING,
            TYPE_CORE_BYTES,
            TYPE_CORE_BOOL,
            TYPE_CORE_INT64,
            TYPE_CORE_FLOAT64,
            TYPE_CORE_OPEN_BRACKET,
            TYPE_CORE_CLOSE_BRACKET,
        ] {
            let id = parse_type_id(builtin).expect("builtin TypeId must parse");
            registry.entries.insert(
                id.canonical(),
                RegistryEntry {
                    type_id: id,
                    schema: None,
                    compatible_from: Vec::new(),
                },
            );
        }

        for component in components.available.iter() {
            for port in component.in_ports.iter().chain(component.out_ports.iter()) {
                let parsed = parse_type_id(&port.allowed_type).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "invalid type declaration '{}.{}' type='{}'",
                            component.name, port.name, port.allowed_type
                        ),
                    )
                })?;
                let key = parsed.canonical();
                registry.entries.entry(key).or_insert_with(|| RegistryEntry {
                    type_id: parsed,
                    schema: port.schema.clone(),
                    compatible_from: Vec::new(),
                });
            }
        }
        Ok(registry)
    }

    fn contains(&self, type_id: &TypeId) -> bool {
        self.entries.contains_key(&type_id.canonical())
    }

    fn compatibility(&self, producer: &TypeId, consumer: &TypeId) -> CompatibilityResult {
        if producer == consumer {
            return CompatibilityResult::CompatibleExact;
        }
        if let Some(entry) = self.entries.get(&consumer.canonical()) {
            if entry.compatible_from.iter().any(|compatible| compatible == producer) {
                return CompatibilityResult::CompatibleDeclared;
            }
        }
        if producer.namespace != consumer.namespace || producer.name != consumer.name {
            return CompatibilityResult::IncompatibleType;
        }
        if producer.major > consumer.major {
            return CompatibilityResult::IncompatibleVersion;
        }
        CompatibilityResult::RequiresAdapter
    }
}

fn validate_graph_contracts(
    graph: &Graph,
    components: &ComponentLibrary,
    profile: SchemaProfile,
) -> ValidationReport {
    let registry = match TypeRegistry::build_from_components(components) {
        Ok(registry) => registry,
        Err(err) => {
            return ValidationReport {
                errors: vec![ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "E_TYPE_UNKNOWN",
                    message: err.to_string(),
                    node_id: None,
                    port_id: None,
                    edge_id: None,
                    iip_id: None,
                }],
                warnings: Vec::new(),
            };
        }
    };

    let mut report = ValidationReport::default();
    let mut join_input_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut join_any_inputs: std::collections::HashMap<String, bool> =
        std::collections::HashMap::new();

    for edge in graph.edges.iter() {
        if edge.data.is_some() {
            continue;
        }

        let edge_id = format!(
            "{}.{}->{}.{}",
            edge.source.process, edge.source.port, edge.target.process, edge.target.port
        );
        let source_port =
            resolve_out_port(components, graph, &edge.source.process, &edge.source.port);
        let target_port =
            resolve_in_port(components, graph, &edge.target.process, &edge.target.port);

        let (source_port, target_port) = match (source_port, target_port) {
            (Ok(source), Ok(target)) => (source, target),
            (Err(message), _) | (_, Err(message)) => {
                report.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "E_TYPE_UNKNOWN",
                    message,
                    node_id: None,
                    port_id: None,
                    edge_id: Some(edge_id),
                    iip_id: None,
                });
                continue;
            }
        };

        let producer = match parse_type_id(&source_port.allowed_type) {
            Ok(parsed) => parsed,
            Err(_) => {
                report.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "E_TYPE_PARSE_INVALID",
                    message: format!("invalid producer TypeId '{}'", source_port.allowed_type),
                    node_id: Some(edge.source.process.clone()),
                    port_id: Some(edge.source.port.clone()),
                    edge_id: Some(edge_id),
                    iip_id: None,
                });
                continue;
            }
        };
        let consumer = match parse_type_id(&target_port.allowed_type) {
            Ok(parsed) => parsed,
            Err(_) => {
                report.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "E_TYPE_PARSE_INVALID",
                    message: format!("invalid consumer TypeId '{}'", target_port.allowed_type),
                    node_id: Some(edge.target.process.clone()),
                    port_id: Some(edge.target.port.clone()),
                    edge_id: Some(edge_id),
                    iip_id: None,
                });
                continue;
            }
        };

        if !registry.contains(&producer) || !registry.contains(&consumer) {
            report.push(ValidationIssue {
                severity: IssueSeverity::Error,
                code: "E_TYPE_UNKNOWN",
                message: format!(
                    "unknown registry entry producer={} consumer={}",
                    producer.canonical(),
                    consumer.canonical()
                ),
                node_id: None,
                port_id: None,
                edge_id: Some(format!(
                    "{}.{}->{}.{}",
                    edge.source.process, edge.source.port, edge.target.process, edge.target.port
                )),
                iip_id: None,
            });
            continue;
        }

        if producer.is_any() || consumer.is_any() {
            report.push(ValidationIssue {
                severity: IssueSeverity::Warning,
                code: "W_UNSAFE_ANY_EDGE",
                message: format!(
                    "unsafe Any edge between {} and {}",
                    producer.canonical(),
                    consumer.canonical()
                ),
                node_id: Some(edge.target.process.clone()),
                port_id: Some(edge.target.port.clone()),
                edge_id: Some(edge_id.clone()),
                iip_id: None,
            });
        } else {
            match registry.compatibility(&producer, &consumer) {
                CompatibilityResult::CompatibleExact | CompatibilityResult::CompatibleDeclared => {}
                CompatibilityResult::RequiresAdapter => report.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "E_TYPE_ADAPTER_REQUIRED",
                    message: format!(
                        "adapter required for {} -> {}",
                        producer.canonical(),
                        consumer.canonical()
                    ),
                    node_id: None,
                    port_id: None,
                    edge_id: Some(edge_id.clone()),
                    iip_id: None,
                }),
                CompatibilityResult::IncompatibleType | CompatibilityResult::IncompatibleVersion => {
                    report.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        code: "E_TYPE_INCOMPATIBLE",
                        message: format!(
                            "incompatible types {} -> {}",
                            producer.canonical(),
                            consumer.canonical()
                        ),
                        node_id: None,
                        port_id: None,
                        edge_id: Some(edge_id.clone()),
                        iip_id: None,
                    })
                }
            }
        }

        let structured = !producer.is_builtin_primitive() && !producer.is_any();
        if structured {
            let source_encoding = source_port
                .encoding
                .as_ref()
                .and_then(|raw| normalize_encoding_id(raw));
            let target_encoding = target_port
                .encoding
                .as_ref()
                .and_then(|raw| normalize_encoding_id(raw));
            if source_encoding.is_none() || target_encoding.is_none() {
                report.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "E_TYPE_PARSE_INVALID",
                    message: "missing or invalid encoding on structured typed port".to_string(),
                    node_id: None,
                    port_id: None,
                    edge_id: Some(edge_id.clone()),
                    iip_id: None,
                });
            } else if source_encoding != target_encoding {
                report.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "E_TYPE_ADAPTER_REQUIRED",
                    message: format!(
                        "encoding adapter required {} -> {}",
                        source_encoding.unwrap_or_default(),
                        target_encoding.unwrap_or_default()
                    ),
                    node_id: None,
                    port_id: None,
                    edge_id: Some(edge_id.clone()),
                    iip_id: None,
                });
            }
        }

        if profile == SchemaProfile::Strict
            && !producer.is_any()
            && !producer.is_builtin_primitive()
            && (source_port.schema.is_none() || target_port.schema.is_none())
        {
            report.push(ValidationIssue {
                severity: IssueSeverity::Error,
                code: "E_SCHEMA_REQUIRED_STRICT",
                message: "strict profile requires schema for non-Any ports".to_string(),
                node_id: Some(edge.target.process.clone()),
                port_id: Some(edge.target.port.clone()),
                edge_id: Some(edge_id.clone()),
                iip_id: None,
            });
        }

        if profile == SchemaProfile::Strict
            && source_port.schema.is_some()
            && target_port.schema.is_some()
            && source_port.schema != target_port.schema
        {
            report.push(ValidationIssue {
                severity: IssueSeverity::Error,
                code: "E_SCHEMA_INCOMPATIBLE",
                message: "producer/consumer schema references differ".to_string(),
                node_id: Some(edge.target.process.clone()),
                port_id: Some(edge.target.port.clone()),
                edge_id: Some(edge_id.clone()),
                iip_id: None,
            });
        }

        let count = join_input_counts
            .entry(edge.target.process.clone())
            .or_insert(0usize);
        *count += 1;
        if producer.is_any() || consumer.is_any() {
            join_any_inputs.insert(edge.target.process.clone(), true);
        }
    }

    for (node, count) in join_input_counts.iter() {
        if *count <= 1 {
            continue;
        }
        if join_any_inputs.get(node).copied().unwrap_or(false) {
            if profile == SchemaProfile::Strict {
                report.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "E_CORRELATION_REQUIRED",
                    message: format!("join node '{}' has Any input and no explicit unsafe override", node),
                    node_id: Some(node.clone()),
                    port_id: None,
                    edge_id: None,
                    iip_id: None,
                });
            } else {
                report.push(ValidationIssue {
                    severity: IssueSeverity::Warning,
                    code: "W_UNSAFE_CORRELATION_BYPASS",
                    message: format!("join node '{}' bypasses correlation guarantees", node),
                    node_id: Some(node.clone()),
                    port_id: None,
                    edge_id: None,
                    iip_id: None,
                });
            }
        }
    }

    for (index, edge) in graph.edges.iter().enumerate() {
        let Some(iip_raw) = &edge.data else {
            continue;
        };
        let target_port =
            match resolve_in_port(components, graph, &edge.target.process, &edge.target.port) {
                Ok(port) => port,
                Err(message) => {
                    report.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        code: "E_TYPE_UNKNOWN",
                        message,
                        node_id: Some(edge.target.process.clone()),
                        port_id: Some(edge.target.port.clone()),
                        edge_id: None,
                        iip_id: Some(index.to_string()),
                    });
                    continue;
                }
            };
        let target_type = match parse_type_id(&target_port.allowed_type) {
            Ok(value) => value,
            Err(_) => {
                report.push(ValidationIssue {
                    severity: IssueSeverity::Error,
                    code: "E_TYPE_PARSE_INVALID",
                    message: format!("invalid target IIP TypeId '{}'", target_port.allowed_type),
                    node_id: Some(edge.target.process.clone()),
                    port_id: Some(edge.target.port.clone()),
                    edge_id: None,
                    iip_id: Some(index.to_string()),
                });
                continue;
            }
        };

        let parsed_iip_type = parse_iip_declared_type(iip_raw);
        match parsed_iip_type {
            Some(Ok(iip_type)) => {
                if registry.compatibility(&iip_type, &target_type)
                    != CompatibilityResult::CompatibleExact
                {
                    report.push(ValidationIssue {
                        severity: if profile == SchemaProfile::Strict {
                            IssueSeverity::Error
                        } else {
                            IssueSeverity::Warning
                        },
                        code: if profile == SchemaProfile::Strict {
                            "E_IIP_TYPE_MISMATCH"
                        } else {
                            "W_IIP_TYPE_MISMATCH"
                        },
                        message: format!(
                            "IIP type '{}' incompatible with target '{}'",
                            iip_type.canonical(),
                            target_type.canonical()
                        ),
                        node_id: Some(edge.target.process.clone()),
                        port_id: Some(edge.target.port.clone()),
                        edge_id: None,
                        iip_id: Some(index.to_string()),
                    });
                }
            }
            Some(Err(_)) | None => {
                report.push(ValidationIssue {
                    severity: if profile == SchemaProfile::Strict {
                        IssueSeverity::Error
                    } else {
                        IssueSeverity::Warning
                    },
                    code: if profile == SchemaProfile::Strict {
                        "E_IIP_TYPE_MISMATCH"
                    } else {
                        "W_IIP_UNTYPED_PAYLOAD"
                    },
                    message: format!(
                        "IIP for {}.{} is untyped or ambiguous",
                        edge.target.process, edge.target.port
                    ),
                    node_id: Some(edge.target.process.clone()),
                    port_id: Some(edge.target.port.clone()),
                    edge_id: None,
                    iip_id: Some(index.to_string()),
                });
            }
        }
    }

    report.sorted()
}

fn parse_iip_declared_type(raw: &str) -> Option<Result<TypeId, &'static str>> {
    let payload: JsonValue = serde_json::from_str(raw).ok()?;
    let object = payload.as_object()?;
    let type_value = object.get("type")?;
    let type_string = type_value.as_str()?;
    Some(parse_type_id(type_string))
}

fn resolve_out_port<'a>(
    components: &'a ComponentLibrary,
    graph: &'a Graph,
    process: &str,
    port: &str,
) -> Result<&'a ComponentPort, String> {
    let node = graph
        .nodes
        .get(process)
        .ok_or_else(|| format!("source process '{}' not found", process))?;
    let component = components
        .available
        .iter()
        .find(|component| component.name == node.component)
        .ok_or_else(|| format!("component '{}' not found in library", node.component))?;
    component
        .out_ports
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(port))
        .ok_or_else(|| format!("source port '{}.{}' not found", process, port))
}

fn resolve_in_port<'a>(
    components: &'a ComponentLibrary,
    graph: &'a Graph,
    process: &str,
    port: &str,
) -> Result<&'a ComponentPort, String> {
    let node = graph
        .nodes
        .get(process)
        .ok_or_else(|| format!("target process '{}' not found", process))?;
    let component = components
        .available
        .iter()
        .find(|component| component.name == node.component)
        .ok_or_else(|| format!("component '{}' not found in library", node.component))?;
    component
        .in_ports
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(port))
        .ok_or_else(|| format!("target port '{}.{}' not found", process, port))
}

pub(crate) fn validate_graph_contracts_or_err(
    graph: &Graph,
    components: &ComponentLibrary,
    profile: SchemaProfile,
) -> Result<ValidationReport, std::io::Error> {
    let report = validate_graph_contracts(graph, components, profile);
    if report.is_ok() {
        Ok(report)
    } else {
        Err(report.into_io_error())
    }
}

#[cfg(test)]
mod type_system_tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn port(name: &str, allowed_type: &str, encoding: Option<&str>, schema: Option<&str>) -> ComponentPort {
        ComponentPort {
            name: name.to_string(),
            allowed_type: allowed_type.to_string(),
            encoding: encoding.map(|value| value.to_string()),
            schema: schema.map(|value| value.to_string()),
            required: true,
            is_arrayport: false,
            description: String::new(),
            values_allowed: vec![],
            value_default: String::new(),
        }
    }

    fn component(name: &str, in_ports: Vec<ComponentPort>, out_ports: Vec<ComponentPort>) -> ComponentComponentPayload {
        ComponentComponentPayload {
            name: name.to_string(),
            description: String::new(),
            icon: String::new(),
            subgraph: false,
            in_ports,
            out_ports,
            support_health: false,
            support_perfdata: false,
            support_reconnect: false,
        }
    }

    fn graph_with_nodes_and_edge(
        src_node: &str,
        src_component: &str,
        src_port: &str,
        tgt_node: &str,
        tgt_component: &str,
        tgt_port: &str,
    ) -> Graph {
        let mut graph = Graph::new("g".to_string(), "".to_string(), "".to_string());
        graph.nodes.insert(
            src_node.to_string(),
            GraphNode {
                component: src_component.to_string(),
                metadata: GraphNodeMetadata::default(),
                protocol_metadata: JsonMap::new(),
            },
        );
        graph.nodes.insert(
            tgt_node.to_string(),
            GraphNode {
                component: tgt_component.to_string(),
                metadata: GraphNodeMetadata::default(),
                protocol_metadata: JsonMap::new(),
            },
        );
        graph.edges.push(GraphEdge {
            source: GraphNodeSpec {
                process: src_node.to_string(),
                port: src_port.to_string(),
                index: None,
            },
            data: None,
            target: GraphNodeSpec {
                process: tgt_node.to_string(),
                port: tgt_port.to_string(),
                index: None,
            },
            metadata: GraphEdgeMetadata::default(),
        });
        graph
    }

    fn graph_with_join_inputs(
        left_component: &str,
        left_type: &str,
        right_component: &str,
        right_type: &str,
        join_component: &str,
        join_type: &str,
    ) -> (Graph, ComponentLibrary) {
        let components = ComponentLibrary::new(vec![
            component(
                left_component,
                vec![],
                vec![port("OUT", left_type, Some("json"), Some("schema/shared@1"))],
            ),
            component(
                right_component,
                vec![],
                vec![port("OUT", right_type, Some("json"), Some("schema/shared@1"))],
            ),
            component(
                join_component,
                vec![
                    port("LEFT", join_type, Some("json"), Some("schema/shared@1")),
                    port("RIGHT", join_type, Some("json"), Some("schema/shared@1")),
                ],
                vec![],
            ),
        ]);

        let mut graph = Graph::new("g".to_string(), "".to_string(), "".to_string());
        for (node_name, component_name) in [
            ("l", left_component),
            ("r", right_component),
            ("j", join_component),
        ] {
            graph.nodes.insert(
                node_name.to_string(),
                GraphNode {
                    component: component_name.to_string(),
                    metadata: GraphNodeMetadata::default(),
                    protocol_metadata: JsonMap::new(),
                },
            );
        }

        graph.edges.push(GraphEdge {
            source: GraphNodeSpec {
                process: "l".to_string(),
                port: "OUT".to_string(),
                index: None,
            },
            data: None,
            target: GraphNodeSpec {
                process: "j".to_string(),
                port: "LEFT".to_string(),
                index: None,
            },
            metadata: GraphEdgeMetadata::default(),
        });
        graph.edges.push(GraphEdge {
            source: GraphNodeSpec {
                process: "r".to_string(),
                port: "OUT".to_string(),
                index: None,
            },
            data: None,
            target: GraphNodeSpec {
                process: "j".to_string(),
                port: "RIGHT".to_string(),
                index: None,
            },
            metadata: GraphEdgeMetadata::default(),
        });

        (graph, components)
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn type_id_parses_canonical_and_primitive_alias() {
        let canonical = parse_type_id("email/EmailRaw@1").expect("canonical TypeId should parse");
        assert_eq!(canonical.canonical(), "email/EmailRaw@1");

        let alias = parse_type_id("string").expect("primitive alias should parse");
        assert_eq!(alias.canonical(), TYPE_CORE_STRING);
    }

    #[test]
    fn type_id_rejects_invalid_values() {
        assert!(parse_type_id("EmailRaw@1").is_err());
        assert!(parse_type_id("Email/email@1").is_err());
        assert!(parse_type_id("email/email@1").is_err());
        assert!(parse_type_id("email/Email@0").is_err());
    }

    #[test]
    fn encoding_id_is_normalized_to_lowercase() {
        assert_eq!(normalize_encoding_id(" JSON "), Some("json".to_string()));
        assert_eq!(normalize_encoding_id("CapNP"), Some("capnp".to_string()));
        assert_eq!(normalize_encoding_id("capn proto"), None);
    }

    #[test]
    fn compatibility_is_directional_and_declared_by_consumer() {
        let producer = parse_type_id("email/Email@1").expect("producer parse");
        let consumer = parse_type_id("email/Email@2").expect("consumer parse");
        let mut registry = TypeRegistry::default();
        registry.entries.insert(
            producer.canonical(),
            RegistryEntry {
                type_id: producer.clone(),
                schema: None,
                compatible_from: vec![],
            },
        );
        registry.entries.insert(
            consumer.canonical(),
            RegistryEntry {
                type_id: consumer.clone(),
                schema: None,
                compatible_from: vec![producer.clone()],
            },
        );

        assert_eq!(
            registry.compatibility(&producer, &consumer),
            CompatibilityResult::CompatibleDeclared
        );
        assert_eq!(
            registry.compatibility(&consumer, &producer),
            CompatibilityResult::IncompatibleVersion
        );
    }

    #[test]
    fn structured_encoding_mismatch_requires_adapter() {
        let components = ComponentLibrary::new(vec![
            component(
                "test/Producer",
                vec![],
                vec![port("OUT", "email/ParsedEmail@1", Some("rkyv"), Some("schema/parsed@1"))],
            ),
            component(
                "test/Consumer",
                vec![port("IN", "email/ParsedEmail@1", Some("json"), Some("schema/parsed@1"))],
                vec![],
            ),
        ]);
        let graph = graph_with_nodes_and_edge(
            "p",
            "test/Producer",
            "OUT",
            "c",
            "test/Consumer",
            "IN",
        );

        let report = validate_graph_contracts(&graph, &components, SchemaProfile::Minimal);
        assert!(
            report
                .errors
                .iter()
                .any(|issue| issue.code == "E_TYPE_ADAPTER_REQUIRED"),
            "expected adapter-required error for encoding mismatch"
        );
    }

    #[test]
    fn strict_profile_requires_schema_for_non_primitive_non_any_ports() {
        let components = ComponentLibrary::new(vec![
            component(
                "test/Producer",
                vec![],
                vec![port("OUT", "email/ParsedEmail@1", Some("json"), None)],
            ),
            component(
                "test/Consumer",
                vec![port("IN", "email/ParsedEmail@1", Some("json"), None)],
                vec![],
            ),
        ]);
        let graph = graph_with_nodes_and_edge(
            "p",
            "test/Producer",
            "OUT",
            "c",
            "test/Consumer",
            "IN",
        );

        let report = validate_graph_contracts(&graph, &components, SchemaProfile::Strict);
        assert!(
            report
                .errors
                .iter()
                .any(|issue| issue.code == "E_SCHEMA_REQUIRED_STRICT"),
            "strict profile should enforce schema presence"
        );
    }

    #[test]
    fn iip_untyped_is_warning_in_minimal_and_error_in_strict() {
        let components = ComponentLibrary::new(vec![component(
            "test/Consumer",
            vec![port("IN", "email/EmailRaw@1", Some("json"), Some("schema/email@1"))],
            vec![],
        )]);
        let mut graph = Graph::new("g".to_string(), "".to_string(), "".to_string());
        graph.nodes.insert(
            "c".to_string(),
            GraphNode {
                component: "test/Consumer".to_string(),
                metadata: GraphNodeMetadata::default(),
                protocol_metadata: JsonMap::new(),
            },
        );
        graph.edges.push(GraphEdge {
            source: GraphNodeSpec {
                process: String::new(),
                port: String::new(),
                index: None,
            },
            data: Some("just-a-string".to_string()),
            target: GraphNodeSpec {
                process: "c".to_string(),
                port: "IN".to_string(),
                index: None,
            },
            metadata: GraphEdgeMetadata::default(),
        });

        let minimal_report = validate_graph_contracts(&graph, &components, SchemaProfile::Minimal);
        assert!(
            minimal_report
                .warnings
                .iter()
                .any(|issue| issue.code == "W_IIP_UNTYPED_PAYLOAD"),
            "minimal profile should emit W_IIP_UNTYPED_PAYLOAD"
        );

        let strict_report = validate_graph_contracts(&graph, &components, SchemaProfile::Strict);
        assert!(
            strict_report
                .errors
                .iter()
                .any(|issue| issue.code == "E_IIP_TYPE_MISMATCH"),
            "strict profile should emit E_IIP_TYPE_MISMATCH"
        );
    }

    #[test]
    fn any_edge_emits_unsafe_warning() {
        let components = ComponentLibrary::new(vec![
            component(
                "test/Producer",
                vec![],
                vec![port("OUT", "core/Any@1", None, None)],
            ),
            component(
                "test/Consumer",
                vec![port("IN", "email/EmailRaw@1", Some("json"), Some("schema/email@1"))],
                vec![],
            ),
        ]);
        let graph = graph_with_nodes_and_edge(
            "p",
            "test/Producer",
            "OUT",
            "c",
            "test/Consumer",
            "IN",
        );

        let report = validate_graph_contracts(&graph, &components, SchemaProfile::Minimal);
        assert!(
            report
                .warnings
                .iter()
                .any(|issue| issue.code == "W_UNSAFE_ANY_EDGE"),
            "Any edges should be flagged as unsafe"
        );
    }

    #[test]
    fn correlation_bypass_is_warning_in_minimal_and_error_in_strict() {
        let (graph, components) = graph_with_join_inputs(
            "test/LeftProducer",
            "core/Any@1",
            "test/RightProducer",
            "email/EmailRaw@1",
            "test/JoinConsumer",
            "email/EmailRaw@1",
        );

        let minimal_report = validate_graph_contracts(&graph, &components, SchemaProfile::Minimal);
        assert!(
            minimal_report
                .warnings
                .iter()
                .any(|issue| issue.code == "W_UNSAFE_CORRELATION_BYPASS"),
            "minimal profile should warn about unsafe correlation bypass"
        );

        let strict_report = validate_graph_contracts(&graph, &components, SchemaProfile::Strict);
        assert!(
            strict_report
                .errors
                .iter()
                .any(|issue| issue.code == "E_CORRELATION_REQUIRED"),
            "strict profile should fail on unsafe correlation bypass"
        );
    }

    #[test]
    fn schema_profile_env_defaults_to_minimal_and_supports_strict() {
        let _guard = env_lock().lock().expect("env lock");

        std::env::remove_var("FLOWD_SCHEMA_PROFILE");
        assert_eq!(schema_profile_from_env(), SchemaProfile::Minimal);

        std::env::set_var("FLOWD_SCHEMA_PROFILE", "strict");
        assert_eq!(schema_profile_from_env(), SchemaProfile::Strict);

        std::env::set_var("FLOWD_SCHEMA_PROFILE", "STRICT");
        assert_eq!(schema_profile_from_env(), SchemaProfile::Strict);

        std::env::set_var("FLOWD_SCHEMA_PROFILE", "minimal");
        assert_eq!(schema_profile_from_env(), SchemaProfile::Minimal);

        std::env::remove_var("FLOWD_SCHEMA_PROFILE");
    }
}
