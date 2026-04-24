use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError,
};
use log::{debug, info, trace, warn};

// component-specific for FileTailer
use staart::TailedFile;
use std::time::Duration;
// component-specific for FileWriter
use std::fs::File;
use std::io::prelude::*;
use std::io::BufWriter;

pub struct FileReaderComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    filenames: std::collections::VecDeque<Vec<u8>>,
    pending_files: std::collections::VecDeque<Vec<u8>>, // files to send, buffered for backpressure
}

impl Component for FileReaderComponent {
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
        FileReaderComponent {
            conf: inports
                .remove("NAMES")
                .expect("found no NAMES inport")
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
            filenames: std::collections::VecDeque::new(),
            pending_files: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("FileReader is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration filenames if we haven't buffered them all yet
        while self.filenames.is_empty() && !self.conf.is_abandoned() {
            if let Ok(filename) = self.conf.pop() {
                trace!("got filename: {}", std::str::from_utf8(&filename).expect("invalid utf-8"));
                self.filenames.push_back(filename);
            } else {
                break;
            }
        }

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

        // First, try to send any pending files that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_files.is_empty() {
            if let Some(pending_content) = self.pending_files.front() {
                match self.out.push(pending_content.clone()) {
                    Ok(()) => {
                        self.pending_files.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("sent pending file content");
                    }
                    Err(PushError::Full(_)) => {
                        // Still can't send, stop trying for now
                        break;
                    }
                }
            }
        }

        // Then, process new filenames within remaining budget
        while context.remaining_budget > 0 && !self.filenames.is_empty() {
            // stay responsive to stop/ping even while processing files
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

            if let Some(filename) = self.filenames.front() {
                let file_path = std::str::from_utf8(filename).expect("non utf-8 data");
                debug!("processing filename: {}", &file_path);

                // read whole file
                //TODO may be big file - add chunking
                //TODO enclose files in brackets to know where its stream of chunks start and end
                debug!("reading file...");
                match std::fs::read(file_path) {
                    Ok(contents) => {
                        // Try to send it
                        debug!("forwarding file contents...");
                        match self.out.push(contents) {
                            Ok(()) => {
                                self.filenames.pop_front();
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("done");
                            }
                            Err(PushError::Full(returned_content)) => {
                                // Output buffer full, buffer internally for later retry
                                debug!("output buffer full, buffering file content internally");
                                self.pending_files.push_back(returned_content);
                                self.filenames.pop_front(); // We processed the file, just couldn't send
                                work_units += 1;
                                context.remaining_budget -= 1;
                                // Continue processing more files that might fit
                            }
                        }
                    }
                    Err(e) => {
                        warn!("failed to read file {}: {}", file_path, e);
                        self.filenames.pop_front(); // Skip this file
                        work_units += 1; // Count as work even if failed
                        context.remaining_budget -= 1;
                    }
                }
            } else {
                break;
            }
        }

        // are we done?
        if self.filenames.is_empty() && self.conf.is_abandoned() && self.pending_files.is_empty() {
            info!("EOF on inport NAMES and all files processed, finishing");
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
            name: String::from("FileReader"),
            description: String::from(
                "Reads the contents of the given files and sends the contents.",
            ),
            icon: String::from("file"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("NAMES"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("filenames, one per IP"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("contents of the given file(s)"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}

pub struct FileTailerComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    filename: Option<String>,
    file: Option<TailedFile>,
    pending_lines: std::collections::VecDeque<Vec<u8>>, // lines to send, buffered for backpressure
}

const READ_TIMEOUT: Duration = Duration::from_millis(500);

impl Component for FileTailerComponent {
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
        FileTailerComponent {
            conf: inports
                .remove("NAME")
                .expect("found no NAME inport")
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
            filename: None,
            file: None,
            pending_lines: std::collections::VecDeque::new(),
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("FileTailer is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration if not yet configured
        if self.filename.is_none() {
            if let Ok(file_name_bytes) = self.conf.pop() {
                let file_name = std::str::from_utf8(&file_name_bytes).expect("failed to parse filename as UTF-8");
                trace!("got filename: {}", file_name);
                self.filename = Some(file_name.to_string());

                // Open the file
                match TailedFile::new(file_name) {
                    Ok(file) => {
                        self.file = Some(file);
                        trace!("opened file for tailing");
                    }
                    Err(e) => {
                        warn!("failed to open file {}: {}", file_name, e);
                        return ProcessResult::NoWork; // Can't proceed without file
                    }
                }
            } else {
                trace!("no config filename available yet");
                return ProcessResult::NoWork;
            }
        }

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

        // First, try to send any pending lines that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_lines.is_empty() {
            if let Some(pending_line) = self.pending_lines.front() {
                match self.out.push(pending_line.clone()) {
                    Ok(()) => {
                        self.pending_lines.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                        debug!("sent pending line");
                    }
                    Err(PushError::Full(_)) => {
                        // Still can't send, stop trying for now
                        break;
                    }
                }
            }
        }

        // Then, check for new lines within remaining budget
        while context.remaining_budget > 0 {
            // stay responsive to stop/ping even while checking file
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

            if let Some(file) = &mut self.file {
                // Check for new lines
                //TODO implement better version using Tokio integrated timeout, this is using additional thread and mpsc channel
                //TODO optimize - the source code of that crate seems inefficient, check
                //NOTE: this can return multiple lines
                if let Ok(lines) = file.read() {
                    if lines.len() > 0 {
                        debug!("got line(s), sending...");
                        // Try to send it
                        match self.out.push(lines) {
                            Ok(()) => {
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("done");
                            }
                            Err(PushError::Full(returned_lines)) => {
                                // Output buffer full, buffer internally for later retry
                                debug!("output buffer full, buffering lines internally");
                                self.pending_lines.push_back(returned_lines);
                                work_units += 1; // We checked the file, just couldn't send
                                context.remaining_budget -= 1;
                            }
                        }
                    } else {
                        // No new lines this time
                        break;
                    }
                } else {
                    // Error reading file
                    warn!("error reading from tailed file");
                    break;
                }
            } else {
                // File not opened
                break;
            }
        }

        // FileTailer never finishes on its own - it keeps tailing until stopped
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
            name: String::from("FileTailer"),
            description: String::from("Seeks to the end of the given file and sends new contents."),
            icon: String::from("file-o"),
            subgraph: false,
            in_ports: vec![ComponentPort {
                name: String::from("NAME"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("filename, one IP"), //TODO add support for multiple filenames
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            out_ports: vec![ComponentPort {
                name: String::from("OUT"),
                allowed_type: String::from("any"),
                schema: None,
                required: true,
                is_arrayport: false,
                description: String::from("contents of the given file"),
                values_allowed: vec![],
                value_default: String::from(""),
            }],
            ..Default::default()
        }
    }
}

pub struct FileWriterComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    filename: Option<String>,
    file: Option<BufWriter<File>>,
}

const BUFFER_SIZE: usize = 65536;

impl Component for FileWriterComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
    ) -> Self
    where
        Self: Sized,
    {
        FileWriterComponent {
            conf: inports
                .remove("CONF")
                .expect("found no CONF inport")
                .pop()
                .unwrap(),
            inn: inports
                .remove("IN")
                .expect("found no IN inport")
                .pop()
                .unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
            filename: None,
            file: None,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("FileWriter is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration if not yet configured
        if self.filename.is_none() {
            if let Ok(file_name_bytes) = self.conf.pop() {
                let file_name = std::str::from_utf8(&file_name_bytes).expect("failed to parse filename as UTF-8");
                trace!("got filename: {}", file_name);
                self.filename = Some(file_name.to_string());

                // Open the file
                match File::create(file_name) {
                    Ok(file) => {
                        let buffer = BufWriter::with_capacity(BUFFER_SIZE, file);
                        self.file = Some(buffer);
                        trace!("opened file for writing");
                    }
                    Err(e) => {
                        warn!("failed to open file {}: {}", file_name, e);
                        return ProcessResult::NoWork; // Can't proceed without file
                    }
                }
            } else {
                trace!("no config filename available yet");
                return ProcessResult::NoWork;
            }
        }

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

        // Write available chunks within remaining budget
        while context.remaining_budget > 0 {
            // stay responsive to stop/ping even while writing
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

            if let Some(buffer) = &mut self.file {
                // Try to read a chunk
                if let Ok(chunk) = self.inn.read_chunk(self.inn.slots()) {
                    if chunk.is_empty() {
                        break; // No more data available
                    }

                    debug!("got {} packets to write", chunk.len());

                    for ip in chunk.into_iter() {
                        if let Err(e) = buffer.write(ip.as_slice()) {
                            warn!("failed to write IP to buffer: {}", e);
                            // Continue with other IPs
                        }
                    }

                    // Flush the buffer
                    if let Err(e) = buffer.flush() {
                        warn!("failed to flush buffer to file: {}", e);
                    }

                    work_units += 1;
                    context.remaining_budget -= 1;
                } else {
                    break; // No more data
                }
            } else {
                // File not opened
                break;
            }
        }

        // Check for EOF on input
        if self.inn.is_abandoned() {
            info!("EOF on inport IN, finishing");
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
            name: String::from("FileWriter"),
            description: String::from("Writes the contents of received IPs to the given file."),
            icon: String::from("file"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("filename, one IP"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("data to be written to the given file"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
            ],
            out_ports: vec![],
            ..Default::default()
        }
    }
}
