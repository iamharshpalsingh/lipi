//! `lipi lsp`: the LiPi language server (Language Server Protocol over stdio).
//!
//! Features: diagnostics while typing (the same errors as `lipi check` plus
//! `lipi lint` warnings), completion, hover documentation, go to definition
//! (also into files loaded with `use`), document outline, find references
//! and formatting.

use lipi_compiler::ast::*;
use lipi_compiler::lexer::{Kw, Lexer, Tok};
use lipi_compiler::{checker, format, lint, Diagnostic, Severity, Span};
use lipi_runtime::{Interpreter, Value};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

// ----- documentation shown on hover and in completion ------------------------------

const DOCS: &[(&str, &str, &str)] = &[
    ("toNumber", "toNumber(value) -> Integer | Decimal | null", "Converts text to a number. Returns null when the text isn't a number."),
    ("toInteger", "toInteger(value) -> Integer | null", "Converts to an Integer, dropping any fraction."),
    ("toDecimal", "toDecimal(value) -> Decimal | null", "Converts to a Decimal."),
    ("toString", "toString(value) -> String", "The text form of any value."),
    ("typeOf", "typeOf(value) -> String", "The type name: \"Integer\", \"String\", \"Array\", a type's name..."),
    ("input", "input(prompt) -> String | null", "Reads a line typed by the user."),
    ("assert", "assert(condition, message)", "Fails with \"assertion failed\" when the condition is false."),
    ("assertEqual", "assertEqual(actual, expected)", "Fails when the two values aren't equal. Used in tests."),
    ("sleep", "sleep(milliseconds) -> Task", "Waits in the background. Use with await."),
    ("all", "all(tasks) -> Array", "Waits for every task and returns their results in order."),
    ("timeout", "timeout(task, milliseconds)", "Waits for a task but fails with LIP4002 if it takes too long."),
    ("page", "page \"/path\" with route", "Declares a page of a web app (lipi build). The block draws it; route.params holds :parts of the path."),
    ("card", "card + block", "A boxed group of elements. The block draws what's inside."),
    ("row", "row + block", "Places the elements drawn in the block side by side."),
    ("column", "column + block", "Stacks the elements drawn in the block."),
    ("section", "section + block", "A section of a page."),
    ("heading", "heading value, level: 2", "A heading (levels 1 to 6)."),
    ("text", "text value, ...", "A paragraph of text."),
    ("button", "button label, disabled: false + block", "A button. The block runs when it's clicked."),
    ("link", "link label, to: \"/page\"", "A link to a page of this app, or to an https:// address."),
    ("image", "image source, alt: \"description\"", "An image."),
    ("field", "field value, placeholder: \"...\" with value", "A text box. The block runs with the new text on every change."),
    ("checkbox", "checkbox checked, label with checked", "A checkbox. The block runs with true or false when it changes."),
    ("element", "element \"tag\", ... + block", "Any other HTML element (scripts and styles aren't allowed)."),
    ("navigate", "navigate(\"/path\")", "Goes to another page of the app."),
    ("get", "get \"/path\" with request", "Declares a GET route for the web server."),
    ("post", "post \"/path\" with request", "Declares a POST route for the web server."),
    ("put", "put \"/path\" with request", "Declares a PUT route."),
    ("patch", "patch \"/path\" with request", "Declares a PATCH route."),
    ("delete", "delete \"/path\" with request", "Declares a DELETE route."),
    ("math.sqrt", "math.sqrt(x) -> Decimal", "Square root."),
    ("math.abs", "math.abs(x)", "Absolute value (keeps Integer or Decimal)."),
    ("math.floor", "math.floor(x) -> Integer", "Rounds down."),
    ("math.ceil", "math.ceil(x) -> Integer", "Rounds up."),
    ("math.round", "math.round(x, digits) -> Integer | Decimal", "Rounds to the nearest whole number, or to `digits` decimal places."),
    ("math.pow", "math.pow(base, exponent)", "Power. Integer ** Integer stays an Integer."),
    ("math.min", "math.min(a, b, ...) or math.min(array)", "The smallest number."),
    ("math.max", "math.max(a, b, ...) or math.max(array)", "The largest number."),
    ("math.random", "math.random() -> Decimal", "A random Decimal from 0 up to (not including) 1."),
    ("math.randomInt", "math.randomInt(min, max) -> Integer", "A random Integer between min and max, inclusive."),
    ("math.clamp", "math.clamp(x, min, max)", "Keeps x between min and max."),
    ("math.pi", "math.pi: Decimal", "3.14159…"),
    ("json.parse", "json.parse(text) -> value", "Turns JSON text into LiPi values."),
    ("json.stringify", "json.stringify(value, pretty: false) -> String", "Turns a value into JSON text."),
    ("fs.read", "fs.read(path) -> String", "Reads a text file."),
    ("fs.write", "fs.write(path, text)", "Writes (replaces) a text file."),
    ("fs.append", "fs.append(path, text)", "Adds text to the end of a file."),
    ("fs.exists", "fs.exists(path) -> Boolean", "Whether a file or folder exists."),
    ("fs.list", "fs.list(path) -> Array", "The names in a folder."),
    ("fs.makeDir", "fs.makeDir(path)", "Creates a folder (and its parents)."),
    ("fs.delete", "fs.delete(path)", "Deletes a file or folder."),
    ("env.get", "env.get(name, default) -> String | null", "An environment variable."),
    ("env.load", "env.load(\".env\") -> Integer", "Loads KEY=value lines from a file into the environment."),
    ("http.get", "http.get(url, options) -> Task", "Sends a GET request. `await` it for {status, ok, headers, body, json()}."),
    ("http.post", "http.post(url, body, options) -> Task", "Sends a POST request. Objects are sent as JSON."),
    ("http.request", "http.request({method, url, headers, body, timeout}) -> Task", "Sends any HTTP request."),
    ("time.now", "time.now() -> Integer", "Milliseconds since 1970 (UTC)."),
    ("time.date", "time.date(ms) -> Object", "{year, month, day, hour, minute, second, weekday} in UTC."),
    ("time.iso", "time.iso(ms) -> String", "An ISO 8601 timestamp, like 2026-09-11T10:30:00Z."),
    ("process.args", "process.args: Array", "The command-line arguments."),
    ("process.exit", "process.exit(code)", "Stops the program."),
    ("process.run", "process.run(command) -> {code, output, error}", "Runs a shell command."),
    ("server.start", "server.start port", "Opens the web server on a port. Requests are served after the file has run."),
    ("server.respond", "server.respond(status, body, headers)", "A response with a status code and headers."),
    ("server.redirect", "server.redirect(url, status: 302)", "A redirect response."),
    ("server.cookie", "server.cookie(name, value, {maxAge, secure, httpOnly}) -> String", "A Set-Cookie value with secure defaults."),
    ("server.before", "server.before with request", "Middleware: return a response to stop the request early."),
    ("server.static", "server.static \"/assets\", \"./public\"", "Serves files from a folder."),
    ("server.websocket", "server.websocket \"/path\" with socket", "Accepts WebSocket connections: socket.on \"message\", socket.send(...)."),
    ("server.broadcast", "server.broadcast(path, message) -> Integer", "Sends a message to every socket on a path."),
    ("crypto.hashPassword", "crypto.hashPassword(password) -> String", "A salted PBKDF2-SHA256 hash, safe to store."),
    ("crypto.verifyPassword", "crypto.verifyPassword(password, hash) -> Boolean", "Checks a password against a stored hash."),
    ("crypto.randomToken", "crypto.randomToken(bytes: 32) -> String", "A secure random token (URL-safe)."),
    ("crypto.uuid", "crypto.uuid() -> String", "A random UUID (version 4)."),
    ("crypto.sha256", "crypto.sha256(text) -> String", "SHA-256 as hex."),
    ("database.open", "database.open(target) -> Database", "Opens SQLite (a file or \":memory:\") or PostgreSQL (postgres://...)."),
    ("Database.query", "db.query(sql, params) -> Array", "Runs SQL and returns rows. Use ? or :name placeholders."),
    ("Database.run", "db.run(sql, params) -> {changes, lastId}", "Runs SQL that changes data."),
    ("Database.transaction", "db.transaction(tx => ...)", "Runs a block in a transaction, rolled back if it fails."),
    ("Database.migrate", "db.migrate(name, sql) -> Boolean", "Runs a migration once."),
    ("Table.all", "db.table.all(order: \"-id\", limit: 10) -> Array", "All rows."),
    ("Table.where", "db.table.where(field: value, order:, limit:) -> Array", "Rows matching the filters."),
    ("Table.find", "db.table.find(id) or find(field: value) -> Object | null", "The first matching row."),
    ("Table.create", "db.table.create({...}) -> Object", "Inserts a row (creating the table or columns if needed)."),
    ("Table.update", "db.table.update(id, {...}) -> Object", "Changes a row and returns it."),
    ("Table.count", "db.table.count(filters) -> Integer", "The number of matching rows."),
    ("String.length", "text.length: Integer", "Number of characters."),
    ("String.upper", "text.upper() -> String", "UPPERCASE."),
    ("String.lower", "text.lower() -> String", "lowercase."),
    ("String.trim", "text.trim() -> String", "Removes surrounding spaces."),
    ("String.split", "text.split(separator) -> Array", "Splits text (on spaces by default)."),
    ("String.replace", "text.replace(old, new) -> String", "Replaces every occurrence."),
    ("String.contains", "text.contains(part) -> Boolean", "Whether the text includes a part."),
    ("String.startsWith", "text.startsWith(part) -> Boolean", ""),
    ("String.endsWith", "text.endsWith(part) -> Boolean", ""),
    ("String.slice", "text.slice(start, end) -> String", "Part of the text. Negative positions count from the end."),
    ("String.padStart", "text.padStart(width, fill) -> String", ""),
    ("Array.push", "items.push(item, ...)", "Adds items to the end (changes the Array)."),
    ("Array.pop", "items.pop() -> item", "Removes and returns the last item."),
    ("Array.map", "items.map(item => ...) -> Array", "A new Array with each item transformed."),
    ("Array.filter", "items.filter(item => condition) -> Array", "The items for which the condition is true."),
    ("Array.reduce", "items.reduce((total, item) => ..., start)", "Combines the items into one value."),
    ("Array.find", "items.find(item => condition) -> item | null", "The first matching item."),
    ("Array.sort", "items.sort() -> Array", "A sorted copy (numbers or Strings)."),
    ("Array.sortBy", "items.sortBy(item => key) -> Array", "A copy sorted by a key."),
    ("Array.join", "items.join(separator) -> String", "Joins the items as text."),
    ("Array.sum", "items.sum()", "Adds up numbers."),
    ("Array.length", "items.length: Integer", "Number of items."),
    ("Array.first", "items.first", "The first item, or null."),
    ("Array.last", "items.last", "The last item, or null."),
    ("Array.isEmpty", "items.isEmpty() -> Boolean", ""),
    ("Object.keys", "obj.keys() -> Array", "The keys."),
    ("Object.values", "obj.values() -> Array", "The values."),
    ("Object.has", "obj.has(key) -> Boolean", "Whether a key exists."),
    ("Object.get", "obj.get(key, default)", "A field, or the default (null) when it's missing."),
];

