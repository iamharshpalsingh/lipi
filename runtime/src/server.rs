//! The web server: HTTP routes, middleware, static files, cookies and WebSockets.
//!
//! ```lipi
//! server.start 3000
//!
//! get "/users/:id" with request
//!     return {id: request.params.id}
//! ```
//!
//! `server.start` opens the port right away, but requests are served only
//! after the rest of the file has run, so routes can be declared after it.
//! Connections are read and written on background threads; Lipi handlers run
//! one at a time on the main thread (like Node.js), so they never race.

use crate::builtins::{callable, int, need, num, opt_int, opt_text, text};
use crate::interp::{Flow, Interpreter, RunError};
use crate::json;
use crate::task;
use crate::value::*;
use base64::Engine;
use lipi_compiler::Span;
use sha1::{Digest, Sha1};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{channel, Sender};
use std::time::Instant;

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

// ----- data crossing threads -------------------------------------------------

struct RawRequest {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    peer: String,
}

impl RawRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

struct RawResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl RawResponse {
    fn json(status: u16, value: serde_json::Value) -> RawResponse {
        RawResponse {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: value.to_string().into_bytes(),
        }
    }
}

enum Event {
    Http { req: RawRequest, reply: Sender<RawResponse> },
    WsOpen { id: u64, req: RawRequest, out: Sender<WsOut> },
    WsMessage { id: u64, text: String },
    WsClose { id: u64 },
}

enum WsOut {
    Text(String),
    Pong(Vec<u8>),
    Close,
}

// ----- server state (main thread) -------------------------------------------------

enum Segment {
    Lit(String),
    Param(String),
    Rest(String),
}

struct Route {
    method: String,
    pattern: Vec<Segment>,
    handler: Value,
    span: Span,
}

struct Socket {
    path: String,
    out: Sender<WsOut>,
    value: Value,
    on_message: Vec<Value>,
    on_close: Vec<Value>,
    span: Span,
}

#[derive(Default)]
pub struct ServerState {
    routes: Vec<Route>,
    ws_routes: Vec<Route>,
    before: Vec<(Value, Span)>,
    statics: Vec<(String, PathBuf)>,
    listener: Option<(TcpListener, u16, String)>,
    sockets: HashMap<u64, Socket>,
    stop: bool,
}

fn parse_pattern(path: &str) -> Vec<Segment> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if let Some(name) = s.strip_prefix(':') {
                Segment::Param(name.to_string())
            } else if let Some(name) = s.strip_prefix('*') {
                Segment::Rest(if name.is_empty() { "rest".to_string() } else { name.to_string() })
            } else {
                Segment::Lit(s.to_string())
            }
        })
        .collect()
}

fn match_pattern(pattern: &[Segment], segments: &[String]) -> Option<Vec<(String, String)>> {
    let mut params = Vec::new();
    for (i, seg) in pattern.iter().enumerate() {
        match seg {
            Segment::Rest(name) => {
                params.push((name.clone(), segments.get(i..).unwrap_or(&[]).join("/")));
                return Some(params);
            }
            Segment::Lit(lit) => {
                if segments.get(i)? != lit {
                    return None;
                }
            }
            Segment::Param(name) => params.push((name.clone(), segments.get(i)?.clone())),
        }
    }
    (segments.len() == pattern.len()).then_some(params)
}

fn percent_decode(s: &str, plus_is_space: bool) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 3;
                    continue;
                }
                _ => out.push(b'%'),
            },
            b'+' if plus_is_space => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_pairs(s: &str, sep: char, plus_is_space: bool) -> Fields {
    let mut out = Fields::new();
    for pair in s.split(sep).map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(percent_decode(k, plus_is_space), Value::string(percent_decode(v, plus_is_space)));
    }
    out
}

fn split_target(target: &str) -> (String, String, Vec<String>) {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let segments: Vec<String> = path.split('/').filter(|s| !s.is_empty()).map(|s| percent_decode(s, false)).collect();
    (path.to_string(), query.to_string(), segments)
}

