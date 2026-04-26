use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

// component-specific
use brotli::{CompressorWriter, DecompressorWriter};
use std::io::prelude::*;

pub struct BrotliCompressComponent {
    //conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    pending_packets: std::collections::VecDeque<Vec<u8>>,
}

const BROTLI_BUFFER_SIZE: usize = 4096;
const BROTLI_QUALITY: u32 = 9; // 0 to 11 for Rust implementation, C implementation has 0 to 9
const BROTLI_LG_WINDOW_SIZE: u32 = 22; // 20 to 22

impl Component for BrotliCompressComponent {
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
        BrotliCompressComponent {
            //conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
            pending_packets: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("BrotliCompress is now process()ing!");
        let mut work_units = 0u32;

        // check signals
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            // stop signal
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

        // First, try to send any pending packets that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_packets.is_empty() {
            if let Some(pending_ip) = self.pending_packets.front() {
                match self.out.push(pending_ip.clone()) {
                    Ok(()) => {
                        self.pending_packets.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        trace!("sent pending packet");
                    }
                    Err(PushError::Full(_)) => {
                        // Still can't send, stop trying for now
                        break;
                    }
                }
            }
        }

        // Then, check in port within remaining budget
        while context.remaining_budget > 0 {
            // stay responsive to stop/ping even while draining a busy input buffer
            if let Ok(sig) = self.signals_in.try_recv() {
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&sig).expect("invalid utf-8")
                );
                if sig == b"stop" {
                    info!("got stop signal while processing, finishing");
                    return ProcessResult::Finished;
                } else if sig == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                }
            }

            if let Ok(ip) = self.inn.pop() {
                // prepare packet
                debug!("got a packet, compressing...");
                // nothing to do

                // compress
                //TODO optimize - currently, every packet is processed with a new Encoder instance
                //TODO support for chunks via open bracket, closing bracket
                let mut vec_out = Vec::new();
                let mut writer = CompressorWriter::new(
                    &mut vec_out,
                    BROTLI_BUFFER_SIZE,
                    BROTLI_QUALITY,
                    BROTLI_LG_WINDOW_SIZE,
                );
                writer.write(&ip).expect("failed to write into compressor");
                // TODO optimize - is the flush necessary or does it do that automatically on drop?
                writer
                    .flush()
                    .expect("failed to flush compressor into output buffer");
                drop(writer); // so that the vec_out becomes borrow-free

                // Try to send the compressed packet
                debug!("sending...");
                match self.out.push(vec_out.clone()) {
                    Ok(()) => {
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("done");
                    }
                    Err(PushError::Full(returned_ip)) => {
                        // Output buffer full, buffer internally for later retry
                        debug!("output buffer full, buffering packet internally");
                        self.pending_packets.push_back(returned_ip);
                        work_units += 1; // We did process the packet, just couldn't forward
                        context.remaining_budget -= 1;
                        // Continue processing more packets that might fit
                    }
                }
            } else {
                break;
            }
        }

        // are we done?
        if self.inn.is_abandoned() {
            info!("EOF on inport, finishing");
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
            name: String::from("BrotliCompress"),
            description: String::from("Reads data IPs, compresses each using Brotli and sends the compressed data to the OUT port."),
            icon: String::from("compress"),
            subgraph: false,
            in_ports: vec![
                /*
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("TODO"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                */
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("IPs to compress"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("compressed IPs"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

pub struct BrotliDecompressComponent {
    //conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    pending_packets: std::collections::VecDeque<Vec<u8>>,
}

impl Component for BrotliDecompressComponent {
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
        BrotliDecompressComponent {
            //conf: inports.remove("CONF").expect("found no CONF inport").pop().unwrap(),
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
            pending_packets: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("BrotliDecompress is now process()ing!");
        let mut work_units = 0u32;

        // check signals
        if let Ok(ip) = self.signals_in.try_recv() {
            trace!(
                "received signal ip: {}",
                std::str::from_utf8(&ip).expect("invalid utf-8")
            );
            // stop signal
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

        // First, try to send any pending packets that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_packets.is_empty() {
            if let Some(pending_ip) = self.pending_packets.front() {
                match self.out.push(pending_ip.clone()) {
                    Ok(()) => {
                        self.pending_packets.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        trace!("sent pending packet");
                    }
                    Err(PushError::Full(_)) => {
                        // Still can't send, stop trying for now
                        break;
                    }
                }
            }
        }

        // Then, check in port within remaining budget
        while context.remaining_budget > 0 {
            // stay responsive to stop/ping even while draining a busy input buffer
            if let Ok(sig) = self.signals_in.try_recv() {
                trace!(
                    "received signal ip: {}",
                    std::str::from_utf8(&sig).expect("invalid utf-8")
                );
                if sig == b"stop" {
                    info!("got stop signal while processing, finishing");
                    return ProcessResult::Finished;
                } else if sig == b"ping" {
                    trace!("got ping signal, responding");
                    let _ = self.signals_out.try_send(b"pong".to_vec());
                }
            }

            if let Ok(ip) = self.inn.pop() {
                // prepare packet
                debug!("got a packet, decompressing...");
                // nothing to do

                // decompress
                //TODO optimize - currently, every packet is processed with a new Decoder instance
                //TODO support for chunks via open bracket, closing bracket
                let mut vec_out = Vec::new();
                let mut writer = DecompressorWriter::new(&mut vec_out, BROTLI_BUFFER_SIZE);
                writer
                    .write(&ip)
                    .expect("failed to write into decompressor");
                //TODO is that flush necessary or will it do that automatically during drop?
                writer
                    .flush()
                    .expect("failed to flush decompressor into output buffer");
                drop(writer); // so that vec_out output buffer becomes un-borrowed and can be sent

                // Try to send the decompressed packet
                debug!("sending...");
                match self.out.push(vec_out.clone()) {
                    Ok(()) => {
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("done");
                    }
                    Err(PushError::Full(returned_ip)) => {
                        // Output buffer full, buffer internally for later retry
                        debug!("output buffer full, buffering packet internally");
                        self.pending_packets.push_back(returned_ip);
                        work_units += 1; // We did process the packet, just couldn't forward
                        context.remaining_budget -= 1;
                        // Continue processing more packets that might fit
                    }
                }
            } else {
                break;
            }
        }

        // are we done?
        if self.inn.is_abandoned() {
            info!("EOF on inport, finishing");
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
            name: String::from("BrotliDecompress"),
            description: String::from("Reads IPs, applies Brotli decompression and forwards the decompressed data to OUT port."),
            icon: String::from("expand"),
            subgraph: false,
            in_ports: vec![
                /*
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("TODO"),
                    values_allowed: vec![],
                    value_default: String::from("")
                },
                */
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("compressed Brotli data"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("decompressed data"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}
