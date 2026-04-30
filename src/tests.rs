    use super::bench_api::{linear_harness_direct, BenchRuntimeHarness};
    use super::{
        Graph, GraphAddinitialRequestPayload, GraphEdge, GraphEdgeMetadata, GraphIIPSpecNetwork,
        GraphNodeMetadata, GraphNodeSpec, GraphNodeSpecNetwork, GraphPort, GraphPortMetadata,
    };
    use std::any::Any;
    use std::fs;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn bench_harness_stop_does_not_deadlock_under_repetition() {
        let (done_tx, done_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                for _ in 0..8 {
                    let harness = linear_harness_direct("bench_deadlock_regression");
                    harness
                        .start()
                        .expect("runtime start failed in regression loop");
                    harness
                        .send_data_to_inport("IN", b"x")
                        .expect("runtime packet send failed in regression loop");
                    harness
                        .wait_for_outport_data("OUT", 1, Duration::from_secs(2))
                        .expect("did not observe expected output packet in regression loop");
                    harness
                        .stop()
                        .expect("runtime stop failed in regression loop");
                }
            });

            let send_result = match result {
                Ok(()) => done_tx.send(Ok(())),
                Err(panic_payload) => {
                    let msg = if let Some(s) = (&*panic_payload as &dyn Any).downcast_ref::<&str>()
                    {
                        (*s).to_string()
                    } else if let Some(s) = (&*panic_payload as &dyn Any).downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic payload".to_string()
                    };
                    done_tx.send(Err(msg))
                }
            };
            send_result.expect("failed to signal regression completion");
        });

        match done_rx.recv_timeout(Duration::from_secs(20)) {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => panic!("regression worker panicked: {msg}"),
            Err(_) => {
                panic!("timeout waiting for regression loop completion (possible stop deadlock)")
            }
        }
    }

    #[test]
    fn filereader_output_drop_auto_shutdowns_after_draining() {
        let graph_name = "filereader_output_drop_auto_shutdown";
        let mut graph = Graph::new(
            graph_name.to_string(),
            "FileReader -> Output -> Drop shutdown regression".to_string(),
            "test".to_string(),
        );

        graph
            .add_node(
                graph_name.to_string(),
                "FileReader".to_string(),
                "file".to_string(),
                GraphNodeMetadata {
                    x: 80,
                    y: 120,
                    width: Some(72),
                    height: Some(72),
                    label: Some("file".to_string()),
                    icon: None,
                },
            )
            .expect("failed to add FileReader node");
        graph
            .add_node(
                graph_name.to_string(),
                "Output".to_string(),
                "out".to_string(),
                GraphNodeMetadata {
                    x: 220,
                    y: 120,
                    width: Some(72),
                    height: Some(72),
                    label: Some("out".to_string()),
                    icon: None,
                },
            )
            .expect("failed to add Output node");
        graph
            .add_node(
                graph_name.to_string(),
                "Drop".to_string(),
                "drop".to_string(),
                GraphNodeMetadata {
                    x: 360,
                    y: 120,
                    width: Some(72),
                    height: Some(72),
                    label: Some("drop".to_string()),
                    icon: None,
                },
            )
            .expect("failed to add Drop node");

        graph
            .add_edge(
                graph_name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "file".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "out".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add edge file.OUT -> out.IN");
        graph
            .add_edge(
                graph_name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "out".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "drop".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add edge out.OUT -> drop.IN");

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let mut file_path = std::env::temp_dir();
        file_path.push(format!(
            "flowd-filereader-test-{}-{}.txt",
            std::process::id(),
            unique
        ));
        fs::write(&file_path, "line 1\nline 2\nline 3\n")
            .expect("failed to write FileReader test input file");

        graph
            .add_initialip(GraphAddinitialRequestPayload {
                graph: graph_name.to_string(),
                metadata: GraphEdgeMetadata::new(None, None, None),
                src: GraphIIPSpecNetwork {
                    data: file_path
                        .to_str()
                        .expect("test file path must be valid UTF-8")
                        .to_string(),
                },
                tgt: GraphNodeSpecNetwork {
                    node: "file".to_string(),
                    port: "NAMES".to_string(),
                    index: None,
                },
                secret: None,
            })
            .expect("failed to add IIP for FileReader.NAMES");

        let harness = BenchRuntimeHarness::new(graph);
        harness.start().expect("runtime failed to start");

        harness
            .wait_for_node_work_units_at_least("out", 1, Duration::from_secs(5))
            .expect("Output did not process expected work");
        harness
            .wait_for_node_work_units_at_least("drop", 1, Duration::from_secs(5))
            .expect("Drop did not receive/process expected packet");

        harness
            .wait_for_network_stopped(Duration::from_secs(20))
            .expect("network did not auto-shutdown after pipeline drained");
        assert!(
            !harness.is_running(),
            "runtime still reports network as running after auto-shutdown"
        );

        fs::remove_file(&file_path).expect("failed to remove FileReader test input file");
    }

    #[test]
    fn iip_and_regular_edge_can_share_same_non_array_inport() {
        let graph_name = "iip_and_regular_edge_same_inport";
        let mut graph = Graph::new(
            graph_name.to_string(),
            "IIP + incoming edge on same inport".to_string(),
            "test".to_string(),
        );

        graph.outports.insert(
            "OUT".to_string(),
            GraphPort {
                process: "target".to_string(),
                port: "OUT".to_string(),
                metadata: GraphPortMetadata { x: 200, y: 0 },
            },
        );

        graph
            .add_node(
                graph_name.to_string(),
                "Repeat".to_string(),
                "source".to_string(),
                GraphNodeMetadata {
                    x: 40,
                    y: 80,
                    width: Some(72),
                    height: Some(72),
                    label: Some("source".to_string()),
                    icon: None,
                },
            )
            .expect("failed to add source Repeat node");

        graph
            .add_node(
                graph_name.to_string(),
                "Repeat".to_string(),
                "target".to_string(),
                GraphNodeMetadata {
                    x: 180,
                    y: 80,
                    width: Some(72),
                    height: Some(72),
                    label: Some("target".to_string()),
                    icon: None,
                },
            )
            .expect("failed to add target Repeat node");

        graph
            .add_edge(
                graph_name.to_string(),
                GraphEdge {
                    source: GraphNodeSpec {
                        process: "source".to_string(),
                        port: "OUT".to_string(),
                        index: None,
                    },
                    data: None,
                    target: GraphNodeSpec {
                        process: "target".to_string(),
                        port: "IN".to_string(),
                        index: None,
                    },
                    metadata: GraphEdgeMetadata::new(None, None, None),
                },
            )
            .expect("failed to add edge source.OUT -> target.IN");

        graph
            .add_initialip(GraphAddinitialRequestPayload {
                graph: graph_name.to_string(),
                src: GraphIIPSpecNetwork {
                    data: "iip-message".to_string(),
                },
                tgt: GraphNodeSpecNetwork {
                    node: "target".to_string(),
                    port: "IN".to_string(),
                    index: None,
                },
                metadata: GraphEdgeMetadata::new(None, None, None),
                secret: None,
            })
            .expect("failed to add IIP to target.IN");

        graph
            .add_initialip(GraphAddinitialRequestPayload {
                graph: graph_name.to_string(),
                src: GraphIIPSpecNetwork {
                    data: "edge-message".to_string(),
                },
                tgt: GraphNodeSpecNetwork {
                    node: "source".to_string(),
                    port: "IN".to_string(),
                    index: None,
                },
                metadata: GraphEdgeMetadata::new(None, None, None),
                secret: None,
            })
            .expect("failed to add IIP to source.IN");

        let harness = BenchRuntimeHarness::new(graph);
        harness.start().expect("runtime failed to start");

        harness
            .wait_for_outport_data("OUT", 2, Duration::from_secs(3))
            .expect("did not receive both packets on OUT");

        harness.assert_outputs_set_equal(
            "OUT",
            &[b"iip-message".as_ref(), b"edge-message".as_ref()],
        );

        harness.stop().expect("runtime stop failed");
    }
