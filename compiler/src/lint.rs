//! The LiPi linter: warnings about code that works but is probably a mistake.
//!
//! | Code    | Warning |
//! |---------|---------|
//! | LIP9001 | a variable is assigned but never used |
//! | LIP9002 | a `use` import is never used |
//! | LIP9003 | a parameter, loop variable or catch name shadows an outer variable |
//! | LIP9004 | code after `return`, `throw`, `break` or `continue` never runs |
//!
//! Names starting with `_` are never reported as unused.

use crate::ast::*;
use crate::checker::module_binding_name;
use crate::diagnostics::{Diagnostic, Span};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Variable,
    Import,
    Definition,
    Local, // parameters, loop variables, catch names
}

struct Binding {
    span: Span,
    kind: Kind,
    used: bool,
}

#[derive(Default)]
struct Scope {
    vars: HashMap<String, Binding>,
}

struct Linter {
    scopes: Vec<Scope>,
    diags: Vec<Diagnostic>,
    exported: HashSet<String>,
}

/// Lint a program (which must already parse).
pub fn lint(program: &Program) -> Vec<Diagnostic> {
    let mut l = Linter { scopes: Vec::new(), diags: Vec::new(), exported: HashSet::new() };
    for stmt in &program.body {
        if let StmtKind::Export { names, .. } = &stmt.kind {
            l.exported.extend(names.iter().map(|n| n.text.clone()));
        }
    }
    l.enter(&program.body, Vec::new());
    l.block(&program.body);
    l.leave();
    l.diags.sort_by_key(|d| d.span.map(|s| s.start));
    l.diags
}

type Declared = (String, Span, Kind);

/// Names a function body (or file) creates, not looking inside nested functions.
fn collect(body: &Block, out: &mut Vec<Declared>) {
    for stmt in body {
        collect_stmt(stmt, out);
    }
}

fn collect_stmt(stmt: &Stmt, out: &mut Vec<Declared>) {
    match &stmt.kind {
        StmtKind::Assign { target: Target::Name(n), op: None, .. } => out.push((n.text.clone(), n.span, Kind::Variable)),
        StmtKind::Func(f) => out.push((f.name.text.clone(), f.name.span, Kind::Definition)),
        StmtKind::TypeDef(t) => out.push((t.name.text.clone(), t.name.span, Kind::Definition)),
        StmtKind::Use { source, alias, names } => match names {
            Some(names) => names.iter().for_each(|n| out.push((n.text.clone(), n.span, Kind::Import))),
            None => {
                let (name, span) = match alias {
                    Some(a) => (a.text.clone(), a.span),
                    None => (module_binding_name(source), stmt.span),
                };
                out.push((name, span, Kind::Import));
            }
        },
        StmtKind::If { branches, otherwise } => {
            branches.iter().for_each(|(_, b)| collect(b, out));
            if let Some(b) = otherwise {
                collect(b, out);
            }
        }
        StmtKind::While { body, .. } | StmtKind::Repeat { body, .. } | StmtKind::For { body, .. } => collect(body, out),
        StmtKind::Try { body, catch, finally } => {
            collect(body, out);
            if let Some((_, b)) = catch {
                collect(b, out);
            }
            if let Some(b) = finally {
                collect(b, out);
            }
        }
        StmtKind::Match { arms, otherwise, .. } => {
            arms.iter().for_each(|a| collect(&a.body, out));
            if let Some(b) = otherwise {
                collect(b, out);
            }
        }
        StmtKind::Export { inner: Some(s), .. } => collect_stmt(s, out),
        _ => {}
    }
}

impl Linter {
    fn warn(&mut self, code: &'static str, message: String, span: Span, hint: &str) {
        self.diags.push(Diagnostic::warning(message, span).with_code(code).with_hint(hint));
    }

