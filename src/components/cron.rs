use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific
use cron::{Schedule, OwnedScheduleIterator};
//use chrono::prelude::*;   //TODO is this necessary?
use chrono::Local;
use std::str::FromStr;

pub struct CronComponent {
    when: ProcessEdgeSource,
    tick: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for CronComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        CronComponent {
            when: inports.remove("WHEN").expect("found no WHEN inport"),
            tick: outports.remove("TICK").expect("found no TICK outport"),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout,
        }
    }

    fn run(mut self) {
        debug!("Count is now run()ning!");
        let when = &mut self.when;
        let tick = &mut self.tick.sink;
        let tick_wakeup = self.tick.wakeup.expect("got no wakeup notify handle for outport TICK");
        let mut schedule: Option<OwnedScheduleIterator<Local>> = None;  //TODO is there a better way instead of Option?

        // check config port
        trace!("read config IP");
        //TODO wait for a while? config IP could come from a file or other previous component and therefore take a bit
        if let Ok(ip) = when.pop() {
            //TODO the cron crate has a non-standard 7-parameter form ranging down to seconds and up to years, is that good? cron-parser has POSIX 5-parameter format
            schedule = Some(Schedule::from_str(std::str::from_utf8(&ip).expect("invalid utf-8")).unwrap().upcoming_owned(Local)); //TODO error handling
        } else {
            error!("no config IP received - exiting");
            //TODO send shutdown signal upstream
            return;
        }

        'outer: loop {
            trace!("begin of outer iteration");

            // calculate next fire
            let next = schedule.as_mut().unwrap().next().unwrap();
            info!("Next fire time: {}", next);

            // sleep again in case we get woken up by watchdog
            //TODO might condition never complete when system time changes into the past?
            while Local::now() < next {
                trace!("begin of inner iteration");
                // check signals
                if let Ok(ip) = self.signals_in.try_recv() {
                    trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                    // stop signal
                    if ip == b"stop" {   //TODO optimize comparison
                        info!("got stop signal, exiting");
                        break 'outer;
                    } else if ip == b"ping" {
                        trace!("got ping signal, responding");
                        self.signals_out.send(b"pong".to_vec()).expect("could not send pong");
                    } else {
                        warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                    }
                }

                let dur_to_next = next - Local::now();

                if let Ok(dur_to_next_std) = dur_to_next.to_std() {
                    // cannot get woken by the condvar
                    std::thread::sleep(dur_to_next_std);
                    // can get woken by timeout or watchdog
                    //condvar_block_timeout!(&*self.wakeup_notify, dur_to_next_std);    //TODO error handling
                } else {
                    // out of range = negative -> we are after next already
                    //TODO optimize this actually happens sometimes
                    break;
                }
            }
            trace!("-- end of inner iteration");

            // send notification downstream
            debug!("sending tick");
            //TODO really send an empty IP or just wake the downstream component? how could the receiver differentiate?
            tick.push(vec![]).unwrap();
            //condvar_notify!(tick_wakeup);
            tick_wakeup.unpark();

            trace!("-- end of outer iteration");
            // we are not blocking on input, but on time
            //###thread::park();
            //condvar_block!(self.wakeup_notify);
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("Cron"),
            description: String::from("Sends an empty packet every time the cron schedule fires"),
            icon: String::from("clock-o"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("WHEN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IP with cron schedule expression"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("TICK"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("tick IP every time the cron schedule fires"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}