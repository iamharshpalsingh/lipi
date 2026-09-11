//! Semantic analysis and gradual type checking.
//!
//! The checker runs before the program executes and reports mistakes that are
//! certain to fail: unknown names, operations on values of the wrong type,
//! wrong argument counts, changing constants, `return` outside a function...
//!
//! It is deliberately conservative: when it can't be sure of a type it
//! assumes `any` and lets the runtime check instead. Adding type annotations
//! (`age: number = 25`) gives it more to work with.

use crate::ast::*;
use crate::diagnostics::{Diagnostic, Span};
use crate::suggest;
use std::collections::{HashMap, HashSet};

// ----- shared vocabulary (also used by the runtime) ------------------------

pub const STRING_MEMBERS: &[&str] = &[
    "length", "upper", "lower", "trim", "trim_start", "trim_end", "split", "contains", "starts_with",
    "ends_with", "replace", "index_of", "slice", "repeat", "chars", "lines", "is_empty", "pad_start",
    "pad_end", "reverse", "to_number",
];

pub const LIST_MEMBERS: &[&str] = &[
    "length", "first", "last", "push", "pop", "insert", "remove_at", "remove", "contains", "index_of",
    "join", "map", "filter", "reduce", "each", "find", "any", "all", "sort", "sort_by", "reverse",
    "slice", "sum", "min", "max", "is_empty", "copy", "unique", "flat", "count",
];

pub const OBJECT_MEMBERS: &[&str] = &["keys", "values", "entries", "has", "get", "remove", "copy", "length", "is_empty"];

pub const NUMBER_MEMBERS: &[&str] = &["round", "floor", "ceil", "abs", "to_string"];

pub const TASK_MEMBERS: &[&str] = &["cancel", "is_done"];

pub const BUILTIN_TYPES: &[&str] = &["number", "string", "bool", "nil", "any", "list", "object", "function", "task"];

/// "a number", "an object"
pub fn with_article(ty: &str) -> String {
    match ty.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => format!("an {ty}"),
        _ => format!("a {ty}"),
    }
}

/// A plain-language hint about why `expr` (of type `ty`) doesn't fit.
pub fn operand_hint(expr: &Expr, ty: &str, op: Option<BinOp>) -> String {
    match (&expr.kind, ty) {
        (_, "string") if op == Some(BinOp::Mul) => {
            "To repeat text, use .repeat(n), for example: \"-\".repeat(20)".to_string()
        }
        (ExprKind::Ident(name), "string") => {
            format!("\"{name}\" is a string. Convert it with to_number({name}), or use a numeric value.")
        }
        (_, "string") => "This is text (a string). Convert it with to_number(...) first.".to_string(),
        (ExprKind::Ident(name), "nil") => {
            format!("\"{name}\" is nil (it has no value yet). Give it a value first, or use {name} ?? 0.")
        }
        (ExprKind::Ident(name), _) => format!("\"{name}\" is {}.", with_article(ty)),
        _ => format!("This value is {}.", with_article(ty)),
    }
}

/// The error for a binary operator applied to values of the wrong types.
/// `l` and `r` are type names such as "number" or "string".
pub fn binary_error(op: BinOp, l: &str, r: &str, left: &Expr, right: &Expr) -> Diagnostic {
    let both = left.span.to(right.span);
    match op {
        BinOp::Add if (l == "string" && r == "number") || (l == "number" && r == "string") => {
            let (text, num) = if l == "string" { (left, right) } else { (right, left) };
            if let ExprKind::Ident(name) = &text.kind {
                Diagnostic::error("expected a number", text.span).with_hint(format!(
                    "\"{name}\" is a string. Convert it with to_number({name}), or use a numeric value."
                ))
            } else {
                Diagnostic::error("can't add a number to text", num.span)
                    .with_hint("To put a value inside text, use interpolation, for example: \"Total: {total}\"")
            }
        }
        BinOp::Add => Diagnostic::error(format!("can't add {} and {}", with_article(l), with_article(r)), both)
            .with_hint("+ works with two numbers, two strings or two lists."),
        op if op.is_arithmetic() => {
            let (bad, ty) = if l != "number" { (left, l) } else { (right, r) };
            Diagnostic::error("expected a number", bad.span).with_hint(operand_hint(bad, ty, Some(op)))
        }
        op if op.is_ordering() => Diagnostic::error(
            format!("can't compare {} with {}", with_article(l), with_article(r)),
            both,
        )
        .with_hint("< and > compare two numbers or two strings."),
        _ => Diagnostic::error(format!("can't look for something `in` {}", with_article(r)), right.span)
            .with_hint("`in` works with lists, text and objects."),
    }
}

