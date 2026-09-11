//! Global functions and standard-library modules.
//!
//! Everyday modules are available everywhere without an import:
//! `math`, `json`, `fs`, `env`, `http`, `time`, `process`.

use crate::http;
use crate::interp::{Flow, Interpreter};
use crate::json;
use crate::task::{self, SendValue};
use crate::value::*;
use lipi_compiler::checker::with_article;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

pub const MODULES: &[&str] = &["math", "json", "fs", "env", "http", "time", "process"];

// ----- argument helpers (shared with methods.rs) ------------------------------

pub fn need<'a>(it: &Interpreter, a: &'a Args, i: usize, pname: &str) -> Result<&'a Value, Flow> {
    a.get(i, pname).ok_or_else(|| it.error(format!("{}() needs `{pname}`", a.name), a.span, None))
}

fn wrong(it: &Interpreter, a: &Args, pname: &str, expected: &str, got: &Value) -> Flow {
    it.error(
        format!("{}() expects `{pname}` to be {}, but got {}", a.name, with_article(expected), with_article(&got.type_name())),
        a.span,
        None,
    )
}

pub fn num(it: &Interpreter, a: &Args, i: usize, pname: &str) -> Result<f64, Flow> {
    match need(it, a, i, pname)? {
        Value::Num(n) => Ok(*n),
        v => Err(wrong(it, a, pname, "number", v)),
    }
}

pub fn opt_num(it: &Interpreter, a: &Args, i: usize, pname: &str) -> Result<Option<f64>, Flow> {
    match a.get(i, pname) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Num(n)) => Ok(Some(*n)),
        Some(v) => Err(wrong(it, a, pname, "number", v)),
    }
}

pub fn text(it: &Interpreter, a: &Args, i: usize, pname: &str) -> Result<Rc<str>, Flow> {
    match need(it, a, i, pname)? {
        Value::Str(s) => Ok(s.clone()),
        v => Err(wrong(it, a, pname, "string", v)),
    }
}

pub fn opt_text(it: &Interpreter, a: &Args, i: usize, pname: &str) -> Result<Option<Rc<str>>, Flow> {
    match a.get(i, pname) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Str(s)) => Ok(Some(s.clone())),
        Some(v) => Err(wrong(it, a, pname, "string", v)),
    }
}

pub fn list(it: &Interpreter, a: &Args, i: usize, pname: &str) -> Result<Rc<RefCell<Vec<Value>>>, Flow> {
    match need(it, a, i, pname)? {
        Value::List(l) => Ok(l.clone()),
        v => Err(wrong(it, a, pname, "list", v)),
    }
}

pub fn callable(it: &Interpreter, a: &Args, i: usize, pname: &str) -> Result<Value, Flow> {
    match need(it, a, i, pname)? {
        v @ (Value::Func(_) | Value::Native(_) | Value::Method(_) | Value::Type(_)) => Ok(v.clone()),
        v => Err(it.error(
            format!("{}() expects a function, but got {}", a.name, with_article(&v.type_name())),
            a.span,
            Some(format!("For example: items.{}(item => item * 2)", a.name)),
        )),
    }
}

fn io_error(it: &Interpreter, a: &Args, path: &str, e: std::io::Error) -> Flow {
    match e.kind() {
        std::io::ErrorKind::NotFound => it.error(
            format!("there's no file or folder at `{path}`"),
            a.span,
            Some(format!(
                "Paths are relative to the folder you ran lipi from ({}).",
                std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default()
            )),
        ),
        std::io::ErrorKind::PermissionDenied => it.error(format!("not allowed to access `{path}`"), a.span, None),
        _ => it.error(format!("couldn't access `{path}`: {e}"), a.span, None),
    }
}

fn module(name: &str, entries: Vec<(&str, Value)>) -> Value {
    let fields: Fields = entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::Object(Rc::new(ObjectData { fields: RefCell::new(fields), ty: None, module: Some(name.to_string()) }))
}

