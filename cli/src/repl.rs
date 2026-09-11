//! The interactive prompt.

use lipi_runtime::{Interpreter, RunError, Value};
use std::io::{BufRead, IsTerminal, Write};

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Does this line open an indented block (so we should keep reading)?
/// `is_function` tells whether a name already refers to a function.
fn opens_block(line: &str, is_function: impl Fn(&str) -> bool) -> bool {
    let t = line.trim_end();
    let first = t.split_whitespace().next().unwrap_or("");
    if matches!(first, "if" | "else" | "for" | "while" | "repeat" | "try" | "catch" | "finally" | "match" | "async" | "function" | "type" | "test") {
        return true;
    }
    // `name(params)` or `name(params) -> type` on its own line defines a
    // function, as long as every parameter looks like one: `a`, `a: type`, `a = default`.
    let (Some(open), Some(close)) = (t.find('('), t.rfind(')')) else { return false };
    let name = &t[..open];
    let after = t[close + 1..].trim();
    if !is_name(name) || close < open || !(after.is_empty() || after.starts_with("->")) || is_function(name) {
        return false;
    }
    t[open + 1..close].split(',').map(str::trim).filter(|p| !p.is_empty()).all(|p| {
        let head = p.split([':', '=']).next().unwrap_or("").trim();
        is_name(head)
    })
}

pub fn start() -> i32 {
    let color = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    println!("LiPi {} — type code and press Enter. Blocks end with an empty line. Type `exit` to leave.", env!("CARGO_PKG_VERSION"));
    let mut it = Interpreter::new();
    let env = it.repl_env();
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        print!("> ");
        let _ = std::io::stdout().flush();
        let Some(Ok(line)) = lines.next() else {
            println!();
            return 0;
        };
        if matches!(line.trim(), "exit" | "quit") {
            return 0;
        }
        if line.trim().is_empty() {
            continue;
        }
        let mut source = line.clone();
        let is_function = |name: &str| matches!(env.get(name), Some(Value::Func(_) | Value::Native(_) | Value::Type(_)));
        if opens_block(&line, is_function) {
            loop {
                print!(". ");
                let _ = std::io::stdout().flush();
                match lines.next() {
                    Some(Ok(more)) if !more.trim().is_empty() => {
                        source.push('\n');
                        source.push_str(&more);
                    }
                    _ => break,
                }
            }
        }
        source.push('\n');
        match it.eval_repl(&source, &env) {
            Ok(Value::Nil) => {}
            Ok(v) => println!("{}", v.repr()),
            Err(RunError::Exit(code)) => return code,
            Err(e) => eprint!("{}", it.render(&e, color)),
        }
    }
}
