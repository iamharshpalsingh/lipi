//! The tree-walking interpreter.

use crate::builtins;
use crate::methods;
use crate::task;
use crate::value::*;
use lipi_compiler::ast::*;
use lipi_compiler::checker::{self, binary_error, condition_error, module_binding_name, operand_hint, unknown_member, unknown_name, with_article};
use lipi_compiler::resolve::{self, find_project_root, Resolved};
use lipi_compiler::suggest;
use lipi_compiler::{Diagnostic, Severity, Span};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// How deep function calls may nest before we report runaway recursion.
const MAX_DEPTH: usize = 5000;

/// An error on its way up the call stack.
#[derive(Clone)]
pub struct Thrown {
    /// What `catch error` receives.
    pub value: Value,
    pub diag: Diagnostic,
    pub file: Rc<str>,
    pub trace: Vec<String>,
}

/// Non-local control flow while executing statements.
pub enum Flow {
    Return(Value),
    Break,
    Continue,
    Throw(Box<Thrown>),
    Exit(i32),
}

/// Why running a program stopped.
pub enum RunError {
    Syntax(Diagnostic, Rc<str>),
    Check(Vec<Diagnostic>, Rc<str>),
    Runtime(Box<Thrown>),
    Exit(i32),
}

/// One active function call, kept cheap: it is only turned into text when an
/// error needs a stack trace.
struct Frame {
    decl: Rc<FuncDecl>,
    file: Rc<str>,
    line: u32,
}

struct TestCase {
    name: String,
    body: Block,
    env: Rc<Env>,
    file: Rc<str>,
}

pub struct TestResult {
    pub name: String,
    /// The rendered error when the test failed.
    pub error: Option<String>,
}