/// The error for a name that isn't defined anywhere.
pub fn unknown_name<'a>(name: &str, span: Span, candidates: impl IntoIterator<Item = &'a str>) -> Diagnostic {
    let hint = suggest::foreign_name_hint(name)
        .map(String::from)
        .or_else(|| suggest::closest(name, candidates).map(|c| format!("Did you mean `{c}`?")))
        .unwrap_or_else(|| "Make sure it's defined before it's used, and check the spelling.".to_string());
    Diagnostic::error(format!("I don't know what `{name}` is"), span).with_hint(hint)
}

/// The error for accessing a member that a built-in type doesn't have.
pub fn unknown_member(ty: &str, name: &str, span: Span, members: &[&str]) -> Diagnostic {
    let plural = match ty {
        "list" => "lists",
        "string" => "strings",
        "number" => "numbers",
        "object" => "objects",
        "task" => "tasks",
        other => return Diagnostic::error(format!("{} has no `{name}`", with_article(other)), span),
    };
    let hint = suggest::closest(name, members.iter().copied())
        .map(|c| format!("Did you mean `{c}`?"))
        .unwrap_or_else(|| format!("Available: {}", members.join(", ")));
    Diagnostic::error(format!("{plural} don't have `{name}`"), span).with_hint(hint)
}

// ----- static types ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Number,
    Str,
    Bool,
    Nil,
    List,
    Object,
    Function,
    Any,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::Number => "number",
            Ty::Str => "string",
            Ty::Bool => "bool",
            Ty::Nil => "nil",
            Ty::List => "list",
            Ty::Object => "object",
            Ty::Function => "function",
            Ty::Any => "any",
        }
    }

    fn known(self) -> bool {
        self != Ty::Any
    }
}

#[derive(Debug, Clone)]
struct Sig {
    name: String,
    params: Vec<(String, Ty, bool)>, // name, declared type, has default
}

#[derive(Debug, Clone)]
struct Var {
    ty: Ty,
    declared: Option<Ty>,
    constant: bool,
    sig: Option<Sig>,
}

struct Scope {
    vars: HashMap<String, Var>,
}

pub struct Checker {
    diags: Vec<Diagnostic>,
    scopes: Vec<Scope>,
    builtins: HashSet<String>,
    /// For every name, the types of all values assigned to it anywhere in the program.
    global_types: HashMap<String, Vec<Option<Ty>>>,
    def_counts: HashMap<String, usize>,
    type_names: HashSet<String>,
    loop_depth: usize,
    fn_depth: usize,
}

/// Check a program. `builtins` are the names the runtime defines globally.
pub fn check(program: &Program, builtins: &[&str]) -> Vec<Diagnostic> {
    let mut c = Checker {
        diags: Vec::new(),
        scopes: Vec::new(),
        builtins: builtins.iter().map(|s| s.to_string()).collect(),
        global_types: HashMap::new(),
        def_counts: HashMap::new(),
        type_names: HashSet::new(),
        loop_depth: 0,
        fn_depth: 0,
    };
    c.survey(&program.body);
    c.push_scope(&program.body, &[]);
    c.block(&program.body);
    c.scopes.pop();
    c.diags
}

/// Type of a literal-ish expression without looking up any names.
fn shallow(expr: &Expr) -> Option<Ty> {
    Some(match &expr.kind {
        ExprKind::Number(_) => Ty::Number,
        ExprKind::Str(_) | ExprKind::Template(_) => Ty::Str,
        ExprKind::Bool(_) => Ty::Bool,
        ExprKind::List(_) | ExprKind::Range { .. } => Ty::List,
        ExprKind::Object(_) => Ty::Object,
        ExprKind::Lambda(_) => Ty::Function,
        ExprKind::Unary(UnaryOp::Not, _) => Ty::Bool,
        ExprKind::Unary(UnaryOp::Neg, inner) => return shallow(inner).filter(|t| *t == Ty::Number),
        ExprKind::Binary(op, l, r) => match op {
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq | BinOp::In | BinOp::NotIn => Ty::Bool,
            _ => {
                let (l, r) = (shallow(l)?, shallow(r)?);
                if l == r && (l == Ty::Number || (*op == BinOp::Add && (l == Ty::Str || l == Ty::List))) {
                    l
                } else {
                    return None;
                }
            }
        },
        _ => return None,
    })
}

