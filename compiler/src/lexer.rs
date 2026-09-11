//! Lexer: turns LiPi source text into tokens.
//!
//! LiPi uses indentation for blocks, so the lexer emits `Indent` / `Dedent`
//! tokens. Newlines inside brackets are ignored, and a line that starts with
//! `.` continues the previous line (for method chains).
//!
//! v0.1 identifiers are ASCII: letters, digits and `_`, not starting with a digit.

use crate::diagnostics::{Diagnostic, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kw {
    If,
    Else,
    For,
    In,
    While,
    Repeat,
    Break,
    Continue,
    Return,
    And,
    Or,
    Not,
    True,
    False,
    Null,
    Const,
    Use,
    From,
    As,
    Export,
    Function,
    Async,
    Await,
    Try,
    Catch,
    Finally,
    Throw,
    Match,
    Show,
}

const KEYWORDS: &[(&str, Kw)] = &[
    ("if", Kw::If),
    ("else", Kw::Else),
    ("for", Kw::For),
    ("in", Kw::In),
    ("while", Kw::While),
    ("repeat", Kw::Repeat),
    ("break", Kw::Break),
    ("continue", Kw::Continue),
    ("return", Kw::Return),
    ("and", Kw::And),
    ("or", Kw::Or),
    ("not", Kw::Not),
    ("true", Kw::True),
    ("false", Kw::False),
    ("null", Kw::Null),
    ("const", Kw::Const),
    ("use", Kw::Use),
    ("from", Kw::From),
    ("as", Kw::As),
    ("export", Kw::Export),
    ("function", Kw::Function),
    ("async", Kw::Async),
    ("await", Kw::Await),
    ("try", Kw::Try),
    ("catch", Kw::Catch),
    ("finally", Kw::Finally),
    ("throw", Kw::Throw),
    ("match", Kw::Match),
    ("show", Kw::Show),
];

impl Kw {
    pub fn lookup(s: &str) -> Option<Kw> {
        KEYWORDS.iter().find(|(k, _)| *k == s).map(|(_, kw)| *kw)
    }

    pub fn as_str(self) -> &'static str {
        KEYWORDS.iter().find(|(_, kw)| *kw == self).map(|(k, _)| *k).unwrap_or("?")
    }
}

/// A piece of a string literal: plain text, or the source span of an
/// interpolated `{expression}`.
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Lit(String),
    Code(Span),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Decimal(f64),
    Str(Vec<StrPart>),
    Ident(String),
    Kw(Kw),
    Plus,
    Minus,
    Star,
    StarStar,
    Slash,
    Percent,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    /// `...` (spread and rest)
    Ellipsis,
    QuestionDot,
    QuestionQuestion,
    Question,
    FatArrow,
    Arrow,
    Newline,
    Indent,
    Dedent,
    Eof,
}

impl Tok {
    /// How the token is described in error messages.
    pub fn describe(&self) -> String {
        let sym = match self {
            Tok::Int(n) => return format!("the number {n}"),
            Tok::Decimal(n) => return format!("the number {n}"),
            Tok::Str(_) => return "a string".into(),
            Tok::Ident(name) => return format!("\"{name}\""),
            Tok::Kw(k) => return format!("the keyword \"{}\"", k.as_str()),
            Tok::Newline => return "the end of the line".into(),
            Tok::Indent => return "an indented line".into(),
            Tok::Dedent => return "the end of the block".into(),
            Tok::Eof => return "the end of the file".into(),
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::StarStar => "**",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::Assign => "=",
            Tok::PlusAssign => "+=",
            Tok::MinusAssign => "-=",
            Tok::StarAssign => "*=",
            Tok::SlashAssign => "/=",
            Tok::Eq => "==",
            Tok::NotEq => "!=",
            Tok::Lt => "<",
            Tok::Gt => ">",
            Tok::LtEq => "<=",
            Tok::GtEq => ">=",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            Tok::Comma => ",",
            Tok::Colon => ":",
            Tok::Dot => ".",
            Tok::Ellipsis => "...",
            Tok::QuestionDot => "?.",
            Tok::QuestionQuestion => "??",
            Tok::Question => "?",
            Tok::FatArrow => "=>",
            Tok::Arrow => "->",
        };
        format!("'{sym}'")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

type Pos = (usize, u32, u32);

pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    end: usize,
    line: u32,
    col: u32,
    tokens: Vec<Token>,
    indents: Vec<usize>,
    brackets: Vec<(char, Span)>,
    at_line_start: bool,
    /// Lexing the inside of a `{...}` interpolation: no indentation rules.
    sub: bool,
}

pub fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

fn is_curly_quote(c: char) -> bool {
    matches!(c, '\u{201C}' | '\u{201D}' | '\u{2018}' | '\u{2019}')
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer {
            src,
            pos: 0,
            end: src.len(),
            line: 1,
            col: 1,
            tokens: Vec::new(),
            indents: vec![0],
            brackets: Vec::new(),
            at_line_start: true,
            sub: false,
        }
    }

