use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};

// component-specific
use cron::{OwnedScheduleIterator, Schedule};
//use chrono::prelude::*;   //TODO is this necessary?
use chrono::Local;
use std::str::FromStr;
use std::time::Instant;

pub struct CronComponent {
    when: ProcessEdgeSource,
    tick: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    schedule: Option<OwnedScheduleIterator<Local>>,
    next_fire_time: Option<chrono::DateTime<Local>>,
    timer_armed: bool,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for CronComponent {
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
        CronComponent {
            when: inports
                .remove("WHEN")
                .expect("found no WHEN inport")
                .pop()
                .unwrap(),
            tick: outports
                .remove("TICK")
                .expect("found no TICK outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            schedule: None,
            next_fire_time: None,
            timer_armed: false,
            //graph_inout,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("Cron process() called");

        // Check signals first
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            if ip == b"stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if ip == b"ping" {
                trace!("got ping signal, responding");
                let _ = self.signals_out.try_send(b"pong".to_vec());
            } else {
                warn!(
                    "received unknown signal ip: {}",
                    std::str::from_utf8(&ip).expect("invalid utf-8")
                )
            }
        }

        // Check if we have configuration
        if self.schedule.is_none() {
            if let Ok(ip) = self.when.pop() {
                let cron_str = std::str::from_utf8(&ip).expect("invalid utf-8 in config IP");
                debug!("received cron schedule: {}", cron_str);
                match Schedule::from_str(cron_str) {
                    Ok(schedule) => {
                        let mut iterator = schedule.upcoming_owned(Local);
                        if let Some(next_time) = iterator.next() {
                            self.schedule = Some(iterator);
                            self.next_fire_time = Some(next_time);
                            self.arm_next_timer(context);
                            info!("Next fire time: {}", next_time);
                            return ProcessResult::DidWork(1); // Configuration processed
                        } else {
                            info!("Cron schedule has no future times, finishing");
                            return ProcessResult::Finished;
                        }
                    }
                    Err(err) => {
                        error!("failed to parse cron schedule: {}", err);
                        return ProcessResult::Finished; // Invalid config, finish
                    }
                }
            } else {
                // No config yet
                return ProcessResult::NoWork;
            }
        }

        // Fire only when scheduler explicitly woke us due to timer expiration.
        if self.timer_armed && context.take_timer_fired().is_some() {
            let Some(next) = self.next_fire_time else {
                info!("Cron schedule exhausted, finishing");
                return ProcessResult::Finished;
            };
            info!("Cron firing at: {}", next);
            debug!("sending tick");

            if let Ok(()) = self.tick.push(vec![]) {
                self.timer_armed = false;
                // Get next schedule time and arm next timer.
                if let Some(schedule) = self.schedule.as_mut() {
                    if let Some(next_time) = schedule.next() {
                        self.next_fire_time = Some(next_time);
                        info!("Next fire time: {}", next_time);
                        self.arm_next_timer(context);
                    } else {
                        self.next_fire_time = None;
                    }
                }
                ProcessResult::DidWork(1)
            } else {
                // Output buffer full, retry on bounded scheduler polling.
                context.wake_at(Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL);
                ProcessResult::NoWork
            }
        } else if self.next_fire_time.is_some() {
            ProcessResult::NoWork
        } else {
            // Schedule exhausted
            info!("Cron schedule exhausted, finishing");
            ProcessResult::Finished
        }
    }

    fn get_metadata() -> ComponentComponentPayload
    where
        Self: Sized,
    {
        ComponentComponentPayload {
            name: String::from("Cron"),
            description: String::from("Sends an empty packet every time the cron schedule fires"),
            icon: String::from("clock-o"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("WHEN"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("IP with cron schedule expression"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("TICK"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("tick IP every time the cron schedule fires"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}

impl CronComponent {
    fn arm_next_timer(&mut self, context: &mut NodeContext) {
        if let Some(next) = self.next_fire_time {
            let now = Local::now();
            if next > now {
                if let Ok(dur) = (next - now).to_std() {
                    context.wake_at(Instant::now() + dur);
                    self.timer_armed = true;
                }
            } else {
                // If schedule time is already due, trigger near-immediate scheduler wake.
                context.wake_at(Instant::now());
                self.timer_armed = true;
            }
        }
    }
}