pub fn to_number(v: &Value) -> Value {
    match v {
        Value::Num(_) => v.clone(),
        Value::Str(s) => {
            let t = s.trim().replace('_', "");
            let plausible = !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'));
            match t.parse::<f64>() {
                Ok(n) if plausible => Value::Num(n),
                _ => Value::Nil,
            }
        }
        _ => Value::Nil,
    }
}

fn numbers_from_args(it: &Interpreter, a: &Args) -> Result<Vec<f64>, Flow> {
    let items: Vec<Value> = match a.pos.as_slice() {
        [Value::List(l)] => l.borrow().clone(),
        other => other.to_vec(),
    };
    items
        .iter()
        .map(|v| match v {
            Value::Num(n) => Ok(*n),
            other => Err(it.error(format!("{}() works with numbers, but got {}", a.name, with_article(&other.type_name())), a.span, None)),
        })
        .collect()
}

fn unix_millis() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
        .floor()
}

/// Days since 1970-01-01 to (year, month, day). From Howard Hinnant's date algorithms.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn date_parts(ms: f64) -> (i64, u32, u32, u32, u32, u32, u32) {
    let secs = (ms / 1000.0).floor() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let weekday = (days + 4).rem_euclid(7) as u32; // 1970-01-01 was a Thursday
    (y, m, d, (rem / 3600) as u32, ((rem % 3600) / 60) as u32, (rem % 60) as u32, weekday)
}

// ----- installation -------------------------------------------------------------

pub fn install(g: &Rc<Env>) {
    let def = |name: &str, v: Value| g.define(name, v);

    def("to_number", Value::native("to_number", |_, a| Ok(to_number(a.pos.first().unwrap_or(&Value::Nil)))));
    def("to_string", Value::native("to_string", |_, a| Ok(Value::string(a.pos.first().unwrap_or(&Value::Nil).display()))));
    def("type_of", Value::native("type_of", |_, a| Ok(Value::string(a.pos.first().unwrap_or(&Value::Nil).type_name()))));
    def(
        "input",
        Value::native("input", |_, a| {
            use std::io::Write;
            if let Some(p) = a.pos.first() {
                print!("{}", p.display());
                let _ = std::io::stdout().flush();
            }
            let mut line = String::new();
            match std::io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => Ok(Value::Nil),
                Ok(_) => Ok(Value::text(line.trim_end_matches(['\n', '\r']))),
            }
        }),
    );
    def(
        "assert",
        Value::native("assert", |it, a| {
            if need(it, a, 0, "condition")?.truthy() {
                return Ok(Value::Nil);
            }
            let message = match a.get(1, "message") {
                Some(m) => format!("assertion failed: {}", m.display()),
                None => "assertion failed".to_string(),
            };
            Err(it.error(message, a.span, None))
        }),
    );
    def(
        "assert_equal",
        Value::native("assert_equal", |it, a| {
            let actual = need(it, a, 0, "actual")?;
            let expected = need(it, a, 1, "expected")?;
            if actual.equals(expected) {
                return Ok(Value::Nil);
            }
            Err(it.error(format!("expected {}, but got {}", expected.repr(), actual.repr()), a.span, None))
        }),
    );
    def(
        "sleep",
        Value::native("sleep", |it, a| {
            let ms = num(it, a, 0, "milliseconds")?.max(0.0) as u64;
            Ok(task::spawn(task::identity, move || {
                std::thread::sleep(Duration::from_millis(ms));
                Ok(SendValue::Nil)
            }))
        }),
    );
    def(
        "all",
        Value::native("all", |it, a| {
            let items = list(it, a, 0, "tasks")?.borrow().clone();
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(match item {
                    Value::Task(t) => task::await_task(it, &t, a.span, None)?,
                    v => v,
                });
            }
            Ok(Value::list(out))
        }),
    );
    def(
        "timeout",
        Value::native("timeout", |it, a| {
            let ms = num(it, a, 1, "milliseconds")?.max(0.0) as u64;
            match need(it, a, 0, "task")? {
                Value::Task(t) => {
                    let t = t.clone();
                    task::await_task(it, &t, a.span, Some(Duration::from_millis(ms)))
                }
                v => Ok(v.clone()),
            }
        }),
    );

    def("math", math_module());
    def("json", json_module());
    def("fs", fs_module());
    def("env", env_module());
    def("http", http_module());
    def("time", time_module());
    def("process", process_module());
}

