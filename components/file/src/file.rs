use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError, PROCESSEDGE_BUFSIZE,
};
use log::{debug, info, trace, warn};

// component-specific for FileTailer
use staart::TailedFile;
// component-specific for FileWriter
use std::fs::File;
use std::io::prelude::*;
use std::io::BufWriter;
use std::sync::mpsc;

pub struct FileReaderComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: GraphInportOutportHandle,
    // Runtime state
    filenames: std::collections::VecDeque<FbpMessage>,
    pending_files: std::collections::VecDeque<FbpMessage>, // files to send, buffered for backpressure
    pending_read: Option<mpsc::Receiver<Result<Vec<u8>, std::io::Error>>>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
}

impl Component for FileReaderComponent {
    fn new(
        mut inports: ProcessInports,
        mut outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
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
            pending_read: None,
            scheduler_waker,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("FileReader is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration filenames if we haven't buffered them all yet
        while self.filenames.is_empty() && !self.conf.is_abandoned() {
            if let Ok(filename) = self.conf.pop() {
                let filename_str = filename.as_text()
                    .or_else(|| filename.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                    .unwrap_or("");
                trace!("got filename: {}", filename_str);
                self.filenames.push_back(filename);
            } else {
                break;
            }
        }

        // check signals
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            // stop signal
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
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

        // Start one asynchronous file read when idle.
        if self.pending_read.is_none() && !self.filenames.is_empty() {
            if let Some(filename) = self.filenames.front() {
                let file_path = filename.as_text()
                    .or_else(|| filename.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                    .unwrap_or("")
                    .to_owned();
                debug!("starting async read for {}", file_path);
                let (tx, rx) = mpsc::channel();
                let scheduler_waker = self.scheduler_waker.clone();
                std::thread::spawn(move || {
                    let result = std::fs::read(&file_path);
                    let _ = tx.send(result);
                    if let Some(waker) = scheduler_waker {
                        waker();
                    }
                });
                self.pending_read = Some(rx);
            }
        }

        // Complete async read if available.
        if context.remaining_budget > 0 {
            if let Some(rx) = &self.pending_read {
                match rx.try_recv() {
                    Ok(Ok(contents)) => {
                        let content_msg = FbpMessage::from_bytes(contents);
                        match self.out.push(content_msg) {
                            Ok(()) => {}
                            Err(PushError::Full(returned_content)) => {
                                self.pending_files.push_back(returned_content);
                            }
                        }
                        self.pending_read = None;
                        self.filenames.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Ok(Err(e)) => {
                        if let Some(filename) = self.filenames.front() {
                            let filename_str = filename.as_text()
                                .or_else(|| filename.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                                .unwrap_or("<invalid utf-8>");
                            warn!("failed to read file {}: {}", filename_str, e);
                        }
                        self.pending_read = None;
                        self.filenames.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.pending_read = None;
                    }
                }
            }
        }

        // are we done?
        if self.filenames.is_empty()
            && self.conf.is_abandoned()
            && self.pending_files.is_empty()
            && self.pending_read.is_none()
        {
            info!("EOF on inport NAMES and all files processed, finishing");
            return ProcessResult::Finished;
        }

        if work_units > 0 {
            ProcessResult::DidWork(work_units)
        } else {
            context.wake_at(
                std::time::Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
            );
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
    filename: Option<&'static str>,
    file: Option<TailedFile<&'static str>>,
    pending_lines: std::collections::VecDeque<Vec<u8>>, // lines to send, buffered for backpressure
}

impl Component for FileTailerComponent {
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
            if let Ok(file_name_msg) = self.conf.pop() {
                let file_name = file_name_msg.as_text()
                    .expect("failed to parse filename as text");
                trace!("got filename: {}", file_name);
                // staart::TailedFile currently requires a `Copy` path type.
                // Keep one leaked &'static str for the lifetime of this component instance.
                let leaked_name: &'static str = Box::leak(file_name.to_owned().into_boxed_str());
                self.filename = Some(leaked_name);

                // Open the file
                match TailedFile::new(leaked_name) {
                    Ok(file) => {
                        self.file = Some(file);
                        trace!("opened file for tailing");
                    }
                    Err(e) => {
                        warn!("failed to open file {}: {}", file_name, e);
                        context.wake_at(
                            std::time::Instant::now()
                                + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                        );
                        return ProcessResult::NoWork; // Can't proceed without file
                    }
                }
            } else {
                trace!("no config filename available yet");
                context.wake_at(
                    std::time::Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
                );
                return ProcessResult::NoWork;
            }
        }

        // check signals
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            // stop signal
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        // First, try to send any pending lines that were buffered due to backpressure
        while context.remaining_budget > 0 && !self.pending_lines.is_empty() {
            if let Some(pending_line) = self.pending_lines.front() {
                match self.out.push(pending_line.clone().into()) {
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
                let signal_text = sig.as_text()
                    .or_else(|| sig.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                    .unwrap_or("");
                trace!("received signal: {}", signal_text);
                if signal_text == "stop" {
                    info!("got stop signal while processing, finishing");
                    return ProcessResult::Finished;
                } else if signal_text == "ping" {
                    trace!("got ping signal, responding");
                    let pong_msg = FbpMessage::from_str("pong");
                    let _ = self.signals_out.try_send(pong_msg);
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
                        let lines_msg = FbpMessage::from_bytes(lines);
                        match self.out.push(lines_msg) {
                            Ok(()) => {
                                work_units += 1;
                                context.remaining_budget -= 1;
                                debug!("done");
                            }
                            Err(PushError::Full(returned_lines)) => {
                                // Output buffer full, buffer internally for later retry
                                debug!("output buffer full, buffering lines internally");
                                let bytes = returned_lines.as_bytes().unwrap_or(&[]).to_vec();
                                self.pending_lines.push_back(bytes);
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
            context.wake_at(
                std::time::Instant::now() + flowd_component_api::DEFAULT_IO_POLL_INTERVAL,
            );
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
    writer_tx: Option<mpsc::SyncSender<Vec<u8>>>,
    writer_result_rx: Option<mpsc::Receiver<std::io::Result<()>>>,
    pending_writes: std::collections::VecDeque<Vec<u8>>,
    scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
}

const BUFFER_SIZE: usize = 65536;

impl Component for FileWriterComponent {
    fn new(
        mut inports: ProcessInports,
        _outports: ProcessOutports,
        signals_in: ProcessSignalSource,
        signals_out: ProcessSignalSink,
        _graph_inout: GraphInportOutportHandle,
        scheduler_waker: Option<flowd_component_api::SchedulerWaker>,
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
            writer_tx: None,
            writer_result_rx: None,
            pending_writes: std::collections::VecDeque::new(),
            scheduler_waker,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("FileWriter is now process()ing!");
        let mut work_units = 0u32;

        // Read configuration if not yet configured
        if self.filename.is_none() || self.writer_tx.is_none() {
            if let Ok(file_name_msg) = self.conf.pop() {
                let file_name = file_name_msg.as_text()
                    .expect("failed to parse filename as text");
                trace!("got filename: {}", file_name);
                self.filename = Some(file_name.to_string());
                let (data_tx, data_rx) = mpsc::sync_channel::<Vec<u8>>(PROCESSEDGE_BUFSIZE);
                let (result_tx, result_rx) = mpsc::channel::<std::io::Result<()>>();
                let file_name_owned = file_name.to_string();
                let scheduler_waker = self.scheduler_waker.clone();
                std::thread::spawn(move || {
                    let file = match File::create(&file_name_owned) {
                        Ok(file) => file,
                        Err(e) => {
                            let _ = result_tx.send(Err(e));
                            if let Some(waker) = scheduler_waker {
                                waker();
                            }
                            return;
                        }
                    };
                    let mut buffer = BufWriter::with_capacity(BUFFER_SIZE, file);
                    let _ = result_tx.send(Ok(()));
                    if let Some(waker) = scheduler_waker.as_ref() {
                        waker();
                    }
                    while let Ok(packet) = data_rx.recv() {
                        let write_result = buffer
                            .write_all(packet.as_slice())
                            .and_then(|_| buffer.flush());
                        let _ = result_tx.send(write_result);
                        if let Some(waker) = scheduler_waker.as_ref() {
                            waker();
                        }
                    }
                });
                self.writer_tx = Some(data_tx);
                self.writer_result_rx = Some(result_rx);
                work_units += 1;
            } else {
                trace!("no config filename available yet");
                return ProcessResult::NoWork;
            }
        }

        // check signals
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            // stop signal
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text)
            }
        }

        if let Some(result_rx) = &self.writer_result_rx {
            while let Ok(result) = result_rx.try_recv() {
                if let Err(e) = result {
                    warn!("failed to write to file: {}", e);
                }
                work_units += 1;
            }
        }

        if let Ok(chunk) = self.inn.read_chunk(self.inn.slots()) {
            for ip in chunk {
                let bytes = ip.as_bytes().unwrap_or(&[]).to_vec();
                self.pending_writes.push_back(bytes);
            }
        }

        while context.remaining_budget > 0 && !self.pending_writes.is_empty() {
            if let (Some(tx), Some(packet)) = (&self.writer_tx, self.pending_writes.front().cloned()) {
                match tx.try_send(packet) {
                    Ok(()) => {
                        self.pending_writes.pop_front();
                        work_units += 1;
                        context.remaining_budget -= 1;
                    }
                    Err(mpsc::TrySendError::Full(_)) => break,
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        warn!("file writer worker disconnected");
                        self.writer_tx = None;
                        break;
                    }
                }
            } else {
                break;
            }
        }

        // Check for EOF on input
        if self.inn.is_abandoned() && self.pending_writes.is_empty() {
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
