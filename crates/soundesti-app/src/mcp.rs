//! Built-in MCP (Model Context Protocol) server: lets AI assistants drive Soundesti.
//!
//! Transport: MCP "Streamable HTTP" in its simple request/response form — every
//! JSON-RPC message is a `POST /mcp` answered with `application/json` (no SSE stream).
//! Connect with e.g. `claude mcp add --transport http soundesti http://127.0.0.1:47821/mcp`.
//!
//! Safety: binds to 127.0.0.1 only, rejects requests whose `Host` is not local or that
//! carry a non-local `Origin` (blocks web pages / DNS rebinding), and is off by default.
//!
//! Threading: the HTTP thread answers protocol messages itself; `tools/call` is forwarded
//! to the UI thread as an [`McpCall`] (the app applies it between frames) and the HTTP
//! thread waits for the reply.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use serde_json::{Value, json};

pub const DEFAULT_PORT: u16 = 47821;
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_BODY: usize = 8 * 1024 * 1024;
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// What a tool returns: text (usually JSON) or an image.
pub enum ToolOutput {
    Text(String),
    Image { png: Vec<u8>, note: String },
}

pub type ToolResult = Result<ToolOutput, String>;

/// A tool invocation waiting for the UI thread.
pub struct McpCall {
    pub name: String,
    pub args: Value,
    pub reply: Sender<ToolResult>,
}

pub struct McpServer {
    pub port: u16,
    pub calls: Receiver<McpCall>,
    stop: Arc<AtomicBool>,
}