    /// A lexer for the code inside a string interpolation. Spans refer to the full source.
    pub fn sub(src: &'a str, span: Span) -> Self {
        Lexer {
            src,
            pos: span.start,
            end: span.end,
            line: span.line,
            col: span.col,
            tokens: Vec::new(),
            indents: vec![0],
            brackets: Vec::new(),
            at_line_start: false,
            sub: true,
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..self.end].chars().next()
    }

    fn peek_n(&self, n: usize) -> Option<char> {
        self.src[self.pos..self.end].chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn here(&self) -> Pos {
        (self.pos, self.line, self.col)
    }

    fn span_from(&self, start: Pos) -> Span {
        Span::new(start.0, self.pos, start.1, start.2)
    }

    fn push(&mut self, tok: Tok, start: Pos) {
        let span = self.span_from(start);
        self.tokens.push(Token { tok, span });
    }

    fn last_is_newline_or_empty(&self) -> bool {
        matches!(self.tokens.last().map(|t| &t.tok), None | Some(Tok::Newline))
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, Diagnostic> {
        loop {
            if self.at_line_start {
                self.at_line_start = false;
                self.line_start()?;
            }
            while let Some(c) = self.peek() {
                let skip = matches!(c, ' ' | '\t' | '\r' | '\u{A0}' | '\u{FEFF}')
                    || (c == '\n' && (self.sub || !self.brackets.is_empty()));
                if !skip {
                    break;
                }
                self.bump();
            }
            let Some(c) = self.peek() else { break };
            let start = self.here();
            match c {
                '\n' => {
                    self.bump();
                    self.at_line_start = true;
                }
                '#' => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                '0'..='9' => self.number(start)?,
                '"' | '\'' => self.string(start, c)?,
                c if is_ident_start(c) => self.ident(start),
                c if c.is_alphabetic() => {
                    self.bump();
                    return Err(Diagnostic::error(format!("names can only use ASCII letters, digits and _ (found '{c}')"), self.span_from(start))
                        .with_code("LIP0004")
                        .with_hint("Non-English letters work inside strings and comments. Unicode names may come in a later LiPi version."));
                }
                _ => self.operator(start, c)?,
            }
        }
        if let Some((open, span)) = self.brackets.last() {
            let close = match open {
                '(' => ')',
                '[' => ']',
                _ => '}',
            };
            return Err(Diagnostic::error(format!("this '{open}' is never closed"), *span)
                .with_code("LIP0001")
                .with_hint(format!("Add a matching '{close}'.")));
        }
        let start = self.here();
        if !self.sub {
            if !self.last_is_newline_or_empty() {
                self.push(Tok::Newline, start);
            }
            while self.indents.len() > 1 {
                self.indents.pop();
                self.push(Tok::Dedent, start);
            }
        }
        self.push(Tok::Eof, start);
        Ok(self.tokens)
    }

    /// Measure indentation at the start of a line and emit Newline/Indent/Dedent.
    fn line_start(&mut self) -> Result<(), Diagnostic> {
        let mut width = 0;
        while let Some(c) = self.peek() {
            match c {
                ' ' | '\u{A0}' => width += 1,
                '\t' => width += 4,
                '\u{FEFF}' => {}
                _ => break,
            }
            self.bump();
        }
        match self.peek() {
            None | Some('\n') | Some('\r') | Some('#') => return Ok(()), // blank or comment-only line
            Some('.') if !matches!(self.peek_n(1), Some('0'..='9' | '.')) && !self.tokens.is_empty() => {
                return Ok(()); // `.method()` continues the previous line
            }
            _ => {}
        }
        let start = self.here();
        if !self.last_is_newline_or_empty() {
            self.push(Tok::Newline, start);
        }
        let top = *self.indents.last().unwrap();
        if width > top {
            self.indents.push(width);
            self.push(Tok::Indent, start);
        } else if width < top {
            while width < *self.indents.last().unwrap() {
                self.indents.pop();
                self.push(Tok::Dedent, start);
            }
            if width != *self.indents.last().unwrap() {
                return Err(Diagnostic::error(
                    "this line's indentation doesn't line up with any block above it",
                    Span::new(start.0, start.0 + 1, start.1, start.2),
                )
                .with_code("LIP0002")
                .with_hint("Use the same number of spaces as the line you want to line up with. LiPi uses 4 spaces per level."));
            }
        }
        Ok(())
    }

    fn number(&mut self, start: Pos) -> Result<(), Diagnostic> {
        let mut text = String::new();
        if self.peek() == Some('0') && matches!(self.peek_n(1), Some('x' | 'X')) {
            self.bump();
            self.bump();
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() {
                    text.push(c);
                } else if c != '_' {
                    break;
                }
                self.bump();
            }
            let value = i64::from_str_radix(&text, 16).map_err(|_| {
                Diagnostic::error("this hexadecimal number is not valid", self.span_from(start)).with_code("LIP0007")
            })?;
            self.push(Tok::Int(value), start);
            return Ok(());
        }
        let mut decimal = false;
        self.digits(&mut text);
        if self.peek() == Some('.') && matches!(self.peek_n(1), Some('0'..='9')) {
            decimal = true;
            text.push('.');
            self.bump();
            self.digits(&mut text);
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let next = self.peek_n(1);
            let after = self.peek_n(2);
            let signed = matches!(next, Some('+' | '-')) && matches!(after, Some('0'..='9'));
            if matches!(next, Some('0'..='9')) || signed {
                decimal = true;
                text.push('e');
                self.bump();
                if signed {
                    text.push(self.bump().unwrap());
                }
                self.digits(&mut text);
            }
        }
        if let Some(c) = self.peek() {
            if is_ident_start(c) {
                let span = self.span_from(start);
                return Err(Diagnostic::error(format!("a number can't be followed directly by '{c}'"), span)
                    .with_code("LIP0007")
                    .with_hint(format!("Did you mean {text} * {c}...? Put an operator between a number and a name.")));
            }
        }
        let tok = if decimal {
            Tok::Decimal(text.parse().map_err(|_| Diagnostic::error("this number is not valid", self.span_from(start)).with_code("LIP0007"))?)
        } else {
            Tok::Int(text.parse().map_err(|_| {
                Diagnostic::error("this Integer is too big", self.span_from(start))
                    .with_code("LIP0007")
                    .with_hint("Integers go up to 9223372036854775807. For bigger values, write a Decimal such as 1e20.")
            })?)
        };
        self.push(tok, start);
        Ok(())
    }

    fn digits(&mut self, text: &mut String) {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                text.push(c);
                self.bump();
            } else if c == '_' && matches!(self.peek_n(1), Some('0'..='9')) {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn string(&mut self, start: Pos, quote: char) -> Result<(), Diagnostic> {
        self.bump();
        let triple = self.peek() == Some(quote) && self.peek_n(1) == Some(quote);
        if triple {
            self.bump();
            self.bump();
            // A newline right after the opening quotes is not part of the text.
            self.eat('\r');
            self.eat('\n');
        }
        let unterminated = |lexer: &Lexer| {
            Diagnostic::error("this string never ends", lexer.span_from(start)).with_code("LIP0003").with_hint(if triple {
                format!("Add {quote}{quote}{quote} where the text should end.")
            } else {
                format!("Add a closing {quote} at the end of the text. For text over several lines, use {quote}{quote}{quote}.")
            })
        };
        let mut parts = Vec::new();
        let mut lit = String::new();
        loop {
            let Some(c) = self.peek() else { return Err(unterminated(self)) };
            if c == quote {
                if !triple {
                    self.bump();
                    break;
                }
                if self.peek_n(1) == Some(quote) && self.peek_n(2) == Some(quote) {
                    self.bump();
                    self.bump();
                    self.bump();
                    break;
                }
                lit.push(c);
                self.bump();
            } else if c == '\n' && !triple {
                return Err(unterminated(self));
            } else if c == '\r' {
                self.bump();
            } else if c == '\\' {
                let esc_start = self.here();
                self.bump();
                let Some(e) = self.bump() else { return Err(unterminated(self)) };
                match e {
                    'n' => lit.push('\n'),
                    't' => lit.push('\t'),
                    'r' => lit.push('\r'),
                    '0' => lit.push('\0'),
                    '\\' | '"' | '\'' | '{' | '}' => lit.push(e),
                    'u' if self.eat('{') => {
                        let mut hex = String::new();
                        while let Some(h) = self.peek() {
                            if h == '}' {
                                break;
                            }
                            hex.push(h);
                            self.bump();
                        }
                        let ok = self.eat('}');
                        let ch = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32);
                        match (ok, ch) {
                            (true, Some(ch)) => lit.push(ch),
                            _ => {
                                return Err(Diagnostic::error("this unicode escape is not valid", self.span_from(esc_start))
                                    .with_code("LIP0003")
                                    .with_hint("Write unicode characters like \\u{1F600}."))
                            }
                        }
                    }
                    // In 'single quotes' other backslashes stay as they are, so
                    // patterns read naturally: regex.find('\d+', text).
                    _ if quote == '\'' => {
                        lit.push('\\');
                        lit.push(e);
                    }
                    _ => {
                        return Err(Diagnostic::error(format!("unknown escape '\\{e}' in string"), self.span_from(esc_start))
                            .with_code("LIP0003")
                            .with_hint("Known escapes: \\n (new line), \\t (tab), \\\\ (backslash), \\\" (quote), \\{ (brace). For a regex pattern, use single quotes: '\\d+'"))
                    }
                }
            } else if c == '{' && quote == '"' {
                // Only double-quoted strings interpolate; 'single quotes' are literal.
                let brace = self.here();
                self.bump();
                let code_start = self.here();
                let mut depth = 1;
                let mut inner_quote: Option<char> = None;
                loop {
                    let Some(c) = self.peek() else {
                        return Err(Diagnostic::error("this '{' in the string is never closed", self.span_from(brace))
                            .with_code("LIP0003")
                            .with_hint("Close the expression with '}', or write \\{ for a literal brace."));
                    };
                    if let Some(q) = inner_quote {
                        if c == '\\' {
                            self.bump();
                        } else if c == q {
                            inner_quote = None;
                        }
                        self.bump();
                        continue;
                    }
                    match c {
                        '"' | '\'' => inner_quote = Some(c),
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        '\n' if !triple => return Err(unterminated(self)),
                        _ => {}
                    }
                    self.bump();
                }
                let code = self.span_from(code_start);
                self.bump(); // closing }
                if self.src[code.start..code.end].trim().is_empty() {
                    return Err(Diagnostic::error("empty {} in a string", self.span_from(brace))
                        .with_code("LIP0003")
                        .with_hint("Put a value inside, like \"Hello {name}\", or write \\{ for a literal brace."));
                }
                if !lit.is_empty() {
                    parts.push(StrPart::Lit(std::mem::take(&mut lit)));
                }
                parts.push(StrPart::Code(code));
            } else {
                lit.push(c);
                self.bump();
            }
        }
        if !lit.is_empty() || parts.is_empty() {
            parts.push(StrPart::Lit(lit));
        }
        self.push(Tok::Str(parts), start);
        Ok(())
    }

    fn ident(&mut self, start: Pos) {
        while let Some(c) = self.peek() {
            if !is_ident_continue(c) {
                break;
            }
            self.bump();
        }
        let text = &self.src[start.0..self.pos];
        let tok = match Kw::lookup(text) {
            Some(k) => Tok::Kw(k),
            None => Tok::Ident(text.to_string()),
        };
        self.push(tok, start);
    }

    fn operator(&mut self, start: Pos, c: char) -> Result<(), Diagnostic> {
        self.bump();
        let habit = |lexer: &Lexer, msg: &str, hint: &str| {
            Err(Diagnostic::error(msg, lexer.span_from(start)).with_code("LIP0008").with_hint(hint))
        };
        let tok = match c {
            '+' => {
                if self.eat('=') {
                    Tok::PlusAssign
                } else if self.peek() == Some('+') {
                    self.bump();
                    return habit(self, "LiPi doesn't have '++'", "To add one, write: count += 1");
                } else {
                    Tok::Plus
                }
            }
            '-' => {
                if self.eat('=') {
                    Tok::MinusAssign
                } else if self.eat('>') {
                    Tok::Arrow
                } else if self.peek() == Some('-') {
                    self.bump();
                    return habit(self, "LiPi doesn't have '--'", "To subtract one, write: count -= 1");
                } else {
                    Tok::Minus
                }
            }
            '*' => {
                if self.eat('*') {
                    Tok::StarStar
                } else if self.eat('=') {
                    Tok::StarAssign
                } else {
                    Tok::Star
                }
            }
            '/' => {
                if self.eat('=') {
                    Tok::SlashAssign
                } else if self.peek() == Some('/') || self.peek() == Some('*') {
                    self.bump();
                    return habit(self, "comments in LiPi start with #", "Write: # this is a comment");
                } else {
                    Tok::Slash
                }
            }
            '%' => Tok::Percent,
            '=' => {
                if self.eat('=') {
                    if self.eat('=') {
                        return habit(self, "LiPi doesn't have '==='", "Use == to compare. It never converts types behind your back.");
                    }
                    Tok::Eq
                } else if self.eat('>') {
                    Tok::FatArrow
                } else {
                    Tok::Assign
                }
            }
            '!' => {
                if self.eat('=') {
                    Tok::NotEq
                } else {
                    return habit(self, "LiPi uses the word `not` instead of '!'", "Write: if not done");
                }
            }
            '<' => {
                if self.eat('=') {
                    Tok::LtEq
                } else {
                    Tok::Lt
                }
            }
            '>' => {
                if self.eat('=') {
                    Tok::GtEq
                } else {
                    Tok::Gt
                }
            }
            '(' | '[' | '{' => {
                let span = self.span_from(start);
                self.brackets.push((c, span));
                match c {
                    '(' => Tok::LParen,
                    '[' => Tok::LBracket,
                    _ => Tok::LBrace,
                }
            }
            ')' | ']' | '}' => {
                let expected_open = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                match self.brackets.pop() {
                    Some((open, _)) if open == expected_open => {}
                    Some((open, span)) => {
                        let want = match open {
                            '(' => ')',
                            '[' => ']',
                            _ => '}',
                        };
                        return Err(Diagnostic::error(format!("found '{c}' but the '{open}' opened earlier needs '{want}'"), self.span_from(start))
                            .with_code("LIP0001")
                            .with_hint(format!("The '{open}' was opened on line {}.", span.line)));
                    }
                    None => {
                        return Err(Diagnostic::error(format!("this '{c}' doesn't close anything"), self.span_from(start))
                            .with_code("LIP0001")
                            .with_hint("Remove it, or add the matching opening bracket."));
                    }
                }
                match c {
                    ')' => Tok::RParen,
                    ']' => Tok::RBracket,
                    _ => Tok::RBrace,
                }
            }
            ',' => Tok::Comma,
            ':' => Tok::Colon,
            '.' if self.peek() == Some('.') && self.peek_n(1) == Some('.') => {
                self.bump();
                self.bump();
                Tok::Ellipsis
            }
            '.' => Tok::Dot,
            '?' => {
                if self.eat('.') {
                    Tok::QuestionDot
                } else if self.eat('?') {
                    Tok::QuestionQuestion
                } else {
                    Tok::Question
                }
            }
            '&' => {
                self.eat('&');
                return habit(self, "LiPi uses the word `and` instead of '&&'", "Write: if ready and valid");
            }
            '|' => {
                self.eat('|');
                return habit(self, "LiPi uses the word `or` instead of '||'", "Write: if empty or broken");
            }
            ';' => return habit(self, "LiPi doesn't use semicolons", "Put each statement on its own line."),
            c if is_curly_quote(c) => {
                return Err(Diagnostic::error("this is a curly quote", self.span_from(start))
                    .with_code("LIP0004")
                    .with_hint("Use straight quotes (\") instead. Word processors often replace them with curly ones."));
            }
            _ => {
                return Err(Diagnostic::error(format!("I don't understand the character '{c}'"), self.span_from(start))
                    .with_code("LIP0004")
                    .with_hint("Remove it, or put it inside a string."))
            }
        };
        self.push(tok, start);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        Lexer::new(src).tokenize().unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn show_hello() {
        assert_eq!(
            kinds("show \"Hello World\""),
            vec![Tok::Kw(Kw::Show), Tok::Str(vec![StrPart::Lit("Hello World".into())]), Tok::Newline, Tok::Eof]
        );
    }

    #[test]
    fn indentation() {
        let toks = kinds("if a\n    b\nc\n");
        assert_eq!(
            toks,
            vec![
                Tok::Kw(Kw::If),
                Tok::Ident("a".into()),
                Tok::Newline,
                Tok::Indent,
                Tok::Ident("b".into()),
                Tok::Newline,
                Tok::Dedent,
                Tok::Ident("c".into()),
                Tok::Newline,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn brackets_ignore_newlines() {
        let toks = kinds("x = [\n  1,\n  2\n]\n");
        assert!(!toks.contains(&Tok::Indent));
        assert_eq!(toks.iter().filter(|t| **t == Tok::Newline).count(), 1);
    }

    #[test]
    fn integers_and_decimals() {
        assert_eq!(kinds("1_000 2.5 1e3 0xff")[..4], [Tok::Int(1000), Tok::Decimal(2.5), Tok::Decimal(1000.0), Tok::Int(255)]);
    }

    #[test]
    fn interpolation_parts() {
        let toks = kinds("\"Hi {name}!\"");
        let Tok::Str(parts) = &toks[0] else { panic!() };
        assert_eq!(parts.len(), 3);
        assert!(matches!(parts[1], StrPart::Code(_)));
    }

    #[test]
    fn helpful_errors() {
        let e = Lexer::new("x = \"abc").tokenize().unwrap_err();
        assert_eq!(e.code, Some("LIP0003"));
        let e = Lexer::new("a && b").tokenize().unwrap_err();
        assert!(e.message.contains("and"));
        let e = Lexer::new("if a\n        b\n    c\n").tokenize().unwrap_err();
        assert_eq!(e.code, Some("LIP0002"));
        let e = Lexer::new("नाम = 1").tokenize().unwrap_err();
        assert_eq!(e.code, Some("LIP0004"));
    }
}
