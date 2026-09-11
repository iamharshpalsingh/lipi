//! The HTTP client.
//!
//! Requests run on a background thread and return a task, so `await` works and
//! several requests can be in flight at once. The transport is the system
//! `curl` program (built into Windows 10+, macOS and most Linux systems),
//! which gives HTTPS without native build dependencies. The Lipi-facing API
//! doesn't depend on this, so the transport can later be swapped for a
//! built-in client.

use crate::builtins::text;
use crate::interp::{Flow, Interpreter};
use crate::json;
use crate::task::{self, SendValue};
use crate::value::{Args, Value};
use std::io::Write;
use std::process::{Command, Stdio};

struct Request {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    timeout_ms: u64,
}

/// `http.get(url, options)`, `http.post(url, body, options)`, ...
pub fn call(it: &mut Interpreter, a: &Args, method: &str, has_body: bool) -> Result<Value, Flow> {
    let url = text(it, a, 0, "url")?.to_string();
    let (body, opts_index) = if has_body { (a.get(1, "body").cloned(), 2) } else { (None, 1) };
    let options = a.get(opts_index, "options").cloned();
    start(it, a, method.to_string(), url, body, options)
}

/// `http.request({method, url, headers, body, timeout})`
pub fn request(it: &mut Interpreter, a: &Args) -> Result<Value, Flow> {
    let Some(Value::Object(o)) = a.get(0, "options").cloned() else {
        return Err(it.error(
            "http.request() needs an object",
            a.span,
            Some("For example: http.request({method: \"GET\", url: \"https://example.com\"})".into()),
        ));
    };
    let fields = o.fields.borrow().clone();
    let method = fields.get("method").map(|m| m.display().to_uppercase()).unwrap_or_else(|| "GET".into());
    let Some(url) = fields.get("url").map(|u| u.display()) else {
        return Err(it.error("http.request() needs a `url`", a.span, None));
    };
    let body = fields.get("body").cloned();
    start(it, a, method, url, body, Some(Value::Object(o)))
}

fn start(it: &mut Interpreter, a: &Args, method: String, mut url: String, body: Option<Value>, options: Option<Value>) -> Result<Value, Flow> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(it.error(
            format!("\"{url}\" isn't a full web address"),
            a.span,
            Some("HTTP requests need a full URL that starts with https:// or http://, like \"https://api.example.com/users\".".into()),
        ));
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut timeout_ms = 30_000u64;
    match &options {
        None | Some(Value::Nil) => {}
        Some(Value::Object(o)) => {
            let fields = o.fields.borrow();
            if let Some(Value::Object(h)) = fields.get("headers") {
                for (k, v) in h.fields.borrow().iter() {
                    headers.push((k.clone(), v.display()));
                }
            }
            if let Some(t) = fields.get("timeout").and_then(Value::as_f64) {
                timeout_ms = t.max(1.0) as u64;
            }
            if let Some(Value::Object(q)) = fields.get("query") {
                let query: Vec<String> =
                    q.fields.borrow().iter().map(|(k, v)| format!("{}={}", encode(k), encode(&v.display()))).collect();
                if !query.is_empty() {
                    url.push(if url.contains('?') { '&' } else { '?' });
                    url.push_str(&query.join("&"));
                }
            }
        }
        Some(other) => {
            return Err(it.error(
                format!("HTTP options should be an object, not {}", lipi_compiler::checker::with_article(&other.type_name())),
                a.span,
                Some("For example: {headers: {Authorization: \"Bearer ...\"}, timeout: 5000}".into()),
            ))
        }
    }
    let has_content_type = headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
    let body = match body {
        None | Some(Value::Nil) => None,
        Some(Value::Str(s)) => {
            if !has_content_type {
                headers.push(("Content-Type".into(), "text/plain; charset=utf-8".into()));
            }
            Some(s.as_bytes().to_vec())
        }
        Some(v) => {
            if !has_content_type {
                headers.push(("Content-Type".into(), "application/json".into()));
            }
            Some(json::stringify(it, &v, false, a.span)?.into_bytes())
        }
    };
    let req = Request { method, url, headers, body, timeout_ms };
    Ok(task::spawn(to_response, move || perform(req)))
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Adds `response.json()` to the response object.
fn to_response(_: &mut Interpreter, v: SendValue) -> Value {
    let value = v.into_value();
    if let Value::Object(o) = &value {
        let body = o.fields.borrow().get("body").map(|b| b.display()).unwrap_or_default();
        o.fields.borrow_mut().insert("json".into(), Value::native("json", move |it, a| json::parse(it, &body, a.span)));
    }
    value
}

fn perform(req: Request) -> Result<SendValue, String> {
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "-L", "-i", "--compressed", "-X", &req.method])
        .arg("--max-time")
        .arg(format!("{:.3}", req.timeout_ms as f64 / 1000.0));
    for (k, v) in &req.headers {
        cmd.arg("-H").arg(format!("{k}: {v}"));
    }
    if req.body.is_some() {
        cmd.args(["--data-binary", "@-"]);
    }
    cmd.arg(&req.url).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "HTTP requests need the `curl` program, which wasn't found on this computer".to_string()
        } else {
            format!("couldn't start the HTTP request: {e}")
        }
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        if let Some(body) = &req.body {
            let _ = stdin.write_all(body);
        }
    }
    let out = child.wait_with_output().map_err(|e| format!("the HTTP request failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr.trim().trim_start_matches("curl: ").to_string();
        return Err(match out.status.code() {
            Some(28) => format!("the request to {} timed out after {} ms", req.url, req.timeout_ms),
            Some(6) => format!("couldn't reach {}: the address wasn't found. Check the URL and your internet connection", req.url),
            Some(7) => format!("couldn't connect to {}: is the server running?", req.url),
            _ => format!("the request to {} failed: {reason}", req.url),
        });
    }
    parse_response(&out.stdout, &req.url)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn parse_response(raw: &[u8], url: &str) -> Result<SendValue, String> {
    let mut rest = raw;
    loop {
        let (head, body) = match find(rest, b"\r\n\r\n") {
            Some(i) => (&rest[..i], &rest[i + 4..]),
            None => (rest, &rest[rest.len()..]),
        };
        let head = String::from_utf8_lossy(head);
        let mut lines = head.lines();
        let status: f64 = lines
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("the response from {url} wasn't valid HTTP"))?;
        // Redirects and "100 Continue" are followed by another header block.
        if (status < 200.0 || (300.0..400.0).contains(&status)) && body.starts_with(b"HTTP/") {
            rest = body;
            continue;
        }
        let headers: Vec<(String, SendValue)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_lowercase(), SendValue::Str(v.trim().to_string())))
            .collect();
        return Ok(SendValue::Obj(vec![
            ("status".into(), SendValue::Int(status as i64)),
            ("ok".into(), SendValue::Bool((200.0..300.0).contains(&status))),
            ("url".into(), SendValue::Str(url.to_string())),
            ("headers".into(), SendValue::Obj(headers)),
            ("body".into(), SendValue::Str(String::from_utf8_lossy(body).into_owned())),
        ]));
    }
}
