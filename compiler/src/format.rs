//! The LiPi formatter: one canonical layout for every program.
//!
//! - 4 spaces per indentation level (tabs become spaces), `\n` line endings,
//!   no trailing whitespace, one final newline.
//! - One space around binary operators, `=`, `=>`, `->`, `??`, `and`, `or`...;
//!   one space after `,` and `:`; none inside `()`, `[]`, `{}`, before `,` or `:`,
//!   around `.`, or between a function and its `(`.
//! - Runs of blank lines collapse to one; comments keep their text and move to
//!   the indentation of the code they precede; inline comments get two spaces.
//! - Strings are copied exactly.
//!
//! The formatter works on tokens rather than the syntax tree, so comments are
//! never lost. As a safety net it re-reads its output and refuses to produce
//! anything whose tokens differ from the input. Formatting is idempotent.

use crate::diagnostics::{Diagnostic, Severity};
use crate::lexer::{Kw, Lexer, StrPart, Tok, Token};

/// Format a whole source file.
pub fn format_source(src: &str) -> Result<String, Diagnostic> {
    let text = src.replace("\r\n", "\n");
    crate::parse_source(&text)?; // only valid programs are formatted
    let tokens = Lexer::new(&text).tokenize()?;
    let out = render(&text, &tokens);
    let again = Lexer::new(&out).tokenize().ok();
    if again.as_deref().map(keys) != Some(keys(&tokens)) {
        return Err(Diagnostic {
            severity: Severity::Error,
            code: Some("LIP0001"),
            message: "internal formatter error: formatting would change this program, so the file was left unchanged".into(),
            span: None,
            hint: Some("Please report this, with the file, so it can be fixed.".into()),
        });
    }
    Ok(out)
}

/// Token kinds without positions, used to prove formatting didn't change the program.
fn keys(tokens: &[Token]) -> Vec<String> {
    tokens
        .iter()
        .map(|t| match &t.tok {
            Tok::Str(parts) => parts
                .iter()
                .map(|p| match p {
                    StrPart::Lit(s) => format!("{s:?}"),
                    StrPart::Code(span) => format!("{{{}}}", span.end - span.start),
                })
                .collect::<Vec<_>>()
                .join(""),
            other => format!("{other:?}"),
        })
        .collect()
}

struct Item {
    token: usize,
    level: usize,
    depth: usize,
}

struct Group {
    items: Vec<usize>,
    start_line: u32,
    end_line: u32,
    indent: usize,
}

fn is_open(t: &Tok) -> bool {
    matches!(t, Tok::LParen | Tok::LBracket | Tok::LBrace)
}

fn is_close(t: &Tok) -> bool {
    matches!(t, Tok::RParen | Tok::RBracket | Tok::RBrace)
}

/// Ends a value, so a following `(` or `[` is a call or index.
fn ends_value(t: &Tok) -> bool {
    matches!(t, Tok::Ident(_) | Tok::Str(_) | Tok::RParen | Tok::RBracket | Tok::RBrace)
}

fn is_member_access(t: Option<&Tok>) -> bool {
    matches!(t, Some(Tok::Dot | Tok::QuestionDot))
}

/// Is `prev` (preceded by `prev2`) something a value can follow, so a `-` negates?
/// A name right after `.` is a value even when it is a keyword (`text.repeat`),
/// and `to`/`step` act as operators when they follow a value (`10 to -5`).
fn unary_context(prev: Option<&Tok>, prev2: Option<&Tok>) -> bool {
    let Some(p) = prev else { return true };
    if is_member_access(prev2) {
        return false;
    }
    match p {
        Tok::Ident(n) if n == "to" || n == "step" => {
            prev2.is_some_and(|q| ends_value(q) || matches!(q, Tok::Int(_) | Tok::Decimal(_) | Tok::Kw(Kw::True | Kw::False | Kw::Null)))
        }
        Tok::Ident(_) | Tok::Int(_) | Tok::Decimal(_) | Tok::Str(_) | Tok::RParen | Tok::RBracket | Tok::RBrace | Tok::Question => false,
        Tok::Kw(k) => !matches!(k, Kw::True | Kw::False | Kw::Null),
        _ => true,
    }
}

fn spacing(prev: &Tok, prev2: Option<&Tok>, next: &Tok, prev_is_unary: bool) -> &'static str {
    if is_close(next) || matches!(next, Tok::Comma | Tok::Colon | Tok::Dot | Tok::QuestionDot | Tok::Question) {
        return "";
    }
    if is_open(prev) || matches!(prev, Tok::Dot | Tok::QuestionDot | Tok::Ellipsis) || prev_is_unary {
        return "";
    }
    if matches!(next, Tok::LParen | Tok::LBracket) && (ends_value(prev) || is_member_access(prev2)) {
        return "";
    }
    " "
}