const KEYWORD_DOCS: &[(&str, &str)] = &[
    ("show", "show value, ...\n\nPrints values on one line."),
    ("if", "if condition\n\nRuns the indented block when the condition is true. Conditions must be Booleans."),
    ("for", "for item in items / for i in 1 to 10\n\nLoops over an Array, String, Object or range."),
    ("while", "while condition\n\nRepeats while the condition is true."),
    ("repeat", "repeat n\n\nRepeats the block n times."),
    ("match", "match value\n\nRuns the first case whose pattern equals the value; `else` otherwise."),
    ("function", "function name(params)\n\nDefines a function (the keyword is optional: `name(params)` works too)."),
    ("async", "async name(params)\n\nA function you can `await` inside. Calling it returns a Task."),
    ("await", "await task\n\nWaits for a Task and returns its result (or rethrows its error)."),
    ("use", "use module / use \"./file.lipi\" as name\n\nLoads a module. Only its exported names are visible."),
    ("export", "export name, ...\n\nMakes names visible to files that `use` this one."),
    ("const", "const name = value\n\nA constant that can't be reassigned."),
    ("try", "try / catch error / finally\n\nHandles errors. error has message, code, category, hint, line."),
    ("throw", "throw \"message\" or throw {message: ...}\n\nRaises an error."),
    ("null", "null\n\nThe empty value. Use `?.` and `??` to handle it."),
];