fn request_value(raw: &RawRequest, path: &str, query: &str, params: Vec<(String, String)>) -> Value {
    let mut headers = Fields::new();
    for (k, v) in &raw.headers {
        headers.insert(k.clone(), Value::text(v));
    }
    let body = String::from_utf8_lossy(&raw.body).into_owned();
    let is_form = raw.header("content-type").is_some_and(|c| c.starts_with("application/x-www-form-urlencoded"));
    let mut f = Fields::new();
    f.insert("method".into(), Value::text(&raw.method));
    f.insert("path".into(), Value::text(path));
    f.insert("params".into(), Value::object(params.into_iter().map(|(k, v)| (k, Value::string(v))).collect()));
    f.insert("query".into(), Value::object(parse_pairs(query, '&', true)));
    f.insert("headers".into(), Value::object(headers));
    f.insert("cookies".into(), Value::object(raw.header("cookie").map(|c| parse_pairs(c, ';', false)).unwrap_or_default()));
    f.insert("form".into(), Value::object(if is_form { parse_pairs(&body, '&', true) } else { Fields::new() }));
    f.insert("ip".into(), Value::text(&raw.peer));
    f.insert("body".into(), Value::text(&body));
    f.insert("json".into(), Value::native("json", move |it, a| json::parse(it, &body, a.span)));
    Value::object(f)
}

fn response_value(status: i64, body: Value, headers: Value) -> Value {
    let mut f = Fields::new();
    f.insert("status".into(), Value::Int(status));
    f.insert("body".into(), body);
    f.insert("headers".into(), if matches!(headers, Value::Nil) { Value::object(Fields::new()) } else { headers });
    Value::Object(Rc::new(ObjectData { fields: RefCell::new(f), ty: None, module: None, tag: Some("response"), payload: None }))
}

fn check_path(it: &Interpreter, a: &Args, path: &str) -> Result<(), Flow> {
    if path.starts_with('/') {
        return Ok(());
    }
    Err(it.error(format!("route paths start with /, like \"/{path}\""), a.span, None))
}

fn route_native(method: &'static str) -> Value {
    Value::native(&method.to_lowercase(), move |it, a| {
        let path = text(it, a, 0, "path")?;
        check_path(it, a, &path)?;
        let handler = callable(it, a, 1, "handler")?;
        it.server.routes.push(Route { method: method.to_string(), pattern: parse_pattern(&path), handler, span: a.span });
        Ok(Value::Nil)
    })
}

/// `get`, `post`, ... as global functions.
pub fn route_globals() -> Vec<(&'static str, Value)> {
    vec![
        ("get", route_native("GET")),
        ("post", route_native("POST")),
        ("put", route_native("PUT")),
        ("patch", route_native("PATCH")),
        ("delete", route_native("DELETE")),
    ]
}

fn outgoing_text(it: &Interpreter, v: &Value, span: Span) -> Result<String, Flow> {
    match v {
        Value::Str(s) => Ok(s.to_string()),
        other => json::stringify(it, other, false, span),
    }
}

