//! JSON-RPC 2.0 framing and the MCP lifecycle / tools / resources methods.

use std::io::{BufRead, Write};
const MAX_MESSAGE: u64 = 4 * 1024 * 1024;

use serde_json::{Value, json};

use crate::backend::Backend;
use crate::tools::{TOOL_RESULT_BYTES, call_tool, tool_definitions};

/// The MCP revision we implement.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_VERSIONS: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const RESOURCE_NOT_FOUND: i64 = -32002;

const INSTRUCTIONS: &str = "MisterDiagram headless Visio core. Strict transactional authoring, inspection and export. Page coordinates are inches, y-up. For drawings generated from data with hundreds of commands, write a script that builds the command list and runs it with the CLI, so the drawing can be rebuilt when data changes. Use MCP tools for interactive edits, inspection and the render-and-look loop. Use export for deliverables and render for previews.";

/// Resource URIs.
pub const DOC_URI: &str = "misterdiagram://document";
pub const COMMANDS_URI: &str = "misterdiagram://commands";

/// An MCP server bound to one backend.
pub struct Server {
    backend: Box<dyn Backend>,
    initialized: bool,
}

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn response_limit() -> String {
    error(Value::Null, INTERNAL_ERROR, "MCP response exceeds cap").to_string()
}
const MAX_REPLY: usize = MAX_MESSAGE as usize - 1;
struct ReplyBuffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl ReplyBuffer {
    fn new(limit: usize) -> Self {
        Self { bytes: Vec::new(), limit }
    }
}
impl Write for ReplyBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        // Reserve one byte for the newline framing emitted by serve.
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(std::io::Error::other("MCP response exceeds cap"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn batch_reply_limit(message: &Value) -> usize {
    let method = message.get("method").and_then(Value::as_str);
    let id = message.get("id");
    if method.is_some() && id.is_none() {
        return 0;
    }
    // Non-ping replies reserve a full tool result. The remaining space covers
    // the JSON-RPC wrapper and the actual escaped request ID.
    let payload = if method == Some("ping") { 0 } else { TOOL_RESULT_BYTES };
    payload + 128 + id.map_or(4, |id| id.to_string().len())
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend, initialized: false }
    }

    pub fn backend(&mut self) -> &mut dyn Backend {
        self.backend.as_mut()
    }

    /// Whether the client has sent `notifications/initialized`.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Serve newline-delimited JSON-RPC until `input` closes. Logs go to stderr only (stdout is
    /// the protocol stream).
    pub fn serve(&mut self, mut input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        // One allocation avoids retaining earlier growth allocations alongside
        // the cap-sized frame on allocators that keep freed blocks resident.
        let mut line = Vec::with_capacity(MAX_MESSAGE as usize);
        loop {
            line.clear();
            if line.capacity() == 0 {
                line.reserve_exact(MAX_MESSAGE as usize);
            }
            let mut oversized = false;
            // Drain one entire frame. Once over cap, consume borrowed chunks
            // without retaining any more bytes, including the terminating LF.
            loop {
                let chunk = input.fill_buf()?;
                if chunk.is_empty() {
                    break;
                }
                let newline = chunk.iter().position(|b| *b == b'\n');
                let count = newline.map_or(chunk.len(), |i| i + 1);
                if !oversized {
                    if count > (MAX_MESSAGE as usize).saturating_sub(line.len()) {
                        oversized = true;
                        // The rejected prefix is no longer useful. Release it
                        // before draining or constructing the error/next reply.
                        line.clear();
                        line.shrink_to_fit();
                    } else {
                        line.extend_from_slice(&chunk[..count]);
                    }
                }
                input.consume(count);
                if newline.is_some() {
                    break;
                }
            }
            if line.is_empty() && !oversized {
                break;
            }
            let reply = if oversized {
                Some(error(Value::Null, PARSE_ERROR, "MCP message exceeds cap").to_string())
            } else {
                match std::str::from_utf8(&line) {
                    Ok(line) => self.handle_line(line),
                    Err(e) => Some(error(Value::Null, PARSE_ERROR, format!("parse error: {e}")).to_string()),
                }
            };
            if let Some(reply) = reply {
                output.write_all(reply.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// Handle one line; returns the reply line (None for notifications and blank lines).
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.len() as u64 > MAX_MESSAGE {
            return Some(error(Value::Null, PARSE_ERROR, "MCP message exceeds cap").to_string());
        }
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) => self.handle_batch(batch),
            Ok(msg) => self.handle(msg).map(|reply| {
                let mut bytes = ReplyBuffer::new(MAX_REPLY);
                if serde_json::to_writer(&mut bytes, &reply).is_err() {
                    return response_limit();
                }
                String::from_utf8(bytes.bytes).unwrap_or_else(|_| response_limit())
            }),
            Err(e) => Some(error(Value::Null, PARSE_ERROR, format!("parse error: {e}")).to_string()),
        }
    }

    fn handle_batch(&mut self, batch: Vec<Value>) -> Option<String> {
        if batch.is_empty() {
            return Some(error(Value::Null, INVALID_REQUEST, "empty batch").to_string());
        }
        let mut remaining = MAX_REPLY - 2;
        for message in &batch {
            let limit = batch_reply_limit(message);
            if limit == 0 {
                continue;
            }
            let Some(left) = remaining.checked_sub(limit + 1) else {
                return Some(error(Value::Null, INVALID_REQUEST, "MCP response exceeds cap: split the batch; no requests ran").to_string());
            };
            remaining = left;
        }

        let mut replies = ReplyBuffer::new(MAX_REPLY);
        replies.bytes.push(b'[');
        for message in batch {
            let limit = batch_reply_limit(&message);
            let Some(reply) = self.handle(message) else { continue };
            if replies.bytes.len() > 1 {
                replies.bytes.push(b',');
            }
            let start = replies.bytes.len();
            replies.limit = start + limit;
            if serde_json::to_writer(&mut replies, &reply).is_err() {
                replies.bytes.truncate(start);
                // Tool results are bounded before they reach this frame. A
                // larger custom read response still gets a correlated reply.
                let id = reply.get("id").cloned().unwrap_or(Value::Null);
                if serde_json::to_writer(&mut replies, &error(id, INTERNAL_ERROR, "MCP response exceeds cap")).is_err() {
                    return Some(response_limit());
                }
            }
        }
        if replies.bytes.len() == 1 {
            return None;
        }
        replies.bytes.push(b']');
        Some(String::from_utf8(replies.bytes).unwrap_or_else(|_| response_limit()))
    }

    /// Handle one JSON-RPC message; `None` for notifications and responses.
    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle_message(msg))) {
            Ok(reply) => reply,
            Err(payload) => {
                let message = misterdiagram_engine::guard::panic_message(payload.as_ref());
                let _ = writeln!(std::io::stderr(), "MCP request panic: {message}");
                Some(error(id, INTERNAL_ERROR, message))
            }
        }
    }

    fn handle_message(&mut self, msg: Value) -> Option<Value> {
        let Value::Object(o) = &msg else { return Some(error(Value::Null, INVALID_REQUEST, "message must be an object")) };
        let id = o.get("id").cloned();
        let Some(method) = o.get("method").and_then(Value::as_str) else {
            // A response to a server→client request (we send none): ignore. Anything else is invalid.
            if o.contains_key("result") || o.contains_key("error") {
                return None;
            }
            return Some(error(id.unwrap_or(Value::Null), INVALID_REQUEST, "missing `method`"));
        };
        let params = o.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            self.notification(method);
            return None;
        };
        if !(id.is_string() || id.is_number()) {
            return Some(error(Value::Null, INVALID_REQUEST, "`id` must be a string or number"));
        }
        Some(match self.request(method, &params) {
            Ok(r) => response(id, r),
            Err((code, m)) => error(id, code, m),
        })
    }

    fn notification(&mut self, method: &str) {
        match method {
            "notifications/initialized" => self.initialized = true,
            "notifications/cancelled" | "notifications/progress" | "notifications/roots/list_changed" => {}
            other => log::debug!("ignoring notification {other}"),
        }
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
                let version = if SUPPORTED_VERSIONS.contains(&asked) { asked } else { PROTOCOL_VERSION };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}, "resources": {}},
                    "serverInfo": {"name": "misterdiagram", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": format!("{INSTRUCTIONS} Backend: {}", self.backend.describe()),
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tool_definitions().map_err(|e| (INTERNAL_ERROR,e))?})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing tool `name`".to_string()))?;
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                #[cfg(any(test, feature = "test-panic-injection"))]
                if name == "__test_panic_dispatch" {
                    std::panic::resume_unwind(Box::new("injected server-dispatch panic"));
                }
                Ok(call_tool(self.backend.as_mut(), name, &args).to_value())
            }
            "resources/list" => Ok(json!({"resources": [
                {"uri": DOC_URI, "name": "document", "title": "Active document", "description": "Pages, layers, evaluated shapes and connections", "mimeType": "application/json"},
                {"uri": COMMANDS_URI, "name": "commands", "title": "Commands", "description": "Every command: id, label, menu path, shortcut, params, enabled (engine.commands)", "mimeType": "application/json"},
            ]})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing `uri`".to_string()))?;
                let v = match uri {
                    DOC_URI => self.backend.call("document.inspect", json!({})),
                    COMMANDS_URI => self.backend.call("engine.commands", json!({})),
                    _ => return Err((RESOURCE_NOT_FOUND, format!("resource not found: {uri}"))),
                }
                .map_err(|e| (INTERNAL_ERROR, e))?;
                Ok(json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}]}))
            }
            other => Err((METHOD_NOT_FOUND, format!("method not found: {other}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headless::Headless;
    #[test]
    fn server_dispatch_panic_is_internal_error_and_next_message_works() {
        let mut s = Server::new(Box::new(Headless::new()));
        let reply = s.handle(json!({"id":"panic-id","method":"tools/call","params":{"name":"__test_panic_dispatch"}})).unwrap();
        assert_eq!(reply["id"], "panic-id");
        assert_eq!(reply["error"]["code"], INTERNAL_ERROR);
        assert_eq!(reply["error"]["message"], "injected server-dispatch panic");
        assert_eq!(s.handle(json!({"id":2,"method":"ping"})).unwrap()["result"], json!({}));
    }
    #[test]
    fn framing_recovers_at_newline_and_eof_with_small_reader_chunks() {
        for bad in [vec![b'x'; MAX_MESSAGE as usize + 1], vec![0xff, 0xfe]] {
            let mut input = bad.clone();
            input.extend_from_slice(b"\n{\"id\":7,\"method\":\"ping\"}\n");
            let mut output = Vec::new();
            let mut s = Server::new(Box::new(Headless::new()));
            s.serve(std::io::BufReader::with_capacity(31, std::io::Cursor::new(input)), &mut output).unwrap();
            let replies = String::from_utf8(output).unwrap().lines().map(|v| serde_json::from_str::<Value>(v).unwrap()).collect::<Vec<_>>();
            assert_eq!(replies.len(), 2);
            assert_eq!(replies[0]["id"], Value::Null);
            assert_eq!(replies[0]["error"]["code"], PARSE_ERROR);
            assert_eq!(replies[1]["id"], 7);
            assert_eq!(replies[1]["result"], json!({}));
            let mut output = Vec::new();
            s.serve(std::io::Cursor::new(bad), &mut output).unwrap();
            assert_eq!(String::from_utf8(output).unwrap().lines().count(), 1);
        }
    }
    #[test]
    fn hostile_json_and_bounded_framing() {
        let mut s = Server::new(Box::new(Headless::new()));
        for text in ["{", "[]", "42", "{\"id\":null,\"method\":\"ping\"}"] {
            assert!(s.handle_line(text).is_some());
        }
        assert!(s.handle_line(&"[".repeat(200)).is_some());
        let mut replies = Vec::new();
        s.serve(std::io::Cursor::new(vec![b'x'; MAX_MESSAGE as usize + 1]), &mut replies).unwrap();
        assert!(String::from_utf8(replies).unwrap().contains("MCP message exceeds cap"));
    }
    #[test]
    fn aggregate_and_escaped_responses_obey_the_framing_cap() {
        let mut s = Server::new(Box::new(Headless::new()));
        let requests = (0..200)
            .map(|id| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"list_commands","arguments":{"detail":"full"}}}))
            .collect::<Vec<_>>();
        let reply = s.handle_line(&serde_json::to_string(&requests).unwrap()).unwrap();
        assert!(reply.len() < MAX_MESSAGE as usize);
        assert!(reply.contains("MCP response exceeds cap"));
        // Escaped IDs can make the response larger than its already bounded input.
        let request = json!({"id":"x".repeat(MAX_MESSAGE as usize-100),"method":"tools/list"});
        let reply = s.handle_line(&request.to_string()).unwrap();
        assert!(reply.len() < MAX_MESSAGE as usize);
        assert!(reply.contains("MCP response exceeds cap"));
    }
    #[test]
    fn rejected_legacy_batch_dispatches_no_edits_and_small_batches_keep_request_ids() {
        let mut s = Server::new(Box::new(Headless::new()));
        let drawing = json!({"jsonrpc":"2.0","id":"edit","method":"tools/call","params":{
            "name":"run_command","arguments":{"command":"shape.draw.rect","params":{"x":1,"y":1,"width":2,"height":1}}
        }});
        let reads =
            (0..200).map(|id| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"list_commands","arguments":{"detail":"full"}}}));
        let oversized = std::iter::once(drawing.clone()).chain(reads).collect::<Vec<_>>();
        let refusal = s.handle_line(&serde_json::to_string(&oversized).unwrap()).unwrap();
        assert!(refusal.contains("no requests ran"));

        let ping = json!({"jsonrpc":"2.0","id":"ping","method":"ping"});
        let small: Value = serde_json::from_str(&s.handle_line(&json!([drawing, ping]).to_string()).unwrap()).unwrap();
        assert_eq!(small[0]["id"], "edit");
        assert_eq!(small[0]["result"]["isError"], false);
        let result: Value = serde_json::from_str(small[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(result["id"], 1, "the rejected batch must not consume a shape ID");
        assert_eq!(small[1]["id"], "ping");
        assert_eq!(small[1]["result"], json!({}));
    }
    #[test]
    fn seventeen_headless_tools() {
        let mut s = Server::new(Box::new(Headless::new()));
        let v = s.handle(json!({"id":1,"method":"tools/list"})).unwrap();
        assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 17);
    }
}
