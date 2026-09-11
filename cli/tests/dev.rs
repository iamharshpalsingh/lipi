//! `lipi dev`: serves the app, rebuilds on save and tells the page to reload
//! (or shows the build error).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

/// Read from the event stream until `needle` appears.
fn wait_for(stream: &mut TcpStream, seen: &mut String, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut buf = [0u8; 4096];
    while !seen.contains(needle) {
        assert!(Instant::now() < deadline, "timed out waiting for {needle:?}; got:\n{seen}");
        match stream.read(&mut buf) {
            Ok(0) => panic!("the event stream closed; got:\n{seen}"),
            Ok(n) => seen.push_str(&String::from_utf8_lossy(&buf[..n])),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => panic!("{e}"),
        }
    }
}

#[test]
fn dev_server_rebuilds_and_reports_errors() {
    let dir = std::env::temp_dir().join(format!("lipi-dev-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("public")).unwrap();
    std::fs::write(dir.join("app.lipi"), "page \"/\"\n    heading \"Version one\"\n").unwrap();
    std::fs::write(dir.join("public").join("style.css"), "body { color: red; }").unwrap();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_lipi"))
            .args(["dev", "app.lipi", "--port", &port.to_string()])
            .current_dir(&dir)
            .env("NO_COLOR", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "the dev server didn't start");
        std::thread::sleep(Duration::from_millis(100));
    }

    let page = get(port, "/");
    assert!(page.contains("EventSource(\"/__lipi/events\")") && page.contains("<script src=\"app.js\"></script>"), "{page}");
    let js = get(port, "/app.js");
    assert!(js.contains("Version one") && js.contains("$start(0)"), "{js}");
    assert!(get(port, "/style.css").contains("color: red"));
    assert!(get(port, "/../app.lipi").starts_with("HTTP/1.1 404"));

    let mut events = TcpStream::connect(("127.0.0.1", port)).unwrap();
    events.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    write!(events, "GET /__lipi/events HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut seen = String::new();
    wait_for(&mut events, &mut seen, "event: version\ndata: 1\n");

    std::thread::sleep(Duration::from_millis(300));
    std::fs::write(dir.join("app.lipi"), "page \"/\"\n    heading \"Version two\"\n").unwrap();
    wait_for(&mut events, &mut seen, "event: version\ndata: 2\n");
    assert!(get(port, "/app.js").contains("Version two"));

    std::fs::write(dir.join("app.lipi"), "page \"/\"\n    heading usr\n").unwrap();
    wait_for(&mut events, &mut seen, "event: build-error\n");
    wait_for(&mut events, &mut seen, "LIP1002");
    drop(events);
    let _ = std::fs::remove_dir_all(&dir);
}
