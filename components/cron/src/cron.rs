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

pub struct CronComponent {
    when: ProcessEdgeSource,
    tick: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    schedule: Option<OwnedScheduleIterator<Local>>,
    next_fire_time: Option<chrono::DateTime<Local>>,
    //graph_inout: GraphInportOutportHandle,
}

impl Component for CronComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
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
            //graph_inout,
        }
    }

    fn process(&mut self, _context: &mut NodeContext) -> ProcessResult {
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

        // Check if it's time to send a tick
        if let Some(next) = self.next_fire_time {
            if Local::now() >= next {
                // Time to send tick
                info!("Cron firing at: {}", next);
                debug!("sending tick");

                if let Ok(()) = self.tick.push(vec![]) {
                    // Get next schedule time
                    if let Some(schedule) = self.schedule.as_mut() {
                        if let Some(next_time) = schedule.next() {
                            self.next_fire_time = Some(next_time);
                            info!("Next fire time: {}", next_time);
                        } else {
                            self.next_fire_time = None;
                        }
                    }
                    ProcessResult::DidWork(1)
                } else {
                    // Output buffer full, try again later
                    ProcessResult::NoWork
                }
            } else {
                // Not time yet
                trace!("Next cron fire: {}", next);
                ProcessResult::NoWork
            }
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