impl Drop for McpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl McpServer {
    /// Starts listening on 127.0.0.1:`port`. `wake` is called when a tool call arrives so
    /// the UI thread can process it promptly.
    pub fn start(port: u16, wake: impl Fn() + Send + Sync + 'static) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = stop.clone();
        let wake = Arc::new(wake);
        std::thread::Builder::new().name("soundesti-mcp".into()).spawn(move || {
            while !stop_t.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let tx = tx.clone();
                        let wake = wake.clone();
                        std::thread::spawn(move || {
                            let _ = serve_connection(stream, &|name, args| {
                                let (rtx, rrx) = channel();
                                if tx.send(McpCall { name: name.to_string(), args, reply: rtx }).is_err() {
                                    return Err("Soundesti is shutting down".into());
                                }
                                wake();
                                rrx.recv_timeout(CALL_TIMEOUT)
                                    .unwrap_or_else(|_| Err("timed out waiting for Soundesti".into()))
                            });
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(30))
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        })?;
        Ok(Self { port, calls: rx, stop })
    }
}

fn is_local_host(host: &str) -> bool {
    let h = host.trim();
    let name = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        h.split(':').next().unwrap_or("")
    };
    matches!(name.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

fn is_local_origin(origin: &str) -> bool {
    let o = origin.trim();
    if o == "null" {
        return false;
    }
    let rest = o.split("://").nth(1).unwrap_or(o);
    is_local_host(rest.split('/').next().unwrap_or(""))
}

fn respond(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let ctype = if body.is_empty() { "" } else { "Content-Type: application/json\r\n" };
    write!(stream, "HTTP/1.1 {status}\r\n{ctype}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())?;
    stream.flush()
}

fn serve_connection(mut stream: TcpStream, dispatch: &dyn Fn(&str, Value) -> ToolResult) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut length = 0usize;
    let mut host = String::new();
    let mut origin = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = v.trim().parse().unwrap_or(0),
                "host" => host = v.trim().to_string(),
                "origin" => origin = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    if length > MAX_BODY {
        return respond(&mut stream, "413 Payload Too Large", "");
    }
    // Always consume the body before answering: on Windows, closing a socket with unread
    // input sends a reset, and the client would see "connection reset" instead of our reply.
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    let path = path.split('?').next().unwrap_or("");
    let result = if !is_local_host(&host) || origin.as_deref().is_some_and(|o| !is_local_origin(o)) {
        respond(&mut stream, "403 Forbidden", r#"{"error":"only local clients may control Soundesti"}"#)
    } else if path != "/mcp" && path != "/" {
        respond(&mut stream, "404 Not Found", "")
    } else if method != "POST" {
        respond(&mut stream, "405 Method Not Allowed", "")
    } else {
        let body = String::from_utf8_lossy(&body);
        match handle_message(&body, dispatch) {
            Some(reply) => respond(&mut stream, "200 OK", &reply),
            None => respond(&mut stream, "202 Accepted", ""),
        }
    };
    let _ = stream.shutdown(std::net::Shutdown::Write);
    result
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Handles one JSON-RPC message (or batch). Returns `None` for notifications.
pub fn handle_message(body: &str, dispatch: &dyn Fn(&str, Value) -> ToolResult) -> Option<String> {
    let body = body.trim_start_matches('\u{feff}');
    let parsed: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return Some(rpc_error(&Value::Null, -32700, &format!("parse error: {e}")).to_string()),
    };
    if let Value::Array(items) = parsed {
        let replies: Vec<Value> = items.into_iter().filter_map(|m| handle_one(m, dispatch)).collect();
        return (!replies.is_empty()).then(|| Value::Array(replies).to_string());
    }
    handle_one(parsed, dispatch).map(|v| v.to_string())
}

fn handle_one(msg: Value, dispatch: &dyn Fn(&str, Value) -> ToolResult) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let id = id?; // notifications (and responses) need no reply
    let result = match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
            json!({
                "protocolVersion": requested,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "soundesti", "title": "Soundesti DAW", "version": env!("CARGO_PKG_VERSION")},
                "instructions": crate::mcp_tools::INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": crate::mcp_tools::tool_definitions()}),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(rpc_error(&id, -32602, "tools/call needs a tool name"));
            };
            if !crate::mcp_tools::tool_definitions().iter().any(|t| t["name"] == name) {
                return Some(rpc_error(&id, -32602, &format!("unknown tool `{name}`")));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match dispatch(name, args) {
                Ok(ToolOutput::Text(t)) => json!({"content": [{"type": "text", "text": t}], "isError": false}),
                Ok(ToolOutput::Image { png, note }) => json!({
                    "content": [
                        {"type": "image", "data": base64_encode(&png), "mimeType": "image/png"},
                        {"type": "text", "text": note},
                    ],
                    "isError": false
                }),
                Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
            }
        }
        "resources/list" => json!({"resources": []}),
        "prompts/list" => json!({"prompts": []}),
        _ => return Some(rpc_error(&id, -32601, &format!("method not found: {method}"))),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn stop_std_handle_inheritance() {
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: plain Win32 calls on this process's own standard handles.
        unsafe {
            let h = GetStdHandle(which);
            if !h.is_null() {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Sends one JSON-RPC message to the running server. Returns the reply body (None for 202).
fn post_to_server(port: u16, body: &str) -> std::io::Result<Option<String>> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(CALL_TIMEOUT + Duration::from_secs(5)))?;
    write!(
        s,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )?;
    let mut resp = String::new();
    s.read_to_string(&mut resp)?;
    let (head, payload) = resp.split_once("\r\n\r\n").unwrap_or((&resp, ""));
    let status = head.split_whitespace().nth(1).unwrap_or("");
    Ok(match status {
        "200" => Some(payload.to_string()),
        "202" => None,
        other => {
            Some(rpc_error(&Value::Null, -32000, &format!("Soundesti MCP server answered HTTP {other}")).to_string())
        }
    })
}

/// `soundesti.exe --mcp-stdio`: a stdio⇄HTTP bridge for MCP clients that can only launch
/// programs (e.g. Claude Desktop). Reads newline-delimited JSON-RPC from stdin, forwards it
/// to the running app, and writes replies to stdout. If Soundesti isn't running (or AI
/// control is off), it launches Soundesti with `--mcp` once and waits for it.
pub fn run_stdio_bridge(port: u16) -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut launched = false;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        // Some Windows clients prefix their first write with a UTF-8 byte-order mark.
        let line = line.trim_start_matches('\u{feff}').to_string();
        if line.trim().is_empty() {
            continue;
        }
        let mut reply = post_to_server(port, &line);
        if reply.is_err() && !launched {
            launched = true;
            // Our stdin/stdout are the AI client's pipes. Windows children inherit every
            // inheritable handle, so the long-lived app would keep those pipes open and the
            // client would never see this bridge's output end. Make them non-inheritable first.
            stop_std_handle_inheritance();
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe)
                    .arg("--mcp")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            for _ in 0..60 {
                std::thread::sleep(Duration::from_millis(500));
                reply = post_to_server(port, &line);
                if reply.is_ok() {
                    break;
                }
            }
        }
        let out = match reply {
            Ok(Some(body)) => body,
            Ok(None) => continue,
            Err(_) => {
                // Only requests (with an id) get an error reply; notifications are dropped.
                let id = serde_json::from_str::<Value>(&line).ok().and_then(|v| v.get("id").cloned());
                match id {
                    Some(id) => {
                        rpc_error(&id, -32000, "Soundesti is not running or AI control (OPTIONS > AI control) is off")
                            .to_string()
                    }
                    None => continue,
                }
            }
        };
        if writeln!(stdout, "{}", out.trim()).and_then(|_| stdout.flush()).is_err() {
            break;
        }
    }
    0
}

pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo(name: &str, args: Value) -> ToolResult {
        if name == "get_state" {
            Ok(ToolOutput::Text(json!({"ok": true, "args": args}).to_string()))
        } else {
            Err("boom".into())
        }
    }

