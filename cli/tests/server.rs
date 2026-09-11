//! End-to-end test of the web server: starts `tests/server/app.lipi` on a free
//! port and talks to it over real TCP connections.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

fn request(port: u16, raw: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

fn get(port: u16, path: &str) -> (u16, String, String) {
    request(port, &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"))
}

fn post(port: u16, path: &str, content_type: &str, body: &str) -> (u16, String, String) {
    request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

#[test]
fn server_routes_middleware_and_websockets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_lipi"))
        .args(["run", "tests/server/app.lipi", "0"])
        .current_dir(root)
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start lipi");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    stdout.read_line(&mut first).unwrap();
    let port: u16 = first
        .split("localhost:")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("unexpected startup line: {first}"));

    let (status, head, body) = get(port, "/");
    assert_eq!(status, 200);
    assert!(head.contains("text/html"), "{head}");
    assert_eq!(body, "<h1>Hello from LiPi</h1>");

    let (status, head, body) = get(port, "/users/42");
    assert_eq!(status, 200);
    assert!(head.contains("application/json"));
    assert_eq!(body, r#"{"id":42,"name":"User 42"}"#);

    let (status, _, body) = post(port, "/echo", "application/json", r#"{"a":[1,2]}"#);
    assert_eq!((status, body.as_str()), (201, r#"{"received":{"a":[1,2]}}"#));

    let (_, _, body) = post(port, "/form", "application/x-www-form-urlencoded", "name=Asha+K");
    assert_eq!(body, "Hi Asha K");

    assert_eq!(get(port, "/secret").0, 401);
    let (status, _, body) = request(port, "GET /secret HTTP/1.1\r\nAuthorization: Bearer token123\r\nConnection: close\r\n\r\n");
    assert_eq!((status, body.as_str()), (200, "the treasure"));

    let (_, _, body) = get(port, "/search?q=hello%20world&page=2");
    assert_eq!(body, r#"{"q":"hello world","page":"2"}"#);

    let (_, head, _) = get(port, "/login");
    assert!(head.contains("Set-Cookie: session=abc; Path=/; HttpOnly; SameSite=Lax"), "{head}");

    let (status, _, body) = get(port, "/boom");
    assert_eq!((status, body.as_str()), (500, r#"{"error":"handler failed"}"#));
    assert_eq!(get(port, "/nothing").0, 204);
    assert_eq!(get(port, "/missing").0, 404);
    assert_eq!(post(port, "/users/1", "text/plain", "").0, 405);

    // WebSocket: handshake, then a masked text frame, expect an echo.
    let mut ws = TcpStream::connect(("127.0.0.1", port)).unwrap();
    ws.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    ws.write_all(b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").unwrap();
    let mut reader = BufReader::new(ws.try_clone().unwrap());
    let mut line = String::new();
    let mut handshake = String::new();
    while reader.read_line(&mut line).unwrap() > 2 {
        handshake.push_str(&line);
        line.clear();
    }
    assert!(handshake.contains("101"), "{handshake}");
    assert!(handshake.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="), "{handshake}"); // RFC 6455 example key
    let mask = [1u8, 2, 3, 4];
    let payload: Vec<u8> = b"hi".iter().enumerate().map(|(i, b)| b ^ mask[i % 4]).collect();
    let mut frame = vec![0x81, 0x80 | 2];
    frame.extend_from_slice(&mask);
    frame.extend_from_slice(&payload);
    ws.write_all(&frame).unwrap();
    let mut header = [0u8; 2];
    reader.read_exact(&mut header).unwrap();
    let mut reply = vec![0u8; (header[1] & 0x7f) as usize];
    reader.read_exact(&mut reply).unwrap();
    assert_eq!(String::from_utf8(reply).unwrap(), "echo: hi");

    assert_eq!(get(port, "/stop").2, "bye");
    let status = child.wait().unwrap();
    assert!(status.success());
    let mut err = String::new();
    child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    assert!(err.contains("handler failed"), "the error should be logged: {err}");
}
