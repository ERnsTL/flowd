use flowd_component_api::{
    Component, ComponentComponentPayload, ComponentPort, FbpMessage, GraphInportOutportHandle, NodeContext,
    ProcessEdgeSink, ProcessEdgeSource, ProcessInports, ProcessOutports, ProcessResult,
    ProcessSignalSink, ProcessSignalSource,
};
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct EmailParserConfig {
    extract_headers: Vec<String>,
    prefer_text_body: bool,
    include_html_body: bool,
}

#[derive(Debug)]
struct BodyPart {
    content_type: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ParsedEmail {
    headers: HashMap<String, String>,
    body_text: Option<String>,
    body_html: Option<String>,
    raw_size: usize,
}

pub struct EmailParserComponent {
    conf: ProcessEdgeSource,
    inn: ProcessEdgeSource,
    out: ProcessEdgeSink,
    signals_in: ProcessSignalSource,
    signals_out: ProcessSignalSink,
    config: Option<EmailParserConfig>,
}

impl Component for EmailParserComponent {
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
        EmailParserComponent {
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
            out: outports
                .remove("OUT")
                .expect("found no OUT outport")
                .pop()
                .unwrap(),
            signals_in,
            signals_out,
            config: None,
        }
    }

    fn process(&mut self, context: &mut NodeContext) -> ProcessResult {
        debug!("EmailParser process() called");

        let mut work_units = 0u32;

        // Check signals first
        if let Ok(signal) = self.signals_in.try_recv() {
            let signal_text = signal.as_text()
                .or_else(|| signal.as_bytes().and_then(|b| std::str::from_utf8(b).ok()))
                .unwrap_or("");
            trace!("received signal: {}", signal_text);
            if signal_text == "stop" {
                info!("got stop signal, finishing");
                return ProcessResult::Finished;
            } else if signal_text == "ping" {
                trace!("got ping signal, responding");
                let pong_msg = FbpMessage::from_str("pong");
                let _ = self.signals_out.try_send(pong_msg);
            } else {
                warn!("received unknown signal: {}", signal_text);
            }
        }

        // Check if we have configuration
        if self.config.is_none() {
            if let Ok(conf_msg) = self.conf.pop() {
                let conf_text = conf_msg.as_text().expect("config must be text");
                debug!("received config: {}", conf_text);
                match serde_json::from_str::<EmailParserConfig>(conf_text) {
                    Ok(config) => {
                        self.config = Some(config);
                        work_units += 1;
                    }
                    Err(err) => {
                        error!("failed to parse config JSON: {}", err);
                        return ProcessResult::Finished;
                    }
                }
            } else {
                return ProcessResult::NoWork;
            }
        }

        let config = self.config.as_ref().unwrap();

        // Process input emails
        while context.remaining_budget > 0 && !self.inn.is_empty() {
            if let Ok(email_msg) = self.inn.pop() {
                let email_bytes = email_msg.as_bytes().unwrap_or(&[]);
                debug!("processing email of {} bytes", email_bytes.len());

                match parse_email(email_bytes, config) {
                    Ok(parsed) => {
                        match serde_json::to_string(&parsed) {
                            Ok(json_str) => {
                                let output_msg = FbpMessage::from_text(json_str);
                                if let Err(_) = self.out.push(output_msg) {
                                    warn!("output buffer full, stopping processing");
                                    break;
                                }
                                work_units += 1;
                                context.remaining_budget -= 1;
                            }
                            Err(err) => {
                                error!("failed to serialize parsed email: {}", err);
                            }
                        }
                    }
                    Err(err) => {
                        warn!("failed to parse email: {}", err);
                    }
                }
            } else {
                break;
            }
        }

        // Check if input is abandoned
        if self.inn.is_abandoned() {
            info!("EOF on inport, shutting down");
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
            name: String::from("EmailParser"),
            description: String::from("Parses raw email bytes into structured JSON with headers and body content."),
            icon: String::from("envelope"),
            subgraph: false,
            in_ports: vec![
                ComponentPort {
                    name: String::from("CONF"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("JSON configuration specifying headers to extract and body preferences"),
                    values_allowed: vec![],
                    value_default: String::from("{\"extract_headers\":[\"Subject\",\"From\",\"To\",\"Date\"],\"prefer_text_body\":true,\"include_html_body\":true}"),
                },
                ComponentPort {
                    name: String::from("IN"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("raw email bytes from IMAP"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                },
            ],
            out_ports: vec![
                ComponentPort {
                    name: String::from("OUT"),
                    allowed_type: String::from("any"),
                    schema: None,
                    required: true,
                    is_arrayport: false,
                    description: String::from("parsed email as JSON"),
                    values_allowed: vec![],
                    value_default: String::from(""),
                }
            ],
            ..Default::default()
        }
    }
}

fn parse_email(email_bytes: &[u8], config: &EmailParserConfig) -> Result<ParsedEmail, String> {
    let mail = mailparse::parse_mail(email_bytes).map_err(|e| e.to_string())?;

    let mut headers = HashMap::new();

    // Extract specified headers
    for header_name in &config.extract_headers {
        for header in &mail.headers {
            if header.get_key().eq_ignore_ascii_case(header_name) {
                let value = header.get_value();
                headers.insert(header_name.clone(), value);
                break; // Only take the first occurrence
            }
        }
    }

    // Extract body content with MIME-aware parsing
    let bodies = extract_bodies(&mail);
    let (body_text, body_html) = select_bodies(bodies, config);

    Ok(ParsedEmail {
        headers,
        body_text,
        body_html,
        raw_size: email_bytes.len(),
    })
}

/// Recursively extract body parts from email, handling MIME structure
fn extract_bodies(part: &mailparse::ParsedMail) -> Vec<BodyPart> {
    let mut bodies = Vec::new();

    // Check if this part has content
    if let Ok(body_raw) = part.get_body_raw() {
        if let Ok(content) = String::from_utf8(body_raw) {
            let content_type = part.ctype.mimetype.clone();

            // Check if this is an attachment
            let is_attachment = part.headers.iter()
                .find(|h| h.get_key().eq_ignore_ascii_case("Content-Disposition"))
                .map(|h| h.get_value().to_lowercase().contains("attachment"))
                .unwrap_or(false);

            if !is_attachment && content_type.starts_with("text/") {
                bodies.push(BodyPart {
                    content_type,
                    content,
                });
            }
        }
    }

    // Recursively process subparts
    for subpart in &part.subparts {
        bodies.extend(extract_bodies(subpart));
    }

    bodies
}

/// Select appropriate bodies based on configuration preferences
fn select_bodies(parts: Vec<BodyPart>, config: &EmailParserConfig) -> (Option<String>, Option<String>) {
    let mut text_parts: Vec<String> = Vec::new();
    let mut html_parts: Vec<String> = Vec::new();

    // Separate text and HTML parts
    for part in parts {
        if part.content_type == "text/plain" {
            text_parts.push(part.content);
        } else if part.content_type == "text/html" {
            html_parts.push(part.content);
        }
    }

    // Apply configuration preferences
    match (text_parts.is_empty(), html_parts.is_empty()) {
        (false, false) => {
            // Both text and HTML available
            if config.prefer_text_body {
                // Prefer text, optionally include HTML
                let text = Some(text_parts.join("\n"));
                let html = if config.include_html_body {
                    Some(html_parts.join("\n"))
                } else {
                    None
                };
                (text, html)
            } else {
                // Prefer HTML, optionally include text
                let html = Some(html_parts.join("\n"));
                let text = if config.include_html_body {
                    Some(text_parts.join("\n"))
                } else {
                    None
                };
                (text, html)
            }
        }
        (false, true) => {
            // Only text available
            (Some(text_parts.join("\n")), None)
        }
        (true, false) => {
            // Only HTML available
            (None, Some(html_parts.join("\n")))
        }
        (true, true) => {
            // No text content found
            (None, None)
        }
    }
}