fn math_module() -> Value {
    fn unary(name: &'static str, f: fn(f64) -> f64) -> (&'static str, Value) {
        (name, Value::native(name, move |it, a| Ok(Value::Num(f(num(it, a, 0, "x")?)))))
    }
    let mut entries = vec![
        ("pi", Value::Num(std::f64::consts::PI)),
        ("e", Value::Num(std::f64::consts::E)),
        ("infinity", Value::Num(f64::INFINITY)),
        unary("sqrt", f64::sqrt),
        unary("abs", f64::abs),
        unary("floor", f64::floor),
        unary("ceil", f64::ceil),
        unary("sin", f64::sin),
        unary("cos", f64::cos),
        unary("tan", f64::tan),
        unary("exp", f64::exp),
        unary("sign", f64::signum),
        unary("log10", f64::log10),
    ];
    entries.push((
        "round",
        Value::native("round", |it, a| {
            let x = num(it, a, 0, "x")?;
            let digits = opt_num(it, a, 1, "digits")?.unwrap_or(0.0);
            let factor = 10f64.powi(digits as i32);
            Ok(Value::Num((x * factor).round() / factor))
        }),
    ));
    entries.push(("pow", Value::native("pow", |it, a| Ok(Value::Num(num(it, a, 0, "base")?.powf(num(it, a, 1, "exponent")?))))));
    entries.push((
        "log",
        Value::native("log", |it, a| {
            let x = num(it, a, 0, "x")?;
            Ok(Value::Num(match opt_num(it, a, 1, "base")? {
                Some(b) => x.log(b),
                None => x.ln(),
            }))
        }),
    ));
    entries.push(("atan2", Value::native("atan2", |it, a| Ok(Value::Num(num(it, a, 0, "y")?.atan2(num(it, a, 1, "x")?))))));
    entries.push((
        "clamp",
        Value::native("clamp", |it, a| {
            let (x, lo, hi) = (num(it, a, 0, "x")?, num(it, a, 1, "min")?, num(it, a, 2, "max")?);
            Ok(Value::Num(x.max(lo).min(hi)))
        }),
    ));
    entries.push((
        "min",
        Value::native("min", |it, a| Ok(numbers_from_args(it, a)?.into_iter().reduce(f64::min).map(Value::Num).unwrap_or(Value::Nil))),
    ));
    entries.push((
        "max",
        Value::native("max", |it, a| Ok(numbers_from_args(it, a)?.into_iter().reduce(f64::max).map(Value::Num).unwrap_or(Value::Nil))),
    ));
    entries.push(("random", Value::native("random", |it, _| Ok(Value::Num(it.next_random())))));
    entries.push((
        "random_int",
        Value::native("random_int", |it, a| {
            let lo = num(it, a, 0, "min")?.ceil();
            let hi = num(it, a, 1, "max")?.floor();
            if hi < lo {
                return Err(it.error("random_int() needs min to be less than or equal to max", a.span, None));
            }
            Ok(Value::Num(lo + (it.next_random() * (hi - lo + 1.0)).floor()))
        }),
    ));
    module("math", entries)
}

fn json_module() -> Value {
    module(
        "json",
        vec![
            ("parse", Value::native("parse", |it, a| json::parse(it, &text(it, a, 0, "text")?, a.span))),
            (
                "stringify",
                Value::native("stringify", |it, a| {
                    let v = need(it, a, 0, "value")?.clone();
                    let pretty = a.get(1, "pretty").is_some_and(Value::truthy);
                    Ok(Value::string(json::stringify(it, &v, pretty, a.span)?))
                }),
            ),
        ],
    )
}

