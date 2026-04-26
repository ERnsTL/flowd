use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

// component-specific
use std::io::prelude::*;
use xz::write::{XzDecoder, XzEncoder};

pub struct XzCompressComponent {
    //conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    pending_packets: std::collections::VecDeque<Vec<u8>>, // packets to send, buffered for backpressure
}

const COMPRESSION_LEVEL: u32 = 9; //TODO maybe make configurable

impl Component for XzCompressComponent {
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
        XzCompressComponent {
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
        debug!("XzCompress is now process()ing!");
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
            if let Some(pending_packet) = self.pending_packets.front() {
                match self.out.push(pending_packet.clone()) {
                    Ok(()) => {
                        self.pending_packets.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("sent pending compressed packet");
                    }
                    Err(PushError::Full(_)) => {
                        // Still can't send, stop trying for now
                        break;
                    }
                }
            }
        }

        // Then, process new packets within remaining budget
        while context.remaining_budget > 0 {
            // stay responsive to stop/ping even while processing packets
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
                let vec_buf = Vec::new();
                let mut compressor = XzEncoder::new(vec_buf, COMPRESSION_LEVEL);
                compressor.write(&ip).expect("failed to write into encoder");
                //TODO this does not take into account the final bytes written by finish(), but when requesting the totals after the call to finish(), there is a borrow checker error
                debug!(
                    "compression: {} bytes in, {} bytes out",
                    compressor.total_in(),
                    compressor.total_out()
                );
                let vec_out = compressor.finish().expect("failed to finish encoding");

                // Try to send it
                debug!("sending...");
                match self.out.push(vec_out) {
                    Ok(()) => {
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("done");
                    }
                    Err(PushError::Full(returned_packet)) => {
                        // Output buffer full, buffer internally for later retry
                        debug!("output buffer full, buffering compressed packet internally");
                        self.pending_packets.push_back(returned_packet);
                        work_units += 1; // We processed the packet, just couldn't send
                        context.remaining_budget -= 1;
                    }
                }
            } else {
                break;
            }
        }

        // are we done?
        if self.inn.is_abandoned() && self.pending_packets.is_empty() {
            info!("EOF on inport and all packets processed, finishing");
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
            name: String::from("XzCompress"),
            description: String::from("Reads data IPs, compresses each using XZ (LZMA2) and sends the compressed data to the OUT port."),
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

pub struct XzDecompressComponent {
    //conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    pending_packets: std::collections::VecDeque<Vec<u8>>, // packets to send, buffered for backpressure
}

impl Component for XzDecompressComponent {
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
        XzDecompressComponent {
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
        debug!("XzDecompress is now process()ing!");
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
            if let Some(pending_packet) = self.pending_packets.front() {
                match self.out.push(pending_packet.clone()) {
                    Ok(()) => {
                        self.pending_packets.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("sent pending decompressed packet");
                    }
                    Err(PushError::Full(_)) => {
                        // Still can't send, stop trying for now
                        break;
                    }
                }
            }
        }

        // Then, process new packets within remaining budget
        while context.remaining_budget > 0 {
            // stay responsive to stop/ping even while processing packets
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
                let vec_buf = Vec::new();
                let mut decompressor = XzDecoder::new(vec_buf);
                decompressor
                    .write(&ip)
                    .expect("failed to write into decoder");
                debug!(
                    "decompression: {} bytes in, {} bytes out",
                    decompressor.total_in(),
                    decompressor.total_out()
                );
                let vec_out = decompressor.finish().expect("failed to finish decoding");

                // Try to send it
                debug!("sending...");
                match self.out.push(vec_out) {
                    Ok(()) => {
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("done");
                    }
                    Err(PushError::Full(returned_packet)) => {
                        // Output buffer full, buffer internally for later retry
                        debug!("output buffer full, buffering decompressed packet internally");
                        self.pending_packets.push_back(returned_packet);
                        work_units += 1; // We processed the packet, just couldn't send
                        context.remaining_budget -= 1;
                    }
                }
            } else {
                break;
            }
        }

        // are we done?
        if self.inn.is_abandoned() && self.pending_packets.is_empty() {
            info!("EOF on inport and all packets processed, finishing");
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
            name: String::from("XzDecompress"),
            description: String::from("Reads IPs, applies XZ (LZMA2) decompression and forwards the decompressed data to OUT port."),
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
                    description: String::from("compressed XZ data"),
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
