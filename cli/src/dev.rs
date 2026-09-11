//! `lipi dev`: builds the web app, serves it on localhost, and rebuilds and
//! reloads the browser whenever a .lipi file (or anything in public/) changes.
//! A build error is printed in the terminal and shown on the page.
//!
//! The page keeps a Server-Sent Events connection to /__lipi/events. The
//! server sends `version` (the build number) and `build-error` events; the
//! page reloads when the version changes.

use lipi_compiler::codegen::{self, Target};
use lipi_runtime::Interpreter;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

#[derive(Default)]
struct Build {
    version: u64,
    js: String,
    error: Option<String>,
}

struct Shared {
    build: Mutex<Build>,
    changed: Condvar,
    html: String,
    public: PathBuf,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Build> {
        self.build.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn run(args: &[String]) -> i32 {
    let mut port: u16 = 3000;
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let value = if a == "--port" || a == "-p" {
            i += 1;
            Some(args.get(i).cloned().unwrap_or_default())
        } else {
            a.strip_prefix("--port=").map(String::from)
        };
        match value {
            Some(v) => match v.parse() {
                Ok(p) => port = p,
                Err(_) => {
                    eprintln!("lipi: --port needs a number, like --port 3000");
                    return 2;
                }
            },
            None => files.push(a.to_string()),
        }
        i += 1;
    }
    let (file, _) = match crate::file_or_entry(&files) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let root = lipi_compiler::resolve::find_project_root(&file);
    let title = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "LiPi app".into());
    let shared = Arc::new(Shared { build: Mutex::new(Build::default()), changed: Condvar::new(), html: crate::web_page(&title, true), public: root.join("public") });
    rebuild(&shared, &file);

    let Some(listener) = (port..=port.saturating_add(20)).find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok()) else {
        eprintln!("lipi: couldn't find a free port starting at {port}. Try another one with --port.");
        return 1;
    };
    let actual = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    println!("LiPi dev server: http://localhost:{actual}/");
    println!("Watching {} for changes. Press Ctrl+C to stop.", root.display());
    let _ = io::stdout().flush();

    let watcher = shared.clone();
    std::thread::spawn(move || watch(&watcher, &file, &root));
    for stream in listener.incoming().flatten() {
        let shared = shared.clone();
        std::thread::spawn(move || {
            let _ = handle(stream, &shared);
        });
    }
    0
}

fn rebuild(shared: &Shared, file: &Path) {
    let started = Instant::now();
    let names = Interpreter::new().builtin_names();
    let builtins: Vec<&str> = names.iter().map(String::as_str).collect();
    let result = codegen::build(file, Target::Web, &builtins);
    let mut b = shared.lock();
    match result {
        Ok(js) => {
            b.js = js;
            b.error = None;
            b.version += 1;
            println!("built {} in {} ms", file.display(), started.elapsed().as_millis());
        }
        Err(e) => {
            eprint!("{}", e.render(crate::color()));
            b.error = Some(e.render(false));
        }
    }
    let _ = io::stdout().flush();
    drop(b);
    shared.changed.notify_all();
}

type Snapshot = Vec<(PathBuf, Option<SystemTime>, u64)>;

fn watch(shared: &Shared, file: &Path, root: &Path) {
    let mut last = snapshot(root);
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let now = snapshot(root);
        if now != last {
            last = now;
            rebuild(shared, file);
        }
    }
}

/// Every .lipi file in the project, and everything in public/.
fn snapshot(root: &Path) -> Snapshot {
    fn walk(dir: &Path, public: &Path, out: &mut Snapshot) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                if !matches!(name.as_str(), "lipi_modules" | "target" | "node_modules" | "dist") {
                    walk(&p, public, out);
                }
            } else if name.ends_with(".lipi") || p.starts_with(public) {
                if let Ok(m) = e.metadata() {
                    out.push((p, m.modified().ok(), m.len()));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &root.join("public"), &mut out);
    out.sort();
    out
}

fn handle(stream: TcpStream, shared: &Shared) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
    }
    let mut parts = request.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let mut stream = stream;
    if method != "GET" {
        return respond(&mut stream, 405, "text/plain", b"only GET is supported");
    }
    let path = target.split(['?', '#']).next().unwrap_or("/");
    match path {
        "/" | "/index.html" => respond(&mut stream, 200, "text/html; charset=utf-8", shared.html.as_bytes()),
        "/app.js" => {
            let js = shared.lock().js.clone();
            respond(&mut stream, 200, "text/javascript; charset=utf-8", js.as_bytes())
        }
        "/__lipi/events" => events(stream, shared),
        other => match public_file(&shared.public, other) {
            Some((bytes, mime)) => respond(&mut stream, 200, mime, &bytes),
            None => respond(&mut stream, 404, "text/plain", b"not found"),
        },
    }
}

fn respond(stream: &mut TcpStream, status: u16, mime: &str, body: &[u8]) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(body)?;
    stream.flush()
}

/// What the page hasn't been told yet.
fn pending(b: &Build, sent_version: &mut Option<u64>, sent_error: &mut Option<String>) -> String {
    let mut msg = String::new();
    if *sent_version != Some(b.version) {
        msg.push_str(&format!("event: version\ndata: {}\n\n", b.version));
        *sent_version = Some(b.version);
    }
    if b.error != *sent_error {
        if let Some(e) = &b.error {
            msg.push_str("event: build-error\n");
            for line in e.lines() {
                msg.push_str("data: ");
                msg.push_str(line);
                msg.push('\n');
            }
            msg.push('\n');
        }
        sent_error.clone_from(&b.error);
    }
    msg
}

fn events(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n")?;
    let (mut sent_version, mut sent_error) = (None, None);
    loop {
        let msg = {
            let mut b = shared.lock();
            let mut msg = pending(&b, &mut sent_version, &mut sent_error);
            if msg.is_empty() {
                b = match shared.changed.wait_timeout(b, Duration::from_secs(15)) {
                    Ok((guard, _)) => guard,
                    Err(e) => e.into_inner().0,
                };
                msg = pending(&b, &mut sent_version, &mut sent_error);
                if msg.is_empty() {
                    // Keeps the connection alive and notices when the page has gone.
                    msg = ": ping\n\n".into();
                }
            }
            msg
        };
        stream.write_all(msg.as_bytes())?;
        stream.flush()?;
    }
}

fn public_file(public: &Path, url: &str) -> Option<(Vec<u8>, &'static str)> {
    let rel = percent_decode(url.trim_start_matches('/'));
    let rel = Path::new(&rel);
    if rel.as_os_str().is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    let path = public.join(rel);
    let bytes = std::fs::read(&path).ok()?;
    Some((bytes, mime(&path)))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_decoded_and_kept_inside_public() {
        assert_eq!(percent_decode("my%20photo.png"), "my photo.png");
        assert!(public_file(Path::new("."), "/../secret.txt").is_none());
        assert!(public_file(Path::new("."), "/").is_none());
    }
}