    fn call_raw(body: &str) -> Value {
        serde_json::from_str(&handle_message(body, &echo).unwrap()).unwrap()
    }

    fn call(body: Value) -> Value {
        serde_json::from_str(&handle_message(&body.to_string(), &echo).unwrap()).unwrap()
    }

    #[test]
    fn initialize_and_list() {
        let r = call(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}));
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "soundesti");
        let r = call(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
        let tools = r["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 15);
        for t in tools {
            assert!(t["name"].is_string() && t["description"].is_string());
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        }
        assert!(handle_message(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &echo).is_none());
    }

    #[test]
    fn tool_calls_and_errors() {
        let r = call(
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_state","arguments":{"x":1}}}),
        );
        assert_eq!(r["result"]["isError"], false);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("\"ok\":true"));
        let r =
            call(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"run_command","arguments":{}}}));
        assert_eq!(r["result"]["isError"], true);
        let r = call(json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}));
        assert_eq!(r["error"]["code"], -32602);
        let r = call(json!({"jsonrpc":"2.0","id":6,"method":"bogus"}));
        assert_eq!(r["error"]["code"], -32601);
        let bom = call_raw("\u{feff}{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}");
        assert!(bom.get("result").is_some(), "a leading BOM must be ignored: {bom}");
        let bad: Value = serde_json::from_str(&handle_message("{not json", &echo).unwrap()).unwrap();
        assert_eq!(bad["error"]["code"], -32700);
    }

    #[test]
    fn host_and_origin_checks() {
        assert!(is_local_host("127.0.0.1:47821"));
        assert!(is_local_host("localhost"));
        assert!(is_local_host("[::1]:47821"));
        assert!(!is_local_host("evil.com"));
        assert!(!is_local_host("127.0.0.1.evil.com:80"));
        assert!(is_local_origin("http://localhost:3000"));
        assert!(!is_local_origin("https://evil.com"));
        assert!(!is_local_origin("null"));
    }

    #[test]
    fn base64_matches_reference() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn real_http_round_trip_and_origin_rejection() {
        let port = 47_900 + (std::process::id() % 50) as u16;
        let srv = McpServer::start(port, || {}).unwrap();
        // The HTTP client runs on a helper thread; this thread plays the UI and answers the
        // tool call, keeping the server alive for the whole test.
        let client = std::thread::spawn(move || {
            let post = |body: &str, origin: Option<&str>| -> String {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                let o = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
                write!(s, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{o}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
                let mut out = String::new();
                s.read_to_string(&mut out).unwrap();
                out
            };
            let ok = post(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_state"}}"#, None);
            let forbidden = post(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#, Some("https://evil.example"));
            (ok, forbidden)
        });
        let call = srv.calls.recv_timeout(Duration::from_secs(5)).expect("tool call forwarded to the UI side");
        assert_eq!(call.name, "get_state");
        call.reply.send(Ok(ToolOutput::Text("{\"bpm\":130}".into()))).unwrap();
        let (ok, forbidden) = client.join().unwrap();
        assert!(ok.starts_with("HTTP/1.1 200") && ok.contains("bpm"), "{ok}");
        assert!(forbidden.starts_with("HTTP/1.1 403"), "{forbidden}");
        drop(srv);
    }
}