    fn defined_outside(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.vars.contains_key(name))
    }

    fn enter(&mut self, body: &Block, locals: Vec<(String, Span)>) {
        let mut scope = Scope::default();
        for (name, span) in locals {
            self.declare_local(&mut scope, name, span);
        }
        let mut declared = Vec::new();
        collect(body, &mut declared);
        let is_module = self.scopes.is_empty();
        for (name, span, kind) in declared {
            if scope.vars.contains_key(&name) {
                continue;
            }
            // Assigning to an outer variable updates it rather than creating a new one.
            if !is_module && kind == Kind::Variable && self.defined_outside(&name) {
                continue;
            }
            scope.vars.insert(name, Binding { span, kind, used: false });
        }
        self.scopes.push(scope);
    }

    fn declare_local(&mut self, scope: &mut Scope, name: String, span: Span) {
        if name != "_" && name != "self" && !self.scopes.is_empty() && self.defined_outside(&name) {
            self.warn(
                "LIP9003",
                format!("\"{name}\" shadows a variable from an outer scope"),
                span,
                "Inside this function the outer variable can't be reached. Rename one of them if that's not what you want.",
            );
        }
        scope.vars.insert(name, Binding { span, kind: Kind::Local, used: false });
    }

    fn leave(&mut self) {
        let scope = self.scopes.pop().expect("balanced scopes");
        let is_module = self.scopes.is_empty();
        let mut unused: Vec<(String, Binding)> = scope.vars.into_iter().filter(|(n, b)| !b.used && !n.starts_with('_')).collect();
        unused.sort_by_key(|(_, b)| b.span.start);
        for (name, b) in unused {
            if is_module && self.exported.contains(&name) {
                continue;
            }
            match b.kind {
                Kind::Variable => self.warn(
                    "LIP9001",
                    format!("\"{name}\" is assigned but never used"),
                    b.span,
                    "Remove it, or start its name with _ if that's intentional.",
                ),
                Kind::Import => self.warn("LIP9002", format!("\"{name}\" is imported but never used"), b.span, "Remove this `use` line."),
                Kind::Definition | Kind::Local => {}
            }
        }
    }

    fn read(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(b) = scope.vars.get_mut(name) {
                b.used = true;
                return;
            }
        }
    }

    fn block(&mut self, body: &Block) {
        let mut ended = false;
        for stmt in body {
            if ended {
                self.warn("LIP9004", "this code never runs".into(), stmt.span, "It comes after a return, throw, break or continue.");
                break;
            }
            self.stmt(stmt);
            ended = matches!(stmt.kind, StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Break | StmtKind::Continue);
        }
    }

    fn function(&mut self, f: &FuncDecl, is_method: bool) {
        for p in &f.params {
            if let Some(d) = &p.default {
                self.expr(d);
            }
        }
        let mut locals: Vec<(String, Span)> = f.params.iter().map(|p| (p.name.text.clone(), p.name.span)).collect();
        if is_method {
            locals.push(("self".into(), f.name.span));
        }
        self.enter(&f.body, locals);
        self.block(&f.body);
        self.leave();
    }

    fn local(&mut self, name: &Name) {
        let mut scope = self.scopes.pop().expect("inside a scope");
        if !scope.vars.contains_key(&name.text) {
            if !self.scopes.is_empty() {
                self.declare_local(&mut scope, name.text.clone(), name.span);
            } else {
                scope.vars.insert(name.text.clone(), Binding { span: name.span, kind: Kind::Local, used: false });
            }
        } else if let Some(b) = scope.vars.get_mut(&name.text) {
            // A loop variable reusing a name keeps that binding; don't flag it as unused.
            b.used |= b.kind != Kind::Variable;
        }
        self.scopes.push(scope);
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Expr(e) | StmtKind::Throw(e) => self.expr(e),
            StmtKind::Show(values) => values.iter().for_each(|v| self.expr(v)),
            StmtKind::Assign { target, op, value, .. } => {
                self.expr(value);
                match target {
                    Target::Name(n) if op.is_some() => self.read(&n.text),
                    Target::Name(_) => {}
                    Target::Field(obj, _) => self.expr(obj),
                    Target::Index(obj, idx) => {
                        self.expr(obj);
                        self.expr(idx);
                    }
                }
            }
            StmtKind::If { branches, otherwise } => {
                for (c, b) in branches {
                    self.expr(c);
                    self.block(b);
                }
                if let Some(b) = otherwise {
                    self.block(b);
                }
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::Repeat { count, body } => {
                self.expr(count);
                self.block(body);
            }
            StmtKind::For { first, second, iter, body } => {
                self.expr(iter);
                self.local(first);
                if let Some(s) = second {
                    self.local(s);
                }
                self.block(body);
            }
            StmtKind::Func(f) => self.function(f, false),
            StmtKind::Return(v) => {
                if let Some(v) = v {
                    self.expr(v);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Try { body, catch, finally } => {
                self.block(body);
                if let Some((name, b)) = catch {
                    if let Some(n) = name {
                        self.local(n);
                    }
                    self.block(b);
                }
                if let Some(b) = finally {
                    self.block(b);
                }
            }
            StmtKind::Match { subject, arms, otherwise } => {
                self.expr(subject);
                for arm in arms {
                    for p in &arm.patterns {
                        if !matches!(&p.kind, ExprKind::Ident(n) if n == "_") {
                            self.expr(p);
                        }
                    }
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    self.block(&arm.body);
                }
                if let Some(b) = otherwise {
                    self.block(b);
                }
            }
            StmtKind::Use { .. } => {}
            StmtKind::Export { names, inner } => {
                if let Some(s) = inner {
                    self.stmt(s);
                }
                names.iter().for_each(|n| self.read(&n.text));
            }
            StmtKind::TypeDef(t) => {
                for f in &t.fields {
                    if let Some(d) = &f.default {
                        self.expr(d);
                    }
                }
                for m in &t.methods {
                    self.function(m, true);
                }
            }
            StmtKind::Test { body, .. } => {
                self.enter(body, Vec::new());
                self.block(body);
                self.leave();
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Ident(name) => self.read(name),
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Expr(x) = p {
                        self.expr(x);
                    }
                }
            }
            ExprKind::List(items) => items.iter().for_each(|i| self.expr(i)),
            ExprKind::Object(fields) => fields.iter().for_each(|(_, v)| self.expr(v)),
            ExprKind::Unary(_, x) | ExprKind::Await(x) => self.expr(x),
            ExprKind::Binary(_, a, b) | ExprKind::And(a, b) | ExprKind::Or(a, b) | ExprKind::Coalesce(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::IfElse { cond, then, otherwise } => {
                self.expr(cond);
                self.expr(then);
                self.expr(otherwise);
            }
            ExprKind::Range { start, end, step } => {
                self.expr(start);
                self.expr(end);
                if let Some(s) = step {
                    self.expr(s);
                }
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                args.iter().for_each(|a| self.expr(&a.value));
            }
            ExprKind::Field { object, .. } => self.expr(object),
            ExprKind::Index { object, index } => {
                self.expr(object);
                self.expr(index);
            }
            ExprKind::Lambda(f) => self.function(f, false),
            ExprKind::Int(_) | ExprKind::Decimal(_) | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Null => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn codes(src: &str) -> Vec<String> {
        lint(&parse_source(src).unwrap())
            .into_iter()
            .map(|d| format!("{} {}", d.code.unwrap_or(""), d.message))
            .collect()
    }

    #[test]
    fn unused_things() {
        let w = codes("use math\nuse json\nx = 1\n_y = 2\nshow json.stringify(3)\n");
        assert_eq!(w, vec!["LIP9002 \"math\" is imported but never used", "LIP9001 \"x\" is assigned but never used"]);
    }

    #[test]
    fn closures_exports_and_outer_updates_are_uses() {
        assert!(codes("total = 0\nadd(n)\n    total = total + n\nadd(1)\nshow total\n").is_empty());
        assert!(codes("limit = 3\nexport limit\n").is_empty());
        assert!(codes("make()\n    count = 0\n    inc()\n        count += 1\n        return count\n    return inc\nshow make()\n").is_empty());
    }

    #[test]
    fn shadowing_and_unreachable() {
        let w = codes("name = \"a\"\ngreet(name)\n    return name\n    show \"never\"\nshow greet(name)\n");
        assert_eq!(w, vec!["LIP9003 \"name\" shadows a variable from an outer scope", "LIP9004 this code never runs"]);
    }
}