fn doc_for(key: &str) -> Option<(&'static str, &'static str)> {
    DOCS.iter().find(|(k, _, _)| *k == key).map(|(_, s, d)| (*s, *d))
}

// ----- positions (LSP uses 0-based lines and UTF-16 columns) -----------------------

fn to_pos(text: &str, byte: usize) -> Json {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    let before = &text[..byte];
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    json!({"line": line, "character": text[start..byte].encode_utf16().count()})
}

fn to_byte(text: &str, pos: &Json) -> usize {
    let line = pos["line"].as_u64().unwrap_or(0) as usize;
    let ch = pos["character"].as_u64().unwrap_or(0) as usize;
    let mut offset = 0;
    for (i, l) in text.split('\n').enumerate() {
        if i == line {
            let mut units = 0;
            for (b, c) in l.char_indices() {
                if units >= ch {
                    return offset + b;
                }
                units += c.len_utf16();
            }
            return offset + l.len();
        }
        offset += l.len() + 1;
    }
    text.len()
}

fn range(text: &str, span: Span) -> Json {
    let end = if span.end > span.start { span.end } else { (span.start + 1).min(text.len()) };
    json!({"start": to_pos(text, span.start), "end": to_pos(text, end)})
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut bytes = Vec::new();
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(b) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                bytes.push(b);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    let path = String::from_utf8_lossy(&bytes).into_owned();
    let path = path.strip_prefix('/').filter(|p| p.as_bytes().get(1) == Some(&b':')).map(String::from).unwrap_or(path);
    Some(PathBuf::from(path))
}

