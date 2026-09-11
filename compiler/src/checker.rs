//! Name resolution and gradual type checking.
//!
//! The checker runs before the program executes and reports mistakes that are
//! certain to fail: undefined names, operations on values of the wrong type,
//! non-Boolean conditions, wrong argument counts, changing constants, `await`
//! outside an async context, `return` outside a function...
//!
//! It is deliberately conservative: when it can't be sure of a type it assumes
//! `Any` and leaves the check to the runtime. Type annotations
//! (`age: Integer = 25`) give it more to work with.

use crate::ast::*;
use crate::diagnostics::{Diagnostic, Span};
use crate::suggest;
use std::collections::{HashMap, HashSet};

// ----- shared vocabulary (also used by the runtime) ------------------------

pub const STRING_MEMBERS: &[&str] = &[
    "length", "upper", "lower", "trim", "trimStart", "trimEnd", "split", "contains", "startsWith", "endsWith",
    "replace", "indexOf", "slice", "repeat", "chars", "lines", "isEmpty", "padStart", "padEnd", "reverse", "toNumber",
];

pub const LIST_MEMBERS: &[&str] = &[
    "length", "first", "last", "push", "pop", "insert", "removeAt", "remove", "contains", "indexOf", "join", "map",
    "filter", "reduce", "each", "find", "any", "all", "sort", "sortBy", "reverse", "slice", "sum", "min", "max",
    "isEmpty", "copy", "unique", "flat", "count",
];

pub const OBJECT_MEMBERS: &[&str] = &["keys", "values", "entries", "has", "get", "remove", "copy", "length", "isEmpty"];

pub const NUMBER_MEMBERS: &[&str] = &["round", "floor", "ceil", "abs", "toString"];

pub const TASK_MEMBERS: &[&str] = &["cancel", "isDone"];

pub const BUILTIN_TYPES: &[&str] = &["Integer", "Decimal", "Number", "String", "Boolean", "Null", "Array", "Object", "Function", "Task", "Any"];

/// "a String", "an Integer"
pub fn with_article(ty: &str) -> String {
    match ty.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => format!("an {ty}"),
        _ => format!("a {ty}"),
    }
}

/// A plain-language hint about why `expr` (of type `ty`) doesn't fit.
pub fn operand_hint(expr: &Expr, ty: &str, op: Option<BinOp>) -> String {
    match (&expr.kind, ty) {
        (_, "String") if op == Some(BinOp::Mul) => "To repeat text, use .repeat(n), for example: \"-\".repeat(20)".to_string(),
        (ExprKind::Ident(name), "String") => format!("\"{name}\" is a String. Convert it to a number or use a numeric value."),
        (_, "String") => "This is a String. Convert it with toNumber(...) first.".to_string(),
        (ExprKind::Ident(name), "Null") => format!("\"{name}\" is null (it has no value yet). Give it a value first, or use {name} ?? 0."),
        (ExprKind::Ident(name), _) => format!("\"{name}\" is {}.", with_article(ty)),
        _ => format!("This value is {}.", with_article(ty)),
    }
}

fn is_numeric_name(t: &str) -> bool {
    matches!(t, "Integer" | "Decimal" | "Number")
}

/// The error for a binary operator applied to values of the wrong types.
/// `l` and `r` are type names such as "Integer" or "String".
pub fn binary_error(op: BinOp, l: &str, r: &str, left: &Expr, right: &Expr) -> Diagnostic {
    let both = left.span.to(right.span);
    let d = match op {
        BinOp::Add if (l == "String" && is_numeric_name(r)) || (is_numeric_name(l) && r == "String") => {
            let text = if l == "String" { left } else { right };
            let message = format!("cannot add {l} and {r}");
            if matches!(text.kind, ExprKind::Ident(_)) {
                Diagnostic::error(message, text.span).with_hint(operand_hint(text, "String", None))
            } else {
                Diagnostic::error(message, both).with_hint("To put a value inside text, use interpolation, for example: \"Total: {total}\"")
            }
        }
        BinOp::Add => Diagnostic::error(format!("cannot add {l} and {r}"), both).with_hint("+ works with two numbers, two Strings or two Arrays."),
        op if op.is_arithmetic() => {
            let message = match op {
                BinOp::Sub => format!("cannot subtract {r} from {l}"),
                BinOp::Mul => format!("cannot multiply {l} by {r}"),
                BinOp::Pow => format!("cannot raise {l} to the power of {r}"),
                _ => format!("cannot divide {l} by {r}"),
            };
            let (bad, ty) = if !is_numeric_name(l) { (left, l) } else { (right, r) };
            Diagnostic::error(message, bad.span).with_hint(operand_hint(bad, ty, Some(op)))
        }
        op if op.is_ordering() => {
            Diagnostic::error(format!("cannot compare {l} with {r}"), both).with_hint("< and > compare two numbers or two Strings.")
        }
        _ => Diagnostic::error(format!("cannot check `in` {}", with_article(r)), right.span).with_hint("`in` works with Arrays, Strings and Objects."),
    };
    d.with_code("LIP2001")
}