impl Checker {
    fn err(&mut self, d: Diagnostic) {
        self.diags.push(d);
    }

    fn annotation(&mut self, t: &TypeExpr) -> Ty {
        match &t.kind {
            TypeKind::Optional(inner) => {
                self.annotation(inner);
                Ty::Any
            }
            TypeKind::List(inner) => {
                self.annotation(inner);
                Ty::List
            }
            TypeKind::Named(n) => match n.as_str() {
                "number" => Ty::Number,
                "string" => Ty::Str,
                "bool" => Ty::Bool,
                "list" => Ty::List,
                "object" => Ty::Object,
                "function" => Ty::Function,
                "nil" | "any" | "task" => Ty::Any,
                other if self.type_names.contains(other) => Ty::Any,
                other => {
                    let candidates: Vec<&str> = BUILTIN_TYPES.iter().copied().chain(self.type_names.iter().map(|s| s.as_str())).collect();
                    let hint = suggest::closest(other, candidates)
                        .map(|c| format!("Did you mean `{c}`?"))
                        .unwrap_or_else(|| format!("Built-in types: {}", BUILTIN_TYPES.join(", ")));
                    let d = Diagnostic::error(format!("unknown type `{other}`"), t.span).with_hint(hint);
                    self.err(d);
                    Ty::Any
                }
            },
        }
    }

    // ----- whole-program survey -----------------------------------------

    /// Record every assignment in the program so variable types stay conservative.
    fn survey(&mut self, body: &Block) {
        for stmt in body {
            match &stmt.kind {
                StmtKind::Assign { target: Target::Name(n), op, ty, value, .. } => {
                    let t = if ty.is_some() { None } else if op.is_some() { None } else { shallow(value) };
                    self.global_types.entry(n.text.clone()).or_default().push(t);
                    self.survey_expr(value);
                }
                StmtKind::Assign { value, .. } | StmtKind::Expr(value) | StmtKind::Throw(value) => self.survey_expr(value),
                StmtKind::Show(values) => values.iter().for_each(|v| self.survey_expr(v)),
                StmtKind::Return(Some(v)) => self.survey_expr(v),
                StmtKind::Func(f) => {
                    *self.def_counts.entry(f.name.text.clone()).or_default() += 1;
                    self.global_types.entry(f.name.text.clone()).or_default().push(Some(Ty::Function));
                    self.survey(&f.body);
                }
                StmtKind::TypeDef(t) => {
                    self.type_names.insert(t.name.text.clone());
                    *self.def_counts.entry(t.name.text.clone()).or_default() += 1;
                    self.global_types.entry(t.name.text.clone()).or_default().push(None);
                    for m in &t.methods {
                        self.survey(&m.body);
                    }
                }
                StmtKind::If { branches, otherwise } => {
                    for (c, b) in branches {
                        self.survey_expr(c);
                        self.survey(b);
                    }
                    if let Some(b) = otherwise {
                        self.survey(b);
                    }
                }
                StmtKind::While { body, .. } | StmtKind::Repeat { body, .. } | StmtKind::Test { body, .. } => self.survey(body),
                StmtKind::For { first, second, body, .. } => {
                    self.global_types.entry(first.text.clone()).or_default().push(None);
                    if let Some(s) = second {
                        self.global_types.entry(s.text.clone()).or_default().push(None);
                    }
                    self.survey(body);
                }
                StmtKind::Try { body, catch, finally } => {
                    self.survey(body);
                    if let Some((name, b)) = catch {
                        if let Some(n) = name {
                            self.global_types.entry(n.text.clone()).or_default().push(None);
                        }
                        self.survey(b);
                    }
                    if let Some(b) = finally {
                        self.survey(b);
                    }
                }
                StmtKind::Match { arms, otherwise, .. } => {
                    for a in arms {
                        self.survey(&a.body);
                    }
                    if let Some(b) = otherwise {
                        self.survey(b);
                    }
                }
                StmtKind::Import { alias, names, source } => {
                    let mut add = |n: &str| self.global_types.entry(n.to_string()).or_default().push(None);
                    if let Some(names) = names {
                        names.iter().for_each(|n| add(&n.text));
                    } else if let Some(a) = alias {
                        add(&a.text);
                    } else {
                        add(&module_binding_name(source));
                    }
                }
                _ => {}
            }
        }
    }