fn path_to_uri(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    let encoded: String = p.chars().map(|c| if c == ' ' { "%20".to_string() } else { c.to_string() }).collect();
    if encoded.starts_with('/') { format!("file://{encoded}") } else { format!("file:///{encoded}") }
}

/// The identifier at a byte offset, with its start, and the name before a `.` (if any).
fn word_at(text: &str, byte: usize) -> Option<(String, usize, Option<String>)> {
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let b = text.as_bytes();
    let mut start = byte.min(b.len());
    while start > 0 && is_word(b[start - 1]) {
        start -= 1;
    }
    let mut end = byte.min(b.len());
    while end < b.len() && is_word(b[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    let word = text[start..end].to_string();
    let mut before = None;
    if start > 0 && b[start - 1] == b'.' {
        let mut s = start - 1;
        while s > 0 && is_word(b[s - 1]) {
            s -= 1;
        }
        if s < start - 1 {
            before = Some(text[s..start - 1].to_string());
        }
    }
    Some((word, start, before))
}

// ----- the server -------------------------------------------------------------------

struct Server {
    docs: HashMap<String, String>,
    builtins: Vec<String>,
    modules: HashMap<String, Vec<String>>,
    shutdown: bool,
}

fn send(msg: &Json) {
    let body = msg.to_string();
    let mut out = std::io::stdout().lock();
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = out.flush();
}

fn read_message(r: &mut impl BufRead) -> Option<Json> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some(v) = l.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok();
        }
    }
    let mut buf = vec![0; len?];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

pub fn run() -> i32 {
    let interpreter = Interpreter::new();
    let builtins = interpreter.builtin_names();
    let mut modules = HashMap::new();
    for name in &builtins {
        if let Some(Value::Object(o)) = interpreter.globals.get(name) {
            if o.module.is_some() {
                modules.insert(name.clone(), o.fields.borrow().keys().cloned().collect());
            }
        }
    }
    let mut server = Server { docs: HashMap::new(), builtins, modules, shutdown: false };
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    while let Some(msg) = read_message(&mut input) {
        if let Some(code) = server.handle(&msg) {
            return code;
        }
    }
    0
}

