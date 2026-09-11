//! The `regex` module: text patterns that mean the same in `lipi run` and in
//! JavaScript builds.
//!
//! `\d` and `\w` are ASCII, `\b` uses ASCII word characters and `.` doesn't
//! match line breaks, as in JavaScript. Lookaround and backreferences aren't
//! available in either engine. Positions count characters, like everywhere
//! else in LiPi.

use crate::builtins::{self, callable, need, text, wrong};
use crate::interp::{Flow, Interpreter};
use crate::value::*;
use lipi_compiler::ast::{Expr, ExprKind};
use lipi_compiler::checker::{condition_error, with_article};
use regex::{Captures, Regex, RegexBuilder};

/// Lookahead, lookbehind and backreferences (JavaScript-only syntax).
fn unsupported(p: &str) -> bool {
    ["(?=", "(?!", "(?<=", "(?<!", "\\k<"].iter().any(|s| p.contains(s)) || p.as_bytes().windows(2).any(|w| w[0] == b'\\' && (b'1'..=b'9').contains(&w[1]))
}

/// A LiPi pattern in the regex crate's syntax.
fn translate(p: &str) -> String {
    let mut out = String::with_capacity(p.len() + 8);
    let mut chars = p.chars().peekable();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let Some(d) = chars.next() else {
                    out.push('\\');
                    break;
                };
                match (d, in_class) {
                    ('d', false) => out.push_str("[0-9]"),
                    ('D', false) => out.push_str("[^0-9]"),
                    ('w', false) => out.push_str("[0-9A-Za-z_]"),
                    ('W', false) => out.push_str("[^0-9A-Za-z_]"),
                    ('d', true) => out.push_str("0-9"),
                    ('w', true) => out.push_str("0-9A-Za-z_"),
                    ('b', false) => out.push_str(r"(?-u:\b)"),
                    ('B', false) => out.push_str(r"(?-u:\B)"),
                    _ => {
                        out.push('\\');
                        out.push(d);
                    }
                }
            }
            '[' if !in_class => {
                in_class = true;
                out.push('[');
                if chars.peek() == Some(&'^') {
                    out.push('^');
                    chars.next();
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            // Characters that mean something else inside a class in the regex crate.
            '[' | '&' | '~' if in_class => {
                out.push('\\');
                out.push(c);
            }
            '.' if !in_class => out.push_str(r"[^\n\r\u{2028}\u{2029}]"),
            _ => out.push(c),
        }
    }
    out
}

/// A Boolean option given by name (`ignoreCase: true`).
fn option(it: &Interpreter, a: &Args, name: &str) -> Result<bool, Flow> {
    match a.named.iter().find(|(n, _)| n == name) {
        None => Ok(false),
        Some((_, Value::Bool(b))) => Ok(*b),
        Some((_, v)) => {
            let d = condition_error(&Expr { res: Default::default(), kind: ExprKind::Null, span: a.span }, &v.type_name());
            Err(it.err("LIP2005", d.message, a.span, d.hint))
        }
    }
}

fn compile(it: &Interpreter, a: &Args) -> Result<Regex, Flow> {
    let pattern = text(it, a, 0, "pattern")?;
    if unsupported(&pattern) {
        return Err(it.err(
            "LIP5008",
            "LiPi patterns can't use lookahead, lookbehind or backreferences",
            a.span,
            Some("Match the text more simply, or check the parts separately.".into()),
        ));
    }
    let ignore_case = option(it, a, "ignoreCase")?;
    let multiline = option(it, a, "multiline")?;
    RegexBuilder::new(&translate(&pattern)).case_insensitive(ignore_case).multi_line(multiline).build().map_err(|_| {
        it.err(
            "LIP5008",
            format!("\"{pattern}\" isn't a valid pattern"),
            a.span,
            Some("Check the brackets and backslashes. To match a symbol like . or ( itself, put \\ before it: \\. or \\(".into()),
        )
    })
}

/// Every match with its position in characters.
fn found<'t>(re: &Regex, text: &'t str) -> Vec<(Captures<'t>, i64)> {
    let mut out = Vec::new();
    let (mut at, mut chars) = (0, 0i64);
    for c in re.captures_iter(text) {
        let start = c.get(0).expect("group 0 is the whole match").start();
        chars += text[at..start].chars().count() as i64;
        at = start;
        out.push((c, chars));
    }
    out
}

/// `{text, index, groups, named}`
fn match_object(re: &Regex, c: &Captures, index: i64) -> Value {
    let groups = (1..c.len()).map(|i| c.get(i).map_or(Value::Nil, |m| Value::text(m.as_str()))).collect();
    let mut named = Fields::new();
    for name in re.capture_names().flatten() {
        named.insert(name.to_string(), c.name(name).map_or(Value::Nil, |m| Value::text(m.as_str())));
    }
    let mut f = Fields::new();
    f.insert("text".into(), Value::text(&c[0]));
    f.insert("index".into(), Value::Int(index));
    f.insert("groups".into(), Value::list(groups));
    f.insert("named".into(), Value::object(named));
    Value::object(f)
}