pub fn entries() -> Vec<(&'static str, Value)> {
    let mut entries = route_globals();
    entries.push((
        "route",
        Value::native("route", |it, a| {
            let method = text(it, a, 0, "method")?.to_uppercase();
            let path = text(it, a, 1, "path")?;
            check_path(it, a, &path)?;
            let handler = callable(it, a, 2, "handler")?;
            it.server.routes.push(Route { method, pattern: parse_pattern(&path), handler, span: a.span });
            Ok(Value::Nil)
        }),
    ));
    entries.push((
        "before",
        Value::native("before", |it, a| {
            let handler = callable(it, a, 0, "handler")?;
            it.server.before.push((handler, a.span));
            Ok(Value::Nil)
        }),
    ));
    entries.push((
        "static",
        Value::native("static", |it, a| {
            let prefix = text(it, a, 0, "path")?;
            check_path(it, a, &prefix)?;
            let dir = PathBuf::from(&*text(it, a, 1, "folder")?);
            if !dir.is_dir() {
                return Err(it.error(format!("the folder `{}` doesn't exist", dir.display()), a.span, None));
            }
            it.server.statics.push((prefix.trim_end_matches('/').to_string(), dir));
            Ok(Value::Nil)
        }),
    ));
    entries.push((
        "websocket",
        Value::native("websocket", |it, a| {
            let path = text(it, a, 0, "path")?;
            check_path(it, a, &path)?;
            let handler = callable(it, a, 1, "handler")?;
            it.server.ws_routes.push(Route { method: "WS".into(), pattern: parse_pattern(&path), handler, span: a.span });
            Ok(Value::Nil)
        }),
    ));
    entries.push((
        "start",
        Value::native("start", |it, a| {
            if it.server.listener.is_some() {
                return Err(it.error("the server was already started", a.span, None));
            }
            let port = num(it, a, 0, "port")?;
            if port.fract() != 0.0 || !(0.0..=65535.0).contains(&port) {
                return Err(it.error("a port is a whole number from 1 to 65535", a.span, Some("Web servers often use 3000 or 8080.".into())));
            }
            let host = opt_text(it, a, 1, "host")?.map(|h| h.to_string()).unwrap_or_else(|| "127.0.0.1".into());
            let listener = TcpListener::bind((host.as_str(), port as u16)).map_err(|e| {
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    it.error(
                        format!("port {port} is already in use"),
                        a.span,
                        Some(format!("Another program is using it. Stop that program or pick another port, like {}.", port + 1.0)),
                    )
                } else {
                    it.error(format!("couldn't open port {port}: {e}"), a.span, None)
                }
            })?;
            let actual = listener.local_addr().map(|addr| addr.port()).unwrap_or(port as u16);
            it.server.listener = Some((listener, actual, host));
            Ok(Value::Int(actual as i64))
        }),
    ));
    entries.push((
        "respond",
        Value::native("respond", |it, a| {
            let status = int(it, a, 0, "status")?;
            Ok(response_value(status, a.get(1, "body").cloned().unwrap_or(Value::Nil), a.get(2, "headers").cloned().unwrap_or(Value::Nil)))
        }),
    ));
    entries.push((
        "redirect",
        Value::native("redirect", |it, a| {
            let url = text(it, a, 0, "url")?;
            let status = opt_int(it, a, 1, "status")?.unwrap_or(302);
            let mut h = Fields::new();
            h.insert("Location".into(), Value::Str(url));
            Ok(response_value(status, Value::Nil, Value::object(h)))
        }),
    ));
    entries.push((
        "cookie",
        Value::native("cookie", |it, a| {
            let name = text(it, a, 0, "name")?;
            let value = need(it, a, 1, "value")?.display();
            let mut cookie = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax");
            if let Some(Value::Object(o)) = a.get(2, "options") {
                let o = o.fields.borrow();
                if let Some(age) = o.get("maxAge").and_then(Value::as_f64) {
                    cookie.push_str(&format!("; Max-Age={}", age as i64));
                }
                if o.get("secure").is_some_and(Value::truthy) {
                    cookie.push_str("; Secure");
                }
                if o.get("httpOnly").is_some_and(|v| !v.truthy()) {
                    cookie = cookie.replace("; HttpOnly", "");
                }
            }
            Ok(Value::string(cookie))
        }),
    ));
    entries.push((
        "broadcast",
        Value::native("broadcast", |it, a| {
            let path = text(it, a, 0, "path")?;
            let message = outgoing_text(it, need(it, a, 1, "message")?, a.span)?;
            let mut sent = 0;
            for s in it.server.sockets.values().filter(|s| *s.path == *path) {
                if s.out.send(WsOut::Text(message.clone())).is_ok() {
                    sent += 1;
                }
            }
            Ok(Value::Int(sent))
        }),
    ));
    entries.push((
        "stop",
        Value::native("stop", |it, _| {
            it.server.stop = true;
            Ok(Value::Nil)
        }),
    ));
    entries
}

// ----- the event loop (main thread) ----------------------------------------------

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