impl Server {
    fn handle(&mut self, msg: &Json) -> Option<i32> {
        let method = msg["method"].as_str().unwrap_or("");
        let id = msg.get("id").cloned();
        let params = &msg["params"];
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        let result = match method {
            "initialize" => Some(json!({
                "capabilities": {
                    "textDocumentSync": 1,
                    "completionProvider": {"triggerCharacters": ["."]},
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "referencesProvider": true,
                    "documentSymbolProvider": true,
                    "documentFormattingProvider": true
                },
                "serverInfo": {"name": "lipi", "version": env!("CARGO_PKG_VERSION")}
            })),
            "shutdown" => {
                self.shutdown = true;
                Some(Json::Null)
            }
            "exit" => return Some(if self.shutdown { 0 } else { 1 }),
            "textDocument/didOpen" => {
                self.docs.insert(uri.clone(), params["textDocument"]["text"].as_str().unwrap_or("").to_string());
                self.publish(&uri);
                None
            }
            "textDocument/didChange" => {
                if let Some(text) = params["contentChanges"].as_array().and_then(|c| c.last()).and_then(|c| c["text"].as_str()) {
                    self.docs.insert(uri.clone(), text.to_string());
                    self.publish(&uri);
                }
                None
            }
            "textDocument/didClose" => {
                self.docs.remove(&uri);
                send(&json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": []}}));
                None
            }
            "textDocument/completion" => Some(self.completion(&uri, &params["position"])),
            "textDocument/hover" => Some(self.hover(&uri, &params["position"])),
            "textDocument/definition" => Some(self.definition(&uri, &params["position"])),
            "textDocument/references" => Some(self.references(&uri, &params["position"])),
            "textDocument/documentSymbol" => Some(self.symbols(&uri)),
            "textDocument/formatting" => Some(self.formatting(&uri)),
            _ => {
                if let Some(id) = id {
                    if !method.starts_with("$/") {
                        send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("{method} is not supported")}}));
                    }
                }
                return None;
            }
        };
        if let (Some(id), Some(result)) = (id, result) {
            send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
        }
        None
    }

    fn text(&self, uri: &str) -> String {
        self.docs.get(uri).cloned().unwrap_or_default()
    }

    fn diagnostics(&self, text: &str) -> Vec<Json> {
        let refs: Vec<&str> = self.builtins.iter().map(String::as_str).collect();
        let diags: Vec<Diagnostic> = match lipi_compiler::parse_source(text) {
            Err(d) => vec![d],
            Ok(p) => {
                let mut ds = checker::check(&p, &refs);
                ds.extend(lint::lint(&p));
                ds
            }
        };
        diags
            .iter()
            .filter_map(|d| {
                let span = d.span?;
                let message = match &d.hint {
                    Some(h) => format!("{}\nHint: {h}", d.message),
                    None => d.message.clone(),
                };
                Some(json!({
                    "range": range(text, span),
                    "severity": if d.severity == Severity::Error { 1 } else { 2 },
                    "code": d.code,
                    "source": "lipi",
                    "message": message
                }))
            })
            .collect()
    }

    fn publish(&self, uri: &str) {
        let diagnostics = self.diagnostics(&self.text(uri));
        send(&json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": diagnostics}}));
    }

    fn completion(&self, uri: &str, pos: &Json) -> Json {
        let text = self.text(uri);
        let byte = to_byte(&text, pos);
        let line_start = text[..byte].rfind('\n').map_or(0, |i| i + 1);
        let line = &text[line_start..byte];
        let mut items = Vec::new();
        let item = |label: &str, kind: u8, detail: Option<(&str, &str)>| {
            let mut v = json!({"label": label, "kind": kind});
            if let Some((sig, doc)) = detail {
                v["detail"] = json!(sig);
                if !doc.is_empty() {
                    v["documentation"] = json!(doc);
                }
            }
            v
        };
        // After `name.`: module members, or methods of values.
        let trimmed = line.trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
        if let Some(before_dot) = trimmed.strip_suffix('.') {
            let base: String = before_dot.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<Vec<_>>().into_iter().rev().collect();
            if let Some(members) = self.modules.get(&base) {
                for m in members {
                    let key = format!("{base}.{m}");
                    items.push(item(m, 2, doc_for(&key)));
                }
            } else {
                let mut seen = std::collections::BTreeSet::new();
                for (ty, list) in [("String", checker::STRING_MEMBERS), ("Array", checker::LIST_MEMBERS), ("Object", checker::OBJECT_MEMBERS), ("Integer", checker::NUMBER_MEMBERS)] {
                    for m in list {
                        if seen.insert(*m) {
                            items.push(item(m, 2, doc_for(&format!("{ty}.{m}"))));
                        }
                    }
                }
                for (key, _, _) in DOCS.iter().filter(|(k, _, _)| k.starts_with("Table.") || k.starts_with("Database.")) {
                    let m = key.split_once('.').map(|(_, m)| m).unwrap_or(key);
                    if seen.insert(m) {
                        items.push(item(m, 2, doc_for(key)));
                    }
                }
            }
            return json!({"isIncomplete": false, "items": items});
        }
        for (kw, doc) in KEYWORD_DOCS {
            items.push(json!({"label": kw, "kind": 14, "documentation": doc}));
        }
        for name in ["else", "in", "break", "continue", "return", "and", "or", "not", "true", "false", "from", "as", "catch", "finally"] {
            items.push(json!({"label": name, "kind": 14}));
        }
        for name in &self.builtins {
            let kind = if self.modules.contains_key(name) { 9 } else { 3 };
            items.push(item(name, kind, doc_for(name)));
        }
        for ty in checker::BUILTIN_TYPES {
            items.push(json!({"label": ty, "kind": 7}));
        }
        // Names defined in this document.
        let mut names = std::collections::BTreeMap::new();
        if let Ok(program) = lipi_compiler::parse_source(&text) {
            for (name, (_, kind)) in definitions(&program) {
                names.insert(name, kind);
            }
        } else if let Ok(tokens) = Lexer::new(&text).tokenize() {
            for t in tokens {
                if let Tok::Ident(n) = t.tok {
                    names.entry(n).or_insert(6);
                }
            }
        }
        for (name, kind) in names {
            if !self.builtins.contains(&name) {
                items.push(json!({"label": name, "kind": kind}));
            }
        }
        json!({"isIncomplete": false, "items": items})
    }

    fn hover(&self, uri: &str, pos: &Json) -> Json {
        let text = self.text(uri);
        let Some((word, start, before)) = word_at(&text, to_byte(&text, pos)) else { return Json::Null };
        let markdown = |sig: &str, doc: &str| json!({"contents": {"kind": "markdown", "value": format!("```lipi\n{sig}\n```\n{doc}")}});
        if let Some(base) = &before {
            if let Some((sig, doc)) = doc_for(&format!("{base}.{word}")) {
                return markdown(sig, doc);
            }
            for prefix in ["String", "Array", "Object", "Table", "Database"] {
                if let Some((sig, doc)) = doc_for(&format!("{prefix}.{word}")) {
                    return markdown(sig, doc);
                }
            }
        }
        if let Some((sig, doc)) = doc_for(&word) {
            return markdown(sig, doc);
        }
        if let Some((_, doc)) = KEYWORD_DOCS.iter().find(|(k, _)| *k == word) {
            let (sig, rest) = doc.split_once("\n\n").unwrap_or((doc, ""));
            return markdown(sig, rest);
        }
        // A function or type from this file: show its first line and the comments above it.
        if let Ok(program) = lipi_compiler::parse_source(&text) {
            if let Some((span, _)) = definitions(&program).get(&word) {
                let line_start = text[..span.start].rfind('\n').map_or(0, |i| i + 1);
                let line_end = text[span.start..].find('\n').map_or(text.len(), |i| span.start + i);
                let signature = text[line_start..line_end].trim();
                let mut comments: Vec<&str> = Vec::new();
                for l in text[..line_start].lines().rev() {
                    match l.trim().strip_prefix('#') {
                        Some(c) => comments.push(c.trim()),
                        None => break,
                    }
                }
                comments.reverse();
                let _ = start;
                return markdown(signature, &comments.join("\n"));
            }
        }
        Json::Null
    }

    fn definition(&self, uri: &str, pos: &Json) -> Json {
        let text = self.text(uri);
        let Some((word, _, before)) = word_at(&text, to_byte(&text, pos)) else { return Json::Null };
        let Ok(program) = lipi_compiler::parse_source(&text) else { return Json::Null };
        let here = uri_to_path(uri);
        // `alias.name` or a name brought in with `from ... use name`: look in the other file.
        for stmt in &program.body {
            if let StmtKind::Use { source, alias, names } = &stmt.kind {
                let bound = alias.as_ref().map(|a| a.text.clone()).unwrap_or_else(|| checker::module_binding_name(source));
                let wanted = match (&before, names) {
                    (Some(b), None) if *b == bound => Some(word.clone()),
                    (None, Some(ns)) if ns.iter().any(|n| n.text == word) => Some(word.clone()),
                    _ => None,
                };
                if let (Some(name), Some(here)) = (wanted, &here) {
                    if let Some(loc) = definition_in_module(here, source, &name) {
                        return loc;
                    }
                }
            }
        }
        if before.is_some() {
            return Json::Null;
        }
        match definitions(&program).get(&word) {
            Some((span, _)) => json!({"uri": uri, "range": range(&text, *span)}),
            None => Json::Null,
        }
    }

    fn references(&self, uri: &str, pos: &Json) -> Json {
        let text = self.text(uri);
        let Some((word, _, _)) = word_at(&text, to_byte(&text, pos)) else { return json!([]) };
        let Ok(tokens) = Lexer::new(&text).tokenize() else { return json!([]) };
        let locations: Vec<Json> = tokens
            .iter()
            .filter(|t| matches!(&t.tok, Tok::Ident(n) if *n == word))
            .map(|t| json!({"uri": uri, "range": range(&text, t.span)}))
            .collect();
        json!(locations)
    }

    fn symbols(&self, uri: &str) -> Json {
        let text = self.text(uri);
        let Ok(program) = lipi_compiler::parse_source(&text) else { return json!([]) };
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for stmt in &program.body {
            let stmt = match &stmt.kind {
                StmtKind::Export { inner: Some(s), .. } => s.as_ref(),
                _ => stmt,
            };
            let symbol = |name: &str, kind: u8, full: Span, sel: Span, children: Vec<Json>| {
                json!({"name": name, "kind": kind, "range": range(&text, full), "selectionRange": range(&text, sel), "children": children})
            };
            match &stmt.kind {
                StmtKind::Func(f) | StmtKind::Component(f) => out.push(symbol(&f.name.text, 12, Span { end: block_end(&f.body).max(f.name.span.end), ..f.name.span }, f.name.span, vec![])),
                StmtKind::State { name, .. } if seen.insert(name.text.clone()) => out.push(symbol(&name.text, 13, stmt.span, name.span, vec![])),
                StmtKind::TypeDef(t) => {
                    let mut children: Vec<Json> = t.fields.iter().map(|f| symbol(&f.name.text, 8, f.name.span, f.name.span, vec![])).collect();
                    let mut end = t.name.span.end;
                    for m in &t.methods {
                        let m_end = block_end(&m.body).max(m.name.span.end);
                        end = end.max(m_end);
                        children.push(symbol(&m.name.text, 6, Span { end: m_end, ..m.name.span }, m.name.span, vec![]));
                    }
                    out.push(symbol(&t.name.text, 5, Span { end, ..t.name.span }, t.name.span, children));
                }
                StmtKind::Assign { target: Target::Name(n), op: None, constant, .. } if seen.insert(n.text.clone()) => {
                    out.push(symbol(&n.text, if *constant { 14 } else { 13 }, stmt.span, n.span, vec![]));
                }
                StmtKind::Test { name, body } => out.push(symbol(&format!("test \"{name}\""), 12, Span { end: block_end(body).max(stmt.span.end), ..stmt.span }, stmt.span, vec![])),
                _ => {}
            }
        }
        json!(out)
    }

    fn formatting(&self, uri: &str) -> Json {
        let text = self.text(uri);
        match format::format_source(&text) {
            Ok(out) if out != text => json!([{"range": {"start": {"line": 0, "character": 0}, "end": to_pos(&text, text.len())}, "newText": out}]),
            _ => json!([]),
        }
    }
}

