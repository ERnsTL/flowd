    use super::bench_api::linear_harness_direct;
    use std::any::Any;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

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
