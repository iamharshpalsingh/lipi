//! Parser: turns tokens into an AST.
//!
//! A hand-written recursive-descent parser. Errors explain the problem in
//! plain language and suggest the LiPi way of writing things.

use crate::ast::*;
use crate::diagnostics::{Diagnostic, Span};
use crate::lexer::{Kw, Lexer, StrPart, Tok, Token};
use crate::suggest;
use std::rc::Rc;

type PResult<T> = Result<T, Diagnostic>;

/// Words kept free for upcoming LiPi features (they can't be used as names).
pub const RESERVED: &[&str] = &["state", "route", "component", "server"];

pub struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    pos: usize,
    /// Set while parsing match patterns, where `if` starts a guard instead.
    no_inline_if: bool,
}

/// A name or dotted name, like `get` or `server.start`.
fn is_path(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Ident(_) => true,
        ExprKind::Field { object, optional: false, .. } => is_path(object),
        _ => false,
    }
}

const BLOCK_EXAMPLE: &str = "Indent the lines that belong to it by 4 spaces, for example:\n    if age >= 18\n        show \"Adult\"";

impl<'a> Parser<'a> {
    pub fn new(src: &'a str, toks: Vec<Token>) -> Self {
        Parser { src, toks, pos: 0, no_inline_if: false }
    }

    // ----- token helpers -------------------------------------------------

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn advance(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }

    fn at_kw(&self, k: Kw) -> bool {
        *self.peek() == Tok::Kw(k)
    }

    fn at_ident(&self, text: &str) -> bool {
        matches!(self.peek(), Tok::Ident(n) if n == text)
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, k: Kw) -> bool {
        self.eat(&Tok::Kw(k))
    }