fn fs_module() -> Value {
    module(
        "fs",
        vec![
            (
                "read",
                Value::native("read", |it, a| {
                    let path = text(it, a, 0, "path")?;
                    std::fs::read_to_string(&*path).map(Value::string).map_err(|e| io_error(it, a, &path, e))
                }),
            ),
            (
                "write",
                Value::native("write", |it, a| {
                    let path = text(it, a, 0, "path")?;
                    let content = match need(it, a, 1, "text")? {
                        Value::Str(s) => s.to_string(),
                        other => {
                            return Err(it.error(
                                format!("fs.write() writes text, but got {}", with_article(&other.type_name())),
                                a.span,
                                Some("Convert it first, for example with json.stringify(value) or to_string(value).".into()),
                            ))
                        }
                    };
                    std::fs::write(&*path, content).map(|_| Value::Nil).map_err(|e| io_error(it, a, &path, e))
                }),
            ),
            (
                "append",
                Value::native("append", |it, a| {
                    use std::io::Write;
                    let path = text(it, a, 0, "path")?;
                    let content = text(it, a, 1, "text")?;
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&*path)
                        .and_then(|mut f| f.write_all(content.as_bytes()))
                        .map(|_| Value::Nil)
                        .map_err(|e| io_error(it, a, &path, e))
                }),
            ),
            ("exists", Value::native("exists", |it, a| Ok(Value::Bool(std::path::Path::new(&*text(it, a, 0, "path")?).exists())))),
            ("is_dir", Value::native("is_dir", |it, a| Ok(Value::Bool(std::path::Path::new(&*text(it, a, 0, "path")?).is_dir())))),
            (
                "list",
                Value::native("list", |it, a| {
                    let path = opt_text(it, a, 0, "path")?.unwrap_or_else(|| Rc::from("."));
                    let entries = std::fs::read_dir(&*path).map_err(|e| io_error(it, a, &path, e))?;
                    let mut names: Vec<String> = entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
                    names.sort();
                    Ok(Value::list(names.into_iter().map(Value::string).collect()))
                }),
            ),
            (
                "make_dir",
                Value::native("make_dir", |it, a| {
                    let path = text(it, a, 0, "path")?;
                    std::fs::create_dir_all(&*path).map(|_| Value::Nil).map_err(|e| io_error(it, a, &path, e))
                }),
            ),
            (
                "delete",
                Value::native("delete", |it, a| {
                    let path = text(it, a, 0, "path")?;
                    let p = std::path::Path::new(&*path);
                    let r = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
                    r.map(|_| Value::Nil).map_err(|e| io_error(it, a, &path, e))
                }),
            ),
        ],
    )
}

fn env_module() -> Value {
    module(
        "env",
        vec![
            (
                "get",
                Value::native("get", |it, a| {
                    let name = text(it, a, 0, "name")?;
                    Ok(match std::env::var(&*name) {
                        Ok(v) => Value::string(v),
                        Err(_) => a.get(1, "default").cloned().unwrap_or(Value::Nil),
                    })
                }),
            ),
            ("has", Value::native("has", |it, a| Ok(Value::Bool(std::env::var_os(&*text(it, a, 0, "name")?).is_some())))),
            (
                "all",
                Value::native("all", |_, _| {
                    let mut vars: Vec<(String, String)> = std::env::vars().collect();
                    vars.sort();
                    Ok(Value::object(vars.into_iter().map(|(k, v)| (k, Value::string(v))).collect()))
                }),
            ),
            (
                "load",
                Value::native("load", |it, a| {
                    let path = opt_text(it, a, 0, "path")?.unwrap_or_else(|| Rc::from(".env"));
                    let content = std::fs::read_to_string(&*path).map_err(|e| io_error(it, a, &path, e))?;
                    let mut count = 0.0;
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') {
                            continue;
                        }
                        let line = line.strip_prefix("export ").unwrap_or(line);
                        if let Some((k, v)) = line.split_once('=') {
                            let v = v.trim();
                            let v = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(v);
                            std::env::set_var(k.trim(), v);
                            count += 1.0;
                        }
                    }
                    Ok(Value::Num(count))
                }),
            ),
        ],
    )
}

