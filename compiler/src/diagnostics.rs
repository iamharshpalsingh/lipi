//! Diagnostics: errors and warnings that teach.
//!
//! Every message states what went wrong in plain language, points at the
//! exact source location and, where possible, offers a safe suggestion.

use std::fmt::Write;

/// A region of source text. `line` and `col` are 1-based and describe `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub fn new(start: usize, end: usize, line: u32, col: u32) -> Self {
        Span { start, end, line, col }
    }

    /// A span covering both `self` and `other` (which must come later).
    pub fn to(self, other: Span) -> Span {
        Span { start: self.start, end: other.end.max(self.end), line: self.line, col: self.col }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
    pub hint: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Diagnostic { severity: Severity::Error, message: message.into(), span: Some(span), hint: None }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Diagnostic { severity: Severity::Warning, message: message.into(), span: Some(span), hint: None }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn maybe_hint(mut self, hint: Option<String>) -> Self {
        if hint.is_some() {
            self.hint = hint;
        }
        self
    }

    /// Render the diagnostic the way the CLI prints it:
    ///
    /// ```text
    /// ERROR: expected a number
    ///   --> main.lipi:2:9
    ///    |
    ///  2 | price = age + 10
    ///    |         ^^^
    /// Hint: "age" is a string. ...
    /// ```
    pub fn render(&self, source: &str, file: &str, color: bool) -> String {
        let (red, yellow, blue, cyan, bold, reset) = if color {
            ("\x1b[31m", "\x1b[33m", "\x1b[34m", "\x1b[36m", "\x1b[1m", "\x1b[0m")
        } else {
            ("", "", "", "", "", "")
        };
        let (label, label_color) = match self.severity {
            Severity::Error => ("ERROR", red),
            Severity::Warning => ("WARNING", yellow),
        };
        let mut out = String::new();
        let _ = writeln!(out, "{bold}{label_color}{label}{reset}{bold}: {}{reset}", self.message);
        if let Some(span) = self.span {
            let _ = writeln!(out, "  {blue}-->{reset} {}:{}:{}", file, span.line, span.col);
            if let Some(line_text) = source.lines().nth(span.line.saturating_sub(1) as usize) {
                let num = span.line.to_string();
                let pad = " ".repeat(num.len());
                let line_text = line_text.trim_end_matches('\r');
                let _ = writeln!(out, " {pad} {blue}|{reset}");
                let _ = writeln!(out, " {blue}{num} |{reset} {}", line_text.replace('\t', "    "));
                // Width of the underline: the span's text on this line, at least one caret.
                let line_start = line_start_offset(source, span.line);
                let line_end = line_start + line_text.len();
                let from = span.start.clamp(line_start, line_end);
                let to = span.end.clamp(from, line_end);
                let prefix: String = source[line_start..from].replace('\t', "    ");
                let width = source[from..to].chars().count().max(1);
                let _ = writeln!(
                    out,
                    " {pad} {blue}|{reset} {}{label_color}{}{reset}",
                    " ".repeat(prefix.chars().count()),
                    "^".repeat(width)
                );
            }
        }
        if let Some(hint) = &self.hint {
            let _ = writeln!(out, "{cyan}Hint:{reset} {hint}");
        }
        out
    }
}

fn line_start_offset(source: &str, line: u32) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut current = 1;
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            current += 1;
            if current == line {
                return i + 1;
            }
        }
    }
    source.len()
}
