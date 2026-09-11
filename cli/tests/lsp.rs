//! Drives `lipi lsp` over the real Language Server Protocol.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};

struct Client {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Client {
    fn send(&mut self, msg: Value) {
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn read(&mut self) -> Value {
        let mut len = 0;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            let l = line.trim_end();
            if l.is_empty() {
                break;
            }
            if let Some(v) = l.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut buf = vec![0; len];
        self.stdout.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let msg = self.read();
            if msg["id"] == json!(id) {
                return msg["result"].clone();
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn diagnostics(&mut self) -> Vec<Value> {
        loop {
            let msg = self.read();
            if msg["method"] == "textDocument/publishDiagnostics" {
                return msg["params"]["diagnostics"].as_array().unwrap().clone();
            }
        }
    }

    fn open(&mut self, uri: &str, text: &str) -> Vec<Value> {
        self.notify("textDocument/didOpen", json!({"textDocument": {"uri": uri, "languageId": "lipi", "version": 1, "text": text}}));
        self.diagnostics()
    }

    fn change(&mut self, uri: &str, text: &str) -> Vec<Value> {
        self.notify("textDocument/didChange", json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": text}]}));
        self.diagnostics()
    }
}

fn at(uri: &str, line: u32, character: u32) -> Value {
    json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}})
}

fn labels(result: &Value) -> Vec<String> {
    result["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_string()).collect()
}

#[test]
fn language_server_features() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lipi")).arg("lsp").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("start lipi lsp");
    let mut c = Client { stdin: child.stdin.take().unwrap(), stdout: BufReader::new(child.stdout.take().unwrap()), next_id: 0 };

    let init = c.request("initialize", json!({"capabilities": {}}));
    assert_eq!(init["capabilities"]["completionProvider"]["triggerCharacters"], json!(["."]));
    c.notify("initialized", json!({}));

    let uri = "file:///C:/lipi-lsp-test/main.lipi";
    let diags = c.open(uri, "age = \"twenty\"\nprice = age + 10\nadd(a, b)\n    return a + b\nshow ad\n");
    let codes: Vec<&str> = diags.iter().map(|d| d["code"].as_str().unwrap()).collect();
    assert!(codes.contains(&"LIP2001") && codes.contains(&"LIP1002"), "{diags:?}");
    let type_error = diags.iter().find(|d| d["code"] == "LIP2001").unwrap();
    assert_eq!(type_error["range"]["start"], json!({"line": 1, "character": 8}));
    assert!(type_error["message"].as_str().unwrap().contains("Hint:"));

    let names = labels(&c.request("textDocument/completion", at(uri, 4, 7)));
    assert!(names.contains(&"add".to_string()) && names.contains(&"show".to_string()) && names.contains(&"toNumber".to_string()));

    c.change(uri, "x = math.sq\n");
    let members = labels(&c.request("textDocument/completion", at(uri, 0, 11)));
    assert!(members.contains(&"sqrt".to_string()) && !members.contains(&"show".to_string()), "{members:?}");
    let hover = c.request("textDocument/hover", at(uri, 0, 9));
    assert!(hover.is_null() || hover["contents"]["value"].as_str().is_some());

    let diags = c.change(uri, "# Adds two numbers.\nadd(a, b)\n    return a + b\nshow add(1, 2)\nshow math.sqrt(16)\n");
    assert!(diags.is_empty(), "{diags:?}");
    let hover = c.request("textDocument/hover", at(uri, 4, 11));
    assert!(hover["contents"]["value"].as_str().unwrap().contains("Decimal"), "{hover}");
    let hover = c.request("textDocument/hover", at(uri, 3, 6));
    assert!(hover["contents"]["value"].as_str().unwrap().contains("Adds two numbers."), "{hover}");
    let def = c.request("textDocument/definition", at(uri, 3, 6));
    assert_eq!(def["range"]["start"], json!({"line": 1, "character": 0}));
    let refs = c.request("textDocument/references", at(uri, 3, 6));
    assert_eq!(refs.as_array().unwrap().len(), 2);
    let symbols = c.request("textDocument/documentSymbol", json!({"textDocument": {"uri": uri}}));
    assert_eq!(symbols[0]["name"], "add");

    c.change(uri, "x=1+2\n");
    let edits = c.request("textDocument/formatting", json!({"textDocument": {"uri": uri}, "options": {"tabSize": 4, "insertSpaces": true}}));
    assert_eq!(edits[0]["newText"], "x = 1 + 2\n");

    let diags = c.change(uri, "items = [1]\nif items\n    show 1\n");
    assert_eq!(diags[0]["code"], "LIP2005");
    let diags = c.change(uri, "use math\nx = 1\n");
    assert!(diags.iter().all(|d| d["severity"] == 2), "lint results are warnings: {diags:?}");

    c.request("shutdown", Value::Null);
    c.notify("exit", Value::Null);
    assert!(child.wait().unwrap().success());
}