    fn survey_expr(&mut self, expr: &Expr) {
        // Parameters of lambdas can shadow names; mark them as unknown.
        if let ExprKind::Lambda(f) = &expr.kind {
            for p in &f.params {
                self.global_types.entry(p.name.text.clone()).or_default().push(None);
            }
        }
    }

    fn var_type(&self, name: &str) -> Ty {
        match self.global_types.get(name) {
            Some(types) if !types.is_empty() => {
                let first = types[0];
                if types.iter().all(|t| *t == first) {
                    match first {
                        Some(Ty::Nil) | None => Ty::Any,
                        Some(t) => t,
                    }
                } else {
                    Ty::Any
                }
            }
            _ => Ty::Any,
        }
    }

    // ----- scopes --------------------------------------------------------

    fn lookup(&self, name: &str) -> Option<&Var> {
        self.scopes.iter().rev().find_map(|s| s.vars.get(name))
    }

    fn visible_names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.scopes.iter().flat_map(|s| s.vars.keys().map(|k| k.as_str())).collect();
        v.extend(self.builtins.iter().map(|s| s.as_str()));
        v
    }

    /// Create a scope for a function (or the module) holding every name it assigns.
    fn push_scope(&mut self, body: &Block, params: &[(String, Var)]) {
        let mut vars: HashMap<String, Var> = params.iter().cloned().collect();
        let mut names = Vec::new();
        collect_names(body, &mut names);
        for (name, ty, constant, sig) in names {
            if vars.contains_key(&name) {
                if let Some(v) = vars.get_mut(&name) {
                    v.sig = None;
                }
                continue;
            }
            // Assigning to a name from an enclosing scope updates that variable.
            if self.lookup(&name).is_some() && !self.scopes.is_empty() {
                continue;
            }
            let declared = ty.as_ref().map(|t| self.annotation(t));
            let inferred = declared.unwrap_or_else(|| self.var_type(&name));
            let sig = sig.filter(|_| self.def_counts.get(&name) == Some(&1) && self.global_types.get(&name).map_or(0, |t| t.len()) == 1);
            vars.insert(name, Var { ty: inferred, declared, constant, sig });
        }
        self.scopes.push(Scope { vars });
    }

    // ----- statements ----------------------------------------------------

    fn block(&mut self, body: &Block) {
        for stmt in body {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Expr(e) => {
                self.expr(e);
            }
            StmtKind::Show(values) => {
                for v in values {
                    self.expr(v);
                }
            }
            StmtKind::Assign { target, op, ty, value, constant } => self.assign(target, *op, ty.as_ref(), value, *constant, stmt.span),
            StmtKind::If { branches, otherwise } => {
                for (cond, body) in branches {
                    self.expr(cond);
                    self.block(body);
                }
                if let Some(b) = otherwise {
                    self.block(b);
                }
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.loop_body(body);
            }
            StmtKind::Repeat { count, body } => {
                let t = self.expr(count);
                if t.known() && t != Ty::Number {
                    self.err(Diagnostic::error("`repeat` needs a number", count.span).with_hint(operand_hint(count, t.name(), None)));
                }
                self.loop_body(body);
            }
            StmtKind::For { iter, body, .. } => {
                let t = self.expr(iter);
                if matches!(t, Ty::Number | Ty::Bool | Ty::Function) {
                    self.err(Diagnostic::error(format!("can't loop over {}", with_article(t.name())), iter.span)
                        .with_hint("Loop over a list, text, an object or a range like 1 to 10."));
                }
                self.loop_body(body);
            }
            StmtKind::Func(f) => self.function(f, None),
            StmtKind::Return(value) => {
                if self.fn_depth == 0 {
                    self.err(Diagnostic::error("`return` can only be used inside a function", stmt.span));
                }
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.loop_depth == 0 {
                    let word = if matches!(stmt.kind, StmtKind::Break) { "break" } else { "continue" };
                    self.err(Diagnostic::error(format!("`{word}` can only be used inside a loop"), stmt.span));
                }
            }
            StmtKind::Throw(v) => {
                self.expr(v);
            }
            StmtKind::Try { body, catch, finally } => {
                self.block(body);
                if let Some((_, b)) = catch {
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
            StmtKind::Import { .. } => {}
            StmtKind::TypeDef(t) => {
                for f in &t.fields {
                    let declared = f.ty.as_ref().map(|ty| self.annotation(ty));
                    if let Some(d) = &f.default {
                        let vt = self.expr(d);
                        if let Some(dt) = declared {
                            self.check_fits(dt, vt, d, &f.name.text);
                        }
                    }
                }
                for m in &t.methods {
                    self.function(m, Some(&t.name.text));
                }
            }
            StmtKind::Test { body, .. } => {
                self.fn_depth += 1;
                self.push_scope(body, &[]);
                self.block(body);
                self.scopes.pop();
                self.fn_depth -= 1;
            }
        }
    }

    fn loop_body(&mut self, body: &Block) {
        self.loop_depth += 1;
        self.block(body);
        self.loop_depth -= 1;
    }

    fn check_fits(&mut self, declared: Ty, actual: Ty, value: &Expr, name: &str) {
        if declared.known() && actual.known() && declared != actual {
            self.err(
                Diagnostic::error(
                    format!("`{name}` should be {}, but this is {}", with_article(declared.name()), with_article(actual.name())),
                    value.span,
                )
                .with_hint(format!("`{name}` was declared as {}. Give it a value of that type.", declared.name())),
            );
        }
    }

    fn assign(&mut self, target: &Target, op: Option<BinOp>, ty: Option<&TypeExpr>, value: &Expr, constant: bool, span: Span) {
        let vt = self.expr(value);
        match target {
            Target::Name(name) => {
                let var = self.lookup(&name.text).cloned();
                if let Some(var) = &var {
                    if var.constant && !constant {
                        self.err(
                            Diagnostic::error(format!("`{}` is a constant and can't be changed", name.text), span)
                                .with_hint("Remove `const` where it's defined if it needs to change."),
                        );
                        return;
                    }
                }
                if let Some(op) = op {
                    if var.is_none() && !self.builtins.contains(&name.text) {
                        let names = self.visible_names();
                        let d = unknown_name(&name.text, name.span, names);
                        self.err(d);
                        return;
                    }
                    let current = var.as_ref().map_or(Ty::Any, |v| v.ty);
                    let target_expr = Expr { kind: ExprKind::Ident(name.text.clone()), span: name.span };
                    self.binary_types(op, current, vt, &target_expr, value);
                    return;
                }
                if let Some(t) = ty {
                    // Already resolved when the scope was created; re-resolve for the message.
                    let declared = self.annotation_quiet(t);
                    self.check_fits(declared, vt, value, &name.text);
                } else if let Some(Var { declared: Some(d), .. }) = var {
                    self.check_fits(d, vt, value, &name.text);
                }
            }
            Target::Field(obj, _) => {
                self.expr(obj);
            }
            Target::Index(obj, index) => {
                self.expr(obj);
                self.expr(index);
            }
        }
    }

    fn annotation_quiet(&mut self, t: &TypeExpr) -> Ty {
        let before = self.diags.len();
        let ty = self.annotation(t);
        self.diags.truncate(before);
        ty
    }

    fn function(&mut self, f: &FuncDecl, owner_type: Option<&str>) {
        let mut params: Vec<(String, Var)> = Vec::new();
        if owner_type.is_some() {
            params.push(("self".into(), Var { ty: Ty::Any, declared: None, constant: false, sig: None }));
        }
        for p in &f.params {
            let declared = p.ty.as_ref().map(|t| self.annotation(t));
            if let Some(d) = &p.default {
                let dt = self.expr(d);
                if let Some(decl) = declared {
                    self.check_fits(decl, dt, d, &p.name.text);
                }
            }
            params.push((p.name.text.clone(), Var { ty: declared.unwrap_or(Ty::Any), declared, constant: false, sig: None }));
        }
        if let Some(r) = &f.ret {
            self.annotation(r);
        }
        let saved_loop = std::mem::replace(&mut self.loop_depth, 0);
        self.fn_depth += 1;
        self.push_scope(&f.body, &params);
        self.block(&f.body);
        self.scopes.pop();
        self.fn_depth -= 1;
        self.loop_depth = saved_loop;
    }

    // ----- expressions ---------------------------------------------------

    fn expr(&mut self, e: &Expr) -> Ty {
        match &e.kind {
            ExprKind::Number(_) => Ty::Number,
            ExprKind::Str(_) => Ty::Str,
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Expr(x) = p {
                        self.expr(x);
                    }
                }
                Ty::Str
            }
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Nil => Ty::Nil,
            ExprKind::Ident(name) => {
                if let Some(v) = self.lookup(name) {
                    return v.ty;
                }
                if !self.builtins.contains(name) {
                    let names = self.visible_names();
                    let d = unknown_name(name, e.span, names);
                    self.err(d);
                }
                Ty::Any
            }
            ExprKind::List(items) => {
                for i in items {
                    self.expr(i);
                }
                Ty::List
            }
            ExprKind::Object(fields) => {
                for (_, v) in fields {
                    self.expr(v);
                }
                Ty::Object
            }
            ExprKind::Unary(UnaryOp::Neg, inner) => {
                let t = self.expr(inner);
                if t.known() && t != Ty::Number {
                    self.err(Diagnostic::error("expected a number", inner.span).with_hint(operand_hint(inner, t.name(), None)));
                }
                Ty::Number
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.expr(inner);
                Ty::Bool
            }
            ExprKind::Binary(op, l, r) => {
                let lt = self.expr(l);
                let rt = self.expr(r);
                self.binary_types(*op, lt, rt, l, r)
            }
            ExprKind::IfElse { cond, then, otherwise } => {
                self.expr(cond);
                let a = self.expr(then);
                let b = self.expr(otherwise);
                if a == b {
                    a
                } else {
                    Ty::Any
                }
            }
            ExprKind::And(l, r) | ExprKind::Or(l, r) | ExprKind::Coalesce(l, r) => {
                let lt = self.expr(l);
                let rt = self.expr(r);
                if lt == rt {
                    lt
                } else {
                    Ty::Any
                }
            }
            ExprKind::Range { start, end, step } => {
                for part in [Some(start), Some(end), step.as_ref()].into_iter().flatten() {
                    let t = self.expr(part);
                    if t.known() && t != Ty::Number {
                        self.err(Diagnostic::error("ranges need numbers", part.span).with_hint(operand_hint(part, t.name(), None)));
                    }
                }
                Ty::List
            }
            ExprKind::Call { callee, args } => self.call(callee, args, e.span),
            ExprKind::Field { object, name, .. } => {
                let t = self.expr(object);
                let members = match t {
                    Ty::Str => Some(("string", STRING_MEMBERS)),
                    Ty::List => Some(("list", LIST_MEMBERS)),
                    Ty::Number => Some(("number", NUMBER_MEMBERS)),
                    _ => None,
                };
                if let Some((tyname, members)) = members {
                    if !members.contains(&name.text.as_str()) {
                        self.err(unknown_member(tyname, &name.text, name.span, members));
                    }
                    if name.text == "length" {
                        return Ty::Number;
                    }
                }
                Ty::Any
            }
            ExprKind::Index { object, index } => {
                let ot = self.expr(object);
                let it = self.expr(index);
                if matches!(ot, Ty::List | Ty::Str) && it.known() && it != Ty::Number {
                    self.err(Diagnostic::error("list and text positions must be numbers", index.span)
                        .with_hint(operand_hint(index, it.name(), None)));
                }
                if ot == Ty::Str {
                    Ty::Str
                } else {
                    Ty::Any
                }
            }
            ExprKind::Lambda(f) => {
                self.function(f, None);
                Ty::Function
            }
            ExprKind::Await(inner) => {
                self.expr(inner);
                Ty::Any
            }
        }
    }

    fn binary_types(&mut self, op: BinOp, lt: Ty, rt: Ty, l: &Expr, r: &Expr) -> Ty {
        let known = lt.known() && rt.known();
        match op {
            BinOp::Eq | BinOp::NotEq => Ty::Bool,
            BinOp::In | BinOp::NotIn => {
                if rt.known() && !matches!(rt, Ty::List | Ty::Str | Ty::Object) {
                    self.err(binary_error(op, lt.name(), rt.name(), l, r));
                }
                Ty::Bool
            }
            op if op.is_ordering() => {
                if known && !(lt == rt && matches!(lt, Ty::Number | Ty::Str)) {
                    self.err(binary_error(op, lt.name(), rt.name(), l, r));
                }
                Ty::Bool
            }
            BinOp::Add => {
                if known && !(lt == rt && matches!(lt, Ty::Number | Ty::Str | Ty::List)) {
                    self.err(binary_error(op, lt.name(), rt.name(), l, r));
                    return Ty::Any;
                }
                if lt == rt { lt } else { Ty::Any }
            }
            _ => {
                let bad = (lt.known() && lt != Ty::Number) || (rt.known() && rt != Ty::Number);
                if bad {
                    let (ln, rn) = (if lt.known() { lt.name() } else { "number" }, if rt.known() { rt.name() } else { "number" });
                    self.err(binary_error(op, ln, rn, l, r));
                }
                Ty::Number
            }
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Arg], span: Span) -> Ty {
        let arg_types: Vec<Ty> = args.iter().map(|a| self.expr(&a.value)).collect();
        let sig = match &callee.kind {
            ExprKind::Ident(name) => {
                if let Some(var) = self.lookup(name) {
                    var.sig.clone()
                } else {
                    if !self.builtins.contains(name) {
                        let names = self.visible_names();
                        let d = unknown_name(name, callee.span, names);
                        self.err(d);
                    }
                    return match name.as_str() {
                        "to_string" | "type_of" | "input" => Ty::Str,
                        _ => Ty::Any,
                    };
                }
            }
            _ => {
                self.expr(callee);
                None
            }
        };
        if let Some(sig) = sig {
            self.check_args(&sig, args, &arg_types, span);
        }
        Ty::Any
    }

    fn check_args(&mut self, sig: &Sig, args: &[Arg], types: &[Ty], span: Span) {
        let positional = args.iter().filter(|a| a.name.is_none()).count();
        if positional > sig.params.len() {
            let n = sig.params.len();
            let d = Diagnostic::error(
                format!(
                    "`{}` takes {} argument{}, but {} were given",
                    sig.name,
                    n,
                    if n == 1 { "" } else { "s" },
                    positional
                ),
                span,
            )
            .with_hint(format!("It is defined as {}({}).", sig.name, sig.params.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(", ")));
            self.err(d);
            return;
        }
        let mut filled = vec![false; sig.params.len()];
        for (i, (arg, ty)) in args.iter().zip(types).enumerate() {
            let idx = match &arg.name {
                None => Some(i),
                Some(n) => {
                    let found = sig.params.iter().position(|p| p.0 == n.text);
                    if found.is_none() {
                        let hint = suggest::closest(&n.text, sig.params.iter().map(|p| p.0.as_str()))
                            .map(|c| format!("Did you mean `{c}`?"))
                            .unwrap_or_else(|| format!("Its parameters are: {}", sig.params.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(", ")));
                        self.err(Diagnostic::error(format!("`{}` has no parameter named `{}`", sig.name, n.text), n.span).with_hint(hint));
                    }
                    found
                }
            };
            if let Some(idx) = idx {
                filled[idx] = true;
                let (pname, pty, _) = &sig.params[idx];
                if pty.known() && ty.known() && pty != ty {
                    self.err(
                        Diagnostic::error(
                            format!("`{pname}` should be {}, but this is {}", with_article(pty.name()), with_article(ty.name())),
                            arg.value.span,
                        )
                        .with_hint(format!("`{}` expects `{pname}` to be {}.", sig.name, with_article(pty.name()))),
                    );
                }
            }
        }
        for (i, (pname, _, has_default)) in sig.params.iter().enumerate() {
            if !filled[i] && !has_default {
                let d = Diagnostic::error(format!("missing argument `{pname}` for `{}`", sig.name), span).with_hint(format!(
                    "It is defined as {}({}).",
                    sig.name,
                    sig.params.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(", ")
                ));
                self.err(d);
                break;
            }
        }
    }
}