/// The error for a condition that isn't true or false.
pub fn condition_error(expr: &Expr, ty: &str) -> Diagnostic {
    let hint = match ty {
        "Null" => "Compare with null explicitly, for example: if user != null",
        "Array" => "Check for items explicitly, for example: if not items.isEmpty()",
        "String" => "Check the text explicitly, for example: if name != \"\"",
        "Integer" | "Decimal" => "Compare it explicitly, for example: if count > 0",
        "Object" => "Compare with null or check a field, for example: if user != null",
        _ => "Conditions must be true or false.",
    };
    Diagnostic::error(format!("expected a Boolean (true or false), but this is {}", with_article(ty)), expr.span)
        .with_code("LIP2005")
        .with_hint(hint)
}

/// The error for a name that isn't defined anywhere.
pub fn unknown_name<'a>(name: &str, span: Span, candidates: impl IntoIterator<Item = &'a str>) -> Diagnostic {
    const KEYWORDS: &[&str] = &["return", "show", "while", "repeat", "break", "continue", "match", "throw", "const", "async", "await", "function", "export"];
    let hint = suggest::foreign_name_hint(name)
        .map(String::from)
        .or_else(|| suggest::did_you_mean(name, candidates.into_iter().chain(KEYWORDS.iter().copied())))
        .unwrap_or_else(|| "Make sure it's defined before it's used, and check the spelling.".to_string());
    Diagnostic::error(format!("undefined variable \"{name}\""), span).with_code("LIP1002").with_hint(hint)
}

/// The error for accessing a member that a built-in type doesn't have.
pub fn unknown_member(ty: &str, name: &str, span: Span, members: &[&str]) -> Diagnostic {
    let hint = suggest::did_you_mean(name, members.iter().copied()).unwrap_or_else(|| format!("Available: {}", members.join(", ")));
    Diagnostic::error(format!("{ty}s don't have \"{name}\""), span).with_code("LIP1004").with_hint(hint)
}

// ----- static types ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Int,
    Dec,
    /// Integer or Decimal
    Num,
    Str,
    Bool,
    Null,
    Array,
    Object,
    Function,
    Any,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::Int => "Integer",
            Ty::Dec => "Decimal",
            Ty::Num => "Number",
            Ty::Str => "String",
            Ty::Bool => "Boolean",
            Ty::Null => "Null",
            Ty::Array => "Array",
            Ty::Object => "Object",
            Ty::Function => "Function",
            Ty::Any => "Any",
        }
    }

    fn known(self) -> bool {
        self != Ty::Any
    }

    fn numeric(self) -> bool {
        matches!(self, Ty::Int | Ty::Dec | Ty::Num)
    }
}

fn named_ty(n: &str) -> Option<Ty> {
    Some(match n {
        "Integer" => Ty::Int,
        "Decimal" => Ty::Dec,
        "Number" => Ty::Num,
        "String" => Ty::Str,
        "Boolean" => Ty::Bool,
        "Array" => Ty::Array,
        "Object" => Ty::Object,
        "Function" => Ty::Function,
        "Null" | "Task" | "Any" => Ty::Any,
        _ => return None,
    })
}