impl Interpreter {
    /// Serve requests if the program called `server.start`. Blocks until `server.stop()`.
    pub fn serve(&mut self, color: bool) -> Result<(), RunError> {
        let Some((listener, port, host)) = self.server.listener.take() else { return Ok(()) };
        if let Some(main) = self.main_file.clone() {
            self.file = main;
        }
        let shown = if host == "127.0.0.1" || host == "0.0.0.0" { "localhost".to_string() } else { host };
        println!("LiPi server running at http://{shown}:{port}  (press Ctrl+C to stop)");
        let (tx, rx) = channel::<Event>();
        std::thread::spawn(move || {
            let mut next_id = 0u64;
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                next_id += 1;
                let (tx, id) = (tx.clone(), next_id);
                std::thread::spawn(move || handle_connection(stream, tx, id));
            }
        });
        while let Ok(event) = rx.recv() {
            match event {
                Event::Http { req, reply } => {
                    let response = self.handle_http(req, color)?;
                    let _ = reply.send(response);
                }
                Event::WsOpen { id, req, out } => self.ws_open(id, req, out, color)?,
                Event::WsMessage { id, text } => {
                    let Some(s) = self.server.sockets.get(&id) else { continue };
                    let (handlers, value, span) = (s.on_message.clone(), s.value.clone(), s.span);
                    for h in handlers {
                        let r = self.call_callback(&h, vec![Value::string(text.clone()), value.clone()], span);
                        self.report(r.map(|_| ()), color)?;
                    }
                }
                Event::WsClose { id } => {
                    if let Some(s) = self.server.sockets.remove(&id) {
                        for h in s.on_close {
                            let r = self.call_callback(&h, vec![s.value.clone()], s.span);
                            self.report(r.map(|_| ()), color)?;
                        }
                    }
                }
            }
            if self.server.stop {
                break;
            }
        }
        Ok(())
    }

    /// Print a handler's error without stopping the server. Only `process.exit` stops it.
    fn report(&self, result: Result<(), Flow>, color: bool) -> Result<(), RunError> {
        match result {
            Err(Flow::Throw(t)) => {
                eprint!("{}", self.render(&RunError::Runtime(t), color));
                Ok(())
            }
            Err(Flow::Exit(code)) => Err(RunError::Exit(code)),
            _ => Ok(()),
        }
    }

    /// Call a handler; a returned task is awaited.
    fn run_handler(&mut self, handler: &Value, arg: Value, span: Span) -> Result<Value, Flow> {
        match self.call_callback(handler, vec![arg], span)? {
            Value::Task(t) => task::await_task(self, &t, span, None),
            v => Ok(v),
        }
    }

    fn handle_http(&mut self, req: RawRequest, color: bool) -> Result<RawResponse, RunError> {
        let started = Instant::now();
        let (path, query, segments) = split_target(&req.target);
        let request = request_value(&req, &path, &query, Vec::new());
        let response = self.dispatch(&req, &path, &segments, request, color)?;
        println!("{} {} {} {:.1}ms", req.method, path, response.status, started.elapsed().as_secs_f64() * 1000.0);
        Ok(response)
    }

    fn dispatch(&mut self, req: &RawRequest, path: &str, segments: &[String], request: Value, color: bool) -> Result<RawResponse, RunError> {
        let before: Vec<(Value, Span)> = self.server.before.clone();
        for (handler, span) in before {
            match self.run_handler(&handler, request.clone(), span) {
                Ok(Value::Nil) => {}
                Ok(v) => return Ok(self.to_raw(&v, span)),
                Err(e) => return self.error_response(e, color),
            }
        }
        let method = if req.method == "HEAD" { "GET" } else { req.method.as_str() };
        let mut other_methods = Vec::new();
        let mut found = None;
        for (i, route) in self.server.routes.iter().enumerate() {
            if let Some(params) = match_pattern(&route.pattern, segments) {
                if route.method == method {
                    found = Some((i, params));
                    break;
                }
                other_methods.push(route.method.clone());
            }
        }
        if let Some((i, params)) = found {
            if let Value::Object(o) = &request {
                let params: Fields = params.into_iter().map(|(k, v)| (k, Value::string(v))).collect();
                o.fields.borrow_mut().insert("params".into(), Value::object(params));
            }
            let (handler, span) = (self.server.routes[i].handler.clone(), self.server.routes[i].span);
            return match self.run_handler(&handler, request, span) {
                Ok(v) => Ok(self.to_raw(&v, span)),
                Err(e) => self.error_response(e, color),
            };
        }
        if method == "GET" {
            if let Some(file) = self.static_file(segments) {
                return Ok(file);
            }
        }
        if !other_methods.is_empty() {
            let mut r = RawResponse::json(405, serde_json::json!({ "error": format!("{} isn't allowed on {path}", req.method) }));
            r.headers.push(("Allow".into(), other_methods.join(", ")));
            return Ok(r);
        }
        Ok(RawResponse::json(404, serde_json::json!({ "error": format!("Not found: {} {path}", req.method) })))
    }

    fn static_file(&self, segments: &[String]) -> Option<RawResponse> {
        let path = format!("/{}", segments.join("/"));
        for (prefix, dir) in &self.server.statics {
            let rest = if prefix.is_empty() {
                path.as_str()
            } else if path == *prefix || path.starts_with(&format!("{prefix}/")) {
                &path[prefix.len()..]
            } else {
                continue;
            };
            let rel = Path::new(rest.trim_start_matches('/'));
            if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
                continue; // no "..", no absolute paths
            }
            let mut file = dir.join(rel);
            if file.is_dir() {
                file = file.join("index.html");
            }
            if let Ok(body) = std::fs::read(&file) {
                return Some(RawResponse { status: 200, headers: vec![("Content-Type".into(), content_type(&file).into())], body });
            }
        }
        None
    }

    fn error_response(&self, e: Flow, color: bool) -> Result<RawResponse, RunError> {
        match e {
            Flow::Throw(t) => {
                let message = t.diag.message.clone();
                eprint!("{}", self.render(&RunError::Runtime(t), color));
                Ok(RawResponse::json(500, serde_json::json!({ "error": message })))
            }
            Flow::Exit(code) => Err(RunError::Exit(code)),
            _ => Ok(RawResponse { status: 204, headers: Vec::new(), body: Vec::new() }),
        }
    }

    /// Turn a handler's return value into an HTTP response.
    fn to_raw(&self, v: &Value, span: Span) -> RawResponse {
        let (status, body, headers) = match v {
            Value::Object(o) if o.tag == Some("response") => {
                let f = o.fields.borrow();
                let status = f.get("status").and_then(Value::as_f64).map_or(200, |n| n as u16);
                (status, f.get("body").cloned().unwrap_or(Value::Nil), f.get("headers").cloned().unwrap_or(Value::Nil))
            }
            Value::Nil => (204, Value::Nil, Value::Nil),
            other => (200, other.clone(), Value::Nil),
        };
        let mut out_headers: Vec<(String, String)> = match &headers {
            Value::Object(h) => h.fields.borrow().iter().map(|(k, v)| (k.clone(), v.display())).collect(),
            _ => Vec::new(),
        };
        let (bytes, kind) = match &body {
            Value::Nil => (Vec::new(), None),
            Value::Str(s) => {
                let html = s.trim_start().starts_with('<');
                (s.as_bytes().to_vec(), Some(if html { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" }))
            }
            other => match json::stringify(self, other, false, span) {
                Ok(text) => (text.into_bytes(), Some("application/json")),
                Err(_) => return RawResponse::json(500, serde_json::json!({ "error": "the response couldn't be converted to JSON" })),
            },
        };
        if let Some(kind) = kind {
            if !out_headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                out_headers.push(("Content-Type".into(), kind.into()));
            }
        }
        RawResponse { status, headers: out_headers, body: bytes }
    }

    fn ws_open(&mut self, id: u64, req: RawRequest, out: Sender<WsOut>, color: bool) -> Result<(), RunError> {
        let (path, query, segments) = split_target(&req.target);
        let found = self.server.ws_routes.iter().find_map(|r| match_pattern(&r.pattern, &segments).map(|p| (r.handler.clone(), r.span, p)));
        let Some((handler, span, params)) = found else {
            let _ = out.send(WsOut::Close);
            return Ok(());
        };
        let request = request_value(&req, &path, &query, params);
        let mut f = Fields::new();
        f.insert("id".into(), Value::Int(id as i64));
        f.insert("path".into(), Value::text(&path));
        f.insert("request".into(), request);
        let send_out = out.clone();
        f.insert(
            "send".into(),
            Value::native("send", move |it, a| {
                let message = outgoing_text(it, need(it, a, 0, "message")?, a.span)?;
                let _ = send_out.send(WsOut::Text(message));
                Ok(Value::Nil)
            }),
        );
        let close_out = out.clone();
        f.insert(
            "close".into(),
            Value::native("close", move |_, _| {
                let _ = close_out.send(WsOut::Close);
                Ok(Value::Nil)
            }),
        );
        f.insert(
            "on".into(),
            Value::native("on", move |it, a| {
                let event = text(it, a, 0, "event")?;
                let handler = callable(it, a, 1, "handler")?;
                let Some(s) = it.server.sockets.get_mut(&id) else { return Ok(Value::Nil) };
                match &*event {
                    "message" => s.on_message.push(handler),
                    "close" => s.on_close.push(handler),
                    other => {
                        return Err(it.error(format!("sockets have no `{other}` event"), a.span, Some("Use \"message\" or \"close\".".into())))
                    }
                }
                Ok(Value::Nil)
            }),
        );
        let socket = Value::object(f);
        self.server.sockets.insert(id, Socket { path, out, value: socket.clone(), on_message: Vec::new(), on_close: Vec::new(), span });
        let r = self.run_handler(&handler, socket, span);
        self.report(r.map(|_| ()), color)
    }
}

// ----- connection threads ------------------------------------------------------

fn read_request(reader: &mut BufReader<TcpStream>, peer: &str) -> Result<Option<RawRequest>, u16> {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return Ok(None),
            Ok(_) if line.trim().is_empty() => continue,
            Ok(_) => break,
        }
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else { return Err(400) };
    let (method, target) = (method.to_uppercase(), target.to_string());
    let mut headers = Vec::new();
    let mut total = line.len();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|_| 400u16)?;
        total += n;
        if total > MAX_HEADER_BYTES {
            return Err(431);
        }
        let l = line.trim_end();
        if n == 0 || l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_lowercase(), v.trim().to_string()));
        }
    }
    let find = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v): &(String, String)| v.clone());
    let mut body = Vec::new();
    if find("transfer-encoding").is_some_and(|t| t.eq_ignore_ascii_case("chunked")) {
        loop {
            line.clear();
            reader.read_line(&mut line).map_err(|_| 400u16)?;
            let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16).map_err(|_| 400u16)?;
            if size == 0 {
                line.clear();
                let _ = reader.read_line(&mut line);
                break;
            }
            if body.len() + size > MAX_BODY_BYTES {
                return Err(413);
            }
            let mut chunk = vec![0; size + 2];
            reader.read_exact(&mut chunk).map_err(|_| 400u16)?;
            body.extend_from_slice(&chunk[..size]);
        }
    } else if let Some(len) = find("content-length") {
        let len: usize = len.parse().map_err(|_| 400u16)?;
        if len > MAX_BODY_BYTES {
            return Err(413);
        }
        body = vec![0; len];
        reader.read_exact(&mut body).map_err(|_| 400u16)?;
    }
    Ok(Some(RawRequest { method, target, headers, body, peer: peer.to_string() }))
}