fn render(text: &str, tokens: &[Token]) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for l in &lines {
        line_starts.push(offset);
        offset += l.len() + 1;
    }

    // Indentation level and bracket depth for every real token.
    let mut items = Vec::new();
    let (mut level, mut depth) = (0usize, 0usize);
    for (i, t) in tokens.iter().enumerate() {
        match &t.tok {
            Tok::Indent => level += 1,
            Tok::Dedent => level = level.saturating_sub(1),
            Tok::Newline | Tok::Eof => {}
            tok => {
                items.push(Item { token: i, level, depth });
                if is_open(tok) {
                    depth += 1;
                } else if is_close(tok) {
                    depth = depth.saturating_sub(1);
                }
            }
        }
    }

    // Group tokens by output line (a multi-line string keeps its tokens together).
    let end_line = |t: &Token| t.span.line + text[t.span.start..t.span.end].matches('\n').count() as u32;
    let mut groups: Vec<Group> = Vec::new();
    for (k, item) in items.iter().enumerate() {
        let t = &tokens[item.token];
        match groups.last_mut() {
            Some(g) if t.span.line <= g.end_line => {
                g.items.push(k);
                g.end_line = g.end_line.max(end_line(t));
            }
            _ => {
                let indent = if item.depth > 0 {
                    item.level + item.depth - usize::from(is_close(&t.tok))
                } else if matches!(t.tok, Tok::Dot | Tok::QuestionDot) {
                    item.level + 1
                } else {
                    item.level
                };
                groups.push(Group { items: vec![k], start_line: t.span.line, end_line: end_line(t), indent });
            }
        }
    }

    let render_group = |g: &Group| -> String {
        let mut s = "    ".repeat(g.indent);
        let mut prev: Option<&Tok> = None;
        let mut prev2: Option<&Tok> = None;
        let mut prev_unary = false;
        for &k in &g.items {
            let t = &tokens[items[k].token];
            if let Some(p) = prev {
                s.push_str(spacing(p, prev2, &t.tok, prev_unary));
            }
            s.push_str(&text[t.span.start..t.span.end]);
            prev_unary = matches!(t.tok, Tok::Minus) && unary_context(prev, prev2);
            prev2 = prev;
            prev = Some(&t.tok);
        }
        let last = &tokens[items[*g.items.last().expect("groups are never empty")].token];
        let idx = (g.end_line - 1) as usize;
        let line_end = line_starts[idx] + lines[idx].len();
        let rest = text[last.span.end.min(line_end)..line_end].trim();
        if rest.starts_with('#') {
            s.push_str("  ");
            s.push_str(rest);
        }
        s
    };

    let mut out = String::new();
    let mut pending_blank = false;
    let emit = |out: &mut String, s: &str, pending: &mut bool| {
        if *pending && !out.is_empty() {
            out.push('\n');
        }
        *pending = false;
        out.push_str(s.trim_end());
        out.push('\n');
    };
    let mut gi = 0;
    let mut line = 1u32;
    while line as usize <= lines.len() {
        if gi < groups.len() && groups[gi].start_line == line {
            let s = render_group(&groups[gi]);
            emit(&mut out, &s, &mut pending_blank);
            line = groups[gi].end_line + 1;
            gi += 1;
            continue;
        }
        let raw = lines[line as usize - 1].trim();
        if raw.is_empty() {
            pending_blank = true;
        } else {
            let indent = groups.get(gi).map_or(0, |g| g.indent);
            emit(&mut out, &format!("{}{raw}", "    ".repeat(indent)), &mut pending_blank);
        }
        line += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_layout() {
        let messy = "x=1+2*3\nif x>=7\n  show  \"big\" ,x   # comment\nitems=[ 1,2 ,-3 ]\n\n\n\nf( a,b )\n\treturn a+b\nuser={name:\"A\" ,age:25}\nname : String = \"x\"\ny = f(-1) - -2\n";
        let expected = "x = 1 + 2 * 3\nif x >= 7\n    show \"big\", x  # comment\nitems = [1, 2, -3]\n\nf(a, b)\n    return a + b\nuser = {name: \"A\", age: 25}\nname: String = \"x\"\ny = f(-1) - -2\n";
        assert_eq!(format_source(messy).unwrap(), expected);
    }

    #[test]
    fn keywords_as_names_and_soft_operators() {
        let src = "line = \"-\".repeat(10)\nfor n in 10 to 0 step -5\n    show n\nto = 3\nx = to - 1\nk = obj.show(1)\n";
        assert_eq!(format_source(src).unwrap(), src);
    }

    #[test]
    fn comments_and_continuations() {
        let src = "# top\ntotal = [\n  1,\n      2,\n]\nnames = people\n.filter(p => p.age >= 18)\nif ok\n    show 1\n        # trailing note\nshow 2\n";
        let out = format_source(src).unwrap();
        assert_eq!(out, "# top\ntotal = [\n    1,\n    2,\n]\nnames = people\n    .filter(p => p.age >= 18)\nif ok\n    show 1\n# trailing note\nshow 2\n");
    }

    #[test]
    fn idempotent_on_all_programs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut count = 0;
        for dir in ["tests/language", "examples", "tests/server", "tests/postgres"] {
            for entry in std::fs::read_dir(root.join(dir)).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "lipi") {
                    let src = std::fs::read_to_string(&path).unwrap();
                    let Ok(once) = format_source(&src) else { continue }; // error test files don't parse
                    let twice = format_source(&once).unwrap();
                    assert_eq!(once, twice, "not idempotent: {}", path.display());
                    count += 1;
                }
            }
        }
        assert!(count > 10);
    }
}
