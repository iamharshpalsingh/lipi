//! Diagnostics: errors and warnings that teach.
//!
//! Every diagnostic answers: what happened, where, why, and what to do next.
//! Each has a stable code (see docs/SPEC.md §24), for example:
//!
//! ```text
//! ERROR LIP1002: undefined variable "usr"
//!
//! main.lipi:8:10
//!     show usr.name
//!          ^^^
//!
//! Hint: did you mean "user"?
//! ```

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
    /// Stable error code such as "LIP1002".
    pub code: Option<&'static str>,
    pub message: String,
    pub span: Option<Span>,
    pub hint: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Diagnostic { severity: Severity::Error, code: None, message: message.into(), span: Some(span), hint: None }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Diagnostic { severity: Severity::Warning, code: None, message: message.into(), span: Some(span), hint: None }
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

    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    /// Set the code unless one was already chosen.
    pub fn code_or(mut self, code: &'static str) -> Self {
        if self.code.is_none() {
            self.code = Some(code);
        }
        self
    }

    /// The diagnostic class, derived from the code.
    pub fn category(&self) -> &'static str {
        match self.code.and_then(|c| c.get(3..4)) {
            Some("0") => "Syntax",
            Some("1") => "Name",
            Some("2") => "Type",
            Some("3") => "Module",
            Some("4") => "Async",
            Some("5") => "Runtime",
            Some("6") => "Security",
            Some("7") => "Package",
            _ => "Error",
        }
    }

    /// Render the diagnostic the way the CLI prints it.
    pub fn render(&self, source: &str, file: &str, color: bool) -> String {
        let (red, yellow, cyan, bold, dim, reset) = if color {
            ("\x1b[31m", "\x1b[33m", "\x1b[36m", "\x1b[1m", "\x1b[2m", "\x1b[0m")
        } else {
            ("", "", "", "", "", "")
        };
        let (label, label_color) = match self.severity {
            Severity::Error => ("ERROR", red),
            Severity::Warning => ("WARNING", yellow),
        };
        let code = self.code.map(|c| format!(" {c}")).unwrap_or_default();
        let mut out = String::new();
        let _ = writeln!(out, "{bold}{label_color}{label}{code}{reset}{bold}: {}{reset}", self.message);
        let _ = writeln!(out);
        match self.span {
            Some(span) => {
                let _ = writeln!(out, "{dim}{}:{}:{}{reset}", file, span.line, span.col);
                if let Some(line_text) = source.lines().nth(span.line.saturating_sub(1) as usize) {
                    let line_text = line_text.trim_end_matches('\r');
                    let _ = writeln!(out, "    {}", line_text.replace('\t', "    "));
                    let line_start = line_start_offset(source, span.line);
                    let line_end = line_start + line_text.len();
                    let from = span.start.clamp(line_start, line_end);
                    let to = span.end.clamp(from, line_end);
                    let prefix = source[line_start..from].replace('\t', "    ");
                    let width = source[from..to].chars().count().max(1);
                    let _ = writeln!(out, "    {}{label_color}{}{reset}", " ".repeat(prefix.chars().count()), "^".repeat(width));
                }
            }
            None => {
                let _ = writeln!(out, "{dim}{file}{reset}");
            }
        }
        if let Some(hint) = &self.hint {
            let _ = writeln!(out);
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
