#![feature(duration_constants)]
#![feature(io_error_more)]
#![feature(map_try_insert)]

// basics and threading
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, Thread};

// network and WebSocket
use std::net::TcpStream;
use tungstenite::Message;

// persistence
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

// arguments
use std::env;

// logging
#[macro_use]
extern crate log;
extern crate simplelog; //TODO check the paris feature flag for tags, useful?

// JSON serialization and deserialization
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use serde_with::skip_serializing_none;

// data structures
use multimap::MultiMap;
use std::collections::HashMap;
//use dashmap::DashMap;

// timekeeping in watchdog thread etc.
use chrono::prelude::*;
use std::time::{Duration, Instant};

// flowd component API crate
pub use flowd_component_api::{
    BudgetClass, Component, ComponentComponentPayload, ComponentPort, FbpMessage,
    GraphInportOutportHandle, MessageBuf, NodeContext, ProcessEdge, ProcessEdgeSink,
    ProcessEdgeSinkConnection, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource, PushError, WakeupNotify, PROCESSEDGE_BUFSIZE,
    PROCESSEDGE_IIP_BUFSIZE, PROCESSEDGE_SIGNAL_BUFSIZE,
};
use flowd_component_api::{
    GraphNodeSpecNetwork as TraceGraphNodeSpecNetwork,
    TraceConnectEventPayload as ApiTraceConnectEventPayload,
    TraceDataEventPayload as ApiTraceDataEventPayload,
    TraceDisconnectEventPayload as ApiTraceDisconnectEventPayload,
};

// configuration
const PROCESS_HEALTHCHECK_DUR: core::time::Duration = Duration::from_secs(7); //NOTE: 7 * core::time::Duration::SECOND is not compile-time calculatable (mul const trait not implemented)
const WATCHDOG_POLL_DUR: core::time::Duration = Duration::from_millis(50);
const WATCHDOG_MAX_MISSED_PONGS: u8 = 2;
const CLIENT_BROADCAST_WRITE_TIMEOUT: Option<Duration> = Some(Duration::from_millis(200));
const NODE_WIDTH_DEFAULT: u32 = 72;
const NODE_HEIGHT_DEFAULT: u32 = 72;
const PERSISTENCE_FILE_NAME: &str = "flowd.graph.json";


include!("runtime.rs");
include!("protocol.rs");
include!("graph.rs");

pub mod bench_api;

#[cfg(test)]
mod tests;