/// The end of the last statement in a block.
fn block_end(body: &Block) -> usize {
    body.iter().map(stmt_end).max().unwrap_or(0)
}

fn stmt_end(stmt: &Stmt) -> usize {
    let nested = match &stmt.kind {
        StmtKind::If { branches, otherwise } => branches.iter().map(|(_, b)| block_end(b)).chain(otherwise.iter().map(block_end)).max().unwrap_or(0),
        StmtKind::While { body, .. } | StmtKind::Repeat { body, .. } | StmtKind::For { body, .. } | StmtKind::Test { body, .. } => block_end(body),
        StmtKind::Func(f) | StmtKind::Component(f) => block_end(&f.body),
        StmtKind::Try { body, catch, finally } => [Some(block_end(body)), catch.as_ref().map(|(_, b)| block_end(b)), finally.as_ref().map(block_end)].into_iter().flatten().max().unwrap_or(0),
        StmtKind::Match { arms, otherwise, .. } => arms.iter().map(|a| block_end(&a.body)).chain(otherwise.iter().map(block_end)).max().unwrap_or(0),
        StmtKind::Export { inner: Some(s), .. } => stmt_end(s),
        _ => 0,
    };
    stmt.span.end.max(nested)
}

/// Where each name is first defined in a program, with an LSP completion kind.
fn definitions(program: &Program) -> HashMap<String, (Span, u8)> {
    fn add(out: &mut HashMap<String, (Span, u8)>, name: &Name, kind: u8) {
        out.entry(name.text.clone()).or_insert((name.span, kind));
    }
    fn block(b: &Block, out: &mut HashMap<String, (Span, u8)>) {
        for s in b {
            stmt(s, out);
        }
    }
    fn func(f: &FuncDecl, out: &mut HashMap<String, (Span, u8)>) {
        for p in &f.params {
            add(out, &p.name, 6);
        }
        block(&f.body, out);
    }
    fn expr(e: &Expr, out: &mut HashMap<String, (Span, u8)>) {
        match &e.kind {
            ExprKind::Lambda(f) => func(f, out),
            ExprKind::Call { callee, args } => {
                expr(callee, out);
                args.iter().for_each(|a| expr(&a.value, out));
            }
            _ => {}
        }
    }
    fn stmt(s: &Stmt, out: &mut HashMap<String, (Span, u8)>) {
        match &s.kind {
            StmtKind::Assign { target: Target::Name(n), value, constant, .. } => {
                add(out, n, if *constant { 21 } else { 6 });
                expr(value, out);
            }
            StmtKind::Assign { value, .. } | StmtKind::Expr(value) => expr(value, out),
            StmtKind::Func(f) | StmtKind::Component(f) => {
                add(out, &f.name, 3);
                func(f, out);
            }
            StmtKind::State { name, value, .. } => {
                add(out, name, 6);
                expr(value, out);
            }
            StmtKind::TypeDef(t) => {
                add(out, &t.name, 7);
                for m in &t.methods {
                    add(out, &m.name, 2);
                    func(m, out);
                }
            }
            StmtKind::For { first, second, body, .. } => {
                add(out, first, 6);
                if let Some(s) = second {
                    add(out, s, 6);
                }
                block(body, out);
            }
            StmtKind::If { branches, otherwise } => {
                branches.iter().for_each(|(_, b)| block(b, out));
                if let Some(b) = otherwise {
                    block(b, out);
                }
            }
            StmtKind::While { body, .. } | StmtKind::Repeat { body, .. } | StmtKind::Test { body, .. } => block(body, out),
            StmtKind::Try { body, catch, finally } => {
                block(body, out);
                if let Some((name, b)) = catch {
                    if let Some(n) = name {
                        add(out, n, 6);
                    }
                    block(b, out);
                }
                if let Some(b) = finally {
                    block(b, out);
                }
            }
            StmtKind::Match { arms, otherwise, .. } => {
                arms.iter().for_each(|a| block(&a.body, out));
                if let Some(b) = otherwise {
                    block(b, out);
                }
            }
            StmtKind::Use { alias, names, .. } => {
                if let Some(a) = alias {
                    add(out, a, 9);
                }
                if let Some(ns) = names {
                    ns.iter().for_each(|n| add(out, n, 6));
                }
            }
            StmtKind::Export { inner: Some(s), .. } => stmt(s, out),
            _ => {}
        }
    }
    let mut out = HashMap::new();
    block(&program.body, &mut out);
    out
}

