use std::sync::{Arc, Mutex};
use crate::{ProcessEdgeSource, ProcessEdgeSink, Component, ProcessSignalSink, ProcessSignalSource, GraphInportOutportHolder, ProcessInports, ProcessOutports, ComponentComponentPayload, ComponentPort};

// component-specific for FileTailer
use std::time::Duration;
use staart::TailedFile;

pub struct FileReaderComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

impl Component for FileReaderComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        FileReaderComponent {
            conf: inports.remove("NAMES").expect("found no NAMES inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("FileReader is now run()ning!");
        let mut filenames = self.conf;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");
        loop {
            trace!("begin of iteration");
            // check signals
            //TODO optimize, there is also try_recv() and recv_timeout()
            if let Ok(ip) = self.signals_in.try_recv() {
                //TODO optimize string conversions
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {   //TODO optimize comparison
                    info!("got stop signal, exiting");
                    break;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    self.signals_out.send(b"pong".to_vec()).expect("cloud not send pong");
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }
            // check in port
            //TODO while !inn.is_empty() {
            loop {
                if let Ok(ip) = filenames.pop() {
                    // read filename on inport
                    let file_path = std::str::from_utf8(&ip).expect("non utf-8 data");
                    debug!("got a filename: {}", &file_path);

                    // read whole file
                    //TODO may be big file - add chunking
                    //TODO enclose files in brackets to know where its stream of chunks start and end
                    debug!("reading file...");
                    let contents = std::fs::read(file_path).expect("should have been able to read the file");

                    // send it
                    debug!("forwarding file contents...");
                    out.push(contents).expect("could not push into OUT");
                    out_wakeup.unpark();
                    debug!("done");
                } else {
                    break;
                }
            }

            // are we done?
            if filenames.is_abandoned() {
                info!("EOF on inport NAMES, shutting down");
                drop(out);
                out_wakeup.unpark();
                break;
            }

            trace!("-- end of iteration");
            std::thread::park();
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("FileReader"),
            description: String::from("Reads the contents of the given files and sends the contents."),
            icon: String::from("file"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("NAMES"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("filenames, one per IP"),
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
                    description: String::from("conents of the given files"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}

pub struct FileTailerComponent {
    conf: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    //graph_inout: Arc<Mutex<GraphInportOutportHolder>>,
}

const READ_TIMEOUT: Duration = Duration::from_millis(500);

impl Component for FileTailerComponent {
    fn new(mut inports: ProcessInports, mut outports: ProcessOutports, signals_in: ProcessSignalSource, signals_out: ProcessSignalSink, _graph_inout: Arc<Mutex<GraphInportOutportHolder>>) -> Self where Self: Sized {
        FileTailerComponent {
            conf: inports.remove("NAME").expect("found no NAME inport").pop().unwrap(),
            out: outports.remove("OUT").expect("found no OUT outport").pop().unwrap(),
            signals_in: signals_in,
            signals_out: signals_out,
            //graph_inout: graph_inout,
        }
    }

    fn run(self) {
        debug!("FileTailer is now run()ning!");
        let mut conf = self.conf;
        let mut out = self.out.sink;
        let out_wakeup = self.out.wakeup.expect("got no wakeup handle for outport OUT");

        // read configuration
        trace!("read config IP");
        /*
        while conf.is_empty() {
            thread::yield_now();
        }
        */
        let Ok(file_name) = conf.pop() else { trace!("no config IP received - exiting"); return; };

        // configure
        let mut file = TailedFile::new(std::str::from_utf8(&file_name).expect("failed to parse filename as UTF-8")).expect("failed to open given file");

        // main loop
        loop {
            trace!("begin of iteration");
            // check signals
            //TODO optimize, there is also try_recv() and recv_timeout()
            if let Ok(ip) = self.signals_in.try_recv() {
                //TODO optimize string conversions
                trace!("received signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"));
                // stop signal
                if ip == b"stop" {   //TODO optimize comparison
                    info!("got stop signal, exiting");
                    break;
                } else if ip == b"ping" {
                    trace!("got ping signal, responding");
                    self.signals_out.send(b"pong".to_vec()).expect("cloud not send pong");
                } else {
                    warn!("received unknown signal ip: {}", std::str::from_utf8(&ip).expect("invalid utf-8"))
                }
            }

            // check file
            //TODO implement better version using Tokio integrated timeout, this is using additional thread and an mpsc channel
            //TODO optimize - the source code of that crate seems unefficient, check
            //NOTE: this can return multiple lines
            if let Ok(lines)  = file.read() {
                if lines.len() > 0 {
                    // send it
                    debug!("got line(s), sending...");
                    // split lines - but that should be done by separate component
                    /*
                    let cur = Cursor::new(&lines);
                    let mut line_iter = cur.lines();
                    while let Some(line_res) = line_iter.next() {
                        match line_res {
                            Ok(line) => {   //TODO optimize - here we get a String so this seems to clone
                                out.push(line.into_bytes()).expect("could not push into OUT");
                            },
                            Err(e) => {
                                error!("could not read line: {}", e);
                                break;
                            }
                        }
                    }
                    */
                    out.push(lines).expect("could not push into OUT");
                    out_wakeup.unpark();
                    debug!("done");
                }
            };

            // are we done?
            //NOTE: no trigger for the side of the file reader, only from FBP network

            trace!("-- end of iteration");
            //std::thread::park();
            std::thread::sleep(READ_TIMEOUT);   //TODO this is basically polling, but maybe this is necessary to handle file replacement (log rotation)
        }
        info!("exiting");
    }

    fn get_metadata() -> ComponentComponentPayload where Self: Sized {
        ComponentComponentPayload {
            name: String::from("FileTailer"),
            description: String::from("Seeks to the end of the given file and sends new contents."),
            icon: String::from("file-o"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("NAME"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("filename, one IP"),  //TODO add support for multiple filenames
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
                    description: String::from("conents of the given file"),
                    values_allowed: vec![],
                    value_default: String::from("")
                }
            ],
            ..Default::default()
        }
    }
}