fn reason(status: u16) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}

fn write_response(stream: &mut TcpStream, resp: &RawResponse, keep_alive: bool, head_only: bool) -> std::io::Result<()> {
    let mut out = format!("HTTP/1.1 {} {}\r\n", resp.status, reason(resp.status));
    for (k, v) in &resp.headers {
        if k.contains(['\r', '\n']) || v.contains(['\r', '\n']) {
            continue; // refuse header injection
        }
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n", resp.body.len()));
    out.push_str("X-Content-Type-Options: nosniff\r\nServer: lipi\r\n");
    out.push_str(if keep_alive { "Connection: keep-alive\r\n\r\n" } else { "Connection: close\r\n\r\n" });
    stream.write_all(out.as_bytes())?;
    if !head_only {
        stream.write_all(&resp.body)?;
    }
    stream.flush()
}

fn handle_connection(stream: TcpStream, tx: Sender<Event>, id: u64) {
    let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read_half);
    let mut stream = stream;
    loop {
        let req = match read_request(&mut reader, &peer) {
            Ok(Some(r)) => r,
            Ok(None) => return,
            Err(status) => {
                let resp = RawResponse::json(status, serde_json::json!({ "error": reason(status) }));
                let _ = write_response(&mut stream, &resp, false, false);
                return;
            }
        };
        let upgrade = req.header("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
        if upgrade {
            websocket_session(stream, reader, req, tx, id);
            return;
        }
        let keep_alive = !req.header("connection").is_some_and(|c| c.eq_ignore_ascii_case("close"));
        let head_only = req.method == "HEAD";
        let (reply_tx, reply_rx) = channel();
        if tx.send(Event::Http { req, reply: reply_tx }).is_err() {
            return;
        }
        let Ok(resp) = reply_rx.recv() else { return };
        if write_response(&mut stream, &resp, keep_alive, head_only).is_err() || !keep_alive {
            return;
        }
    }
}

fn read_frame(r: &mut impl Read) -> std::io::Result<(bool, u8, Vec<u8>)> {
    let mut h = [0u8; 2];
    r.read_exact(&mut h)?;
    let fin = h[0] & 0x80 != 0;
    let opcode = h[0] & 0x0f;
    let masked = h[1] & 0x80 != 0;
    let mut len = (h[1] & 0x7f) as u64;
    if len == 126 {
        let mut b = [0u8; 2];
        r.read_exact(&mut b)?;
        len = u16::from_be_bytes(b) as u64;
    } else if len == 127 {
        let mut b = [0u8; 8];
        r.read_exact(&mut b)?;
        len = u64::from_be_bytes(b);
    }
    if len > MAX_BODY_BYTES as u64 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut mask = [0u8; 4];
    if masked {
        r.read_exact(&mut mask)?;
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    Ok((fin, opcode, payload))
}

fn write_frame(w: &mut impl Write, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut header = vec![0x80 | opcode];
    match payload.len() {
        n if n < 126 => header.push(n as u8),
        n if n <= u16::MAX as usize => {
            header.push(126);
            header.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            header.push(127);
            header.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    w.write_all(&header)?;
    w.write_all(payload)?;
    w.flush()
}

fn websocket_session(mut stream: TcpStream, mut reader: BufReader<TcpStream>, req: RawRequest, tx: Sender<Event>, id: u64) {
    let Some(key) = req.header("sec-websocket-key") else {
        let resp = RawResponse::json(400, serde_json::json!({ "error": "missing Sec-WebSocket-Key" }));
        let _ = write_response(&mut stream, &resp, false, false);
        return;
    };
    let accept = base64::engine::general_purpose::STANDARD.encode(Sha1::digest(format!("{key}{WEBSOCKET_GUID}").as_bytes()));
    let handshake = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n");
    if stream.write_all(handshake.as_bytes()).is_err() {
        return;
    }
    let (out_tx, out_rx) = channel::<WsOut>();
    let mut writer = stream;
    std::thread::spawn(move || {
        for message in out_rx {
            let result = match message {
                WsOut::Text(t) => write_frame(&mut writer, 0x1, t.as_bytes()),
                WsOut::Pong(p) => write_frame(&mut writer, 0xA, &p),
                WsOut::Close => {
                    let _ = write_frame(&mut writer, 0x8, &[]);
                    let _ = writer.shutdown(Shutdown::Both);
                    break;
                }
            };
            if result.is_err() {
                break;
            }
        }
    });
    if tx.send(Event::WsOpen { id, req, out: out_tx.clone() }).is_err() {
        return;
    }
    let mut message = Vec::new();
    while let Ok((fin, opcode, data)) = read_frame(&mut reader) {
        match opcode {
            0x0..=0x2 => {
                message.extend_from_slice(&data);
                if fin {
                    let text = String::from_utf8_lossy(&std::mem::take(&mut message)).into_owned();
                    if tx.send(Event::WsMessage { id, text }).is_err() {
                        break;
                    }
                }
            }
            0x8 => {
                let _ = out_tx.send(WsOut::Close);
                break;
            }
            0x9 => {
                let _ = out_tx.send(WsOut::Pong(data));
            }
            _ => {}
        }
    }
    let _ = tx.send(Event::WsClose { id });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_patterns() {
        let segs = |s: &str| s.split('/').filter(|x| !x.is_empty()).map(String::from).collect::<Vec<_>>();
        let p = parse_pattern("/users/:id");
        assert_eq!(match_pattern(&p, &segs("/users/42")), Some(vec![("id".into(), "42".into())]));
        assert_eq!(match_pattern(&p, &segs("/users")), None);
        assert_eq!(match_pattern(&p, &segs("/users/1/posts")), None);
        let rest = parse_pattern("/files/*path");
        assert_eq!(match_pattern(&rest, &segs("/files/a/b.txt")), Some(vec![("path".into(), "a/b.txt".into())]));
        assert_eq!(match_pattern(&parse_pattern("/"), &segs("/")), Some(vec![]));
    }

    #[test]
    fn decoding() {
        assert_eq!(percent_decode("a%20b+c", true), "a b c");
        assert_eq!(percent_decode("100%", false), "100%");
        let q = parse_pairs("name=Dezy&city=New+Delhi", '&', true);
        assert_eq!(q.get("city").unwrap().display(), "New Delhi");
    }

    #[test]
    fn websocket_frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, 0x1, b"hello").unwrap();
        let (fin, op, data) = read_frame(&mut buf.as_slice()).unwrap();
        assert!(fin);
        assert_eq!(op, 1);
        assert_eq!(data, b"hello");
    }
}
