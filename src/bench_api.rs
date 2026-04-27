    use super::*;
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant};

    pub struct BenchRuntimeHarness {
        runtime: Arc<RwLock<Runtime>>,
        pub graph: Arc<RwLock<Graph>>,
        components: Arc<RwLock<ComponentLibrary>>,
        graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
        packet_rx: Receiver<RuntimePacketResponsePayload>,
        collected_outputs: Arc<Mutex<HashMap<String, Vec<MessageBuf>>>>,
    }

    impl BenchRuntimeHarness {
        fn message_data_bytes(payload: &MessageBuf) -> Option<&[u8]> {
            match payload {
                FbpMessage::Bytes(bytes) => Some(bytes),
                FbpMessage::Text(text) => Some(text.as_bytes()),
                _ => None,
            }
        }

        fn drain_runtime_packets(&self) {
            while let Ok(pkt) = self.packet_rx.try_recv() {
                if matches!(pkt.event, RuntimePacketEvent::Data) {
                    if let Some(payload) = &pkt.payload {
                        let mut outputs = self.collected_outputs.lock().expect("lock poisoned");
                        outputs
                            .entry(pkt.port.clone())
                            .or_insert_with(Vec::new)
                            .push(FbpMessage::from_text(payload.clone()));
                    }
                }
            }
        }

        pub fn new(graph: Graph) -> Self {
            let runtime: Arc<RwLock<Runtime>> =
                Arc::new(RwLock::new(Runtime::new(graph.properties.name.clone())));
            let components: Arc<RwLock<ComponentLibrary>> = build_component_library();
            let (packet_tx, packet_rx) = mpsc::sync_channel(65_536);
            let graph_inout = Arc::new(Mutex::new(GraphInportOutportHolder {
                inports: None,
                outports: None,
                websockets: HashMap::new(),
                packet_tap: Some(packet_tx),
            }));
            let graph = Arc::new(RwLock::new(graph));

            BenchRuntimeHarness {
                runtime,
                graph,
                components,
                graph_inout,
                packet_rx,
                collected_outputs: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        pub fn start(&self) -> std::result::Result<(), std::io::Error> {
            run_graph(
                self.runtime.clone(),
                self.graph.clone(),
                self.components.clone(),
                self.graph_inout.clone(),
            )
        }

        pub fn stop(&self) -> std::result::Result<(), std::io::Error> {
            self.runtime
                .write()
                .expect("lock poisoned")
                .stop(self.graph_inout.clone(), false)?;
            Ok(())
        }

        pub fn send_data_to_inport(
            &self,
            inport: &str,
            payload: &[u8],
        ) -> std::result::Result<(), std::io::Error> {
            let graph_name = self.runtime.read().expect("lock poisoned").graph.clone();
            let payload_text = std::str::from_utf8(payload).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("runtime packet payload must be valid UTF-8: {err}"),
                )
            })?;
            let packet = RuntimePacketRequestPayload {
                port: inport.to_string(),
                event: RuntimePacketEvent::Data,
                typ: None,
                schema: None,
                graph: graph_name,
                payload: Some(payload_text.to_string()),
                secret: None,
            };
            Runtime::packet(&packet, self.graph_inout.clone(), self.runtime.clone())?;
            Ok(())
        }

        pub fn wait_for_outport_data(
            &self,
            outport: &str,
            expected_count: usize,
            timeout: Duration,
        ) -> std::result::Result<(), std::io::Error> {
            let started = Instant::now();
            let mut count = 0usize;
            while count < expected_count {
                let elapsed = started.elapsed();
                if elapsed >= timeout {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("timed out waiting for outport data: {count}/{expected_count}"),
                    ));
                }
                let remaining = timeout - elapsed;
                let pkt = self.packet_rx.recv_timeout(remaining).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timed out waiting for runtime packet",
                    )
                })?;
                if pkt.port == outport && matches!(pkt.event, RuntimePacketEvent::Data) {
                    // Collect output for testing
                    if let Some(payload) = &pkt.payload {
                        let mut outputs = self.collected_outputs.lock().expect("lock poisoned");
                        outputs
                            .entry(outport.to_string())
                            .or_insert_with(Vec::new)
                            .push(FbpMessage::from_text(payload.clone()));
                    }
                    count += 1;
                }
            }
            Ok(())
        }

        /// Collect all outputs from a specific outport
        pub fn collect_outputs(&self, outport: &str) -> Vec<MessageBuf> {
            self.drain_runtime_packets();
            let outputs = self.collected_outputs.lock().expect("lock poisoned");
            outputs.get(outport).cloned().unwrap_or_default()
        }

        /// Assert that outputs match expected values (set-based comparison)
        pub fn assert_outputs_set_equal(&self, outport: &str, expected: &[&[u8]]) {
            let outputs = self.collect_outputs(outport);
            let actual_set: std::collections::HashSet<Vec<u8>> = outputs
                .iter()
                .map(|payload| {
                    Self::message_data_bytes(payload)
                        .expect("expected byte/text payload while comparing output set")
                        .to_vec()
                })
                .collect();
            let expected_set: std::collections::HashSet<Vec<u8>> =
                expected.iter().map(|payload| payload.to_vec()).collect();

            assert_eq!(
                actual_set, expected_set,
                "Output sets do not match for port {}",
                outport
            );
        }

        /// Assert that outputs match expected values in sequence
        pub fn assert_outputs_sequence_equal(&self, outport: &str, expected: &[&[u8]]) {
            let outputs = self.collect_outputs(outport);
            assert_eq!(
                outputs.len(),
                expected.len(),
                "Output count mismatch for port {}",
                outport
            );

            for (i, (actual, &expected)) in outputs.iter().zip(expected.iter()).enumerate() {
                let actual_bytes = Self::message_data_bytes(actual)
                    .expect("expected byte/text payload while comparing output sequence");
                assert_eq!(
                    actual_bytes, expected,
                    "Output {} does not match for port {}",
                    i, outport
                );
            }
        }

        /// Assert no message loss (all inputs should produce outputs)
        pub fn assert_no_message_loss(&self, input_count: usize, outport: &str) {
            let outputs = self.collect_outputs(outport);
            assert_eq!(
                outputs.len(),
                input_count,
                "Message loss detected: expected {} outputs, got {} for port {}",
                input_count,
                outputs.len(),
                outport
            );
        }

        /// Clear collected outputs
        pub fn clear_outputs(&self) {
            let mut outputs = self.collected_outputs.lock().expect("lock poisoned");
            outputs.clear();
        }
    }

    fn load_graph_from_persistence_path(
        path: &std::path::Path,
    ) -> std::result::Result<Graph, std::io::Error> {
        let json_data = std::fs::read_to_string(path)?;
        let graph: Graph = serde_json::from_str(&json_data).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("failed to parse graph from persistence file: {e}"),
            )
        })?;
        Ok(graph)
    }

    fn graph_to_persistence_json(graph: &Graph) -> std::result::Result<String, std::io::Error> {
        Ok(graph.get_source(graph.properties.name.clone())?.code)
    }

    fn default_node_metadata(x: i32, y: i32, label: &str) -> GraphNodeMetadata {
        GraphNodeMetadata {
            x,
            y,
            height: Some(NODE_HEIGHT_DEFAULT),
            width: Some(NODE_WIDTH_DEFAULT),
            label: Some(label.to_string()),
            icon: None,
        }
    }

    fn build_linear_graph(name: &str) -> Graph {
        let mut graph = Graph::new(
            name.to_string(),
            "benchmark".to_string(),
            "random".to_string(),
        );

        graph.inports.insert(
            "IN".to_string(),
            GraphPort {
                process: "RepeatA".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 0 },
            },
        );
        graph.outports.insert(
            "OUT".to_string(),
            GraphPort {
                process: "RepeatB".to_string(),
                port: "OUT".to_string(),
                metadata: GraphPortMetadata { x: 300, y: 0 },
            },
        );

        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "RepeatA".to_string(),
                default_node_metadata(80, 80, "RepeatA"),
            )
            .expect("failed to add RepeatA node");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "RepeatB".to_string(),
                default_node_metadata(180, 80, "RepeatB"),
            )
            .expect("failed to add RepeatB node");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "RepeatA".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "RepeatB".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add edge RepeatA.OUT -> RepeatB.IN");

        graph
    }

    fn build_fan_out_graph(name: &str) -> Graph {
        let mut graph = Graph::new(
            name.to_string(),
            "benchmark".to_string(),
            "random".to_string(),
        );

        graph.inports.insert(
            "IN".to_string(),
            GraphPort {
                process: "Demux".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 0 },
            },
        );
        graph.outports.insert(
            "OUT".to_string(),
            GraphPort {
                process: "Merge".to_string(),
                port: "OUT".to_string(),
                metadata: GraphPortMetadata { x: 600, y: 0 },
            },
        );

        graph
            .add_node(
                name.to_string(),
                "Demux3".to_string(),
                "Demux".to_string(),
                default_node_metadata(100, 80, "Demux"),
            )
            .expect("failed to add Demux");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "A".to_string(),
                default_node_metadata(240, 40, "A"),
            )
            .expect("failed to add A");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "B".to_string(),
                default_node_metadata(240, 80, "B"),
            )
            .expect("failed to add B");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "C".to_string(),
                default_node_metadata(240, 120, "C"),
            )
            .expect("failed to add C");
        graph
            .add_node(
                name.to_string(),
                "Muxer".to_string(),
                "Merge".to_string(),
                default_node_metadata(420, 80, "Merge"),
            )
            .expect("failed to add Merge");

        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Demux".to_string(),
                        port: "A".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "A".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Demux.A -> A.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Demux".to_string(),
                        port: "B".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "B".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Demux.B -> B.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Demux".to_string(),
                        port: "C".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "C".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Demux.C -> C.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "A".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Merge".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add A.OUT -> Merge.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "B".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Merge".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add B.OUT -> Merge.IN");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "C".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Merge".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add C.OUT -> Merge.IN");

        graph
    }

    fn build_fan_in_graph(name: &str) -> Graph {
        let mut graph = Graph::new(
            name.to_string(),
            "benchmark".to_string(),
            "random".to_string(),
        );

        graph.inports.insert(
            "IN_A".to_string(),
            GraphPort {
                process: "Merge".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 0 },
            },
        );
        graph.inports.insert(
            "IN_B".to_string(),
            GraphPort {
                process: "Merge".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 40 },
            },
        );
        graph.outports.insert(
            "OUT".to_string(),
            GraphPort {
                process: "Node".to_string(),
                port: "OUT".to_string(),
                metadata: GraphPortMetadata { x: 400, y: 0 },
            },
        );

        graph
            .add_node(
                name.to_string(),
                "Muxer".to_string(),
                "Merge".to_string(),
                default_node_metadata(120, 80, "Merge"),
            )
            .expect("failed to add Merge");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "Node".to_string(),
                default_node_metadata(260, 80, "Node"),
            )
            .expect("failed to add Node");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Merge".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Node".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Merge.OUT -> Node.IN");

        graph
    }

    fn build_io_sim_graph(name: &str) -> Graph {
        let mut graph = Graph::new(
            name.to_string(),
            "benchmark".to_string(),
            "random".to_string(),
        );

        graph.inports.insert(
            "IN".to_string(),
            GraphPort {
                process: "Delay".to_string(),
                port: "IN".to_string(),
                metadata: GraphPortMetadata { x: 0, y: 0 },
            },
        );
        graph.outports.insert(
            "OUT".to_string(),
            GraphPort {
                process: "Node".to_string(),
                port: "OUT".to_string(),
                metadata: GraphPortMetadata { x: 400, y: 0 },
            },
        );

        graph
            .add_node(
                name.to_string(),
                "Delay".to_string(),
                "Delay".to_string(),
                default_node_metadata(100, 80, "Delay"),
            )
            .expect("failed to add Delay");
        graph
            .add_node(
                name.to_string(),
                "Repeat".to_string(),
                "Node".to_string(),
                default_node_metadata(260, 80, "Node"),
            )
            .expect("failed to add Node");
        graph
            .add_edge(
                name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "Delay".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "Node".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add Delay.OUT -> Node.IN");
        graph
            .add_initialip(GraphAddinitialRequestPayload {
                graph: name.to_string(),
                metadata: GraphEdgeMetadata::new(None, None, None),
                src: GraphIIPSpecNetwork {
                    data: "?delay=50us".to_string(),
                },
                tgt: GraphNodeSpecNetwork {
                    node: "Delay".to_string(),
                    port: "CONF".to_string(),
                    index: None,
                },
                secret: None,
            })
            .expect("failed to add IIP to Delay.CONF");

        graph
    }

    pub fn linear_harness_direct(name: &str) -> BenchRuntimeHarness {
        BenchRuntimeHarness::new(build_linear_graph(name))
    }

    pub fn linear_harness_from_persistence(
        name: &str,
    ) -> std::result::Result<BenchRuntimeHarness, std::io::Error> {
        let graph = build_linear_graph(name);
        let graph_json = graph_to_persistence_json(&graph)?;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "flowd-bench-graph-load-{}-{}.json",
            std::process::id(),
            name
        ));
        std::fs::write(&path, graph_json)?;
        let loaded = load_graph_from_persistence_path(&path)?;
        let _ = std::fs::remove_file(&path);
        Ok(BenchRuntimeHarness::new(loaded))
    }

    pub fn fan_out_harness_direct(name: &str) -> BenchRuntimeHarness {
        BenchRuntimeHarness::new(build_fan_out_graph(name))
    }

    pub fn fan_out_harness_from_persistence(
        name: &str,
    ) -> std::result::Result<BenchRuntimeHarness, std::io::Error> {
        let graph = build_fan_out_graph(name);
        let graph_json = graph_to_persistence_json(&graph)?;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "flowd-bench-graph-load-{}-{}.json",
            std::process::id(),
            name
        ));
        std::fs::write(&path, graph_json)?;
        let loaded = load_graph_from_persistence_path(&path)?;
        let _ = std::fs::remove_file(&path);
        Ok(BenchRuntimeHarness::new(loaded))
    }

    pub fn fan_in_harness_direct(name: &str) -> BenchRuntimeHarness {
        BenchRuntimeHarness::new(build_fan_in_graph(name))
    }

    pub fn fan_in_harness_from_persistence(
        name: &str,
    ) -> std::result::Result<BenchRuntimeHarness, std::io::Error> {
        let graph = build_fan_in_graph(name);
        let graph_json = graph_to_persistence_json(&graph)?;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "flowd-bench-graph-load-{}-{}.json",
            std::process::id(),
            name
        ));
        std::fs::write(&path, graph_json)?;
        let loaded = load_graph_from_persistence_path(&path)?;
        let _ = std::fs::remove_file(&path);
        Ok(BenchRuntimeHarness::new(loaded))
    }

    pub fn io_sim_harness_direct(name: &str) -> BenchRuntimeHarness {
        BenchRuntimeHarness::new(build_io_sim_graph(name))
    }

    pub fn io_sim_harness_from_persistence(
        name: &str,
    ) -> std::result::Result<BenchRuntimeHarness, std::io::Error> {
        let graph = build_io_sim_graph(name);
        let graph_json = graph_to_persistence_json(&graph)?;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "flowd-bench-graph-load-{}-{}.json",
            std::process::id(),
            name
        ));
        std::fs::write(&path, graph_json)?;
        let loaded = load_graph_from_persistence_path(&path)?;
        let _ = std::fs::remove_file(&path);
        Ok(BenchRuntimeHarness::new(loaded))
    }
