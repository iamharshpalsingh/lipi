//! Before a program runs, every variable gets a numbered slot, so the
//! interpreter reads and writes variables by position instead of looking
//! names up in hash maps.
//!
//! The scope rule is LiPi's (see `lipi_compiler::scope`, which the JavaScript
//! backend uses too): assignment updates the nearest existing variable,
//! otherwise it creates one in the current function. Resolutions are stored in
//! the syntax tree (`Expr::res`, `Name::res`). Each function's slot names (its
//! "layout") are kept by the interpreter, keyed by the function's address.

use crate::value::{Env, Names};
use lipi_compiler::ast::*;
use lipi_compiler::scope;
use std::collections::HashMap;
use std::rc::Rc;

/// How a `for` loop's scope is keyed in the layout table: by the address of
/// its body, which the syntax tree keeps alive for as long as the program runs.
pub fn loop_key(body: &[Stmt]) -> usize {
    body.as_ptr() as usize
}

struct Scope {
    names: Names,
    index: HashMap<Rc<str>, u32>,
}

impl Scope {
    fn new(names: Names) -> Scope {
        let index = names.borrow().iter().enumerate().map(|(i, n)| (n.clone(), i as u32)).collect();
        Scope { names, index }
    }

    fn has(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    fn add(&mut self, name: &str) {
        if !self.has(name) {
            let n: Rc<str> = Rc::from(name);
            let mut names = self.names.borrow_mut();
            self.index.insert(n.clone(), names.len() as u32);
            names.push(n);
        }
    }
}

pub struct Resolver<'a> {
    globals: &'a Env,
    scopes: Vec<Scope>,
    layouts: HashMap<usize, Names>,
}

impl<'a> Resolver<'a> {
    pub fn new(globals: &'a Env) -> Self {
        Resolver { globals, scopes: Vec::new(), layouts: HashMap::new() }
    }

    /// The layouts of every function (and test block) resolved so far, by address.
    pub fn finish(self) -> HashMap<usize, Names> {
        self.layouts
    }

    /// Resolve a file's top level. `names` are the module environment's names,
    /// which may already hold variables (the interactive prompt keeps one module).
    pub fn module(&mut self, body: &[Stmt], names: &Names) {
        let mut s = Scope::new(names.clone());
        let found = scope::collect(body);
        for (n, _) in &found.defined {
            s.add(n);
        }
        for n in &found.assigned {
            s.add(n);
        }
        self.scopes.push(s);
        self.block(body);
        self.scopes.pop();
    }

    /// `method`: a type's method, which sees `self` (and `super` when the type extends another).
    fn function(&mut self, key: usize, params: &[Param], method: Option<bool>, body: &[Stmt]) {
        let mut s = Scope::new(Names::default());
        if let Some(has_parent) = method {
            s.add("self");
            if has_parent {
                s.add("super");
            }
        }
        for p in params {
            s.add(&p.name.text);
        }
        let found = scope::collect(body);
        for (n, _) in &found.defined {
            s.add(n);
        }
        for n in &found.assigned {
            if !s.has(n) && !self.visible(n) {
                s.add(n);
            }
        }
        let names = s.names.clone();
        self.scopes.push(s);
        for p in params {
            self.name(&p.name);
            if let Some(d) = &p.default {
                self.expr(d);
            }
        }
        self.block(body);
        self.scopes.pop();
        self.layouts.insert(key, names);
    }

    fn visible(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.has(name))
    }

    fn lookup(&self, name: &str) -> Res {
        for (depth, s) in self.scopes.iter().rev().enumerate() {
            if let Some(&index) = s.index.get(name) {
                return Res::Local { depth: depth as u16, index };
            }
        }
        match self.globals.index_of(name) {
            Some(i) => Res::Global(i as u32),
            None => Res::Unknown,
        }
    }

    fn name(&mut self, n: &Name) {
        n.res.set(self.lookup(&n.text));
    }

    fn block(&mut self, body: &[Stmt]) {
        for stmt in body {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::Return(Some(e)) => self.expr(e),
            StmtKind::Show(values) => values.iter().for_each(|v| self.expr(v)),
            StmtKind::Assign { target, value, .. } => {
                self.expr(value);
                match target {
                    Target::Name(n) => self.name(n),
                    Target::Field(obj, _) => self.expr(obj),
                    Target::Index(obj, index) => {
                        self.expr(obj);
                        self.expr(index);
                    }
                    Target::Pattern(p) => p.names().into_iter().for_each(|n| self.name(n)),
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
            StmtKind::For { first, second, iter, body, pattern } => {
                // What is looped over is read outside the loop; the loop's own
                // variables get a scope of their own, so each round can hand a
                // function made in the body its own copy of them.
                self.expr(iter);
                let mut s = Scope::new(Names::default());
                for n in scope::loop_names(first, second.as_ref(), pattern.as_ref()) {
                    s.add(&n);
                }
                let names = s.names.clone();
                self.scopes.push(s);
                self.name(first);
                if let Some(s) = second {
                    self.name(s);
                }
                if let Some(p) = pattern {
                    p.names().into_iter().for_each(|n| self.name(n));
                }
                self.block(body);
                self.scopes.pop();
                self.layouts.insert(loop_key(body), names);
            }
            StmtKind::Func(f) | StmtKind::Component(f) => {
                self.name(&f.name);
                self.function(Rc::as_ptr(f) as usize, &f.params, None, &f.body);
            }
            StmtKind::TypeDef(t) => {
                self.name(&t.name);
                if let Some(p) = &t.parent {
                    self.name(p);
                }
                for f in &t.fields {
                    if let Some(d) = &f.default {
                        self.expr(d);
                    }
                }
                for m in &t.methods {
                    self.function(Rc::as_ptr(m) as usize, &m.params, Some(t.parent.is_some()), &m.body);
                }
            }
            StmtKind::State { name, value, .. } => {
                self.expr(value);
                self.name(name);
            }
            StmtKind::Try { body, catch, finally } => {
                self.block(body);
                if let Some((name, handler)) = catch {
                    if let Some(n) = name {
                        self.name(n);
                    }
                    self.block(handler);
                }
                if let Some(f) = finally {
                    self.block(f);
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
            StmtKind::Use { alias, names, .. } => {
                if let Some(a) = alias {
                    self.name(a);
                }
                for n in names.iter().flatten() {
                    self.name(n);
                }
            }
            StmtKind::Export { inner: Some(inner), .. } => self.stmt(inner),
            StmtKind::Test { body, .. } => self.function(body.as_ptr() as usize, &[], None, body),
            _ => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Ident(n) => e.res.set(self.lookup(n)),
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Expr(x) = p {
                        self.expr(x);
                    }
                }
            }
            ExprKind::List(items) => items.iter().for_each(|x| self.expr(x)),
            ExprKind::Object(fields) => fields.iter().for_each(|(_, v)| self.expr(v)),
            ExprKind::Unary(_, x) | ExprKind::Await(x) | ExprKind::Spread(x) => self.expr(x),
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
            ExprKind::Lambda(f) => self.function(Rc::as_ptr(f) as usize, &f.params, None, &f.body),
            _ => {}
        }
    }
}