fn http_module() -> Value {
    module(
        "http",
        vec![
            ("get", Value::native("get", |it, a| http::call(it, a, "GET", false))),
            ("delete", Value::native("delete", |it, a| http::call(it, a, "DELETE", false))),
            ("post", Value::native("post", |it, a| http::call(it, a, "POST", true))),
            ("put", Value::native("put", |it, a| http::call(it, a, "PUT", true))),
            ("patch", Value::native("patch", |it, a| http::call(it, a, "PATCH", true))),
            ("request", Value::native("request", |it, a| http::request(it, a))),
        ],
    )
}

fn time_module() -> Value {
    module(
        "time",
        vec![
            ("now", Value::native("now", |_, _| Ok(Value::Num(unix_millis())))),
            (
                "date",
                Value::native("date", |it, a| {
                    let ms = opt_num(it, a, 0, "time")?.unwrap_or_else(unix_millis);
                    let (y, mo, d, h, mi, s, wd) = date_parts(ms);
                    const DAYS: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
                    let mut f = Fields::new();
                    f.insert("year".into(), Value::Num(y as f64));
                    f.insert("month".into(), Value::Num(mo as f64));
                    f.insert("day".into(), Value::Num(d as f64));
                    f.insert("hour".into(), Value::Num(h as f64));
                    f.insert("minute".into(), Value::Num(mi as f64));
                    f.insert("second".into(), Value::Num(s as f64));
                    f.insert("weekday".into(), Value::text(DAYS[wd as usize]));
                    Ok(Value::object(f))
                }),
            ),
            (
                "iso",
                Value::native("iso", |it, a| {
                    let ms = opt_num(it, a, 0, "time")?.unwrap_or_else(unix_millis);
                    let (y, mo, d, h, mi, s, _) = date_parts(ms);
                    Ok(Value::string(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")))
                }),
            ),
        ],
    )
}

fn process_module() -> Value {
    module(
        "process",
        vec![
            ("args", Value::list(Vec::new())),
            ("platform", Value::text(std::env::consts::OS)),
            (
                "exit",
                Value::native("exit", |it, a| {
                    let code = opt_num(it, a, 0, "code")?.unwrap_or(0.0);
                    Err(Flow::Exit(code as i32))
                }),
            ),
            (
                "cwd",
                Value::native("cwd", |_, _| Ok(Value::string(std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default()))),
            ),
            (
                "run",
                Value::native("run", |it, a| {
                    let command = text(it, a, 0, "command")?;
                    let output = if cfg!(windows) {
                        std::process::Command::new("cmd").args(["/C", &command]).output()
                    } else {
                        std::process::Command::new("sh").args(["-c", &command]).output()
                    };
                    let out = output.map_err(|e| it.error(format!("couldn't run `{command}`: {e}"), a.span, None))?;
                    let mut f = Fields::new();
                    f.insert("code".into(), Value::Num(out.status.code().unwrap_or(-1) as f64));
                    f.insert("output".into(), Value::string(String::from_utf8_lossy(&out.stdout).into_owned()));
                    f.insert("error".into(), Value::string(String::from_utf8_lossy(&out.stderr).into_owned()));
                    Ok(Value::object(f))
                }),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        let (y, m, d, _, _, _, wd) = date_parts(1_789_084_800_000.0); // 2026-09-11
        assert_eq!((y, m, d, wd), (2026, 9, 11, 5));
    }

    #[test]
    fn number_parsing() {
        assert!(matches!(to_number(&Value::text(" 42 ")), Value::Num(n) if n == 42.0));
        assert!(matches!(to_number(&Value::text("abc")), Value::Nil));
        assert!(matches!(to_number(&Value::text("inf")), Value::Nil));
    }
}