/// The variable name an `import` statement binds when no alias is given.
pub fn module_binding_name(source: &str) -> String {
    let base = source.rsplit(['/', '\\']).next().unwrap_or(source);
    let base = base.strip_suffix(".lipi").unwrap_or(base);
    base.rsplit('.').next().unwrap_or(base).replace('-', "_")
}

type Collected = (String, Option<TypeExpr>, bool, Option<Sig>);

/// Names assigned in a function body (not inside nested functions).
fn collect_names(body: &Block, out: &mut Vec<Collected>) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Assign { target: Target::Name(n), ty, constant, .. } => out.push((n.text.clone(), ty.clone(), *constant, None)),
            StmtKind::Func(f) => {
                let sig = Sig {
                    name: f.name.text.clone(),
                    params: f
                        .params
                        .iter()
                        .map(|p| {
                            let ty = match p.ty.as_ref().map(|t| &t.kind) {
                                Some(TypeKind::Named(n)) => match n.as_str() {
                                    "number" => Ty::Number,
                                    "string" => Ty::Str,
                                    "bool" => Ty::Bool,
                                    "list" => Ty::List,
                                    "object" => Ty::Object,
                                    "function" => Ty::Function,
                                    _ => Ty::Any,
                                },
                                Some(TypeKind::List(_)) => Ty::List,
                                _ => Ty::Any,
                            };
                            (p.name.text.clone(), ty, p.default.is_some())
                        })
                        .collect(),
                };
                out.push((f.name.text.clone(), None, false, Some(sig)));
            }
            StmtKind::TypeDef(t) => {
                let sig = Sig {
                    name: t.name.text.clone(),
                    params: t
                        .fields
                        .iter()
                        .map(|f| {
                            let optional = f.default.is_some() || matches!(f.ty.as_ref().map(|t| &t.kind), Some(TypeKind::Optional(_)));
                            (f.name.text.clone(), Ty::Any, optional)
                        })
                        .collect(),
                };
                out.push((t.name.text.clone(), None, false, Some(sig)));
            }
            StmtKind::For { first, second, body, .. } => {
                out.push((first.text.clone(), None, false, None));
                if let Some(s) = second {
                    out.push((s.text.clone(), None, false, None));
                }
                collect_names(body, out);
            }
            StmtKind::If { branches, otherwise } => {
                for (_, b) in branches {
                    collect_names(b, out);
                }
                if let Some(b) = otherwise {
                    collect_names(b, out);
                }
            }
            StmtKind::While { body, .. } | StmtKind::Repeat { body, .. } => collect_names(body, out),
            StmtKind::Try { body, catch, finally } => {
                collect_names(body, out);
                if let Some((name, b)) = catch {
                    if let Some(n) = name {
                        out.push((n.text.clone(), None, false, None));
                    }
                    collect_names(b, out);
                }
                if let Some(b) = finally {
                    collect_names(b, out);
                }
            }
            StmtKind::Match { arms, otherwise, .. } => {
                for a in arms {
                    collect_names(&a.body, out);
                }
                if let Some(b) = otherwise {
                    collect_names(b, out);
                }
            }
            StmtKind::Import { source, alias, names } => {
                if let Some(names) = names {
                    for n in names {
                        out.push((n.text.clone(), None, false, None));
                    }
                } else {
                    let bound = alias.as_ref().map(|a| a.text.clone()).unwrap_or_else(|| module_binding_name(source));
                    out.push((bound, None, false, None));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn errors(src: &str) -> Vec<String> {
        let p = parse_source(src).unwrap();
        check(&p, &["show_all", "to_number", "http"]).into_iter().map(|d| d.message).collect()
    }

    #[test]
    fn doc_example_string_plus_number() {
        let p = parse_source("age = \"twenty\"\nprice = age + 10\n").unwrap();
        let d = check(&p, &[]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].message, "expected a number");
        assert!(d[0].hint.as_ref().unwrap().contains("\"age\" is a string"));
    }

    #[test]
    fn unknown_name_suggestion() {
        let p = parse_source("name = 1\nshow nmae\n").unwrap();
        let d = check(&p, &[]);
        assert!(d[0].hint.as_ref().unwrap().contains("`name`"));
    }

    #[test]
    fn arity_and_constants() {
        assert!(errors("add(a, b)\n    return a + b\nshow add(1)\n")[0].contains("missing argument"));
        assert!(errors("const PI = 3\nPI = 4\n")[0].contains("constant"));
        assert!(errors("return 1\n")[0].contains("inside a function"));
    }

    #[test]
    fn reassigned_variables_stay_dynamic() {
        assert!(errors("x = nil\nx = 5\nshow x + 1\n").is_empty());
        assert!(errors("x = \"a\"\nx = 1\nshow x + 1\n").is_empty());
    }

    #[test]
    fn closures_and_forward_references() {
        assert!(errors("make()\n    count = 0\n    inc = () => count + 1\n    return inc\nshow helper()\nhelper()\n    return 1\n").is_empty());
    }
}
