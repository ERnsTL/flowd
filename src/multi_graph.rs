use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GraphMetadata {
    name: String,                  // human-readable name
    library: String,               // component library identifier
    main: bool,                    // whether this is a main graph (not a subgraph component)
    icon: Option<String>,          // icon for graph-as-component
    description: Option<String>,   // description for graph-as-component
    created_at: crate::UtcTime,    // creation timestamp
    last_modified: crate::UtcTime, // last modification timestamp
}

#[derive(Clone, Debug)]
pub struct MultiGraphManager {
    graphs: HashMap<String, Arc<RwLock<Graph>>>,
    active_graph: String,
}

impl MultiGraphManager {
    pub fn new() -> Self {
        MultiGraphManager {
            graphs: HashMap::new(),
            active_graph: String::from("main_graph"),
        }
    }

    pub fn get_graph(&self, graph_id: &str) -> Option<Arc<RwLock<Graph>>> {
        self.graphs.get(graph_id).cloned()
    }

    pub fn get_active_graph(&self) -> Option<Arc<RwLock<Graph>>> {
        self.get_graph(&self.active_graph)
    }

    pub fn set_active_graph(&mut self, graph_id: &str) -> Result<(), std::io::Error> {
        if self.graphs.contains_key(graph_id) {
            self.active_graph = graph_id.to_string();
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Graph '{}' not found", graph_id),
            ))
        }
    }

    pub fn add_graph(&mut self, graph_id: String, graph: Arc<RwLock<Graph>>) {
        self.graphs.insert(graph_id, graph);
    }

    pub fn remove_graph(&mut self, graph_id: &str) -> Option<Arc<RwLock<Graph>>> {
        self.graphs.remove(graph_id)
    }

    pub fn list_graphs(&self) -> Vec<String> {
        self.graphs.keys().cloned().collect()
    }

    pub fn get_active_graph_id(&self) -> &str {
        &self.active_graph
    }
}

// Re-export Graph from lib.rs for convenience
pub use crate::Graph;
