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
    /// Marks special runtime objects, such as "response" from server.respond()
    /// or "db" for database connections.
    pub tag: Option<&'static str>,
    /// Native data attached to special objects (e.g. a database connection).
    pub payload: Option<Rc<dyn std::any::Any>>,
}

/// A user-defined function together with the environment it closes over.
pub struct Closure {
    pub decl: Rc<FuncDecl>,
    pub env: Rc<Env>,
    pub file: Rc<str>,
    /// The instance a method is bound to (available as `self`).
    pub this: Option<Value>,
    /// For a method: the type that declares it (its parent is what `super` reaches).
    pub owner: Option<Rc<TypeInfo>>,
    /// The names of the function's variable slots.
    pub layout: Names,
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
    /// `null`
    Nil,
    Bool(bool),
    /// Integer: 64-bit, overflow is an error.
    Int(i64),
    /// Decimal: 64-bit floating point.
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
        Value::Object(Rc::new(ObjectData { fields: RefCell::new(fields), ty: None, module: None, tag: None, payload: None }))
    }

    pub fn native(name: &str, f: impl Fn(&mut Interpreter, &mut Args) -> Result<Value, Flow> + 'static) -> Value {
        Value::Native(Rc::new(Native { name: name.to_string(), f: Box::new(f) }))
    }

    /// The numeric value of an Integer or Decimal.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Num(_))
    }

    pub fn type_name(&self) -> String {
        match self {
            Value::Nil => "Null".into(),
            Value::Bool(_) => "Boolean".into(),
            Value::Int(_) => "Integer".into(),
            Value::Num(_) => "Decimal".into(),
            Value::Str(_) => "String".into(),
            Value::List(_) => "Array".into(),
            Value::Object(o) => match &o.ty {
                Some(t) => t.decl.name.text.clone(),
                None => "Object".into(),
            },
            Value::Func(_) | Value::Native(_) | Value::Method(_) => "Function".into(),
            Value::Type(_) => "Type".into(),
            Value::Task(_) => "Task".into(),
        }
    }

    /// Lenient truthiness, used only for runtime options (not for language conditions,
    /// which must be real Booleans).
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

    /// A printable form where text is quoted, as it appears inside Arrays and Objects.
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
            Value::Nil => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(n) => out.push_str(&n.to_string()),
            Value::Num(n) => out.push_str(&format_decimal(*n)),
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
                if let Some(m) = &o.module {
                    out.push_str(&format!("<module {m}>"));
                    return;
                }
                if let Some(t) = &o.ty {
                    out.push_str(&t.decl.name.text);
                    out.push(' ');
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

    /// Structural equality: Arrays and Objects are equal when their contents are.
    /// Integers and Decimals compare by numeric value (`5 == 5.0`).
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (a, b) if a.is_number() && b.is_number() => a.as_f64() == b.as_f64(),
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

/// Decimals always show a fractional part (`5.0`), so they're easy to tell from Integers.
pub fn format_decimal(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "infinity".into() } else { "-infinity".into() }
    } else if n == n.trunc() && n.abs() < 1e16 {
        format!("{n:.1}")
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
    /// None until the variable is first assigned.
    pub value: Option<Value>,
    pub constant: bool,
    pub declared: Option<TypeExpr>,
}

impl Slot {
    pub fn new(value: Value) -> Slot {
        Slot { value: Some(value), constant: false, declared: None }
    }

    pub fn empty() -> Slot {
        Slot { value: None, constant: false, declared: None }
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

/// The names of an environment's slots, in slot order. All the environments
/// of one function share a single list, made when the program is resolved.
pub type Names = Rc<RefCell<Vec<Rc<str>>>>;

/// Variables of one function call (or module), in numbered slots. Blocks
/// such as `if` and `for` don't create their own scope.
pub struct Env {
    pub slots: RefCell<Vec<Slot>>,
    pub names: Names,
    pub parent: Option<Rc<Env>>,
    pub kind: EnvKind,
}

/// The environment `depth` levels up from `env`.
pub fn env_at(env: &Rc<Env>, depth: u16) -> Option<&Env> {
    let mut e: &Env = env;
    for _ in 0..depth {
        e = e.parent.as_deref()?;
    }
    Some(e)
}

impl Env {
    /// An environment whose names grow as variables are defined (modules, built-ins).
    pub fn new(parent: Option<Rc<Env>>, kind: EnvKind) -> Rc<Env> {
        Rc::new(Env { slots: RefCell::new(Vec::new()), names: Names::default(), parent, kind })
    }

    /// A function call's environment: one empty slot per variable in `names`.
    pub fn with_layout(parent: Option<Rc<Env>>, kind: EnvKind, names: &Names) -> Rc<Env> {
        let slots = (0..names.borrow().len()).map(|_| Slot::empty()).collect();
        Rc::new(Env { slots: RefCell::new(slots), names: names.clone(), parent, kind })
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.names.borrow().iter().position(|n| &**n == name)
    }

    /// Make sure there's a slot for every name (after the names list grew).
    pub fn ensure_slots(&self) {
        let n = self.names.borrow().len();
        let mut slots = self.slots.borrow_mut();
        if slots.len() < n {
            slots.resize_with(n, Slot::empty);
        }
    }

    /// The slot for `name` in this environment, adding one if needed.
    pub fn slot_index(&self, name: &str) -> usize {
        let i = match self.index_of(name) {
            Some(i) => i,
            None => {
                let mut names = self.names.borrow_mut();
                names.push(Rc::from(name));
                names.len() - 1
            }
        };
        self.ensure_slots();
        i
    }

    /// A variable of this environment (not its parents), by name.
    pub fn get_local(&self, name: &str) -> Option<Value> {
        let i = self.index_of(name)?;
        self.slots.borrow().get(i)?.value.clone()
    }

    /// A variable by name, looking outward.
    pub fn get(&self, name: &str) -> Option<Value> {
        match self.get_local(name) {
            Some(v) => Some(v),
            None => self.parent.as_ref()?.get(name),
        }
    }

    pub fn define(&self, name: &str, value: Value) {
        let i = self.slot_index(name);
        self.slots.borrow_mut()[i] = Slot::new(value);
    }

    /// Every visible variable that has a value, sorted (used for "did you mean" suggestions).
    pub fn names(&self) -> Vec<String> {
        let mut out: Vec<String> = {
            let names = self.names.borrow();
            let slots = self.slots.borrow();
            names.iter().zip(slots.iter()).filter(|(_, s)| s.value.is_some()).map(|(n, _)| n.to_string()).collect()
        };
        if let Some(p) = &self.parent {
            out.extend(p.names());
        }
        out.sort();
        out.dedup();
        out
    }
}
