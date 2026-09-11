//! Runtime values and variable environments.

use crate::interp::{Flow, Interpreter};
use crate::task::TaskState;
use indexmap::IndexMap;
use lipi_compiler::ast::{FuncDecl, TypeDecl, TypeExpr};
use lipi_compiler::lexer::is_ident_start;
use lipi_compiler::Span;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type Fields = IndexMap<String, Value>;

/// An object: a plain `{...}` map, an instance of a `type`, or a module.
pub struct ObjectData {
    pub fields: RefCell<Fields>,
    pub ty: Option<Rc<TypeInfo>>,
    pub module: Option<String>,
}

/// A user-defined function together with the environment it closes over.
pub struct Closure {
    pub decl: Rc<FuncDecl>,
    pub env: Rc<Env>,
    pub file: Rc<str>,
    /// The instance a method is bound to (available as `self`).
    pub this: Option<Value>,
}

pub type NativeFn = dyn Fn(&mut Interpreter, &mut Args) -> Result<Value, Flow>;

/// A function implemented in Rust (standard library).
pub struct Native {
    pub name: String,
    pub f: Box<NativeFn>,
}

/// A user-defined `type`.
pub struct TypeInfo {
    pub decl: Rc<TypeDecl>,
    pub env: Rc<Env>,
    pub file: Rc<str>,
}

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    List(Rc<RefCell<Vec<Value>>>),
    Object(Rc<ObjectData>),
    Func(Rc<Closure>),
    Native(Rc<Native>),
    Type(Rc<TypeInfo>),
    /// A built-in method taken without calling it, e.g. `items.push`.
    Method(Rc<(Value, String)>),
    Task(Rc<RefCell<TaskState>>),
}

/// Arguments passed to a native function.
pub struct Args {
    pub pos: Vec<Value>,
    pub named: Vec<(String, Value)>,
    pub span: Span,
    pub name: String,
}

impl Args {
    /// Argument `name`, given by name or at position `i`.
    pub fn get(&self, i: usize, name: &str) -> Option<&Value> {
        self.named.iter().find(|(n, _)| n == name).map(|(_, v)| v).or_else(|| self.pos.get(i))
    }
}

impl Value {
    pub fn text(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }

    pub fn string(s: String) -> Value {
        Value::Str(Rc::from(s))
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(RefCell::new(items)))
    }

    pub fn object(fields: Fields) -> Value {
        Value::Object(Rc::new(ObjectData { fields: RefCell::new(fields), ty: None, module: None }))
    }

    pub fn native(name: &str, f: impl Fn(&mut Interpreter, &mut Args) -> Result<Value, Flow> + 'static) -> Value {
        Value::Native(Rc::new(Native { name: name.to_string(), f: Box::new(f) }))
    }

    pub fn type_name(&self) -> String {
        match self {
            Value::Nil => "nil".into(),
            Value::Bool(_) => "bool".into(),
            Value::Num(_) => "number".into(),
            Value::Str(_) => "string".into(),
            Value::List(_) => "list".into(),
            Value::Object(o) => match &o.ty {
                Some(t) => t.decl.name.text.clone(),
                None => "object".into(),
            },
            Value::Func(_) | Value::Native(_) | Value::Method(_) => "function".into(),
            Value::Type(_) => "type".into(),
            Value::Task(_) => "task".into(),
        }
    }

    /// Only `nil` and `false` are false; everything else counts as true.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    /// How `show` prints a value: text as-is, everything else like `repr`.
    pub fn display(&self) -> String {
        match self {
            Value::Str(s) => s.to_string(),
            _ => self.repr(),
        }
    }

    /// A printable form where text is quoted, as it appears inside lists and objects.
    pub fn repr(&self) -> String {
        let mut out = String::new();
        self.write_repr(&mut out, 0);
        out
    }

    fn write_repr(&self, out: &mut String, depth: usize) {
        if depth > 40 {
            out.push_str("...");
            return;
        }
        match self {
            Value::Nil => out.push_str("nil"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => out.push_str(&format_number(*n)),
            Value::Str(s) => out.push_str(&quote(s)),
            Value::List(items) => {
                out.push('[');
                for (i, item) in items.borrow().iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write_repr(out, depth + 1);
                }
                out.push(']');
            }
            Value::Object(o) => {
                if let Some(t) = &o.ty {
                    out.push_str(&t.decl.name.text);
                    out.push(' ');
                }
                if let Some(m) = &o.module {
                    out.push_str(&format!("<module {m}>"));
                    return;
                }
                let fields = o.fields.borrow();
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    if is_plain_key(k) {
                        out.push_str(k);
                    } else {
                        out.push_str(&quote(k));
                    }
                    out.push_str(": ");
                    v.write_repr(out, depth + 1);
                }
                out.push('}');
            }
            Value::Func(c) => {
                if c.decl.is_lambda {
                    out.push_str("<function>");
                } else {
                    out.push_str(&format!("<function {}>", c.decl.name.text));
                }
            }
            Value::Native(n) => out.push_str(&format!("<function {}>", n.name)),
            Value::Method(m) => out.push_str(&format!("<method {}>", m.1)),
            Value::Type(t) => out.push_str(&format!("<type {}>", t.decl.name.text)),
            Value::Task(t) => out.push_str(match &*t.borrow() {
                TaskState::Pending { .. } => "<task running>",
                TaskState::Done(_) => "<task done>",
                TaskState::Cancelled => "<task cancelled>",
            }),
        }
    }

    /// Structural equality: lists and objects are equal when their contents are.
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::List(a), Value::List(b)) => {
                if Rc::ptr_eq(a, b) {
                    return true;
                }
                let (a, b) = (a.borrow(), b.borrow());
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
            }
            (Value::Object(a), Value::Object(b)) => {
                if Rc::ptr_eq(a, b) {
                    return true;
                }
                let same_type = match (&a.ty, &b.ty) {
                    (None, None) => true,
                    (Some(x), Some(y)) => Rc::ptr_eq(x, y),
                    _ => false,
                };
                let (fa, fb) = (a.fields.borrow(), b.fields.borrow());
                same_type && fa.len() == fb.len() && fa.iter().all(|(k, v)| fb.get(k).is_some_and(|w| v.equals(w)))
            }
            (Value::Func(a), Value::Func(b)) => Rc::ptr_eq(a, b),
            (Value::Native(a), Value::Native(b)) => Rc::ptr_eq(a, b),
            (Value::Type(a), Value::Type(b)) => Rc::ptr_eq(a, b),
            (Value::Task(a), Value::Task(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// Whole numbers print without a decimal point: `3`, not `3.0`.
pub fn format_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "infinity".into() } else { "-infinity".into() }
    } else if n == n.trunc() && n.abs() < 1e16 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn is_plain_key(k: &str) -> bool {
    let mut chars = k.chars();
    matches!(chars.next(), Some(c) if is_ident_start(c)) && chars.all(|c| is_ident_start(c) || c.is_ascii_digit())
}

// ----- environments ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvKind {
    Builtins,
    Module,
    Function,
}

pub struct Slot {
    pub value: Value,
    pub constant: bool,
    pub declared: Option<TypeExpr>,
}

impl Slot {
    pub fn new(value: Value) -> Slot {
        Slot { value, constant: false, declared: None }
    }
}

/// FNV-1a: a fast hash for the short names used as variable keys.
#[derive(Default)]
pub struct FnvHasher(u64);

impl std::hash::Hasher for FnvHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
}

pub type VarMap = HashMap<String, Slot, std::hash::BuildHasherDefault<FnvHasher>>;

/// Variables of one function call (or module). Blocks such as `if` and
/// `for` don't create their own scope.
pub struct Env {
    pub vars: RefCell<VarMap>,
    pub parent: Option<Rc<Env>>,
    pub kind: EnvKind,
}

impl Env {
    pub fn new(parent: Option<Rc<Env>>, kind: EnvKind) -> Rc<Env> {
        Rc::new(Env { vars: RefCell::new(VarMap::default()), parent, kind })
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(slot) = self.vars.borrow().get(name) {
            return Some(slot.value.clone());
        }
        self.parent.as_ref()?.get(name)
    }

    pub fn define(&self, name: &str, value: Value) {
        let mut vars = self.vars.borrow_mut();
        match vars.get_mut(name) {
            Some(slot) => *slot = Slot::new(value),
            None => {
                vars.insert(name.to_string(), Slot::new(value));
            }
        }
    }

    /// Every visible name, sorted (used for "did you mean" suggestions).
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.vars.borrow().keys().cloned().collect();
        if let Some(p) = &self.parent {
            names.extend(p.names());
        }
        names.sort();
        names.dedup();
        names
    }
}