    fn at_line_end(&self) -> bool {
        matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::Dedent)
    }

    fn unexpected(&self, expected: &str) -> Diagnostic {
        if matches!(self.peek(), Tok::Newline | Tok::Eof) {
            let prev = self.prev_span();
            return Diagnostic::error(
                format!("this line ends too early: I expected {expected}"),
                Span::new(prev.end, prev.end + 1, prev.line, prev.col + (prev.end - prev.start) as u32),
            );
        }
        Diagnostic::error(format!("expected {expected}, but found {}", self.peek().describe()), self.span())
    }

    fn expect(&mut self, t: &Tok, expected: &str) -> PResult<Span> {
        if self.at(t) {
            Ok(self.advance().span)
        } else {
            Err(self.unexpected(expected))
        }
    }

    fn ident(&mut self, what: &str) -> PResult<Name> {
        match self.peek().clone() {
            Tok::Ident(text) => {
                let span = self.advance().span;
                Ok(Name { res: Default::default(), text, span })
            }
            Tok::Kw(k) => Err(Diagnostic::error(format!("\"{}\" is a keyword, so it can't be used as {what}", k.as_str()), self.span())
                .with_code("LIP1005")
                .with_hint("Pick a different name.")),
            _ => Err(self.unexpected(what)),
        }
    }

    /// A name that will be bound (variable, parameter, function...): reserved words aren't allowed.
    fn binding(&mut self, what: &str) -> PResult<Name> {
        let name = self.ident(what)?;
        check_binding(&name)?;
        Ok(name)
    }

    /// A field name after `.` or an object key: identifiers and keywords both work.
    fn field_name(&mut self) -> PResult<Name> {
        match self.peek().clone() {
            Tok::Ident(text) => {
                let span = self.advance().span;
                Ok(Name { res: Default::default(), text, span })
            }
            Tok::Kw(k) => {
                let span = self.advance().span;
                Ok(Name { res: Default::default(), text: k.as_str().to_string(), span })
            }
            _ => Err(self.unexpected("a field name")),
        }
    }

    fn end_statement(&mut self) -> PResult<()> {
        match self.peek() {
            Tok::Newline => {
                self.advance();
                Ok(())
            }
            Tok::Eof | Tok::Dedent => Ok(()),
            Tok::Str(_) | Tok::Int(_) | Tok::Decimal(_) | Tok::Ident(_) => {
                Err(Diagnostic::error(format!("unexpected {} here", self.peek().describe()), self.span())
                    .with_hint("To show several values, separate them with commas: show \"Total:\", total"))
            }
            Tok::Colon => Err(Diagnostic::error("unexpected ':'", self.span())
                .with_hint("Blocks in LiPi don't need ':'. Start the block on the next line, indented.")),
            _ => Err(Diagnostic::error(format!("unexpected {}: I expected the end of the line", self.peek().describe()), self.span())
                .with_hint("Each statement goes on its own line.")),
        }
    }

    // ----- program & blocks ----------------------------------------------

    pub fn parse_program(mut self) -> PResult<Program> {
        let mut body = Vec::new();
        loop {
            while self.eat(&Tok::Newline) {}
            if self.at(&Tok::Eof) {
                break;
            }
            if self.at(&Tok::Dedent) {
                self.advance();
                continue;
            }
            body.push(self.parse_statement()?);
        }
        Ok(Program { body })
    }

    fn parse_block(&mut self, owner: &str, owner_span: Span) -> PResult<Block> {
        match self.peek() {
            Tok::Newline => {}
            Tok::Colon if matches!(self.peek_at(1), Tok::Newline) => {
                return Err(Diagnostic::error("LiPi doesn't use ':' to start a block", self.span())
                    .with_hint(format!("Remove the ':'. The indented lines below `{owner}` form its block.")));
            }
            Tok::Assign => {
                return Err(Diagnostic::error("to compare two values, use ==", self.span())
                    .with_hint("A single = stores a value. == checks whether two values are equal."));
            }
            Tok::LBrace => {
                return Err(Diagnostic::error("LiPi uses indentation instead of { } for blocks", self.span()).with_hint(BLOCK_EXAMPLE));
            }
            Tok::Eof => {
                return Err(Diagnostic::error(format!("expected an indented block after `{owner}`"), owner_span)
                    .with_code("LIP0005")
                    .with_hint(BLOCK_EXAMPLE));
            }
            _ => return Err(self.unexpected(&format!("the end of the line after `{owner}`"))),
        }
        self.advance();
        if !self.at(&Tok::Indent) {
            return Err(Diagnostic::error(format!("expected an indented block after `{owner}`"), owner_span)
                .with_code("LIP0005")
                .with_hint(BLOCK_EXAMPLE));
        }
        self.advance();
        let mut body = Vec::new();
        while !self.at(&Tok::Dedent) && !self.at(&Tok::Eof) {
            if self.eat(&Tok::Newline) {
                continue;
            }
            body.push(self.parse_statement()?);
        }
        self.eat(&Tok::Dedent);
        Ok(body)
    }

    // ----- statements ----------------------------------------------------

    fn stmt(&self, kind: StmtKind, start: Span) -> Stmt {
        Stmt { kind, span: start.to(self.prev_span()) }
    }

    pub fn parse_statement(&mut self) -> PResult<Stmt> {
        let start = self.span();
        match self.peek().clone() {
            Tok::Kw(Kw::Show) => {
                self.advance();
                let mut values = Vec::new();
                if !self.at_line_end() {
                    values.push(self.parse_expression()?);
                    while self.eat(&Tok::Comma) {
                        values.push(self.parse_expression()?);
                    }
                }
                let s = self.stmt(StmtKind::Show(values), start);
                self.end_statement()?;
                Ok(s)
            }
            Tok::Kw(Kw::If) => self.parse_if(),
            Tok::Kw(Kw::While) => {
                self.advance();
                let cond = self.parse_expression()?;
                let body = self.parse_block("while", start)?;
                Ok(Stmt { kind: StmtKind::While { cond, body }, span: start })
            }
            Tok::Kw(Kw::For) => self.parse_for(),
            Tok::Kw(Kw::Repeat) => {
                self.advance();
                let count = self.parse_expression()?;
                let body = self.parse_block("repeat", start)?;
                Ok(Stmt { kind: StmtKind::Repeat { count, body }, span: start })
            }
            Tok::Kw(Kw::Return) => {
                self.advance();
                let value = if self.at_line_end() { None } else { Some(self.parse_expression()?) };
                let s = self.stmt(StmtKind::Return(value), start);
                self.end_statement()?;
                Ok(s)
            }
            Tok::Kw(Kw::Break) => {
                self.advance();
                self.end_statement()?;
                Ok(Stmt { kind: StmtKind::Break, span: start })
            }
            Tok::Kw(Kw::Continue) => {
                self.advance();
                self.end_statement()?;
                Ok(Stmt { kind: StmtKind::Continue, span: start })
            }
            Tok::Kw(Kw::Throw) => {
                self.advance();
                let value = self.parse_expression()?;
                let s = self.stmt(StmtKind::Throw(value), start);
                self.end_statement()?;
                Ok(s)
            }
            Tok::Kw(Kw::Const) => {
                self.advance();
                let name = self.binding("a constant name")?;
                let ty = if self.eat(&Tok::Colon) { Some(self.parse_type()?) } else { None };
                self.expect(&Tok::Assign, "'=' and a value for the constant")?;
                let value = self.parse_expression()?;
                let s = self.stmt(StmtKind::Assign { target: Target::Name(name), op: None, ty, value, constant: true }, start);
                self.end_statement()?;
                Ok(s)
            }
            Tok::Kw(Kw::Try) => self.parse_try(),
            Tok::Kw(Kw::Match) => self.parse_match(),
            Tok::Kw(Kw::Use) => self.parse_use(),
            Tok::Kw(Kw::From) => self.parse_from_use(),
            Tok::Kw(Kw::Export) => self.parse_export(),
            Tok::Kw(Kw::Function) => {
                self.advance();
                match self.try_func_def(false, true)? {
                    Some(f) => Ok(Stmt { kind: StmtKind::Func(f), span: start }),
                    None => unreachable!("explicit definitions report their own errors"),
                }
            }
            Tok::Kw(Kw::Async) => {
                self.advance();
                self.eat_kw(Kw::Function);
                match self.try_func_def(true, true)? {
                    Some(f) => Ok(Stmt { kind: StmtKind::Func(f), span: start }),
                    None => unreachable!("explicit definitions report their own errors"),
                }
            }
            Tok::Kw(Kw::Else) => Err(Diagnostic::error("`else` must come right after an `if` block", start)
                .with_hint("Make sure `else` lines up exactly with its `if`.")),
            Tok::Kw(Kw::Catch) | Tok::Kw(Kw::Finally) => {
                Err(Diagnostic::error(format!("{} must come right after a `try` block", self.peek().describe()), start)
                    .with_hint("Make sure it lines up exactly with its `try`."))
            }
            Tok::Indent => {
                let next = self.toks[(self.pos + 1).min(self.toks.len() - 1)].span;
                Err(Diagnostic::error("this line is indented, but it isn't inside a block", next)
                    .with_code("LIP0002")
                    .with_hint("Remove the extra spaces at the start of the line. Only lines after `if`, `for`, `while`, a function definition and similar can be indented."))
            }
            Tok::Ident(name) => {
                if name == "type" && matches!(self.peek_at(1), Tok::Ident(_)) && matches!(self.peek_at(2), Tok::Newline) {
                    return self.parse_type_def();
                }
                if name == "test" && matches!(self.peek_at(1), Tok::Str(_)) {
                    return self.parse_test();
                }
                if name == "component" && matches!(self.peek_at(1), Tok::Ident(_)) && matches!(self.peek_at(2), Tok::LParen) {
                    self.advance();
                    match self.try_func_def(false, true)? {
                        Some(f) => return Ok(Stmt { kind: StmtKind::Component(f), span: start }),
                        None => unreachable!("explicit definitions report their own errors"),
                    }
                }
                if name == "state" && matches!(self.peek_at(1), Tok::Ident(_)) && matches!(self.peek_at(2), Tok::Assign | Tok::Colon) {
                    self.advance();
                    let name = self.binding("a name for the state")?;
                    let ty = if self.eat(&Tok::Colon) { Some(self.parse_type()?) } else { None };
                    self.expect(&Tok::Assign, "'=' and a starting value, like: state count = 0")?;
                    let value = self.parse_expression()?;
                    let s = self.stmt(StmtKind::State { name, ty, value }, start);
                    self.end_statement()?;
                    return Ok(s);
                }
                if matches!(self.peek_at(1), Tok::LParen) {
                    if let Some(f) = self.try_func_def(false, false)? {
                        return Ok(Stmt { kind: StmtKind::Func(f), span: start });
                    }
                }
                if matches!(self.peek_at(1), Tok::Colon) && !matches!(self.peek_at(2), Tok::Newline) {
                    return self.parse_typed_assign();
                }
                if let Some(err) = self.foreign_statement(&name) {
                    return Err(err);
                }
                self.parse_simple_statement()
            }
            _ => self.parse_simple_statement(),
        }
    }

    /// Catch habits from other languages (`let x = 1`, `def f()`, `elif`, `import x`) with a friendly hint.
    fn foreign_statement(&self, name: &str) -> Option<Diagnostic> {
        let next = self.peek_at(1);
        let next2 = self.peek_at(2);
        let msg = match name {
            "let" | "var" if matches!(next, Tok::Ident(_)) && matches!(next2, Tok::Assign) => format!("LiPi doesn't need `{name}`"),
            "def" | "fn" | "func" if matches!(next, Tok::Ident(_)) => format!("LiPi doesn't use `{name}` to define functions"),
            "elif" | "elsif" | "elseif" => format!("LiPi writes `{name}` as `else if`"),
            "import" | "require" if matches!(next, Tok::Ident(_) | Tok::Str(_)) => format!("LiPi uses `use` instead of `{name}`"),
            _ => return None,
        };
        Some(Diagnostic::error(msg, self.span()).with_code("LIP0008").maybe_hint(suggest::foreign_name_hint(name).map(String::from)))
    }

    fn parse_simple_statement(&mut self) -> PResult<Stmt> {
        let start = self.span();
        let mut expr = self.parse_expression()?;
        // A call without parentheses: `server.start 3000`, `get "/users"`
        if is_path(&expr) && self.command_arg_ahead() {
            let args = self.parse_command_args()?;
            let span = expr.span.to(self.prev_span());
            expr = Expr { res: Default::default(), kind: ExprKind::Call { callee: Box::new(expr), args }, span };
        }
        // A trailing block becomes a function passed as the last argument.
        let block_ahead = self.at_ident("with") || (self.at(&Tok::Newline) && matches!(self.peek_at(1), Tok::Indent));
        if block_ahead && is_path(&expr) {
            // `server.before with request` + block: a call whose only argument is the block
            let span = expr.span;
            expr = Expr { res: Default::default(), kind: ExprKind::Call { callee: Box::new(expr), args: Vec::new() }, span };
        }
        if matches!(expr.kind, ExprKind::Call { .. }) && block_ahead {
            let block = self.parse_trailing_block()?;
            if let ExprKind::Call { args, .. } = &mut expr.kind {
                args.push(Arg { name: None, value: block });
            }
            return Ok(Stmt { kind: StmtKind::Expr(expr), span: start });
        }
        let op = match self.peek() {
            Tok::Assign => None,
            Tok::PlusAssign => Some(BinOp::Add),
            Tok::MinusAssign => Some(BinOp::Sub),
            Tok::StarAssign => Some(BinOp::Mul),
            Tok::SlashAssign => Some(BinOp::Div),
            _ => {
                let s = self.stmt(StmtKind::Expr(expr), start);
                self.end_statement()?;
                return Ok(s);
            }
        };
        self.advance();
        let target = self.to_target(expr)?;
        if let Target::Name(n) = &target {
            check_binding(n)?;
        }
        let value = self.parse_expression()?;
        let s = self.stmt(StmtKind::Assign { target, op, ty: None, value, constant: false }, start);
        self.end_statement()?;
        Ok(s)
    }

    fn command_arg_ahead(&self) -> bool {
        match self.peek() {
            Tok::Str(_) | Tok::Int(_) | Tok::Decimal(_) | Tok::LBrace | Tok::Kw(Kw::True | Kw::False | Kw::Null) => true,
            Tok::Ident(name) => name != "with",
            _ => false,
        }
    }

    /// Arguments of a call without parentheses, up to the end of the line (or `with`).
    fn parse_command_args(&mut self) -> PResult<Vec<Arg>> {
        let mut args: Vec<Arg> = Vec::new();
        loop {
            if matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek_at(1), Tok::Colon) {
                let name = self.ident("an argument name")?;
                self.advance();
                args.push(Arg { name: Some(name), value: self.parse_expression()? });
            } else {
                let value = self.parse_expression()?;
                if args.iter().any(|a| a.name.is_some()) {
                    return Err(Diagnostic::error("positional arguments must come before named ones", value.span));
                }
                args.push(Arg { name: None, value });
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        Ok(args)
    }

    /// `[with a, b]` followed by an indented block, as a function value.
    fn parse_trailing_block(&mut self) -> PResult<Expr> {
        let start = self.span();
        let mut params = Vec::new();
        if self.at_ident("with") {
            self.advance();
            loop {
                let name = self.binding("a name for the block's input, like: with request")?;
                let ty = if self.eat(&Tok::Colon) { Some(self.parse_type()?) } else { None };
                params.push(Param { name, ty, default: None });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        let body = self.parse_block("this line", start)?;
        let decl = FuncDecl { name: Name { res: Default::default(), text: "<block>".into(), span: start }, params, ret: None, body, is_async: false, is_lambda: true, span: start };
        Ok(Expr { res: Default::default(), kind: ExprKind::Lambda(Rc::new(decl)), span: start })
    }

    fn to_target(&self, expr: Expr) -> PResult<Target> {
        match expr.kind {
            ExprKind::Ident(text) => Ok(Target::Name(Name { res: Default::default(), text, span: expr.span })),
            ExprKind::Field { object, name, optional: false } => Ok(Target::Field(*object, name)),
            ExprKind::Index { object, index } => Ok(Target::Index(*object, *index)),
            _ => Err(Diagnostic::error("can't assign to this", expr.span).with_code("LIP0006").with_hint(
                "Only a name (x), a field (user.name) or an item (items[0]) can be on the left of =. To compare values, use ==.",
            )),
        }
    }

    fn parse_typed_assign(&mut self) -> PResult<Stmt> {
        let start = self.span();
        let name = self.binding("a variable name")?;
        self.advance(); // :
        let ty = self.parse_type()?;
        if !self.at(&Tok::Assign) {
            return Err(Diagnostic::error(format!("give \"{}\" a starting value", name.text), self.span())
                .with_hint(format!("For example: {}: {} = ...", name.text, ty)));
        }
        self.advance();
        let value = self.parse_expression()?;
        let s = self.stmt(StmtKind::Assign { target: Target::Name(name), op: None, ty: Some(ty), value, constant: false }, start);
        self.end_statement()?;
        Ok(s)
    }

    fn parse_if(&mut self) -> PResult<Stmt> {
        let start = self.advance().span;
        let cond = self.parse_expression()?;
        let body = self.parse_block("if", start)?;
        let mut branches = vec![(cond, body)];
        let mut otherwise = None;
        while self.at_kw(Kw::Else) {
            let else_span = self.advance().span;
            if self.eat_kw(Kw::If) {
                let cond = self.parse_expression()?;
                let body = self.parse_block("else if", else_span)?;
                branches.push((cond, body));
            } else {
                otherwise = Some(self.parse_block("else", else_span)?);
                break;
            }
        }
        Ok(Stmt { kind: StmtKind::If { branches, otherwise }, span: start })
    }

    fn parse_for(&mut self) -> PResult<Stmt> {
        let start = self.advance().span;
        let first = self.binding("a loop variable name, like: for item in items")?;
        let second = if self.eat(&Tok::Comma) { Some(self.binding("a second loop variable name")?) } else { None };
        if !self.eat_kw(Kw::In) {
            return Err(self.unexpected("`in`").with_hint("Write loops like: for item in items   or   for i in 1 to 10"));
        }
        let iter = self.parse_expression()?;
        let body = self.parse_block("for", start)?;
        Ok(Stmt { kind: StmtKind::For { first, second, iter, body }, span: start })
    }

    fn parse_try(&mut self) -> PResult<Stmt> {
        let start = self.advance().span;
        let body = self.parse_block("try", start)?;
        let mut catch = None;
        let mut finally = None;
        if self.at_kw(Kw::Catch) {
            let span = self.advance().span;
            let name = if matches!(self.peek(), Tok::Ident(_)) { Some(self.binding("an error name")?) } else { None };
            catch = Some((name, self.parse_block("catch", span)?));
        }
        if self.at_kw(Kw::Finally) {
            let span = self.advance().span;
            finally = Some(self.parse_block("finally", span)?);
        }
        if catch.is_none() && finally.is_none() {
            return Err(Diagnostic::error("`try` needs a `catch` block", start)
                .with_hint("For example:\n    try\n        risky()\n    catch error\n        show error.message"));
        }
        Ok(Stmt { kind: StmtKind::Try { body, catch, finally }, span: start })
    }

    fn parse_match(&mut self) -> PResult<Stmt> {
        let start = self.advance().span;
        let subject = self.parse_expression()?;
        self.expect(&Tok::Newline, "the end of the line after `match`")?;
        const EXAMPLE: &str = "For example:\n    match status\n        \"paid\"\n            show \"Complete\"\n        \"pending\", \"new\"\n            show \"Waiting\"\n        else\n            show \"Unknown\"";
        if !self.eat(&Tok::Indent) {
            return Err(Diagnostic::error("expected indented cases after `match`", start).with_code("LIP0005").with_hint(EXAMPLE));
        }
        let mut arms = Vec::new();
        let mut otherwise = None;
        loop {
            while self.eat(&Tok::Newline) {}
            if self.at(&Tok::Dedent) || self.at(&Tok::Eof) {
                break;
            }
            if self.at_kw(Kw::Else) {
                let span = self.advance().span;
                otherwise = Some(self.parse_block("else", span)?);
                continue;
            }
            if self.at_ident("when") && !matches!(self.peek_at(1), Tok::Newline) {
                return Err(Diagnostic::error("match cases don't need `when`", self.span()).with_code("LIP0008").with_hint(EXAMPLE));
            }
            let case_span = self.span();
            self.no_inline_if = true;
            let mut patterns = vec![self.parse_expression()];
            while patterns.last().is_some_and(|p| p.is_ok()) && self.eat(&Tok::Comma) {
                patterns.push(self.parse_expression());
            }
            self.no_inline_if = false;
            let patterns = patterns.into_iter().collect::<PResult<Vec<Expr>>>()?;
            let guard = if self.eat_kw(Kw::If) { Some(self.parse_expression()?) } else { None };
            let body = self.parse_block("this case", case_span)?;
            arms.push(MatchArm { patterns, guard, body });
        }
        self.eat(&Tok::Dedent);
        Ok(Stmt { kind: StmtKind::Match { subject, arms, otherwise }, span: start })
    }

    fn plain_string(&mut self, what: &str) -> PResult<String> {
        match self.peek().clone() {
            Tok::Str(parts) => {
                let span = self.advance().span;
                match parts.as_slice() {
                    [StrPart::Lit(s)] => Ok(s.clone()),
                    _ => Err(Diagnostic::error(format!("{what} can't contain {{interpolation}}"), span)),
                }
            }
            _ => Err(self.unexpected(what)),
        }
    }

    /// A module name (`math`, `payments.razorpay`) or a path in quotes.
    fn module_source(&mut self) -> PResult<String> {
        if matches!(self.peek(), Tok::Str(_)) {
            return self.plain_string("a module path");
        }
        let mut name = self.ident("a module name or a path in quotes")?.text;
        while self.eat(&Tok::Dot) {
            name.push('.');
            name.push_str(&self.field_name()?.text);
        }
        Ok(name)
    }

    fn parse_use(&mut self) -> PResult<Stmt> {
        let start = self.advance().span;
        let source = self.module_source()?;
        let alias = if self.eat_kw(Kw::As) { Some(self.binding("a name after `as`")?) } else { None };
        let s = self.stmt(StmtKind::Use { source, alias, names: None }, start);
        self.end_statement()?;
        Ok(s)
    }

    fn parse_from_use(&mut self) -> PResult<Stmt> {
        let start = self.advance().span;
        let source = self.module_source()?;
        if !self.eat_kw(Kw::Use) {
            return Err(self.unexpected("`use`").with_hint("Write: from math use add, subtract"));
        }
        let mut names = vec![self.binding("a name to use")?];
        while self.eat(&Tok::Comma) {
            names.push(self.binding("a name to use")?);
        }
        let s = self.stmt(StmtKind::Use { source, alias: None, names: Some(names) }, start);
        self.end_statement()?;
        Ok(s)
    }

    fn parse_export(&mut self) -> PResult<Stmt> {
        let start = self.advance().span;
        let names_only = matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek_at(1), Tok::Comma | Tok::Newline | Tok::Eof | Tok::Dedent);
        if names_only {
            let mut names = vec![self.ident("a name to export")?];
            while self.eat(&Tok::Comma) {
                names.push(self.ident("a name to export")?);
            }
            let s = self.stmt(StmtKind::Export { names, inner: None }, start);
            self.end_statement()?;
            return Ok(s);
        }
        let inner = self.parse_statement()?;
        let name = match &inner.kind {
            StmtKind::Func(f) | StmtKind::Component(f) => f.name.clone(),
            StmtKind::TypeDef(t) => t.name.clone(),
            StmtKind::State { name, .. } => name.clone(),
            StmtKind::Assign { target: Target::Name(n), op: None, .. } => n.clone(),
            _ => {
                return Err(Diagnostic::error("`export` needs a name or a definition", start)
                    .with_code("LIP3005")
                    .with_hint("For example: export add   or   export const limit = 10"))
            }
        };
        Ok(Stmt { kind: StmtKind::Export { names: vec![name], inner: Some(Box::new(inner)) }, span: start })
    }

    fn parse_test(&mut self) -> PResult<Stmt> {
        let start = self.advance().span;
        let name = self.plain_string("a test name")?;
        let body = self.parse_block("test", start)?;
        Ok(Stmt { kind: StmtKind::Test { name, body }, span: start })
    }

    fn parse_type_def(&mut self) -> PResult<Stmt> {
        let start = self.advance().span;
        let name = self.binding("a type name")?;
        self.advance(); // newline
        if !self.eat(&Tok::Indent) {
            return Err(Diagnostic::error(format!("expected the fields of \"{}\" on indented lines", name.text), start)
                .with_code("LIP0005")
                .with_hint("For example:\n    type User\n        name: String\n        age = 0"));
        }
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        loop {
            while self.eat(&Tok::Newline) {}
            if self.at(&Tok::Dedent) || self.at(&Tok::Eof) {
                break;
            }
            let is_async = self.eat_kw(Kw::Async);
            let explicit = self.eat_kw(Kw::Function) || is_async;
            match (self.peek().clone(), self.peek_at(1).clone()) {
                (Tok::Ident(_), Tok::LParen) => match self.try_func_def(is_async, explicit)? {
                    Some(f) => methods.push(f),
                    None => {
                        return Err(Diagnostic::error("expected a method definition with an indented body", self.span())
                            .with_hint("For example:\n    greet()\n        return \"Hi, \" + self.name"))
                    }
                },
                (Tok::Ident(_), Tok::Colon) | (Tok::Ident(_), Tok::Assign) if !explicit => {
                    let fname = self.binding("a field name")?;
                    let ty = if self.eat(&Tok::Colon) { Some(self.parse_type()?) } else { None };
                    let default = if self.eat(&Tok::Assign) { Some(self.parse_expression()?) } else { None };
                    self.end_statement()?;
                    fields.push(FieldDecl { name: fname, ty, default });
                }
                _ => {
                    return Err(Diagnostic::error("inside a type, write fields and methods", self.span()).with_hint(
                        "Fields look like `name: String` or `age = 0`. Methods look like `greet()` followed by an indented body.",
                    ))
                }
            }
        }
        self.eat(&Tok::Dedent);
        Ok(Stmt { kind: StmtKind::TypeDef(Rc::new(TypeDecl { name, fields, methods })), span: start })
    }

    /// Try to parse `name(params) [-> Type]` followed by an indented body.
    /// Returns `None` (and rewinds) when this is really a call statement,
    /// unless `explicit` (after `function`/`async`), where problems are errors.
    fn try_func_def(&mut self, is_async: bool, explicit: bool) -> PResult<Option<Rc<FuncDecl>>> {
        let save = self.pos;
        let header = self.func_header();
        let has_body = matches!(self.peek(), Tok::Newline) && matches!(self.peek_at(1), Tok::Indent);
        match header {
            Ok((name, params, ret)) if has_body => {
                let owner = format!("{}(...)", name.text);
                let body = self.parse_block(&owner, name.span)?;
                let span = name.span;
                Ok(Some(Rc::new(FuncDecl { name, params, ret, body, is_async, is_lambda: false, span })))
            }
            Err(e) if explicit => Err(e),
            Ok((name, _, _)) if explicit => Err(Diagnostic::error(format!("expected an indented body for \"{}\"", name.text), name.span)
                .with_code("LIP0005")
                .with_hint("For example:\n    function add(a, b)\n        return a + b")),
            _ => {
                self.pos = save;
                Ok(None)
            }
        }
    }

    fn func_header(&mut self) -> PResult<(Name, Vec<Param>, Option<TypeExpr>)> {
        let name = self.binding("a function name")?;
        self.expect(&Tok::LParen, "'('").map_err(|e| e.with_hint("Function definitions look like: function add(a, b)"))?;
        let params = self.parse_params(&Tok::RParen)?;
        let ret = if self.eat(&Tok::Arrow) { Some(self.parse_type()?) } else { None };
        Ok((name, params, ret))
    }

    fn parse_params(&mut self, close: &Tok) -> PResult<Vec<Param>> {
        let mut params: Vec<Param> = Vec::new();
        loop {
            if self.eat(close) {
                break;
            }
            let name = self.binding("a parameter name")?;
            if params.iter().any(|p| p.name.text == name.text) {
                return Err(Diagnostic::error(format!("the parameter \"{}\" appears twice", name.text), name.span).with_code("LIP1001"));
            }
            let ty = if self.eat(&Tok::Colon) { Some(self.parse_type()?) } else { None };
            let default = if self.eat(&Tok::Assign) { Some(self.parse_expression()?) } else { None };
            params.push(Param { name, ty, default });
            if !self.eat(&Tok::Comma) {
                self.expect(close, "',' or ')' after function parameters")?;
                break;
            }
        }
        Ok(params)
    }

    pub fn parse_type(&mut self) -> PResult<TypeExpr> {
        let start = self.span();
        let mut ty = if self.eat(&Tok::LBracket) {
            let inner = self.parse_type()?;
            self.expect(&Tok::RBracket, "']'")?;
            TypeExpr { kind: TypeKind::List(Box::new(inner)), span: start.to(self.prev_span()) }
        } else if self.eat_kw(Kw::Null) {
            TypeExpr { kind: TypeKind::Named("Null".into()), span: start }
        } else {
            let name = self.ident("a type name like Integer, String, Boolean, Array or Object")?;
            if (name.text == "Array" || name.text == "List") && self.eat(&Tok::LBracket) {
                let inner = self.parse_type()?;
                self.expect(&Tok::RBracket, "']'")?;
                TypeExpr { kind: TypeKind::List(Box::new(inner)), span: start.to(self.prev_span()) }
            } else {
                TypeExpr { kind: TypeKind::Named(name.text), span: name.span }
            }
        };
        while self.eat(&Tok::Question) {
            ty = TypeExpr { kind: TypeKind::Optional(Box::new(ty)), span: start.to(self.prev_span()) };
        }
        Ok(ty)
    }

    // ----- expressions ---------------------------------------------------

    pub fn parse_expression(&mut self) -> PResult<Expr> {
        if self.lambda_ahead() {
            return self.parse_lambda();
        }
        let value = self.parse_coalesce()?;
        if !self.at_kw(Kw::If) || self.no_inline_if {
            return Ok(value);
        }
        // Inline choice: value if condition else other
        self.advance();
        let cond = self.parse_coalesce()?;
        if !self.eat_kw(Kw::Else) {
            return Err(self
                .unexpected("`else` and a value")
                .with_hint("An inline choice looks like: label = \"adult\" if age >= 18 else \"minor\""));
        }
        let otherwise = self.parse_expression()?;
        let span = value.span.to(otherwise.span);
        Ok(Expr { res: Default::default(), kind: ExprKind::IfElse { cond: Box::new(cond), then: Box::new(value), otherwise: Box::new(otherwise) }, span })
    }

    fn lambda_ahead(&self) -> bool {
        match self.peek() {
            Tok::Ident(_) => matches!(self.peek_at(1), Tok::FatArrow),
            Tok::LParen => {
                let mut depth = 0usize;
                for i in self.pos..self.toks.len() {
                    match &self.toks[i].tok {
                        Tok::LParen | Tok::LBracket | Tok::LBrace => depth += 1,
                        Tok::RParen | Tok::RBracket | Tok::RBrace => {
                            depth -= 1;
                            if depth == 0 {
                                return matches!(self.toks.get(i + 1).map(|t| &t.tok), Some(Tok::FatArrow));
                            }
                        }
                        Tok::Eof | Tok::Newline => return false,
                        _ => {}
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn parse_lambda(&mut self) -> PResult<Expr> {
        let start = self.span();
        let params = if matches!(self.peek(), Tok::Ident(_)) {
            let name = self.binding("a parameter name")?;
            vec![Param { name, ty: None, default: None }]
        } else {
            self.advance();
            self.parse_params(&Tok::RParen)?
        };
        self.expect(&Tok::FatArrow, "'=>'")?;
        let body = self.parse_expression()?;
        let span = start.to(body.span);
        let ret = Stmt { span: body.span, kind: StmtKind::Return(Some(body)) };
        let decl = FuncDecl { name: Name { res: Default::default(), text: "<lambda>".into(), span: start }, params, ret: None, body: vec![ret], is_async: false, is_lambda: true, span };
        Ok(Expr { res: Default::default(), kind: ExprKind::Lambda(Rc::new(decl)), span })
    }

    fn parse_coalesce(&mut self) -> PResult<Expr> {
        let mut left = self.parse_or()?;
        while self.eat(&Tok::QuestionQuestion) {
            let right = self.parse_or()?;
            let span = left.span.to(right.span);
            left = Expr { res: Default::default(), kind: ExprKind::Coalesce(Box::new(left), Box::new(right)), span };
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let mut left = self.parse_and()?;
        while self.eat_kw(Kw::Or) {
            let right = self.parse_and()?;
            let span = left.span.to(right.span);
            left = Expr { res: Default::default(), kind: ExprKind::Or(Box::new(left), Box::new(right)), span };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let mut left = self.parse_not()?;
        while self.eat_kw(Kw::And) {
            let right = self.parse_not()?;
            let span = left.span.to(right.span);
            left = Expr { res: Default::default(), kind: ExprKind::And(Box::new(left), Box::new(right)), span };
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> PResult<Expr> {
        if self.at_kw(Kw::Not) {
            let start = self.advance().span;
            let inner = self.parse_not()?;
            let span = start.to(inner.span);
            return Ok(Expr { res: Default::default(), kind: ExprKind::Unary(UnaryOp::Not, Box::new(inner)), span });
        }
        self.parse_comparison()
    }

    fn comparison_op(&self) -> Option<(BinOp, usize)> {
        Some(match self.peek() {
            Tok::Eq => (BinOp::Eq, 1),
            Tok::NotEq => (BinOp::NotEq, 1),
            Tok::Lt => (BinOp::Lt, 1),
            Tok::Gt => (BinOp::Gt, 1),
            Tok::LtEq => (BinOp::LtEq, 1),
            Tok::GtEq => (BinOp::GtEq, 1),
            Tok::Kw(Kw::In) => (BinOp::In, 1),
            Tok::Kw(Kw::Not) if matches!(self.peek_at(1), Tok::Kw(Kw::In)) => (BinOp::NotIn, 2),
            _ => return None,
        })
    }

    fn parse_comparison(&mut self) -> PResult<Expr> {
        let left = self.parse_range()?;
        let Some((op, width)) = self.comparison_op() else { return Ok(left) };
        for _ in 0..width {
            self.advance();
        }
        let right = self.parse_range()?;
        if self.comparison_op().is_some() {
            return Err(Diagnostic::error("to combine two comparisons, use `and`", self.span()).with_hint("For example: 0 < x and x < 10"));
        }
        let span = left.span.to(right.span);
        Ok(Expr { res: Default::default(), kind: ExprKind::Binary(op, Box::new(left), Box::new(right)), span })
    }

    fn parse_range(&mut self) -> PResult<Expr> {
        let start = self.parse_additive()?;
        if !self.at_ident("to") {
            return Ok(start);
        }
        self.advance();
        let end = self.parse_additive()?;
        let step = if self.at_ident("step") {
            self.advance();
            Some(Box::new(self.parse_additive()?))
        } else {
            None
        };
        let span = start.span.to(self.prev_span());
        Ok(Expr { res: Default::default(), kind: ExprKind::Range { start: Box::new(start), end: Box::new(end), step }, span })
    }

    fn parse_additive(&mut self) -> PResult<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            let span = left.span.to(right.span);
            left = Expr { res: Default::default(), kind: ExprKind::Binary(op, Box::new(left), Box::new(right)), span };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> PResult<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            let span = left.span.to(right.span);
            left = Expr { res: Default::default(), kind: ExprKind::Binary(op, Box::new(left), Box::new(right)), span };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        let start = self.span();
        if self.eat(&Tok::Minus) {
            let inner = self.parse_unary()?;
            let span = start.to(inner.span);
            return Ok(Expr { res: Default::default(), kind: ExprKind::Unary(UnaryOp::Neg, Box::new(inner)), span });
        }
        if self.eat_kw(Kw::Await) {
            let inner = self.parse_unary()?;
            let span = start.to(inner.span);
            return Ok(Expr { res: Default::default(), kind: ExprKind::Await(Box::new(inner)), span });
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> PResult<Expr> {
        let base = self.parse_postfix()?;
        if self.eat(&Tok::StarStar) {
            let exp = self.parse_unary()?;
            let span = base.span.to(exp.span);
            return Ok(Expr { res: Default::default(), kind: ExprKind::Binary(BinOp::Pow, Box::new(base), Box::new(exp)), span });
        }
        Ok(base)
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek() {
                Tok::LParen => {
                    self.advance();
                    let args = self.parse_args()?;
                    let span = expr.span.to(self.prev_span());
                    expr = Expr { res: Default::default(), kind: ExprKind::Call { callee: Box::new(expr), args }, span };
                }
                Tok::Dot | Tok::QuestionDot => {
                    let optional = self.advance().tok == Tok::QuestionDot;
                    let name = self.field_name()?;
                    let span = expr.span.to(name.span);
                    expr = Expr { res: Default::default(), kind: ExprKind::Field { object: Box::new(expr), name, optional }, span };
                }
                Tok::LBracket => {
                    self.advance();
                    let index = self.parse_expression()?;
                    self.expect(&Tok::RBracket, "']'")?;
                    let span = expr.span.to(self.prev_span());
                    expr = Expr { res: Default::default(), kind: ExprKind::Index { object: Box::new(expr), index: Box::new(index) }, span };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> PResult<Vec<Arg>> {
        let mut args: Vec<Arg> = Vec::new();
        loop {
            if self.eat(&Tok::RParen) {
                break;
            }
            let arg = if matches!(self.peek(), Tok::Ident(_)) && matches!(self.peek_at(1), Tok::Colon) {
                let name = self.ident("an argument name")?;
                self.advance();
                Arg { name: Some(name), value: self.parse_expression()? }
            } else {
                let value = self.parse_expression()?;
                if args.iter().any(|a| a.name.is_some()) {
                    return Err(Diagnostic::error("positional arguments must come before named ones", value.span)
                        .with_hint("For example: resize(image, width: 100)"));
                }
                Arg { name: None, value }
            };
            args.push(arg);
            if !self.eat(&Tok::Comma) {
                self.expect(&Tok::RParen, "',' or ')'")?;
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(n) => {
                self.advance();
                ExprKind::Int(n)
            }
            Tok::Decimal(n) => {
                self.advance();
                ExprKind::Decimal(n)
            }
            Tok::Str(parts) => {
                self.advance();
                return self.string_expr(parts, span);
            }
            Tok::Kw(Kw::True) => {
                self.advance();
                ExprKind::Bool(true)
            }
            Tok::Kw(Kw::False) => {
                self.advance();
                ExprKind::Bool(false)
            }
            Tok::Kw(Kw::Null) => {
                self.advance();
                ExprKind::Null
            }
            Tok::Ident(name) => {
                self.advance();
                ExprKind::Ident(name)
            }
            Tok::LParen => {
                self.advance();
                let inner = self.parse_expression()?;
                self.expect(&Tok::RParen, "a closing ')'")?;
                return Ok(Expr { res: Default::default(), kind: inner.kind, span: span.to(self.prev_span()) });
            }
            Tok::LBracket => {
                self.advance();
                let mut items = Vec::new();
                while !self.eat(&Tok::RBracket) {
                    items.push(self.parse_expression()?);
                    if !self.eat(&Tok::Comma) {
                        self.expect(&Tok::RBracket, "',' or ']'")?;
                        break;
                    }
                }
                ExprKind::List(items)
            }
            Tok::LBrace => {
                self.advance();
                let mut fields: Vec<(Name, Expr)> = Vec::new();
                while !self.eat(&Tok::RBrace) {
                    let kspan = self.span();
                    let (key, ident_key) = match self.peek().clone() {
                        Tok::Str(_) => (Name { res: Default::default(), text: self.plain_string("a key")?, span: kspan }, false),
                        Tok::Int(n) => {
                            self.advance();
                            (Name { res: Default::default(), text: n.to_string(), span: kspan }, false)
                        }
                        _ => (self.field_name()?, true),
                    };
                    let value = if self.eat(&Tok::Colon) {
                        self.parse_expression()?
                    } else if ident_key && matches!(self.peek(), Tok::Comma | Tok::RBrace) {
                        // `{name, age}` is short for `{name: name, age: age}`
                        Expr { res: Default::default(), kind: ExprKind::Ident(key.text.clone()), span: key.span }
                    } else {
                        return Err(self.unexpected("':' after the key").with_hint("Objects look like: { name: \"Dezy\", age: 25 }"));
                    };
                    if fields.iter().any(|(k, _)| k.text == key.text) {
                        return Err(Diagnostic::error(format!("the key \"{}\" appears twice", key.text), key.span).with_code("LIP1001"));
                    }
                    fields.push((key, value));
                    if !self.eat(&Tok::Comma) {
                        self.expect(&Tok::RBrace, "',' or '}'")?;
                        break;
                    }
                }
                ExprKind::Object(fields)
            }
            Tok::Kw(k) => {
                return Err(Diagnostic::error(format!("`{}` can't be used as a value here", k.as_str()), span).maybe_hint(match k {
                    Kw::If => Some("To pick between two values, write: value if condition else other".to_string()),
                    Kw::Show => Some("`show` is a statement: put it at the start of its own line.".to_string()),
                    _ => None,
                }))
            }
            _ => return Err(self.unexpected("a value")),
        };
        Ok(Expr { res: Default::default(), kind, span: span.to(self.prev_span()) })
    }

    fn string_expr(&mut self, parts: Vec<StrPart>, span: Span) -> PResult<Expr> {
        if let [StrPart::Lit(s)] = parts.as_slice() {
            return Ok(Expr { res: Default::default(), kind: ExprKind::Str(s.clone()), span });
        }
        let mut out = Vec::new();
        for part in parts {
            match part {
                StrPart::Lit(s) => out.push(TemplatePart::Lit(s)),
                StrPart::Code(code) => {
                    const HINT: &str = "In double quotes, { } holds code, like \"Hi {name}\". For a literal brace write \\{, or use single quotes: '{\"a\": 1}'";
                    let parsed = Lexer::sub(self.src, code).tokenize().and_then(|toks| {
                        let mut sub = Parser::new(self.src, toks);
                        let expr = sub.parse_expression()?;
                        if !sub.at(&Tok::Eof) {
                            return Err(Diagnostic::error(format!("unexpected {} inside {{...}}", sub.peek().describe()), sub.span()));
                        }
                        Ok(expr)
                    });
                    out.push(TemplatePart::Expr(parsed.map_err(|d| d.with_hint(HINT))?));
                }
            }
        }
        Ok(Expr { res: Default::default(), kind: ExprKind::Template(out), span })
    }
}

fn check_binding(name: &Name) -> PResult<()> {
    if !RESERVED.contains(&name.text.as_str()) {
        return Ok(());
    }
    let hint = if name.text == "server" {
        "`server` is the built-in web server module. Pick another name, like app or api."
    } else {
        "It is reserved for upcoming LiPi features. Pick another name."
    };
    Err(Diagnostic::error(format!("\"{}\" is a reserved word", name.text), name.span).with_code("LIP1005").with_hint(hint))
}

#[cfg(test)]
mod tests {
    use crate::ast::*;
    use crate::parse_source;

    fn parse(src: &str) -> Program {
        parse_source(src).unwrap_or_else(|e| panic!("{}", e.render(src, "test.lipi", false)))
    }

    #[test]
    fn function_definitions() {
        let p = parse("add(a, b)\n    return a + b\nfunction sub(a, b)\n    return a - b\nshow add(1, 2)\n");
        assert!(matches!(p.body[0].kind, StmtKind::Func(_)));
        assert!(matches!(p.body[1].kind, StmtKind::Func(_)));
        assert!(matches!(p.body[2].kind, StmtKind::Show(_)));
    }

    #[test]
    fn call_statement_is_not_a_definition() {
        let p = parse("greet(name)\nshow 1\n");
        assert!(matches!(p.body[0].kind, StmtKind::Expr(_)));
    }

    #[test]
    fn if_else_chain() {
        let p = parse("if a\n    show 1\nelse if b\n    show 2\nelse\n    show 3\n");
        let StmtKind::If { branches, otherwise } = &p.body[0].kind else { panic!() };
        assert_eq!(branches.len(), 2);
        assert!(otherwise.is_some());
    }

    #[test]
    fn precedence() {
        let p = parse("x = 1 + 2 * 3\n");
        let StmtKind::Assign { value, .. } = &p.body[0].kind else { panic!() };
        let ExprKind::Binary(BinOp::Add, _, right) = &value.kind else { panic!() };
        assert!(matches!(right.kind, ExprKind::Binary(BinOp::Mul, _, _)));
    }

    #[test]
    fn typed_declaration_and_lambda() {
        let p = parse("name: String = \"Dezy\"\ndouble = x => x * 2\n");
        let StmtKind::Assign { ty, .. } = &p.body[0].kind else { panic!() };
        assert_eq!(ty.as_ref().unwrap().to_string(), "String");
        let StmtKind::Assign { value, .. } = &p.body[1].kind else { panic!() };
        assert!(matches!(value.kind, ExprKind::Lambda(_)));
    }

    #[test]
    fn modules_and_match() {
        let p = parse("use math\nfrom \"./x.lipi\" use a, b\nexport add\nexport const limit = 3\nmatch s\n    \"paid\"\n        show 1\n    else\n        show 2\n");
        assert!(matches!(p.body[0].kind, StmtKind::Use { .. }));
        assert!(matches!(&p.body[1].kind, StmtKind::Use { names: Some(n), .. } if n.len() == 2));
        assert!(matches!(&p.body[2].kind, StmtKind::Export { inner: None, .. }));
        assert!(matches!(&p.body[3].kind, StmtKind::Export { inner: Some(_), .. }));
        let StmtKind::Match { arms, otherwise, .. } = &p.body[4].kind else { panic!() };
        assert_eq!(arms.len(), 1);
        assert!(otherwise.is_some());
    }

    #[test]
    fn command_calls_and_trailing_blocks() {
        let p = parse("server.start 3000\nget \"/users/:id\" with request\n    return request.params.id\nretry(3)\n    show 1\n");
        let StmtKind::Expr(Expr { kind: ExprKind::Call { args, .. }, .. }) = &p.body[0].kind else { panic!() };
        assert_eq!(args.len(), 1);
        let StmtKind::Expr(Expr { kind: ExprKind::Call { args, .. }, .. }) = &p.body[1].kind else { panic!() };
        let ExprKind::Lambda(block) = &args[1].value.kind else { panic!() };
        assert_eq!(block.params[0].name.text, "request");
        let StmtKind::Expr(Expr { kind: ExprKind::Call { args, .. }, .. }) = &p.body[2].kind else { panic!() };
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn friendly_errors() {
        let e = parse_source("if x = 5\n    show x\n").unwrap_err();
        assert!(e.message.contains("=="));
        let e = parse_source("if x > 1\nshow x\n").unwrap_err();
        assert_eq!(e.code, Some("LIP0005"));
        let e = parse_source("let x = 5\n").unwrap_err();
        assert_eq!(e.code, Some("LIP0008"));
        let e = parse_source("state = 1\n").unwrap_err();
        assert_eq!(e.code, Some("LIP1005"));
    }
}