/// Find `name` defined in the module `source` used from the file `here`.
fn definition_in_module(here: &Path, source: &str, name: &str) -> Option<Json> {
    let base = here.parent()?;
    let is_path = source.starts_with('.') || source.starts_with('/') || source.ends_with(".lipi") || source.contains(['/', '\\']);
    let mut candidates = Vec::new();
    if is_path {
        let p = base.join(source);
        candidates.push(if p.extension().is_some_and(|e| e == "lipi") { p } else { PathBuf::from(format!("{}.lipi", p.to_string_lossy())) });
    } else {
        let rel = format!("{}.lipi", source.replace('.', "/"));
        candidates.push(base.join(&rel));
        let mut dir = Some(base);
        while let Some(d) = dir {
            if d.join("lipi.json").is_file() {
                candidates.push(d.join("src").join(&rel));
                candidates.push(d.join("lipi_modules").join(source).join("main.lipi"));
                break;
            }
            dir = d.parent();
        }
    }
    let file = candidates.into_iter().find(|p| p.is_file())?;
    let text = std::fs::read_to_string(&file).ok()?;
    let program = lipi_compiler::parse_source(&text).ok()?;
    let (span, _) = *definitions(&program).get(name)?;
    Some(json!({"uri": path_to_uri(&file), "range": range(&text, span)}))
}

#[allow(dead_code)]
fn is_keyword(word: &str) -> bool {
    Kw::lookup(word).is_some()
}