pub struct Interpreter {
    pub globals: Rc<Env>,
    sources: HashMap<Rc<str>, Rc<str>>,
    modules: HashMap<PathBuf, Value>,
    loading: Vec<PathBuf>,
    /// Names exported by each module currently being loaded.
    exports: Vec<Vec<String>>,
    pub(crate) file: Rc<str>,
    /// The program's main file (used for code that runs after it, such as server handlers).
    pub(crate) main_file: Option<Rc<str>>,
    depth: usize,
    frames: Vec<Frame>,
    test_mode: bool,
    tests: Vec<TestCase>,
    pub(crate) rng: u64,
    pub project_root: PathBuf,
    pub(crate) server: crate::server::ServerState,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        let globals = Env::new(None, EnvKind::Builtins);
        builtins::install(&globals);
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Interpreter {
            globals,
            sources: HashMap::new(),
            modules: HashMap::new(),
            loading: Vec::new(),
            exports: Vec::new(),
            file: Rc::from("<main>"),
            main_file: None,
            depth: 0,
            frames: Vec::new(),
            test_mode: false,
            tests: Vec::new(),
            rng: seed | 1,
            project_root: PathBuf::from("."),
            server: Default::default(),
        }
    }

    pub fn builtin_names(&self) -> Vec<String> {
        self.globals.names()
    }

    /// Make the command-line arguments available as `process.args`.
    pub fn set_script_args(&mut self, args: &[String]) {
        if let Some(Value::Object(p)) = self.globals.get("process") {
            let list = Value::list(args.iter().map(|a| Value::text(a)).collect());
            p.fields.borrow_mut().insert("args".into(), list);
        }
    }

    // ----- errors ----------------------------------------------------------

    fn trace(&self) -> Vec<String> {
        self.frames
            .iter()
            .map(|f| {
                let name = match f.decl.name.text.as_str() {
                    "<block>" => "<block>",
                    _ if f.decl.is_lambda => "<function>",
                    n => n,
                };
                format!("{name}() called at {}:{}", f.file, f.line)
            })
            .collect()
    }

    /// A runtime error value (code LIP5000 unless the diagnostic has one).
    pub fn thrown(&self, message: impl Into<String>, span: Span, hint: Option<String>) -> Box<Thrown> {
        self.thrown_diag(Diagnostic::error(message, span).maybe_hint(hint))
    }

    fn thrown_diag(&self, diag: Diagnostic) -> Box<Thrown> {
        let diag = diag.code_or("LIP5000");
        let mut f = Fields::new();
        f.insert("message".into(), Value::text(&diag.message));
        f.insert("code".into(), Value::text(diag.code.unwrap_or("LIP5000")));
        f.insert("category".into(), Value::text(diag.category()));
        f.insert("hint".into(), diag.hint.as_deref().map(Value::text).unwrap_or(Value::Nil));
        f.insert("line".into(), diag.span.map(|s| Value::Int(s.line as i64)).unwrap_or(Value::Nil));
        f.insert("file".into(), Value::text(&self.file));
        Box::new(Thrown { value: Value::object(f), diag, file: self.file.clone(), trace: self.trace() })
    }

    /// A general runtime error at `span` in the current file.
    pub fn error(&self, message: impl Into<String>, span: Span, hint: Option<String>) -> Flow {
        Flow::Throw(self.thrown(message, span, hint))
    }

    /// A runtime error with a specific code.
    pub fn err(&self, code: &'static str, message: impl Into<String>, span: Span, hint: Option<String>) -> Flow {
        self.throw(Diagnostic::error(message, span).with_code(code).maybe_hint(hint))
    }

    fn throw(&self, diag: Diagnostic) -> Flow {
        Flow::Throw(self.thrown_diag(diag))
    }

    pub fn render(&self, err: &RunError, color: bool) -> String {
        match err {
            RunError::Syntax(d, f) => self.render_diag(d, f, color),
            RunError::Check(ds, f) => ds.iter().map(|d| self.render_diag(d, f, color)).collect::<Vec<_>>().join("\n"),
            RunError::Runtime(t) => {
                let mut s = self.render_diag(&t.diag, &t.file, color);
                if !t.trace.is_empty() {
                    s.push('\n');
                }
                for frame in t.trace.iter().rev().take(8) {
                    s.push_str(&format!("  in {frame}\n"));
                }
                if t.trace.len() > 8 {
                    s.push_str(&format!("  ... and {} more calls\n", t.trace.len() - 8));
                }
                s
            }
            RunError::Exit(_) => String::new(),
        }
    }

    fn render_diag(&self, d: &Diagnostic, file: &Rc<str>, color: bool) -> String {
        let src = self.sources.get(file).cloned().unwrap_or_else(|| Rc::from(""));
        d.render(&src, file, color)
    }

    // ----- entry points ----------------------------------------------------

    fn compile(&mut self, source: &str, file: &Rc<str>, run_checker: bool) -> Result<Program, RunError> {
        self.sources.insert(file.clone(), Rc::from(source));
        let program = lipi_compiler::parse_source(source).map_err(|d| RunError::Syntax(d, file.clone()))?;
        if run_checker {
            let names = self.builtin_names();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let errors: Vec<Diagnostic> = checker::check(&program, &refs).into_iter().filter(|d| d.severity == Severity::Error).collect();
            if !errors.is_empty() {
                return Err(RunError::Check(errors, file.clone()));
            }
        }
        Ok(program)
    }

    fn read_source(path: &Path, file: &Rc<str>) -> Result<String, RunError> {
        std::fs::read_to_string(path).map_err(|e| {
            let hint = if e.kind() == std::io::ErrorKind::NotFound { "Check the file name and the folder you're in.".to_string() } else { e.to_string() };
            RunError::Syntax(
                Diagnostic { severity: Severity::Error, code: Some("LIP3001"), message: format!("couldn't read {file}"), span: None, hint: Some(hint) },
                file.clone(),
            )
        })
    }

    /// Parse, check and run a file as the main program.
    pub fn run_file(&mut self, path: &Path) -> Result<Rc<Env>, RunError> {
        let file: Rc<str> = Rc::from(path.to_string_lossy().as_ref());
        let source = Self::read_source(path, &file)?;
        self.project_root = find_project_root(path);
        self.main_file = Some(file.clone());
        if let Ok(canon) = path.canonicalize() {
            self.loading.push(canon);
        }
        let result = self.run_source(&source, file);
        self.loading.clear();
        result.map(|(env, _)| env)
    }

    /// Parse and check a file without running it.
    pub fn check_file(&mut self, path: &Path) -> Result<(), RunError> {
        let file: Rc<str> = Rc::from(path.to_string_lossy().as_ref());
        let source = Self::read_source(path, &file)?;
        self.compile(&source, &file, true).map(|_| ())
    }

    /// Run a module's code. Returns its variables and the names it exported.
    fn run_source(&mut self, source: &str, file: Rc<str>) -> Result<(Rc<Env>, Vec<String>), RunError> {
        let program = self.compile(source, &file, true)?;
        let env = Env::new(Some(self.globals.clone()), EnvKind::Module);
        let prev = std::mem::replace(&mut self.file, file);
        self.exports.push(Vec::new());
        self.hoist(&program.body, &env);
        let result = self.exec_block(&program.body, &env);
        let exported = self.exports.pop().unwrap_or_default();
        self.file = prev;
        match result {
            Ok(()) | Err(Flow::Return(_)) | Err(Flow::Break) | Err(Flow::Continue) => Ok((env, exported)),
            Err(Flow::Throw(t)) => Err(RunError::Runtime(t)),
            Err(Flow::Exit(code)) => Err(RunError::Exit(code)),
        }
    }

    /// A fresh top-level environment for the interactive prompt.
    pub fn repl_env(&self) -> Rc<Env> {
        Env::new(Some(self.globals.clone()), EnvKind::Module)
    }

    /// Run a snippet typed at the prompt. Returns the value of a trailing expression.
    pub fn eval_repl(&mut self, source: &str, env: &Rc<Env>) -> Result<Value, RunError> {
        let file: Rc<str> = Rc::from("<repl>");
        let program = self.compile(source, &file, false)?;
        let prev = std::mem::replace(&mut self.file, file);
        self.hoist(&program.body, env);
        let mut last = Value::Nil;
        let mut result = Ok(());
        for stmt in &program.body {
            last = Value::Nil;
            let r = match &stmt.kind {
                StmtKind::Expr(e) => self.eval(e, env).map(|v| last = v),
                _ => self.exec(stmt, env),
            };
            if let Err(f) = r {
                result = Err(f);
                break;
            }
        }
        self.file = prev;
        match result {
            Ok(()) | Err(Flow::Return(_)) | Err(Flow::Break) | Err(Flow::Continue) => Ok(last),
            Err(Flow::Throw(t)) => Err(RunError::Runtime(t)),
            Err(Flow::Exit(code)) => Err(RunError::Exit(code)),
        }
    }

    /// Run a file and then each of its `test "..."` blocks.
    pub fn run_tests(&mut self, path: &Path, color: bool) -> Result<Vec<TestResult>, RunError> {
        self.test_mode = true;
        self.run_file(path)?;
        let cases = std::mem::take(&mut self.tests);
        let mut results = Vec::new();
        for case in cases {
            let env = Env::new(Some(case.env.clone()), EnvKind::Function);
            let prev = std::mem::replace(&mut self.file, case.file.clone());
            self.hoist(&case.body, &env);
            let r = self.exec_block(&case.body, &env);
            self.file = prev;
            let error = match r {
                Err(Flow::Throw(t)) => Some(self.render(&RunError::Runtime(t), color)),
                Err(Flow::Exit(code)) => Some(format!("the test called process.exit({code})\n")),
                _ => None,
            };
            results.push(TestResult { name: case.name, error });
        }
        Ok(results)
    }

    // ----- statements ------------------------------------------------------

    /// Define functions and types before the code around them runs, so they
    /// can be called from anywhere in the file.
    fn hoist(&mut self, body: &Block, env: &Rc<Env>) {
        for stmt in body {
            let stmt = match &stmt.kind {
                StmtKind::Export { inner: Some(inner), .. } => inner,
                _ => stmt,
            };
            match &stmt.kind {
                StmtKind::Func(f) | StmtKind::Component(f) => env.define(&f.name.text, self.closure(f, env)),
                StmtKind::TypeDef(t) => env.define(&t.name.text, Value::Type(Rc::new(TypeInfo { decl: t.clone(), env: env.clone(), file: self.file.clone() }))),
                _ => {}
            }
        }
    }

    fn closure(&self, f: &Rc<FuncDecl>, env: &Rc<Env>) -> Value {
        Value::Func(Rc::new(Closure { decl: f.clone(), env: env.clone(), file: self.file.clone(), this: None }))
    }

    fn exec_block(&mut self, body: &[Stmt], env: &Rc<Env>) -> Result<(), Flow> {
        for stmt in body {
            self.exec(stmt, env)?;
        }
        Ok(())
    }

    /// Run a loop body. Returns Ok(true) to keep looping, Ok(false) on `break`.
    fn loop_body(&mut self, body: &[Stmt], env: &Rc<Env>) -> Result<bool, Flow> {
        match self.exec_block(body, env) {
            Ok(()) | Err(Flow::Continue) => Ok(true),
            Err(Flow::Break) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// A condition must be a real Boolean.
    fn truth(&self, v: &Value, e: &Expr) -> Result<bool, Flow> {
        match v {
            Value::Bool(b) => Ok(*b),
            other => Err(self.throw(condition_error(e, &other.type_name()))),
        }
    }

    /// A callback used as a test (filter, find, ...) must return a Boolean.
    pub fn expect_bool(&self, v: &Value, span: Span, what: &str) -> Result<bool, Flow> {
        match v {
            Value::Bool(b) => Ok(*b),
            other => Err(self.err(
                "LIP2005",
                format!("{what} must return true or false, but it returned {}", with_article(&other.type_name())),
                span,
                Some("Return a comparison, for example: items.filter(x => x > 3)".into()),
            )),
        }
    }

    fn eval_cond(&mut self, e: &Expr, env: &Rc<Env>) -> Result<bool, Flow> {
        let v = self.eval(e, env)?;
        self.truth(&v, e)
    }

    fn exec(&mut self, stmt: &Stmt, env: &Rc<Env>) -> Result<(), Flow> {
        match &stmt.kind {
            StmtKind::Expr(e) => {
                self.eval(e, env)?;
            }
            StmtKind::Show(values) => {
                let mut parts = Vec::with_capacity(values.len());
                for v in values {
                    parts.push(self.eval(v, env)?.display());
                }
                use std::io::Write;
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{}", parts.join(" "));
            }
            StmtKind::Assign { target, op, ty, value, constant } => self.assign(target, *op, ty.as_ref(), value, *constant, env)?,
            StmtKind::If { branches, otherwise } => {
                for (cond, body) in branches {
                    if self.eval_cond(cond, env)? {
                        return self.exec_block(body, env);
                    }
                }
                if let Some(body) = otherwise {
                    self.exec_block(body, env)?;
                }
            }
            StmtKind::While { cond, body } => {
                while self.eval_cond(cond, env)? {
                    if !self.loop_body(body, env)? {
                        break;
                    }
                }
            }
            StmtKind::Repeat { count, body } => {
                let n = match self.eval(count, env)? {
                    Value::Int(n) if n >= 0 => n,
                    Value::Int(_) => return Err(self.err("LIP5008", "`repeat` needs an Integer that isn't negative", count.span, None)),
                    other => return Err(self.err("LIP2001", "`repeat` needs an Integer", count.span, Some(operand_hint(count, &other.type_name(), None)))),
                };
                for _ in 0..n {
                    if !self.loop_body(body, env)? {
                        break;
                    }
                }
            }
            StmtKind::For { first, second, iter, body } => self.exec_for(first, second.as_ref(), iter, body, env)?,
            StmtKind::Func(f) | StmtKind::Component(f) => env.define(&f.name.text, self.closure(f, env)),
            StmtKind::State { name, ty, value } => {
                let v = self.eval(value, env)?;
                if let Some(t) = ty {
                    self.check_declared(&v, t, &name.text, value.span)?;
                }
                env.vars.borrow_mut().insert(name.text.clone(), Slot { value: v, constant: false, declared: ty.clone() });
            }
            StmtKind::Return(value) => {
                let v = match value {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Nil,
                };
                return Err(Flow::Return(v));
            }
            StmtKind::Break => return Err(Flow::Break),
            StmtKind::Continue => return Err(Flow::Continue),
            StmtKind::Throw(e) => {
                let v = self.eval(e, env)?;
                let message = match &v {
                    Value::Str(s) => return Err(self.err("LIP5006", s.to_string(), stmt.span, None)),
                    Value::Object(o) => o.fields.borrow().get("message").map(|m| m.display()).unwrap_or_else(|| v.repr()),
                    other => other.repr(),
                };
                let diag = Diagnostic::error(message, stmt.span).with_code("LIP5006");
                return Err(Flow::Throw(Box::new(Thrown { value: v, diag, file: self.file.clone(), trace: self.trace() })));
            }
            StmtKind::Try { body, catch, finally } => {
                let mut result = self.exec_block(body, env);
                if let Some((name, handler)) = catch {
                    if let Err(Flow::Throw(t)) = result {
                        if let Some(n) = name {
                            env.define(&n.text, t.value.clone());
                        }
                        result = self.exec_block(handler, env);
                    }
                }
                if let Some(f) = finally {
                    self.exec_block(f, env)?;
                }
                return result;
            }
            StmtKind::Match { subject, arms, otherwise } => {
                let v = self.eval(subject, env)?;
                for arm in arms {
                    let mut matched = false;
                    for p in &arm.patterns {
                        matched = match &p.kind {
                            ExprKind::Ident(n) if n == "_" => true,
                            ExprKind::Range { start, end, step: None } => {
                                let lo = self.eval(start, env)?;
                                let hi = self.eval(end, env)?;
                                match (v.as_f64(), lo.as_f64(), hi.as_f64()) {
                                    (Some(x), Some(a), Some(b)) => x >= a.min(b) && x <= a.max(b),
                                    _ => false,
                                }
                            }
                            _ => v.equals(&self.eval(p, env)?),
                        };
                        if matched {
                            break;
                        }
                    }
                    if matched {
                        if let Some(g) = &arm.guard {
                            if !self.eval_cond(g, env)? {
                                continue;
                            }
                        }
                        return self.exec_block(&arm.body, env);
                    }
                }
                if let Some(body) = otherwise {
                    self.exec_block(body, env)?;
                }
            }
            StmtKind::Use { source, alias, names } => {
                let module = self.use_module(source, stmt.span)?;
                match names {
                    Some(names) => {
                        for n in names {
                            let value = match &module {
                                Value::Object(o) => o.fields.borrow().get(&n.text).cloned(),
                                _ => None,
                            };
                            match value {
                                Some(v) => env.define(&n.text, v),
                                None => {
                                    let available: Vec<String> = match &module {
                                        Value::Object(o) => o.fields.borrow().keys().cloned().collect(),
                                        _ => Vec::new(),
                                    };
                                    let hint = suggest::did_you_mean(&n.text, available.iter().map(String::as_str))
                                        .unwrap_or_else(|| format!("If \"{}\" is defined there, add `export {}` to that file.", n.text, n.text));
                                    return Err(self.err("LIP3003", format!("\"{source}\" doesn't export \"{}\"", n.text), n.span, Some(hint)));
                                }
                            }
                        }
                    }
                    None => {
                        let bound = alias.as_ref().map(|a| a.text.clone()).unwrap_or_else(|| module_binding_name(source));
                        env.define(&bound, module);
                    }
                }
            }
            StmtKind::Export { names, inner } => {
                if let Some(s) = inner {
                    self.exec(s, env)?;
                }
                if let Some(list) = self.exports.last_mut() {
                    for n in names {
                        if !list.contains(&n.text) {
                            list.push(n.text.clone());
                        }
                    }
                }
            }
            StmtKind::TypeDef(t) => env.define(&t.name.text, Value::Type(Rc::new(TypeInfo { decl: t.clone(), env: env.clone(), file: self.file.clone() }))),
            StmtKind::Test { name, body } => {
                if self.test_mode {
                    self.tests.push(TestCase { name: name.clone(), body: body.clone(), env: env.clone(), file: self.file.clone() });
                }
            }
        }
        Ok(())
    }

    fn exec_for(&mut self, first: &Name, second: Option<&Name>, iter: &Expr, body: &[Stmt], env: &Rc<Env>) -> Result<(), Flow> {
        if let ExprKind::Range { start, end, step } = &iter.kind {
            let (from, to, step) = self.range_parts(start, end, step.as_deref(), env)?;
            let mut i = from;
            let mut index = 0i64;
            while (step > 0 && i <= to) || (step < 0 && i >= to) {
                match second {
                    Some(s) => {
                        env.define(&first.text, Value::Int(index));
                        env.define(&s.text, Value::Int(i));
                    }
                    None => env.define(&first.text, Value::Int(i)),
                }
                if !self.loop_body(body, env)? {
                    break;
                }
                let Some(next) = i.checked_add(step) else { break };
                i = next;
                index += 1;
            }
            return Ok(());
        }
        let collection = self.eval(iter, env)?;
        let (pairs, keyed): (Vec<(Value, Value)>, bool) = match &collection {
            Value::List(items) => (items.borrow().iter().enumerate().map(|(i, v)| (Value::Int(i as i64), v.clone())).collect(), false),
            Value::Str(s) => (s.chars().enumerate().map(|(i, c)| (Value::Int(i as i64), Value::string(c.to_string()))).collect(), false),
            Value::Object(o) => (o.fields.borrow().iter().map(|(k, v)| (Value::text(k), v.clone())).collect(), true),
            other => {
                return Err(self.err(
                    "LIP2001",
                    format!("cannot loop over {}", with_article(&other.type_name())),
                    iter.span,
                    Some("Loop over an Array, a String, an Object or a range like 1 to 10.".into()),
                ))
            }
        };
        for (key, value) in pairs {
            match second {
                Some(s) => {
                    env.define(&first.text, key);
                    env.define(&s.text, value);
                }
                None => env.define(&first.text, if keyed { key } else { value }),
            }
            if !self.loop_body(body, env)? {
                break;
            }
        }
        Ok(())
    }

    fn range_parts(&mut self, start: &Expr, end: &Expr, step: Option<&Expr>, env: &Rc<Env>) -> Result<(i64, i64, i64), Flow> {
        let int = |this: &mut Self, e: &Expr| -> Result<i64, Flow> {
            match this.eval(e, env)? {
                Value::Int(n) => Ok(n),
                other => Err(this.err("LIP2001", "ranges need Integers", e.span, Some(operand_hint(e, &other.type_name(), None)))),
            }
        };
        let from = int(self, start)?;
        let to = int(self, end)?;
        let step = match step {
            Some(s) => {
                let n = int(self, s)?;
                if n == 0 {
                    return Err(self.err("LIP5008", "a range's step can't be 0", s.span, None));
                }
                n
            }
            None if to >= from => 1,
            None => -1,
        };
        Ok((from, to, step))
    }

    fn assign(&mut self, target: &Target, op: Option<BinOp>, ty: Option<&TypeExpr>, value: &Expr, constant: bool, env: &Rc<Env>) -> Result<(), Flow> {
        match target {
            Target::Name(n) => {
                let mut v = self.eval(value, env)?;
                if let Some(op) = op {
                    let current = self.lookup(&n.text, n.span, env)?;
                    let target_expr = Expr { kind: ExprKind::Ident(n.text.clone()), span: n.span };
                    v = self.binary(op, current, v, &target_expr, value)?;
                }
                if constant || ty.is_some() {
                    if let Some(t) = ty {
                        self.check_declared(&v, t, &n.text, value.span)?;
                    }
                    env.vars.borrow_mut().insert(n.text.clone(), Slot { value: v, constant, declared: ty.cloned() });
                    Ok(())
                } else {
                    self.assign_name(&n.text, v, env, value.span)
                }
            }
            Target::Field(obj_expr, name) => {
                let obj = self.eval(obj_expr, env)?;
                let v = match op {
                    Some(op) => {
                        let current = self.get_field(&obj, name, false, obj_expr)?;
                        let rhs = self.eval(value, env)?;
                        let target_expr = Expr { kind: ExprKind::Ident(name.text.clone()), span: name.span };
                        self.binary(op, current, rhs, &target_expr, value)?
                    }
                    None => self.eval(value, env)?,
                };
                self.set_field(&obj, name, v, obj_expr, value.span)
            }
            Target::Index(obj_expr, index_expr) => {
                let obj = self.eval(obj_expr, env)?;
                let index = self.eval(index_expr, env)?;
                let v = match op {
                    Some(op) => {
                        let current = self.get_index(&obj, &index, obj_expr, index_expr)?;
                        let rhs = self.eval(value, env)?;
                        self.binary(op, current, rhs, index_expr, value)?
                    }
                    None => self.eval(value, env)?,
                };
                self.set_index(&obj, &index, v, obj_expr, index_expr)
            }
        }
    }

    /// Assignment updates the nearest existing variable (up to module level),
    /// otherwise it creates a new one in the current function.
    fn assign_name(&self, name: &str, v: Value, env: &Rc<Env>, value_span: Span) -> Result<(), Flow> {
        let mut scope = Some(env.clone());
        while let Some(e) = scope {
            if e.kind == EnvKind::Builtins {
                break;
            }
            {
                let mut vars = e.vars.borrow_mut();
                if let Some(slot) = vars.get_mut(name) {
                    if slot.constant {
                        return Err(self.err(
                            "LIP1003",
                            format!("\"{name}\" is a constant and can't be changed"),
                            value_span,
                            Some("Remove `const` where it's defined if it needs to change.".into()),
                        ));
                    }
                    if let Some(t) = &slot.declared {
                        if !self.matches_type(&v, t) {
                            return Err(self.err(
                                "LIP2002",
                                format!("\"{name}\" should be {}, but this is {}", with_article(&t.to_string()), with_article(&v.type_name())),
                                value_span,
                                Some(format!("\"{name}\" was declared as {t}.")),
                            ));
                        }
                    }
                    slot.value = v;
                    return Ok(());
                }
            }
            scope = e.parent.clone();
        }
        env.vars.borrow_mut().insert(name.to_string(), Slot::new(v));
        Ok(())
    }

    fn check_declared(&self, v: &Value, t: &TypeExpr, name: &str, span: Span) -> Result<(), Flow> {
        if self.matches_type(v, t) {
            return Ok(());
        }
        Err(self.err(
            "LIP2002",
            format!("\"{name}\" should be {}, but this is {}", with_article(&t.to_string()), with_article(&v.type_name())),
            span,
            Some(format!("\"{name}\" was declared as {t}.")),
        ))
    }

    pub fn matches_type(&self, v: &Value, t: &TypeExpr) -> bool {
        match &t.kind {
            TypeKind::Optional(inner) => matches!(v, Value::Nil) || self.matches_type(v, inner),
            TypeKind::List(inner) => match v {
                Value::List(items) => items.borrow().iter().all(|x| self.matches_type(x, inner)),
                _ => false,
            },
            TypeKind::Named(n) => match n.as_str() {
                "Any" => true,
                "Integer" => matches!(v, Value::Int(_)),
                "Decimal" | "Number" => v.is_number(),
                "String" => matches!(v, Value::Str(_)),
                "Boolean" => matches!(v, Value::Bool(_)),
                "Null" => matches!(v, Value::Nil),
                "Array" => matches!(v, Value::List(_)),
                "Object" => matches!(v, Value::Object(_)),
                "Function" => matches!(v, Value::Func(_) | Value::Native(_) | Value::Type(_) | Value::Method(_)),
                "Task" => matches!(v, Value::Task(_)),
                other => matches!(v, Value::Object(o) if o.ty.as_ref().is_some_and(|ty| ty.decl.name.text == other)),
            },
        }
    }

    fn lookup(&self, name: &str, span: Span, env: &Rc<Env>) -> Result<Value, Flow> {
        match env.get(name) {
            Some(v) => Ok(v),
            None => {
                let names = env.names();
                Err(self.throw(unknown_name(name, span, names.iter().map(String::as_str))))
            }
        }
    }

    // ----- expressions -----------------------------------------------------

    pub fn eval(&mut self, e: &Expr, env: &Rc<Env>) -> Result<Value, Flow> {
        Ok(match &e.kind {
            ExprKind::Int(n) => Value::Int(*n),
            ExprKind::Decimal(n) => Value::Num(*n),
            ExprKind::Str(s) => Value::text(s),
            ExprKind::Template(parts) => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        TemplatePart::Lit(s) => out.push_str(s),
                        TemplatePart::Expr(x) => out.push_str(&self.eval(x, env)?.display()),
                    }
                }
                Value::string(out)
            }
            ExprKind::Bool(b) => Value::Bool(*b),
            ExprKind::Null => Value::Nil,
            ExprKind::Ident(name) => self.lookup(name, e.span, env)?,
            ExprKind::List(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.eval(item, env)?);
                }
                Value::list(out)
            }
            ExprKind::Object(fields) => {
                let mut map = Fields::with_capacity(fields.len());
                for (k, v) in fields {
                    map.insert(k.text.clone(), self.eval(v, env)?);
                }
                Value::object(map)
            }
            ExprKind::Unary(UnaryOp::Neg, inner) => match self.eval(inner, env)? {
                Value::Int(n) => Value::Int(n.checked_neg().ok_or_else(|| self.overflow(inner.span))?),
                Value::Num(n) => Value::Num(-n),
                other => {
                    return Err(self.err("LIP2001", format!("cannot negate {}", with_article(&other.type_name())), inner.span, Some(operand_hint(inner, &other.type_name(), None))))
                }
            },
            ExprKind::Unary(UnaryOp::Not, inner) => Value::Bool(!self.eval_cond(inner, env)?),
            ExprKind::Binary(op, l, r) => {
                let lv = self.eval(l, env)?;
                let rv = self.eval(r, env)?;
                self.binary(*op, lv, rv, l, r)?
            }
            ExprKind::And(l, r) => Value::Bool(self.eval_cond(l, env)? && self.eval_cond(r, env)?),
            ExprKind::Or(l, r) => Value::Bool(self.eval_cond(l, env)? || self.eval_cond(r, env)?),
            ExprKind::IfElse { cond, then, otherwise } => {
                if self.eval_cond(cond, env)? {
                    self.eval(then, env)?
                } else {
                    self.eval(otherwise, env)?
                }
            }
            ExprKind::Coalesce(l, r) => match self.eval(l, env)? {
                Value::Nil => self.eval(r, env)?,
                v => v,
            },
            ExprKind::Range { start, end, step } => {
                let (from, to, step) = self.range_parts(start, end, step.as_deref(), env)?;
                let count = (to as i128 - from as i128) / step as i128;
                if count > 10_000_000 {
                    return Err(self.err("LIP5008", "this range is too big to turn into an Array", e.span, Some("Loop over it directly with `for i in a to b` instead.".into())));
                }
                let mut items = Vec::new();
                let mut i = from;
                while (step > 0 && i <= to) || (step < 0 && i >= to) {
                    items.push(Value::Int(i));
                    match i.checked_add(step) {
                        Some(n) => i = n,
                        None => break,
                    }
                }
                Value::list(items)
            }
            ExprKind::Call { callee, args } => {
                if let ExprKind::Field { object, name, optional } = &callee.kind {
                    let obj = if *optional { self.eval_lenient(object, env)? } else { self.eval(object, env)? };
                    if *optional && matches!(obj, Value::Nil) {
                        return Ok(Value::Nil);
                    }
                    let (pos, named) = self.eval_args(args, env)?;
                    return self.call_method(obj, name, pos, named, e.span, object);
                }
                let f = self.eval(callee, env)?;
                let (pos, named) = self.eval_args(args, env)?;
                self.call_value(f, pos, named, e.span, Some(callee))?
            }
            ExprKind::Field { object, name, optional } => {
                let obj = if *optional { self.eval_lenient(object, env)? } else { self.eval(object, env)? };
                self.get_field(&obj, name, *optional, object)?
            }
            ExprKind::Index { object, index } => {
                let obj = self.eval(object, env)?;
                let idx = self.eval(index, env)?;
                self.get_index(&obj, &idx, object, index)?
            }
            ExprKind::Lambda(f) => self.closure(f, env),
            ExprKind::Await(inner) => match self.eval(inner, env)? {
                Value::Task(t) => task::await_task(self, &t, e.span, None)?,
                v => v,
            },
        })
    }

    /// The value before `?.`: like `eval`, except that a field missing from a
    /// plain Object gives null, so `user.address?.city` is null when there's no address.
    fn eval_lenient(&mut self, e: &Expr, env: &Rc<Env>) -> Result<Value, Flow> {
        if let ExprKind::Field { object, name, optional } = &e.kind {
            let obj = if *optional { self.eval_lenient(object, env)? } else { self.eval(object, env)? };
            if let Value::Object(o) = &obj {
                if o.ty.is_none() && o.module.is_none() && o.tag.is_none() && !o.fields.borrow().contains_key(&name.text) {
                    return Ok(Value::Nil);
                }
            }
            return self.get_field(&obj, name, *optional, object);
        }
        self.eval(e, env)
    }

    fn eval_args(&mut self, args: &[Arg], env: &Rc<Env>) -> Result<(Vec<Value>, Vec<(String, Value)>), Flow> {
        let mut pos = Vec::with_capacity(args.len());
        let mut named = Vec::new();
        for a in args {
            let v = self.eval(&a.value, env)?;
            match &a.name {
                Some(n) => named.push((n.text.clone(), v)),
                None => pos.push(v),
            }
        }
        Ok((pos, named))
    }

    fn overflow(&self, span: Span) -> Flow {
        self.err(
            "LIP5009",
            "this Integer calculation overflowed",
            span,
            Some("Integers go up to 9223372036854775807. For bigger values, use Decimals (for example 1.0 * x).".into()),
        )
    }

    pub fn binary(&self, op: BinOp, l: Value, r: Value, le: &Expr, re: &Expr) -> Result<Value, Flow> {
        use Value::*;
        let both = le.span.to(re.span);
        let div_zero = || self.err("LIP5002", "cannot divide by zero", re.span, Some("Check that the number you divide by isn't 0 first.".into()));
        Ok(match (op, &l, &r) {
            (BinOp::Eq, _, _) => Bool(l.equals(&r)),
            (BinOp::NotEq, _, _) => Bool(!l.equals(&r)),
            (BinOp::Add, Int(a), Int(b)) => Int(a.checked_add(*b).ok_or_else(|| self.overflow(both))?),
            (BinOp::Sub, Int(a), Int(b)) => Int(a.checked_sub(*b).ok_or_else(|| self.overflow(both))?),
            (BinOp::Mul, Int(a), Int(b)) => Int(a.checked_mul(*b).ok_or_else(|| self.overflow(both))?),
            (BinOp::Mod, Int(_), Int(0)) => return Err(div_zero()),
            (BinOp::Mod, Int(a), Int(b)) => {
                let m = a.wrapping_rem(*b);
                Int(if m != 0 && ((m < 0) != (*b < 0)) { m + b } else { m })
            }
            (BinOp::Pow, Int(a), Int(b)) if *b >= 0 => {
                Int(u32::try_from(*b).ok().and_then(|e| a.checked_pow(e)).ok_or_else(|| self.overflow(both))?)
            }
            (op, a, b) if op.is_arithmetic() && a.is_number() && b.is_number() => {
                let (x, y) = (a.as_f64().unwrap_or(0.0), b.as_f64().unwrap_or(0.0));
                match op {
                    BinOp::Add => Num(x + y),
                    BinOp::Sub => Num(x - y),
                    BinOp::Mul => Num(x * y),
                    BinOp::Div if y == 0.0 => return Err(div_zero()),
                    BinOp::Div => Num(x / y),
                    BinOp::Mod if y == 0.0 => return Err(div_zero()),
                    BinOp::Mod => Num(x - y * (x / y).floor()),
                    _ => Num(x.powf(y)),
                }
            }
            (BinOp::Add, Str(a), Str(b)) => {
                let mut s = String::with_capacity(a.len() + b.len());
                s.push_str(a);
                s.push_str(b);
                Value::string(s)
            }
            (BinOp::Add, List(a), List(b)) => {
                let mut items = a.borrow().clone();
                items.extend(b.borrow().iter().cloned());
                Value::list(items)
            }
            (op, a, b) if op.is_ordering() && a.is_number() && b.is_number() => Bool(compare(op, a.as_f64().partial_cmp(&b.as_f64()))),
            (op, Str(a), Str(b)) if op.is_ordering() => Bool(compare(op, a.partial_cmp(b))),
            (BinOp::In | BinOp::NotIn, _, List(items)) => {
                let found = items.borrow().iter().any(|x| x.equals(&l));
                Bool(found == (op == BinOp::In))
            }
            (BinOp::In | BinOp::NotIn, Str(needle), Str(hay)) => Bool(hay.contains(&**needle) == (op == BinOp::In)),
            (BinOp::In | BinOp::NotIn, Str(key), Object(o)) => Bool(o.fields.borrow().contains_key(&**key) == (op == BinOp::In)),
            _ => return Err(self.throw(binary_error(op, &l.type_name(), &r.type_name(), le, re))),
        })
    }

    // ----- fields and indexes ----------------------------------------------

    fn null_hint(obj_expr: &Expr, what: &str) -> Option<String> {
        match &obj_expr.kind {
            ExprKind::Ident(n) => Some(format!("\"{n}\" is null. Use {n}?{what} to get null instead of an error.")),
            _ => Some(format!("Use ?{what} to get null instead of an error when the value is null.")),
        }
    }

    fn get_field(&mut self, obj: &Value, name: &Name, optional: bool, obj_expr: &Expr) -> Result<Value, Flow> {
        let key = name.text.as_str();
        match obj {
            Value::Nil if optional => Ok(Value::Nil),
            Value::Nil => Err(self.err("LIP5003", format!("cannot read \".{key}\" of null"), name.span, Self::null_hint(obj_expr, &format!(".{key}")))),
            Value::Object(o) => {
                if let Some(v) = o.fields.borrow().get(key) {
                    return Ok(v.clone());
                }
                if o.tag == Some("db") {
                    // `db.users` is the users table
                    if let Some(table) = crate::db::table(o, key) {
                        return Ok(table);
                    }
                }
                if let Some(ty) = &o.ty {
                    if let Some(m) = ty.decl.methods.iter().find(|m| m.name.text == key) {
                        return Ok(Value::Func(Rc::new(Closure { decl: m.clone(), env: ty.env.clone(), file: ty.file.clone(), this: Some(obj.clone()) })));
                    }
                }
                if o.module.is_none() {
                    if key == "length" {
                        return Ok(Value::Int(o.fields.borrow().len() as i64));
                    }
                    if checker::OBJECT_MEMBERS.contains(&key) {
                        return Ok(Value::Method(Rc::new((obj.clone(), key.to_string()))));
                    }
                }
                Err(self.missing_field(o, name))
            }
            Value::Type(t) => Err(self.err(
                "LIP5004",
                format!("\"{}\" is a type; create one first to use \".{key}\"", t.decl.name.text),
                name.span,
                Some(format!("For example: item = {}(...) and then item.{key}", t.decl.name.text)),
            )),
            _ => {
                if let Some(v) = methods::property(obj, key) {
                    return Ok(v);
                }
                match methods::members_for(obj) {
                    Some((_, members)) if members.contains(&key) => Ok(Value::Method(Rc::new((obj.clone(), key.to_string())))),
                    Some((tyname, members)) => Err(self.throw(unknown_member(tyname, key, name.span, members))),
                    None => Err(self.err("LIP1004", format!("{} has no \".{key}\"", with_article(&obj.type_name())), name.span, None)),
                }
            }
        }
    }

    fn missing_field(&self, o: &ObjectData, name: &Name) -> Flow {
        let key = name.text.as_str();
        let mut available: Vec<String> = o.fields.borrow().keys().cloned().collect();
        if let Some(ty) = &o.ty {
            available.extend(ty.decl.methods.iter().map(|m| m.name.text.clone()));
        }
        let suggestion = suggest::did_you_mean(key, available.iter().map(String::as_str));
        let (code, message, fallback) = match (&o.module, &o.ty) {
            (Some(m), _) if builtins::MODULES.contains(&m.as_str()) => {
                ("LIP1004", format!("the {m} module has no \"{key}\""), Some(format!("Available: {}", available.join(", "))))
            }
            (Some(m), _) => (
                "LIP3003",
                format!("module \"{m}\" doesn't export \"{key}\""),
                Some(format!("If \"{key}\" is defined in {m}.lipi, add: export {key}")),
            ),
            (_, Some(t)) => ("LIP5004", format!("\"{}\" has no field or method \"{key}\"", t.decl.name.text), Some(format!("Available: {}", available.join(", ")))),
            _ => (
                "LIP5004",
                format!("this Object has no field \"{key}\""),
                Some(format!("If the field may be missing, use obj.get(\"{key}\") or obj[\"{key}\"], which give null instead of an error.")),
            ),
        };
        self.err(code, message, name.span, suggestion.or(fallback))
    }

    fn set_field(&mut self, obj: &Value, name: &Name, v: Value, obj_expr: &Expr, value_span: Span) -> Result<(), Flow> {
        match obj {
            Value::Object(o) => {
                if o.module.is_some() {
                    return Err(self.err("LIP5008", "modules can't be changed from outside", name.span, None));
                }
                if let Some(ty) = &o.ty {
                    match ty.decl.fields.iter().find(|f| f.name.text == name.text) {
                        Some(field) => {
                            if let Some(t) = &field.ty {
                                self.check_declared(&v, t, &name.text, value_span)?;
                            }
                        }
                        None => {
                            let names: Vec<&str> = ty.decl.fields.iter().map(|f| f.name.text.as_str()).collect();
                            let hint = suggest::did_you_mean(&name.text, names.iter().copied()).unwrap_or_else(|| format!("Its fields are: {}", names.join(", ")));
                            return Err(self.err("LIP5004", format!("\"{}\" has no field \"{}\"", ty.decl.name.text, name.text), name.span, Some(hint)));
                        }
                    }
                }
                o.fields.borrow_mut().insert(name.text.clone(), v);
                Ok(())
            }
            Value::Nil => Err(self.err("LIP5003", format!("cannot set \".{}\" on null", name.text), name.span, Self::null_hint(obj_expr, &format!(".{}", name.text)))),
            other => Err(self.err("LIP5008", format!("cannot set a field on {}", with_article(&other.type_name())), name.span, None)),
        }
    }

    fn list_position(&self, index: &Value, len: usize, index_expr: &Expr) -> Result<usize, Flow> {
        let Value::Int(n) = index else {
            return Err(self.err("LIP2001", "positions in Arrays and Strings must be Integers", index_expr.span, Some(operand_hint(index_expr, &index.type_name(), None))));
        };
        let i = if *n < 0 { len as i64 + n } else { *n };
        if i < 0 || i >= len as i64 {
            let hint = if len == 0 {
                "The Array is empty. Add items with .push(item) first.".to_string()
            } else {
                format!("Positions start at 0, so the last one is {}. Negative positions count from the end: -1 is the last item.", len - 1)
            };
            return Err(self.err("LIP5001", format!("index {n} is outside array length {len}"), index_expr.span, Some(hint)));
        }
        Ok(i as usize)
    }

    fn get_index(&mut self, obj: &Value, index: &Value, obj_expr: &Expr, index_expr: &Expr) -> Result<Value, Flow> {
        match obj {
            Value::List(items) => {
                let len = items.borrow().len();
                let i = self.list_position(index, len, index_expr)?;
                Ok(items.borrow()[i].clone())
            }
            Value::Str(s) => {
                let chars: Vec<char> = s.chars().collect();
                let i = self.list_position(index, chars.len(), index_expr)?;
                Ok(Value::string(chars[i].to_string()))
            }
            Value::Object(o) => match index {
                Value::Str(k) => Ok(o.fields.borrow().get(&**k).cloned().unwrap_or(Value::Nil)),
                other => Err(self.err("LIP2001", format!("Object keys are Strings, not {}", with_article(&other.type_name())), index_expr.span, None)),
            },
            Value::Nil => Err(self.err("LIP5003", "cannot read an item from null", index_expr.span, Self::null_hint(obj_expr, "[...]"))),
            other => Err(self.err("LIP2001", format!("cannot use [ ] on {}", with_article(&other.type_name())), index_expr.span, None)),
        }
    }

    fn set_index(&mut self, obj: &Value, index: &Value, v: Value, obj_expr: &Expr, index_expr: &Expr) -> Result<(), Flow> {
        match obj {
            Value::List(items) => {
                let len = items.borrow().len();
                if matches!(index, Value::Int(n) if *n == len as i64) {
                    return Err(self.err(
                        "LIP5001",
                        format!("index {len} is just past the end of the array"),
                        index_expr.span,
                        Some("To add an item to the end, use .push(item).".into()),
                    ));
                }
                let i = self.list_position(index, len, index_expr)?;
                items.borrow_mut()[i] = v;
                Ok(())
            }
            Value::Object(o) => match index {
                Value::Str(k) => {
                    if o.ty.is_some() || o.module.is_some() {
                        let name = Name { text: k.to_string(), span: index_expr.span };
                        return self.set_field(obj, &name, v, obj_expr, index_expr.span);
                    }
                    o.fields.borrow_mut().insert(k.to_string(), v);
                    Ok(())
                }
                other => Err(self.err("LIP2001", format!("Object keys are Strings, not {}", with_article(&other.type_name())), index_expr.span, None)),
            },
            Value::Str(_) => Err(self.err(
                "LIP5008",
                "Strings can't be changed in place",
                index_expr.span,
                Some("Build a new String instead, for example with .replace() or .slice().".into()),
            )),
            Value::Nil => Err(self.err("LIP5003", "cannot set an item on null", index_expr.span, Self::null_hint(obj_expr, "[...]"))),
            other => Err(self.err("LIP2001", format!("cannot use [ ] on {}", with_article(&other.type_name())), index_expr.span, None)),
        }
    }

    // ----- calls -----------------------------------------------------------

    fn call_method(&mut self, obj: Value, name: &Name, pos: Vec<Value>, named: Vec<(String, Value)>, span: Span, obj_expr: &Expr) -> Result<Value, Flow> {
        let key = name.text.as_str();
        match &obj {
            Value::Object(o) => {
                let field = o.fields.borrow().get(key).cloned();
                if let Some(f) = field {
                    return self.call_value(f, pos, named, span, None);
                }
                if let Some(ty) = &o.ty {
                    if let Some(m) = ty.decl.methods.iter().find(|m| m.name.text == key) {
                        let c = Rc::new(Closure { decl: m.clone(), env: ty.env.clone(), file: ty.file.clone(), this: Some(obj.clone()) });
                        return self.call_function(&c, pos, named, span);
                    }
                }
                if o.module.is_none() {
                    let mut args = Args { pos, named, span, name: key.to_string() };
                    if let Some(r) = methods::call(self, &obj, key, &mut args) {
                        return r;
                    }
                }
                Err(self.missing_field(o, name))
            }
            Value::Nil => Err(self.err("LIP5003", format!("cannot call \".{key}()\" on null"), name.span, Self::null_hint(obj_expr, &format!(".{key}()")))),
            _ => {
                let mut args = Args { pos, named, span, name: key.to_string() };
                if let Some(r) = methods::call(self, &obj, key, &mut args) {
                    return r;
                }
                match methods::members_for(&obj) {
                    Some((tyname, members)) => Err(self.throw(unknown_member(tyname, key, name.span, members))),
                    None => Err(self.err("LIP1004", format!("{} has no method \"{key}\"", with_article(&obj.type_name())), name.span, None)),
                }
            }
        }
    }

    /// Call any callable value.
    pub fn call_value(&mut self, f: Value, pos: Vec<Value>, named: Vec<(String, Value)>, span: Span, callee: Option<&Expr>) -> Result<Value, Flow> {
        match f {
            Value::Func(c) => self.call_function(&c, pos, named, span),
            Value::Native(n) => {
                let mut args = Args { pos, named, span, name: n.name.clone() };
                (n.f)(self, &mut args)
            }
            Value::Type(t) => self.construct(&t, pos, named, span),
            Value::Method(m) => {
                let name = Name { text: m.1.clone(), span };
                let dummy = Expr { kind: ExprKind::Null, span };
                self.call_method(m.0.clone(), &name, pos, named, span, &dummy)
            }
            other => {
                let what = match callee.map(|c| &c.kind) {
                    Some(ExprKind::Ident(n)) => format!("\"{n}\""),
                    _ => "this value".to_string(),
                };
                Err(self.err(
                    "LIP2008",
                    format!("{what} is {}, not a function", with_article(&other.type_name())),
                    callee.map_or(span, |c| c.span),
                    Some("Only functions can be called with ( ).".into()),
                ))
            }
        }
    }

    /// Call a function passed as a callback, dropping arguments it doesn't take
    /// (so `items.map(x => x * 2)` works even though map offers the index too).
    pub fn call_callback(&mut self, f: &Value, mut args: Vec<Value>, span: Span) -> Result<Value, Flow> {
        if let Value::Func(c) = f {
            args.truncate(c.decl.params.len());
        }
        self.call_value(f.clone(), args, Vec::new(), span, None)
    }

    fn signature(decl: &FuncDecl) -> String {
        let params: Vec<&str> = decl.params.iter().map(|p| p.name.text.as_str()).collect();
        format!("{}({})", decl.name.text, params.join(", "))
    }

    fn call_function(&mut self, c: &Rc<Closure>, pos: Vec<Value>, named: Vec<(String, Value)>, span: Span) -> Result<Value, Flow> {
        let decl = c.decl.clone();
        let fname = || if decl.is_lambda { "this function".to_string() } else { format!("\"{}\"", decl.name.text) };
        if self.depth >= MAX_DEPTH {
            return Err(self.err(
                "LIP5005",
                format!("too much recursion: {} called itself too many times", fname()),
                span,
                Some("Make sure the recursion has a case where it stops calling itself.".into()),
            ));
        }
        let params = &decl.params;
        if pos.len() > params.len() {
            let n = params.len();
            return Err(self.err(
                "LIP2003",
                format!("{} takes {n} argument{}, but {} were given", fname(), if n == 1 { "" } else { "s" }, pos.len()),
                span,
                Some(format!("It is defined as {}.", Self::signature(&decl))),
            ));
        }
        let mut values: Vec<Option<Value>> = pos.into_iter().map(Some).collect();
        values.resize(params.len(), None);
        for (n, v) in named {
            match params.iter().position(|p| p.name.text == n) {
                Some(i) if values[i].is_some() => {
                    return Err(self.err("LIP2003", format!("the argument \"{n}\" was given twice"), span, None));
                }
                Some(i) => values[i] = Some(v),
                None => {
                    let hint = suggest::did_you_mean(&n, params.iter().map(|p| p.name.text.as_str())).unwrap_or_else(|| format!("It is defined as {}.", Self::signature(&decl)));
                    return Err(self.err("LIP1007", format!("{} has no parameter named \"{n}\"", fname()), span, Some(hint)));
                }
            }
        }
        for (i, p) in params.iter().enumerate() {
            match &values[i] {
                None if p.default.is_none() => {
                    return Err(self.err(
                        "LIP2003",
                        format!("missing argument \"{}\" for {}", p.name.text, fname()),
                        span,
                        Some(format!("It is defined as {}.", Self::signature(&decl))),
                    ));
                }
                Some(v) => {
                    if let Some(t) = &p.ty {
                        if !self.matches_type(v, t) {
                            return Err(self.err(
                                "LIP2004",
                                format!("\"{}\" should be {}, but this is {}", p.name.text, with_article(&t.to_string()), with_article(&v.type_name())),
                                span,
                                Some(format!("{} expects \"{}\" to be {t}.", fname(), p.name.text)),
                            ));
                        }
                    }
                }
                None => {}
            }
        }

        let env = Env::new(Some(c.env.clone()), EnvKind::Function);
        if let Some(this) = &c.this {
            env.define("self", this.clone());
        }
        let caller_file = std::mem::replace(&mut self.file, c.file.clone());
        self.frames.push(Frame { decl: decl.clone(), file: caller_file.clone(), line: span.line });
        self.depth += 1;

        let result = (|| -> Result<Value, Flow> {
            for (i, p) in params.iter().enumerate() {
                let v = match values[i].take() {
                    Some(v) => v,
                    None => {
                        let d = p.default.as_ref().expect("checked above");
                        let v = self.eval(d, &env)?;
                        if let Some(t) = &p.ty {
                            self.check_declared(&v, t, &p.name.text, d.span)?;
                        }
                        v
                    }
                };
                env.vars.borrow_mut().insert(p.name.text.clone(), Slot { value: v, constant: false, declared: p.ty.clone() });
            }
            self.hoist(&decl.body, &env);
            let value = match self.exec_block(&decl.body, &env) {
                Ok(()) | Err(Flow::Break) | Err(Flow::Continue) => Value::Nil,
                Err(Flow::Return(v)) => v,
                Err(e) => return Err(e),
            };
            if let Some(t) = &decl.ret {
                if !self.matches_type(&value, t) {
                    return Err(self.err(
                        "LIP2007",
                        format!("{} should return {}, but it returned {}", fname(), with_article(&t.to_string()), with_article(&value.type_name())),
                        t.span,
                        Some(format!("The definition says it returns {t}.")),
                    ));
                }
            }
            Ok(value)
        })();

        self.depth -= 1;
        self.frames.pop();
        self.file = caller_file;

        if decl.is_async {
            return match result {
                Ok(v) => Ok(task::done(Ok(v))),
                Err(Flow::Throw(t)) => Ok(task::done(Err(t))),
                Err(other) => Err(other),
            };
        }
        result
    }

    fn construct(&mut self, t: &Rc<TypeInfo>, pos: Vec<Value>, named: Vec<(String, Value)>, span: Span) -> Result<Value, Flow> {
        let decl = &t.decl;
        let tname = &decl.name.text;
        let field_list = || decl.fields.iter().map(|f| f.name.text.as_str()).collect::<Vec<_>>().join(", ");
        if pos.len() > decl.fields.len() {
            return Err(self.err(
                "LIP2003",
                format!("\"{tname}\" has {} field{}, but {} values were given", decl.fields.len(), if decl.fields.len() == 1 { "" } else { "s" }, pos.len()),
                span,
                Some(format!("Its fields are: {}", field_list())),
            ));
        }
        let mut values: Vec<Option<Value>> = pos.into_iter().map(Some).collect();
        values.resize(decl.fields.len(), None);
        for (n, v) in named {
            match decl.fields.iter().position(|f| f.name.text == n) {
                Some(i) => values[i] = Some(v),
                None => {
                    let hint = suggest::did_you_mean(&n, decl.fields.iter().map(|f| f.name.text.as_str())).unwrap_or_else(|| format!("Its fields are: {}", field_list()));
                    return Err(self.err("LIP1007", format!("\"{tname}\" has no field \"{n}\""), span, Some(hint)));
                }
            }
        }
        let mut fields = Fields::new();
        for (i, f) in decl.fields.iter().enumerate() {
            let v = match values[i].take() {
                Some(v) => v,
                None => match &f.default {
                    Some(d) => {
                        let prev = std::mem::replace(&mut self.file, t.file.clone());
                        let r = self.eval(d, &t.env);
                        self.file = prev;
                        r?
                    }
                    None if matches!(f.ty.as_ref().map(|t| &t.kind), Some(TypeKind::Optional(_))) => Value::Nil,
                    None => {
                        return Err(self.err(
                            "LIP2003",
                            format!("missing \"{}\" when creating {}", f.name.text, with_article(tname)),
                            span,
                            Some(format!("For example: {tname}({}: ...)", f.name.text)),
                        ))
                    }
                },
            };
            if let Some(ty) = &f.ty {
                self.check_declared(&v, ty, &f.name.text, span)?;
            }
            fields.insert(f.name.text.clone(), v);
        }
        Ok(Value::Object(Rc::new(ObjectData { fields: RefCell::new(fields), ty: Some(t.clone()), module: None, tag: None, payload: None })))
    }

    // ----- modules ---------------------------------------------------------

    /// Resolve `use <source>`.
    ///
    /// - A path (`"./helpers.lipi"`, `"../lib/x"`) is relative to the current file.
    /// - A name (`math`, `utils.strings`) is looked up, in order, as a project file
    ///   next to the current file or in `src/`, then as a package in
    ///   `lipi_modules/`, then as a standard module. A name that matches both a
    ///   project file and a package is an error.
    fn use_module(&mut self, source: &str, span: Span) -> Result<Value, Flow> {
        match resolve::resolve_use(source, &self.file, &self.project_root, builtins::MODULES) {
            Ok(Resolved::File(path)) => self.load_module(&path, span),
            Ok(Resolved::Std(name)) => match self.globals.get(&name) {
                Some(v) => Ok(v),
                None => Err(self.err("LIP3001", format!("module not found: \"{source}\""), span, None)),
            },
            Err(e) => Err(self.err(e.code, e.message, span, Some(e.hint))),
        }
    }

    fn load_module(&mut self, path: &Path, span: Span) -> Result<Value, Flow> {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(m) = self.modules.get(&canon) {
            return Ok(m.clone());
        }
        if self.loading.contains(&canon) {
            return Err(self.err(
                "LIP3002",
                format!("circular use: \"{}\" is already being loaded", path.to_string_lossy()),
                span,
                Some("Two files use each other. Move the shared code into a third file that both use.".into()),
            ));
        }
        let file: Rc<str> = Rc::from(path.to_string_lossy().as_ref());
        let source = match Self::read_source(path, &file) {
            Ok(s) => s,
            Err(_) => return Err(self.err("LIP3001", format!("couldn't read \"{file}\""), span, None)),
        };
        self.loading.push(canon.clone());
        let result = self.run_source(&source, file.clone());
        self.loading.pop();
        let (env, exported) = match result {
            Ok(r) => r,
            Err(RunError::Syntax(diag, f)) => return Err(self.module_error(diag, f)),
            Err(RunError::Check(mut diags, f)) => return Err(self.module_error(diags.remove(0), f)),
            Err(RunError::Runtime(t)) => return Err(Flow::Throw(t)),
            Err(RunError::Exit(code)) => return Err(Flow::Exit(code)),
        };
        let mut fields = Fields::new();
        for name in exported {
            let value = env.vars.borrow().get(&name).map(|s| s.value.clone());
            match value {
                Some(v) => {
                    fields.insert(name, v);
                }
                None => return Err(self.err("LIP3005", format!("\"{file}\" exports \"{name}\", but never defines it"), span, None)),
            }
        }
        let module_name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let value = Value::Object(Rc::new(ObjectData { fields: RefCell::new(fields), ty: None, module: Some(module_name), tag: None, payload: None }));
        self.modules.insert(canon, value.clone());
        Ok(value)
    }

    fn module_error(&self, diag: Diagnostic, file: Rc<str>) -> Flow {
        let mut t = self.thrown_diag(diag);
        t.file = file;
        t.trace = Vec::new();
        Flow::Throw(t)
    }

    pub(crate) fn next_random(&mut self) -> f64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (r >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn compare(op: BinOp, ord: Option<std::cmp::Ordering>) -> bool {
    use std::cmp::Ordering::*;
    match (op, ord) {
        (BinOp::Lt, Some(Less)) => true,
        (BinOp::Gt, Some(Greater)) => true,
        (BinOp::LtEq, Some(Less | Equal)) => true,
        (BinOp::GtEq, Some(Greater | Equal)) => true,
        _ => false,
    }
}