/// "$1", "$<name>", "$&" and "$$" in a replacement, as in JavaScript.
fn expand(rep: &str, re: &Regex, c: &Captures) -> String {
    let chars: Vec<char> = rep.chars().collect();
    let group = |n: usize| c.get(n).map_or("", |m| m.as_str()).to_string();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch != '$' || i + 1 >= chars.len() {
            out.push(ch);
            i += 1;
            continue;
        }
        let d = chars[i + 1];
        match d {
            '$' => {
                out.push('$');
                i += 2;
            }
            '&' => {
                out.push_str(&c[0]);
                i += 2;
            }
            '<' => {
                let close = chars[i + 2..].iter().position(|&x| x == '>').map(|p| p + i + 2);
                let name: Option<String> = close.map(|close| chars[i + 2..close].iter().collect());
                match (close, name) {
                    (Some(close), Some(name)) if re.capture_names().flatten().any(|n| n == name) => {
                        out.push_str(c.name(&name).map_or("", |m| m.as_str()));
                        i = close + 1;
                    }
                    _ => {
                        out.push(ch);
                        i += 1;
                    }
                }
            }
            '0'..='9' => {
                let two = chars.get(i + 2).filter(|x| x.is_ascii_digit()).map(|x| (d.to_digit(10).unwrap_or(0) * 10 + x.to_digit(10).unwrap_or(0)) as usize);
                let one = d.to_digit(10).unwrap_or(0) as usize;
                if let Some(n) = two.filter(|&n| n >= 1 && n < c.len()) {
                    out.push_str(&group(n));
                    i += 3;
                } else if one >= 1 && one < c.len() {
                    out.push_str(&group(one));
                    i += 2;
                } else {
                    out.push(ch);
                    i += 1;
                }
            }
            _ => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

pub fn module() -> Value {
    builtins::module(
        "regex",
        vec![
            (
                "test",
                Value::native("test", |it, a| {
                    let re = compile(it, a)?;
                    Ok(Value::Bool(re.is_match(&text(it, a, 1, "text")?)))
                }),
            ),
            (
                "find",
                Value::native("find", |it, a| {
                    let re = compile(it, a)?;
                    let t = text(it, a, 1, "text")?;
                    Ok(found(&re, &t).first().map_or(Value::Nil, |(c, i)| match_object(&re, c, *i)))
                }),
            ),
            (
                "findAll",
                Value::native("findAll", |it, a| {
                    let re = compile(it, a)?;
                    let t = text(it, a, 1, "text")?;
                    Ok(Value::list(found(&re, &t).iter().map(|(c, i)| match_object(&re, c, *i)).collect()))
                }),
            ),
            (
                "replace",
                Value::native("replace", |it, a| {
                    let re = compile(it, a)?;
                    let t = text(it, a, 1, "text")?;
                    let rep = need(it, a, 2, "replacement")?.clone();
                    if !matches!(rep, Value::Str(_) | Value::Func(_) | Value::Native(_) | Value::Method(_)) {
                        return Err(wrong(it, a, "replacement", "String", &rep));
                    }
                    let function = if matches!(rep, Value::Str(_)) { None } else { Some(callable(it, a, 2, "replacement")?) };
                    let mut out = String::new();
                    let mut last = 0;
                    for (c, index) in found(&re, &t) {
                        let m = c.get(0).expect("group 0 is the whole match");
                        out.push_str(&t[last..m.start()]);
                        match (&function, &rep) {
                            (None, Value::Str(r)) => out.push_str(&expand(r, &re, &c)),
                            (Some(f), _) => match it.call_callback(f, vec![match_object(&re, &c, index)], a.span)? {
                                Value::Str(s) => out.push_str(&s),
                                other => {
                                    return Err(it.err(
                                        "LIP5008",
                                        format!("the function given to regex.replace() must return a String, but it returned {}", with_article(&other.type_name())),
                                        a.span,
                                        Some("For example: regex.replace(\"\\\\d+\", text, m => \"<{m.text}>\")".into()),
                                    ))
                                }
                            },
                            _ => unreachable!("checked above"),
                        }
                        last = m.end();
                    }
                    out.push_str(&t[last..]);
                    Ok(Value::string(out))
                }),
            ),
            (
                "split",
                Value::native("split", |it, a| {
                    let re = compile(it, a)?;
                    let t = text(it, a, 1, "text")?;
                    let mut out = Vec::new();
                    let mut last = 0;
                    for m in re.find_iter(&t) {
                        out.push(Value::text(&t[last..m.start()]));
                        last = m.end();
                    }
                    out.push(Value::text(&t[last..]));
                    Ok(Value::list(out))
                }),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_follow_javascript() {
        let re = Regex::new(&translate(r"\d+\w*.")).unwrap();
        assert!(re.is_match("12ab!"));
        assert!(!Regex::new(&translate(r"^\d$")).unwrap().is_match("٣")); // Arabic-Indic three isn't a \d
        assert!(!Regex::new(&translate("^a.b$")).unwrap().is_match("a\nb"));
        assert!(Regex::new(&translate(r"[\d.]+")).unwrap().is_match("3.5"));
        assert!(Regex::new(&translate(r"\bcat\b")).unwrap().is_match("a cat!"));
        assert!(unsupported("(?=x)") && unsupported(r"(a)\1") && !unsupported(r"(?<year>\d+)"));
        // Empty matches right after a match are skipped (JavaScript's matchAll is filtered the same way).
        let re = Regex::new("b*").unwrap();
        let starts: Vec<usize> = re.find_iter("abc").map(|m| m.start()).collect();
        assert_eq!(starts, vec![0, 1, 3]);
    }
}