/// Can a value of type `actual` be stored where `declared` is expected?
fn fits(declared: Ty, actual: Ty) -> bool {
    if !declared.known() || !actual.known() || declared == actual {
        return true;
    }
    match (declared, actual) {
        (Ty::Num, a) => a.numeric(),
        (Ty::Dec, Ty::Int | Ty::Num) => true,
        (Ty::Int, Ty::Num) => true,
        _ => false,
    }
}

/// The result type of arithmetic on two numbers.
fn arith_result(op: BinOp, l: Ty, r: Ty) -> Ty {
    if op == BinOp::Div {
        return Ty::Dec;
    }
    match (l, r) {
        (Ty::Int, Ty::Int) => Ty::Int,
        (Ty::Dec, _) | (_, Ty::Dec) => Ty::Dec,
        _ => Ty::Num,
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
    async_ok: bool,
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
        async_ok: true,
    };
    c.survey(&program.body);
    c.push_scope(&program.body, &[]);
    c.block(&program.body);
    c.scopes.pop();
    c.diags.into_iter().map(|d| d.code_or("LIP2000")).collect()
}

/// Type of a literal-ish expression without looking up any names.
fn shallow(expr: &Expr) -> Option<Ty> {
    Some(match &expr.kind {
        ExprKind::Int(_) => Ty::Int,
        ExprKind::Decimal(_) => Ty::Dec,
        ExprKind::Str(_) | ExprKind::Template(_) => Ty::Str,
        ExprKind::Bool(_) => Ty::Bool,
        ExprKind::Null => Ty::Null,
        ExprKind::List(_) | ExprKind::Range { .. } => Ty::Array,
        ExprKind::Object(_) => Ty::Object,
        ExprKind::Lambda(_) => Ty::Function,
        ExprKind::Unary(UnaryOp::Not, _) | ExprKind::And(..) | ExprKind::Or(..) => Ty::Bool,
        ExprKind::Unary(UnaryOp::Neg, inner) => return shallow(inner).filter(|t| t.numeric()),
        ExprKind::Binary(op, l, r) => match op {
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq | BinOp::In | BinOp::NotIn => Ty::Bool,
            _ => {
                let (l, r) = (shallow(l)?, shallow(r)?);
                if l.numeric() && r.numeric() {
                    arith_result(*op, l, r)
                } else if l == r && *op == BinOp::Add && (l == Ty::Str || l == Ty::Array) {
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
                Ty::Array
            }
            TypeKind::Named(n) => match named_ty(n) {
                Some(ty) => ty,
                None if self.type_names.contains(n) => Ty::Any,
                None => {
                    let capitalized: String = n.chars().take(1).flat_map(char::to_uppercase).chain(n.chars().skip(1)).collect();
                    let hint = if named_ty(&capitalized).is_some() || capitalized == "Bool" || capitalized == "List" {
                        let fixed = match capitalized.as_str() {
                            "Bool" => "Boolean",
                            "List" => "Array",
                            other => other,
                        };
                        format!("type names are capitalized: \"{fixed}\"")
                    } else {
                        let candidates: Vec<&str> = BUILTIN_TYPES.iter().copied().chain(self.type_names.iter().map(|s| s.as_str())).collect();
                        suggest::did_you_mean(n, candidates).unwrap_or_else(|| format!("Built-in types: {}", BUILTIN_TYPES.join(", ")))
                    };
                    let d = Diagnostic::error(format!("unknown type \"{n}\""), t.span).with_code("LIP2006").with_hint(hint);
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
            self.survey_stmt(stmt);
        }
    }

    fn survey_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Assign { target: Target::Name(n), op, ty, value, .. } => {
                let t = if ty.is_some() || op.is_some() { None } else { shallow(value) };
                self.global_types.entry(n.text.clone()).or_default().push(t);
                self.survey_expr(value);
            }
            StmtKind::Assign { value, .. } | StmtKind::Expr(value) | StmtKind::Throw(value) => self.survey_expr(value),
            StmtKind::Show(values) => values.iter().for_each(|v| self.survey_expr(v)),
            StmtKind::Return(Some(v)) => self.survey_expr(v),
            StmtKind::State { name, value, .. } => {
                self.global_types.entry(name.text.clone()).or_default().push(None);
                self.survey_expr(value);
            }
            StmtKind::Func(f) | StmtKind::Component(f) => {
                *self.def_counts.entry(f.name.text.clone()).or_default() += 1;
                self.global_types.entry(f.name.text.clone()).or_default().push(Some(Ty::Function));
                self.mark_params(&f.params);
                self.survey(&f.body);
            }
            StmtKind::TypeDef(t) => {
                self.type_names.insert(t.name.text.clone());
                *self.def_counts.entry(t.name.text.clone()).or_default() += 1;
                self.global_types.entry(t.name.text.clone()).or_default().push(None);
                for m in &t.methods {
                    self.mark_params(&m.params);
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
            StmtKind::Use { alias, names, source } => {
                let bound: Vec<String> = match (names, alias) {
                    (Some(names), _) => names.iter().map(|n| n.text.clone()).collect(),
                    (None, Some(a)) => vec![a.text.clone()],
                    (None, None) => vec![module_binding_name(source)],
                };
                for n in bound {
                    self.global_types.entry(n).or_default().push(None);
                }
            }
            StmtKind::Export { inner: Some(s), .. } => self.survey_stmt(s),
            _ => {}
        }
    }

    fn mark_params(&mut self, params: &[Param]) {
        for p in params {
            self.global_types.entry(p.name.text.clone()).or_default().push(None);
        }
    }

    fn survey_expr(&mut self, expr: &Expr) {
        // Parameters of lambdas can shadow names; mark them as unknown.
        if let ExprKind::Lambda(f) = &expr.kind {
            self.mark_params(&f.params);
        }
        if let ExprKind::Call { args, .. } = &expr.kind {
            for a in args {
                self.survey_expr(&a.value);
            }
        }
    }

    fn var_type(&self, name: &str) -> Ty {
        match self.global_types.get(name) {
            Some(types) if !types.is_empty() => {
                let first = types[0];
                if types.iter().all(|t| *t == first) {
                    match first {
                        Some(Ty::Null) | None => Ty::Any,
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
        collect_names(body, &mut names, 0);
        let mut defined: HashMap<String, u32> = HashMap::new();
        for item in names {
            if let Some(span) = item.def_span {
                match defined.get(&item.name) {
                    Some(line) => {
                        let d = Diagnostic::error(format!("\"{}\" is already defined on line {line}", item.name), span)
                            .with_code("LIP1001")
                            .with_hint("Rename one of them, or remove the extra definition.");
                        self.err(d);
                    }
                    None => {
                        defined.insert(item.name.clone(), span.line);
                    }
                }
            }
            if vars.contains_key(&item.name) {
                if let Some(v) = vars.get_mut(&item.name) {
                    v.sig = None;
                }
                continue;
            }
            // Assigning to a name from an enclosing scope updates that variable.
            if self.lookup(&item.name).is_some() && !self.scopes.is_empty() {
                continue;
            }
            let declared = item.ty.as_ref().map(|t| self.annotation(t));
            let inferred = declared.unwrap_or_else(|| self.var_type(&item.name));
            let unique = self.def_counts.get(&item.name) == Some(&1) && self.global_types.get(&item.name).map_or(0, |t| t.len()) == 1;
            let sig = item.sig.filter(|_| unique);
            vars.insert(item.name, Var { ty: inferred, declared, constant: item.constant, sig });
        }
        self.scopes.push(Scope { vars });
    }

    // ----- statements ----------------------------------------------------

    fn block(&mut self, body: &Block) {
        for stmt in body {
            self.stmt(stmt);
        }
    }

    fn condition(&mut self, e: &Expr) {
        let t = self.expr(e);
        if t.known() && t != Ty::Bool {
            self.err(condition_error(e, t.name()));
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
                    self.condition(cond);
                    self.block(body);
                }
                if let Some(b) = otherwise {
                    self.block(b);
                }
            }
            StmtKind::While { cond, body } => {
                self.condition(cond);
                self.loop_body(body);
            }
            StmtKind::Repeat { count, body } => {
                let t = self.expr(count);
                if t.known() && t != Ty::Int && t != Ty::Num {
                    self.err(Diagnostic::error("`repeat` needs an Integer", count.span).with_code("LIP2001").with_hint(operand_hint(count, t.name(), None)));
                }
                self.loop_body(body);
            }
            StmtKind::For { iter, body, .. } => {
                let t = self.expr(iter);
                if t.numeric() || matches!(t, Ty::Bool | Ty::Function | Ty::Null) {
                    self.err(Diagnostic::error(format!("cannot loop over {}", with_article(t.name())), iter.span)
                        .with_code("LIP2001")
                        .with_hint("Loop over an Array, a String, an Object or a range like 1 to 10."));
                }
                self.loop_body(body);
            }
            StmtKind::Func(f) | StmtKind::Component(f) => self.function(f, None),
            StmtKind::State { name, ty, value } => self.assign(&Target::Name(name.clone()), None, ty.as_ref(), value, false, stmt.span),
            StmtKind::Return(value) => {
                if self.fn_depth == 0 {
                    self.err(Diagnostic::error("`return` can only be used inside a function", stmt.span).with_code("LIP1006"));
                }
                if let Some(v) = value {
                    self.expr(v);
                }
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.loop_depth == 0 {
                    let word = if matches!(stmt.kind, StmtKind::Break) { "break" } else { "continue" };
                    self.err(Diagnostic::error(format!("`{word}` can only be used inside a loop"), stmt.span).with_code("LIP1006"));
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
                        self.condition(g);
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
                if self.fn_depth > 0 || self.scopes.len() > 1 {
                    self.err(Diagnostic::error("`export` can only be used at the top level of a file", stmt.span).with_code("LIP3005"));
                    return;
                }
                for n in names {
                    if !self.scopes[0].vars.contains_key(&n.text) {
                        let names: Vec<&str> = self.scopes[0].vars.keys().map(|k| k.as_str()).collect();
                        let hint = suggest::did_you_mean(&n.text, names).unwrap_or_else(|| "Define it in this file before exporting it.".into());
                        self.err(Diagnostic::error(format!("cannot export \"{}\": it isn't defined in this file", n.text), n.span)
                            .with_code("LIP3005")
                            .with_hint(hint));
                    }
                }
            }
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
                let saved = std::mem::replace(&mut self.async_ok, true);
                self.fn_depth += 1;
                self.push_scope(body, &[]);
                self.block(body);
                self.scopes.pop();
                self.fn_depth -= 1;
                self.async_ok = saved;
            }
        }
    }

    fn loop_body(&mut self, body: &Block) {
        self.loop_depth += 1;
        self.block(body);
        self.loop_depth -= 1;
    }

    fn check_fits(&mut self, declared: Ty, actual: Ty, value: &Expr, name: &str) {
        if !fits(declared, actual) {
            self.err(
                Diagnostic::error(
                    format!("\"{name}\" should be {}, but this is {}", with_article(declared.name()), with_article(actual.name())),
                    value.span,
                )
                .with_code("LIP2002")
                .with_hint(format!("\"{name}\" was declared as {}. Give it a value of that type.", declared.name())),
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
                            Diagnostic::error(format!("\"{}\" is a constant and can't be changed", name.text), span)
                                .with_code("LIP1003")
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
                    let target_expr = Expr { res: Default::default(), kind: ExprKind::Ident(name.text.clone()), span: name.span };
                    self.binary_types(op, current, vt, &target_expr, value);
                    return;
                }
                if let Some(t) = ty {
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
        let saved_async = self.async_ok;
        if !f.is_lambda {
            self.async_ok = f.is_async;
        }
        self.fn_depth += 1;
        self.push_scope(&f.body, &params);
        self.block(&f.body);
        self.scopes.pop();
        self.fn_depth -= 1;
        self.loop_depth = saved_loop;
        self.async_ok = saved_async;
    }

    // ----- expressions ---------------------------------------------------

    fn expr(&mut self, e: &Expr) -> Ty {
        match &e.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Decimal(_) => Ty::Dec,
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
            ExprKind::Null => Ty::Null,
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
                Ty::Array
            }
            ExprKind::Object(fields) => {
                for (_, v) in fields {
                    self.expr(v);
                }
                Ty::Object
            }
            ExprKind::Unary(UnaryOp::Neg, inner) => {
                let t = self.expr(inner);
                if t.known() && !t.numeric() {
                    self.err(Diagnostic::error(format!("cannot negate {}", with_article(t.name())), inner.span)
                        .with_code("LIP2001")
                        .with_hint(operand_hint(inner, t.name(), None)));
                    return Ty::Any;
                }
                t
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.condition(inner);
                Ty::Bool
            }
            ExprKind::Binary(op, l, r) => {
                let lt = self.expr(l);
                let rt = self.expr(r);
                self.binary_types(*op, lt, rt, l, r)
            }
            ExprKind::IfElse { cond, then, otherwise } => {
                self.condition(cond);
                let a = self.expr(then);
                let b = self.expr(otherwise);
                if a == b {
                    a
                } else {
                    Ty::Any
                }
            }
            ExprKind::And(l, r) | ExprKind::Or(l, r) => {
                self.condition(l);
                self.condition(r);
                Ty::Bool
            }
            ExprKind::Coalesce(l, r) => {
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
                    if t.known() && t != Ty::Int && t != Ty::Num {
                        self.err(Diagnostic::error("ranges need Integers", part.span).with_code("LIP2001").with_hint(operand_hint(part, t.name(), None)));
                    }
                }
                Ty::Array
            }
            ExprKind::Call { callee, args } => self.call(callee, args, e.span),
            ExprKind::Field { object, name, .. } => {
                let t = self.expr(object);
                let members = match t {
                    Ty::Str => Some(("String", STRING_MEMBERS)),
                    Ty::Array => Some(("Array", LIST_MEMBERS)),
                    Ty::Int | Ty::Dec | Ty::Num => Some((t.name(), NUMBER_MEMBERS)),
                    _ => None,
                };
                if let Some((tyname, members)) = members {
                    if !members.contains(&name.text.as_str()) {
                        self.err(unknown_member(tyname, &name.text, name.span, members));
                    }
                    if name.text == "length" {
                        return Ty::Int;
                    }
                }
                Ty::Any
            }
            ExprKind::Index { object, index } => {
                let ot = self.expr(object);
                let it = self.expr(index);
                if matches!(ot, Ty::Array | Ty::Str) && it.known() && it != Ty::Int && it != Ty::Num {
                    self.err(Diagnostic::error("positions in Arrays and Strings must be Integers", index.span)
                        .with_code("LIP2001")
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
                if !self.async_ok {
                    self.err(Diagnostic::error("await used outside async context", e.span)
                        .with_code("LIP4001")
                        .with_hint("Mark the function as async, for example: async loadUser(id)"));
                }
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
                if rt.known() && !matches!(rt, Ty::Array | Ty::Str | Ty::Object) {
                    self.err(binary_error(op, lt.name(), rt.name(), l, r));
                }
                Ty::Bool
            }
            op if op.is_ordering() => {
                if known && !((lt.numeric() && rt.numeric()) || (lt == Ty::Str && rt == Ty::Str)) {
                    self.err(binary_error(op, lt.name(), rt.name(), l, r));
                }
                Ty::Bool
            }
            BinOp::Add => {
                if lt.numeric() && rt.numeric() {
                    return arith_result(op, lt, rt);
                }
                if known && !(lt == rt && matches!(lt, Ty::Str | Ty::Array)) {
                    self.err(binary_error(op, lt.name(), rt.name(), l, r));
                    return Ty::Any;
                }
                if lt == rt {
                    lt
                } else {
                    Ty::Any
                }
            }
            _ => {
                let bad = (lt.known() && !lt.numeric()) || (rt.known() && !rt.numeric());
                if bad {
                    let (ln, rn) = (if lt.known() { lt.name() } else { "Number" }, if rt.known() { rt.name() } else { "Number" });
                    self.err(binary_error(op, ln, rn, l, r));
                    return Ty::Any;
                }
                if lt.known() && rt.known() {
                    arith_result(op, lt, rt)
                } else if op == BinOp::Div {
                    Ty::Dec
                } else {
                    Ty::Any
                }
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
                        "toString" | "typeOf" | "input" => Ty::Str,
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
        let signature = || format!("It is defined as {}({}).", sig.name, sig.params.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join(", "));
        let positional = args.iter().filter(|a| a.name.is_none()).count();
        if positional > sig.params.len() {
            let n = sig.params.len();
            let d = Diagnostic::error(format!("\"{}\" takes {} argument{}, but {} were given", sig.name, n, if n == 1 { "" } else { "s" }, positional), span)
                .with_code("LIP2003")
                .with_hint(signature());
            self.err(d);
            return;
        }
        let mut filled = vec![false; sig.params.len()];
        let mut next_positional = 0;
        for (arg, ty) in args.iter().zip(types) {
            let idx = match &arg.name {
                None => {
                    next_positional += 1;
                    Some(next_positional - 1)
                }
                Some(n) => {
                    let found = sig.params.iter().position(|p| p.0 == n.text);
                    if found.is_none() {
                        let hint = suggest::did_you_mean(&n.text, sig.params.iter().map(|p| p.0.as_str())).unwrap_or_else(signature);
                        self.err(Diagnostic::error(format!("\"{}\" has no parameter named \"{}\"", sig.name, n.text), n.span).with_code("LIP1007").with_hint(hint));
                    }
                    found
                }
            };
            if let Some(idx) = idx {
                filled[idx] = true;
                let (pname, pty, _) = &sig.params[idx];
                if !fits(*pty, *ty) {
                    self.err(
                        Diagnostic::error(format!("\"{pname}\" should be {}, but this is {}", with_article(pty.name()), with_article(ty.name())), arg.value.span)
                            .with_code("LIP2004")
                            .with_hint(format!("\"{}\" expects \"{pname}\" to be {}.", sig.name, with_article(pty.name()))),
                    );
                }
            }
        }
        for (i, (pname, _, has_default)) in sig.params.iter().enumerate() {
            if !filled[i] && !has_default {
                let d = Diagnostic::error(format!("missing argument \"{pname}\" for \"{}\"", sig.name), span).with_code("LIP2003").with_hint(signature());
                self.err(d);
                break;
            }
        }
    }
}

/// The variable name a `use` statement binds when no alias is given.
pub fn module_binding_name(source: &str) -> String {
    let base = source.rsplit(['/', '\\']).next().unwrap_or(source);
    let base = base.strip_suffix(".lipi").unwrap_or(base);
    base.rsplit('.').next().unwrap_or(base).replace('-', "_")
}

struct Collected {
    name: String,
    ty: Option<TypeExpr>,
    constant: bool,
    sig: Option<Sig>,
    /// Set for definitions (functions, types, constants) at the top of the body,
    /// which may not be repeated.
    def_span: Option<Span>,
}

fn param_ty(p: &Param) -> Ty {
    match p.ty.as_ref().map(|t| &t.kind) {
        Some(TypeKind::Named(n)) => named_ty(n).unwrap_or(Ty::Any),
        Some(TypeKind::List(_)) => Ty::Array,
        _ => Ty::Any,
    }
}

/// Names assigned in a function body (not inside nested functions).
fn collect_names(body: &Block, out: &mut Vec<Collected>, depth: usize) {
    for stmt in body {
        collect_stmt(stmt, out, depth);
    }
}

fn collect_stmt(stmt: &Stmt, out: &mut Vec<Collected>, depth: usize) {
    let top = |span: Span| if depth == 0 { Some(span) } else { None };
    let plain = |name: &str| Collected { name: name.to_string(), ty: None, constant: false, sig: None, def_span: None };
    match &stmt.kind {
        StmtKind::Assign { target: Target::Name(n), ty, constant, .. } => out.push(Collected {
            name: n.text.clone(),
            ty: ty.clone(),
            constant: *constant,
            sig: None,
            def_span: if *constant { top(n.span) } else { None },
        }),
        StmtKind::State { name, ty, .. } => out.push(Collected { name: name.text.clone(), ty: ty.clone(), constant: false, sig: None, def_span: None }),
        StmtKind::Func(f) | StmtKind::Component(f) => {
            let sig = Sig { name: f.name.text.clone(), params: f.params.iter().map(|p| (p.name.text.clone(), param_ty(p), p.default.is_some())).collect() };
            out.push(Collected { name: f.name.text.clone(), ty: None, constant: false, sig: Some(sig), def_span: top(f.name.span) });
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
            out.push(Collected { name: t.name.text.clone(), ty: None, constant: false, sig: Some(sig), def_span: top(t.name.span) });
        }
        StmtKind::For { first, second, body, .. } => {
            out.push(plain(&first.text));
            if let Some(s) = second {
                out.push(plain(&s.text));
            }
            collect_names(body, out, depth + 1);
        }
        StmtKind::If { branches, otherwise } => {
            for (_, b) in branches {
                collect_names(b, out, depth + 1);
            }
            if let Some(b) = otherwise {
                collect_names(b, out, depth + 1);
            }
        }
        StmtKind::While { body, .. } | StmtKind::Repeat { body, .. } => collect_names(body, out, depth + 1),
        StmtKind::Try { body, catch, finally } => {
            collect_names(body, out, depth + 1);
            if let Some((name, b)) = catch {
                if let Some(n) = name {
                    out.push(plain(&n.text));
                }
                collect_names(b, out, depth + 1);
            }
            if let Some(b) = finally {
                collect_names(b, out, depth + 1);
            }
        }
        StmtKind::Match { arms, otherwise, .. } => {
            for a in arms {
                collect_names(&a.body, out, depth + 1);
            }
            if let Some(b) = otherwise {
                collect_names(b, out, depth + 1);
            }
        }
        StmtKind::Use { source, alias, names } => match names {
            Some(names) => names.iter().for_each(|n| out.push(plain(&n.text))),
            None => out.push(plain(&alias.as_ref().map(|a| a.text.clone()).unwrap_or_else(|| module_binding_name(source)))),
        },
        StmtKind::Export { inner: Some(s), .. } => collect_stmt(s, out, depth),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn errors(src: &str) -> Vec<String> {
        let p = parse_source(src).unwrap();
        check(&p, &["toNumber", "http"]).into_iter().map(|d| format!("{} {}", d.code.unwrap_or(""), d.message)).collect()
    }

    #[test]
    fn doc_example_string_plus_integer() {
        let p = parse_source("age = \"twenty\"\nprice = age + 10\n").unwrap();
        let d = check(&p, &[]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].message, "cannot add String and Integer");
        assert_eq!(d[0].hint.as_deref(), Some("\"age\" is a String. Convert it to a number or use a numeric value."));
    }

    #[test]
    fn undefined_variable() {
        let p = parse_source("user = 1\nshow usr\n").unwrap();
        let d = check(&p, &[]);
        assert_eq!(d[0].code, Some("LIP1002"));
        assert_eq!(d[0].message, "undefined variable \"usr\"");
        assert_eq!(d[0].hint.as_deref(), Some("did you mean \"user\"?"));
    }

    #[test]
    fn strict_conditions_and_async() {
        assert!(errors("items = [1]\nif items\n    show 1\n")[0].starts_with("LIP2005"));
        assert!(errors("load()\n    return await 1\n")[0].starts_with("LIP4001"));
        assert!(errors("async load()\n    return await 1\nx = await load()\n").is_empty());
    }

    #[test]
    fn arity_constants_duplicates_exports() {
        assert!(errors("add(a, b)\n    return a + b\nshow add(1)\n")[0].contains("missing argument"));
        assert!(errors("const limit = 3\nlimit = 4\n")[0].starts_with("LIP1003"));
        assert!(errors("return 1\n")[0].starts_with("LIP1006"));
        assert!(errors("f()\n    return 1\nf()\n    return 2\n")[0].starts_with("LIP1001"));
        assert!(errors("export nothing\n")[0].starts_with("LIP3005"));
    }

    #[test]
    fn numbers() {
        assert!(errors("x: Decimal = 5\ny: Integer = 2.5\n")[0].starts_with("LIP2002"));
        assert!(errors("x = 10 / 4\ny: Decimal = x\n").is_empty());
    }

    #[test]
    fn reassigned_variables_stay_dynamic() {
        assert!(errors("x = null\nx = 5\nshow x + 1\n").is_empty());
        assert!(errors("x = \"a\"\nx = 1\nshow x + 1\n").is_empty());
    }
}
