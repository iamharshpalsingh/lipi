//! The tree-walking interpreter.

use crate::builtins;
use crate::methods;
use crate::resolver::Resolver;
use crate::task;
use crate::value::*;
use lipi_compiler::ast::*;
use lipi_compiler::checker::{self, binary_error, condition_error, module_binding_name, operand_hint, unknown_member, unknown_name, with_article};
use lipi_compiler::resolve::{self, find_project_root, Resolved};
use lipi_compiler::suggest;
use lipi_compiler::{Diagnostic, Severity, Span};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Something `time.after` or `time.every` scheduled. Timers run after the main
/// program finishes, in time order, like JavaScript's event loop.
pub(crate) struct Timer {
    pub id: u64,
    pub due: Instant,
    pub every: Option<Duration>,
    pub f: Value,
    pub stopped: Rc<Cell<bool>>,
    pub span: Span,
    pub file: Rc<str>,
}

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
    layout: Names,
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
    /// The slot names of every resolved function, keyed by its address (see resolver.rs).
    layouts: HashMap<usize, Names>,
    pub(crate) timers: Vec<Timer>,
    pub(crate) next_timer: u64,
    /// Whether errors printed while timers run use colours (set by the command line).
    pub color: bool,
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
            layouts: HashMap::new(),
            timers: Vec::new(),
            next_timer: 0,
            color: false,
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
            let hint = if e.kind() == std::io::ErrorKind::NotFound { resolve::missing_file_hint(path) } else { e.to_string() };
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
        let (env, _) = result?;
        // With a web server about to start, `serve` runs the timers between
        // requests instead; running them here would never reach the server.
        if self.server.listener.is_none() {
            self.run_timers()?;
        }
        Ok(env)
    }

    /// Run what `time.after` and `time.every` scheduled, earliest first, until
    /// none are left. An error stops its timer and is shown; the others go on
    /// (as in JavaScript), and the program then exits with code 1.
    fn run_timers(&mut self) -> Result<(), RunError> {
        let mut failed = false;
        while let Some((i, due)) = self.next_timer_due() {
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            if !self.fire_timer(i)? {
                failed = true;
            }
        }
        if failed {
            return Err(RunError::Exit(1));
        }
        Ok(())
    }

    /// The timer that runs next, and when. Stopped timers are dropped here.
    pub(crate) fn next_timer_due(&mut self) -> Option<(usize, Instant)> {
        self.timers.retain(|t| !t.stopped.get());
        let i = (0..self.timers.len()).min_by_key(|&i| (self.timers[i].due, self.timers[i].id))?;
        Some((i, self.timers[i].due))
    }

    /// Run one timer's block. Returns false if it ended in an error, which is
    /// shown and stops that timer while the others go on.
    pub(crate) fn fire_timer(&mut self, i: usize) -> Result<bool, RunError> {
        let t = &mut self.timers[i];
        let (f, span, stopped, file) = (t.f.clone(), t.span, t.stopped.clone(), t.file.clone());
        match t.every {
            Some(d) => t.due += d,
            None => t.stopped.set(true),
        }
        let prev = std::mem::replace(&mut self.file, file);
        let r = self.call_callback(&f, Vec::new(), span);
        self.file = prev;
        match r {
            Ok(_) | Err(Flow::Return(_)) | Err(Flow::Break) | Err(Flow::Continue) => Ok(true),
            Err(Flow::Exit(code)) => Err(RunError::Exit(code)),
            Err(Flow::Throw(t)) => {
                stopped.set(true);
                use std::io::Write;
                let _ = std::io::stdout().flush();
                eprint!("{}", self.render(&RunError::Runtime(t), self.color));
                Ok(false)
            }
        }
    }

    /// `time.after(ms, block)` / `time.every(ms, block)`: returns the timer's stop() switch.
    pub(crate) fn schedule(&mut self, ms: f64, every: bool, f: Value, span: Span) -> Rc<Cell<bool>> {
        let wait = Duration::from_secs_f64(ms.max(0.0) / 1000.0);
        let stopped = Rc::new(Cell::new(false));
        self.next_timer += 1;
        self.timers.push(Timer {
            id: self.next_timer,
            due: Instant::now() + wait,
            every: if every { Some(wait) } else { None },
            f,
            stopped: stopped.clone(),
            span,
            file: self.file.clone(),
        });
        stopped
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
        self.resolve(&program, &env);
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
        self.resolve(&program, env);
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
            let env = Env::with_layout(Some(case.env.clone()), EnvKind::Function, &case.layout);
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
                StmtKind::Func(f) | StmtKind::Component(f) => self.define_name(env, &f.name, self.closure(f, env)),
                StmtKind::TypeDef(t) => self.define_name(env, &t.name, Value::Type(Rc::new(TypeInfo { decl: t.clone(), env: env.clone(), file: self.file.clone() }))),
                _ => {}
            }
        }
    }

    fn closure(&self, f: &Rc<FuncDecl>, env: &Rc<Env>) -> Value {
        Value::Func(Rc::new(Closure { decl: f.clone(), env: env.clone(), file: self.file.clone(), this: None, owner: None, layout: self.layout_of(Rc::as_ptr(f) as usize) }))
    }

    /// A type's method bound to an object.
    fn method_closure(&self, m: Rc<FuncDecl>, owner: Rc<TypeInfo>, this: Value) -> Rc<Closure> {
        let layout = self.layout_of(Rc::as_ptr(&m) as usize);
        Rc::new(Closure { decl: m, env: owner.env.clone(), file: owner.file.clone(), this: Some(this), owner: Some(owner), layout })
    }

    // ----- types that extend other types ---------------------------------------

    /// A type and the types it extends, nearest first.
    fn type_chain(&self, t: &Rc<TypeInfo>) -> Result<Vec<Rc<TypeInfo>>, Flow> {
        let mut chain = vec![t.clone()];
        loop {
            let cur = chain.last().expect("never empty").clone();
            let Some(p) = &cur.decl.parent else { break };
            let in_its_file = |this: &Self, diag: Diagnostic| {
                let mut thrown = this.thrown_diag(diag);
                thrown.file = cur.file.clone();
                Flow::Throw(thrown)
            };
            let parent = match self.read(&p.text, p.res.get(), p.span, &cur.env)? {
                Value::Type(pt) => pt,
                other => {
                    let message = format!("\"{}\" can't extend \"{}\": that's {}, not a type", cur.decl.name.text, p.text, with_article(&other.type_name()));
                    return Err(in_its_file(self, Diagnostic::error(message, p.span).with_code("LIP2001")));
                }
            };
            if chain.iter().any(|x| Rc::ptr_eq(x, &parent)) {
                let message = format!("\"{}\" extends itself through \"{}\"", t.decl.name.text, cur.decl.name.text);
                return Err(in_its_file(self, Diagnostic::error(message, p.span).with_code("LIP2001").with_hint("Types can't extend each other in a circle.")));
            }
            chain.push(parent);
        }
        Ok(chain)
    }

    /// Every field of a type, the parents' first (a field declared again keeps
    /// its place), each with the type that declares it and its position there.
    fn all_fields(&self, t: &Rc<TypeInfo>) -> Result<Vec<(Rc<TypeInfo>, usize)>, Flow> {
        if t.decl.parent.is_none() {
            return Ok((0..t.decl.fields.len()).map(|i| (t.clone(), i)).collect());
        }
        let mut out: Vec<(Rc<TypeInfo>, usize)> = Vec::new();
        for ty in self.type_chain(t)?.iter().rev() {
            for (i, f) in ty.decl.fields.iter().enumerate() {
                match out.iter().position(|(o, j)| o.decl.fields[*j].name.text == f.name.text) {
                    Some(k) => out[k] = (ty.clone(), i),
                    None => out.push((ty.clone(), i)),
                }
            }
        }
        Ok(out)
    }

    /// A method of a type or of a type it extends, with the type that declares it.
    fn find_method(&self, t: &Rc<TypeInfo>, name: &str) -> Result<Option<(Rc<FuncDecl>, Rc<TypeInfo>)>, Flow> {
        if let Some(m) = t.decl.methods.iter().find(|m| m.name.text == name) {
            return Ok(Some((m.clone(), t.clone())));
        }
        if t.decl.parent.is_none() {
            return Ok(None);
        }
        for ty in self.type_chain(t)?.into_iter().skip(1) {
            if let Some(m) = ty.decl.methods.iter().find(|m| m.name.text == name) {
                return Ok(Some((m.clone(), ty)));
            }
        }
        Ok(None)
    }

    /// `super` inside a method of `owner`: the methods of the types it extends, bound to the same object.
    fn super_object(&self, owner: &Rc<TypeInfo>, this: &Value) -> Result<Value, Flow> {
        let chain = self.type_chain(owner)?;
        let mut fields = Fields::new();
        for ty in chain.iter().skip(1) {
            for m in &ty.decl.methods {
                if !fields.contains_key(&m.name.text) {
                    fields.insert(m.name.text.clone(), Value::Func(self.method_closure(m.clone(), ty.clone(), this.clone())));
                }
            }
        }
        let parent_name: Rc<dyn std::any::Any> = Rc::new(chain.get(1).map(|p| p.decl.name.text.clone()).unwrap_or_default());
        Ok(Value::Object(Rc::new(ObjectData { fields: RefCell::new(fields), ty: None, module: None, tag: Some("super"), payload: Some(parent_name) })))
    }

    // ----- spread and patterns ------------------------------------------------------

    /// `...v` in an Array literal or a call's arguments.
    fn spread_into(&self, out: &mut Vec<Value>, v: Value, span: Span, into: &str) -> Result<(), Flow> {
        match v {
            Value::List(l) => out.extend(l.borrow().iter().cloned()),
            Value::Str(s) => out.extend(s.chars().map(|c| Value::string(c.to_string()))),
            other => return Err(self.throw(checker::spread_error(into, &other.type_name(), span))),
        }
        Ok(())
    }

    /// Take `v` apart into a pattern's variables: `{name, age} = user`, `[a, b] = pair`.
    fn destructure(&mut self, p: &Pattern, v: Value, value_span: Span, env: &Rc<Env>) -> Result<(), Flow> {
        match p {
            Pattern::Object { fields, rest, .. } => {
                let Value::Object(o) = &v else { return Err(self.throw(checker::pattern_error(true, &v.type_name(), value_span))) };
                let o = o.clone();
                let source = Expr { res: Default::default(), kind: ExprKind::Null, span: value_span };
                for (key, name) in fields {
                    let x = self.get_field(&v, key, false, &source)?;
                    self.assign_to(env, name, x, name.span)?;
                }
                if let Some(r) = rest {
                    let named: Vec<&str> = fields.iter().map(|(k, _)| k.text.as_str()).collect();
                    let others: Fields = o.fields.borrow().iter().filter(|(k, _)| !named.contains(&k.as_str())).map(|(k, x)| (k.clone(), x.clone())).collect();
                    self.assign_to(env, r, Value::object(others), r.span)?;
                }
            }
            Pattern::List { items, rest, .. } => {
                let Value::List(l) = &v else { return Err(self.throw(checker::pattern_error(false, &v.type_name(), value_span))) };
                let values = l.borrow().clone();
                let n = items.len();
                if values.len() < n {
                    return Err(self.err(
                        "LIP5001",
                        format!("this pattern needs {n} item{}, but the Array has {}", if n == 1 { "" } else { "s" }, values.len()),
                        value_span,
                        Some("Check the Array's length first, or take fewer items.".into()),
                    ));
                }
                for (i, item) in items.iter().enumerate() {
                    if let Some(name) = item {
                        self.assign_to(env, name, values[i].clone(), name.span)?;
                    }
                }
                if let Some(r) = rest {
                    self.assign_to(env, r, Value::list(values[n..].to_vec()), r.span)?;
                }
            }
        }
        Ok(())
    }

    fn layout_of(&self, key: usize) -> Names {
        self.layouts.get(&key).cloned().unwrap_or_default()
    }

    /// Give every variable in `program` a numbered slot (see resolver.rs).
    fn resolve(&mut self, program: &Program, env: &Rc<Env>) {
        let layouts = {
            let mut r = Resolver::new(&self.globals);
            r.module(&program.body, &env.names);
            r.finish()
        };
        env.ensure_slots();
        self.layouts.extend(layouts);
    }

    // ----- variables ---------------------------------------------------------

    /// Where a definition (`for x`, `catch e`, a function...) stores its value: its own scope.
    fn def_slot<'e>(env: &'e Rc<Env>, name: &Name) -> (&'e Env, usize) {
        if let Res::Local { depth, index } = name.res.get() {
            if let Some(e) = env_at(env, depth) {
                return (e, index as usize);
            }
        }
        (&**env, env.slot_index(&name.text))
    }

    /// Where an assignment stores its value: the nearest existing variable, else a new one here.
    fn target_slot<'e>(env: &'e Rc<Env>, name: &Name) -> (&'e Env, usize) {
        if let Res::Local { depth, index } = name.res.get() {
            if let Some(e) = env_at(env, depth) {
                return (e, index as usize);
            }
        }
        let mut e: &Env = env;
        loop {
            if e.kind == EnvKind::Builtins {
                break;
            }
            if let Some(i) = e.index_of(&name.text) {
                return (e, i);
            }
            match e.parent.as_deref() {
                Some(p) => e = p,
                None => break,
            }
        }
        (&**env, env.slot_index(&name.text))
    }

    fn put(e: &Env, index: usize, slot: Slot) {
        let mut slots = e.slots.borrow_mut();
        if index >= slots.len() {
            slots.resize_with(index + 1, Slot::empty);
        }
        slots[index] = slot;
    }

    fn define_name(&self, env: &Rc<Env>, name: &Name, v: Value) {
        let (e, i) = Self::def_slot(env, name);
        Self::put(e, i, Slot::new(v));
    }

    /// A constant, typed or `state` variable (the definition itself isn't checked against earlier ones).
    fn declare(&self, env: &Rc<Env>, name: &Name, v: Value, constant: bool, declared: Option<TypeExpr>) {
        let (e, i) = Self::def_slot(env, name);
        Self::put(e, i, Slot { value: Some(v), constant, declared });
    }

    /// Assignment updates the nearest existing variable, otherwise it creates
    /// one in the current function (decided when the program was resolved).
    fn assign_to(&self, env: &Rc<Env>, name: &Name, v: Value, value_span: Span) -> Result<(), Flow> {
        let (e, i) = Self::target_slot(env, name);
        let mut slots = e.slots.borrow_mut();
        if i >= slots.len() {
            slots.resize_with(i + 1, Slot::empty);
        }
        let slot = &mut slots[i];
        let n = &name.text;
        if slot.constant {
            return Err(self.err(
                "LIP1003",
                format!("\"{n}\" is a constant and can't be changed"),
                value_span,
                Some("Remove `const` where it's defined if it needs to change.".into()),
            ));
        }
        if let Some(t) = &slot.declared {
            if !self.matches_type(&v, t) {
                return Err(self.err(
                    "LIP2002",
                    format!("\"{n}\" should be {}, but this is {}", with_article(&t.to_string()), with_article(&v.type_name())),
                    value_span,
                    Some(format!("\"{n}\" was declared as {t}.")),
                ));
            }
        }
        slot.value = Some(v);
        Ok(())
    }

    /// Read a variable. Unset slots (a variable read before it's assigned) are "undefined".
    fn read(&self, name: &str, res: Res, span: Span, env: &Rc<Env>) -> Result<Value, Flow> {
        let found = match res {
            Res::Local { depth, index } => env_at(env, depth).and_then(|e| e.slots.borrow().get(index as usize).and_then(|s| s.value.clone())),
            Res::Global(i) => self.globals.slots.borrow().get(i as usize).and_then(|s| s.value.clone()),
            Res::Unresolved | Res::Unknown => env.get(name),
        };
        match found {
            Some(v) => Ok(v),
            None => {
                let names = env.names();
                Err(self.throw(unknown_name(name, span, names.iter().map(String::as_str))))
            }
        }
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
            StmtKind::For { first, second, iter, body, pattern } => self.exec_for(first, second.as_ref(), iter, body, pattern.as_ref(), env)?,
            StmtKind::Func(f) | StmtKind::Component(f) => self.define_name(env, &f.name, self.closure(f, env)),
            StmtKind::State { name, ty, value } => {
                let v = self.eval(value, env)?;
                if let Some(t) = ty {
                    self.check_declared(&v, t, &name.text, value.span)?;
                }
                self.declare(env, name, v, false, ty.clone());
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
                            self.define_name(env, n, t.value.clone());
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
                                Some(v) => self.define_name(env, n, v),
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
            StmtKind::TypeDef(t) => self.define_name(env, &t.name, Value::Type(Rc::new(TypeInfo { decl: t.clone(), env: env.clone(), file: self.file.clone() }))),
            StmtKind::Test { name, body } => {
                if self.test_mode {
                    let layout = self.layout_of(body.as_ptr() as usize);
                    self.tests.push(TestCase { name: name.clone(), body: body.clone(), env: env.clone(), file: self.file.clone(), layout });
                }
            }
        }
        Ok(())
    }

    fn exec_for(&mut self, first: &Name, second: Option<&Name>, iter: &Expr, body: &[Stmt], pattern: Option<&Pattern>, env: &Rc<Env>) -> Result<(), Flow> {
        let layout = self.layout_of(crate::resolver::loop_key(body));
        let mut round = Env::with_layout(Some(env.clone()), EnvKind::Block, &layout);
        if let ExprKind::Range { start, end, step } = &iter.kind {
            let (from, to, step) = self.range_parts(start, end, step.as_deref(), env)?;
            let mut i = from;
            let mut index = 0i64;
            while (step > 0 && i <= to) || (step < 0 && i >= to) {
                Self::next_round(&mut round, env, &layout);
                let env = &round;
                match second {
                    Some(s) => {
                        self.define_name(env, first, Value::Int(index));
                        self.define_name(env, s, Value::Int(i));
                    }
                    None => {
                        self.define_name(env, first, Value::Int(i));
                        if let Some(p) = pattern {
                            self.destructure(p, Value::Int(i), iter.span, env)?;
                        }
                    }
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
            Self::next_round(&mut round, env, &layout);
            let env = &round;
            match second {
                Some(s) => {
                    self.define_name(env, first, key);
                    self.define_name(env, s, value);
                }
                None => {
                    let item = if keyed { key } else { value };
                    self.define_name(env, first, item.clone());
                    if let Some(p) = pattern {
                        self.destructure(p, item, iter.span, env)?;
                    }
                }
            }
            if !self.loop_body(body, env)? {
                break;
            }
        }
        Ok(())
    }

    /// Start the next round of a `for` loop. The loop's variables live in a
    /// small environment of their own so that a function made in the body
    /// keeps the item it was made with. Nothing outside is holding the last
    /// round's environment in the usual case, and then it is simply emptied
    /// and used again; a fresh one is made only when something kept it.
    fn next_round(round: &mut Rc<Env>, parent: &Rc<Env>, layout: &Names) {
        if Rc::strong_count(round) == 1 {
            let mut slots = round.slots.borrow_mut();
            slots.clear();
            slots.resize_with(layout.borrow().len(), Slot::empty);
            return;
        }
        *round = Env::with_layout(Some(parent.clone()), EnvKind::Block, layout);
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
                    let current = self.read(&n.text, n.res.get(), n.span, env)?;
                    v = match int_pair(&current, &v).and_then(|(a, b)| int_op(op, a, b)) {
                        Some(fast) => fast,
                        None => {
                            let target_expr = Expr { res: Default::default(), kind: ExprKind::Ident(n.text.clone()), span: n.span };
                            self.binary(op, current, v, &target_expr, value)?
                        }
                    };
                }
                if constant || ty.is_some() {
                    if let Some(t) = ty {
                        self.check_declared(&v, t, &n.text, value.span)?;
                    }
                    self.declare(env, n, v, constant, ty.cloned());
                    Ok(())
                } else {
                    self.assign_to(env, n, v, value.span)
                }
            }
            Target::Field(obj_expr, name) => {
                let obj = self.eval(obj_expr, env)?;
                let v = match op {
                    Some(op) => {
                        let current = self.get_field(&obj, name, false, obj_expr)?;
                        let rhs = self.eval(value, env)?;
                        match int_pair(&current, &rhs).and_then(|(a, b)| int_op(op, a, b)) {
                            Some(fast) => fast,
                            None => {
                                let target_expr = Expr { res: Default::default(), kind: ExprKind::Ident(name.text.clone()), span: name.span };
                                self.binary(op, current, rhs, &target_expr, value)?
                            }
                        }
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
            Target::Pattern(p) => {
                let v = self.eval(value, env)?;
                self.destructure(p, v, value.span, env)
            }
        }
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
                // An Admin (type Admin extends User) is also a User.
                other => matches!(v, Value::Object(o) if o.ty.as_ref().is_some_and(|ty| {
                    ty.decl.name.text == other || (ty.decl.parent.is_some() && self.type_chain(ty).is_ok_and(|c| c.iter().any(|x| x.decl.name.text == other)))
                })),
            },
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
            ExprKind::Ident(name) => self.read(name, e.res.get(), e.span, env)?,
            ExprKind::List(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    match &item.kind {
                        ExprKind::Spread(inner) => {
                            let v = self.eval(inner, env)?;
                            self.spread_into(&mut out, v, inner.span, "an Array")?;
                        }
                        _ => out.push(self.eval(item, env)?),
                    }
                }
                Value::list(out)
            }
            ExprKind::Object(fields) => {
                let mut map = Fields::with_capacity(fields.len());
                for (k, v) in fields {
                    match &v.kind {
                        // `{...defaults, color: "red"}`: a later key replaces an earlier one's value, keeping its place.
                        ExprKind::Spread(inner) => match self.eval(inner, env)? {
                            Value::Object(o) if o.module.is_none() => {
                                for (fk, fv) in o.fields.borrow().iter() {
                                    map.insert(fk.clone(), fv.clone());
                                }
                            }
                            other => return Err(self.throw(checker::spread_error("an Object", &other.type_name(), inner.span))),
                        },
                        _ => {
                            map.insert(k.text.clone(), self.eval(v, env)?);
                        }
                    }
                }
                Value::object(map)
            }
            // Only valid inside [ ], { } and arguments, which handle it themselves.
            ExprKind::Spread(inner) => self.eval(inner, env)?,
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
                if let Some(v) = int_pair(&lv, &rv).and_then(|(a, b)| int_op(*op, a, b)) {
                    return Ok(v);
                }
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
            if let ExprKind::Spread(inner) = &a.value.kind {
                let v = self.eval(inner, env)?;
                self.spread_into(&mut pos, v, inner.span, "the arguments")?;
                continue;
            }
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
                if o.module.as_deref() == Some("js") {
                    return Err(self.js_only(key, name.span));
                }
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
                    if let Some((m, owner)) = self.find_method(ty, key)? {
                        return Ok(Value::Func(self.method_closure(m, owner, obj.clone())));
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

    /// The `js` module needs a JavaScript engine, so it only works in `lipi build` output.
    fn js_only(&self, key: &str, span: Span) -> Flow {
        self.err(
            "LIP3007",
            format!("\"js.{key}\" only works in JavaScript builds"),
            span,
            Some("Build the program with `lipi build` (or `lipi build --target node`) and run the output.".into()),
        )
    }

    fn missing_field(&self, o: &ObjectData, name: &Name) -> Flow {
        let key = name.text.as_str();
        let mut available: Vec<String> = o.fields.borrow().keys().cloned().collect();
        if let Some(ty) = &o.ty {
            for t in self.type_chain(ty).unwrap_or_else(|_| vec![ty.clone()]) {
                for m in &t.decl.methods {
                    if !available.contains(&m.name.text) {
                        available.push(m.name.text.clone());
                    }
                }
            }
        }
        let suggestion = suggest::did_you_mean(key, available.iter().map(String::as_str));
        if o.tag == Some("super") {
            let parent = o.payload.as_ref().and_then(|p| p.downcast_ref::<String>()).cloned().unwrap_or_default();
            return self.err("LIP5004", format!("\"{parent}\" has no method \"{key}\""), name.span, suggestion.or_else(|| Some(format!("Available: {}", available.join(", ")))));
        }
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
                    let all = self.all_fields(ty)?;
                    match all.iter().map(|(t, i)| &t.decl.fields[*i]).find(|f| f.name.text == name.text) {
                        Some(field) => {
                            if let Some(t) = &field.ty {
                                self.check_declared(&v, t, &name.text, value_span)?;
                            }
                        }
                        None => {
                            let names: Vec<&str> = all.iter().map(|(t, i)| t.decl.fields[*i].name.text.as_str()).collect();
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
                        let name = Name { res: Default::default(), text: k.to_string(), span: index_expr.span };
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
                if o.module.as_deref() == Some("js") {
                    return Err(self.js_only(key, name.span));
                }
                let field = o.fields.borrow().get(key).cloned();
                if let Some(f) = field {
                    return self.call_value(f, pos, named, span, None);
                }
                if let Some(ty) = &o.ty {
                    if let Some((m, owner)) = self.find_method(ty, key)? {
                        let c = self.method_closure(m, owner, obj.clone());
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
                let name = Name { res: Default::default(), text: m.1.clone(), span };
                let dummy = Expr { res: Default::default(), kind: ExprKind::Null, span };
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
            if !c.decl.params.iter().any(|p| p.rest) {
                args.truncate(c.decl.params.len());
            }
        }
        self.call_value(f.clone(), args, Vec::new(), span, None)
    }

    fn signature(decl: &FuncDecl) -> String {
        let params: Vec<String> = decl.params.iter().map(|p| if p.rest { format!("...{}", p.name.text) } else { p.name.text.clone() }).collect();
        format!("{}({})", decl.name.text, params.join(", "))
    }

    fn call_function(&mut self, c: &Rc<Closure>, pos: Vec<Value>, named: Vec<(String, Value)>, span: Span) -> Result<Value, Flow> {
        let decl = c.decl.clone();
        // `key:` gives a component its identity on a page; it isn't a parameter.
        let named = if decl.is_component && !decl.params.iter().any(|p| p.name.text == "key") {
            named.into_iter().filter(|(n, _)| n != "key").collect()
        } else {
            named
        };
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
        let rest_at = params.iter().position(|p| p.rest);
        let fixed = rest_at.unwrap_or(params.len());
        if pos.len() > fixed && rest_at.is_none() {
            let n = params.len();
            return Err(self.err(
                "LIP2003",
                format!("{} takes {n} argument{}, but {} were given", fname(), if n == 1 { "" } else { "s" }, pos.len()),
                span,
                Some(format!("It is defined as {}.", Self::signature(&decl))),
            ));
        }
        let mut pos = pos;
        // `...rest` collects the positional arguments after the others.
        let extra = if pos.len() > fixed { pos.split_off(fixed) } else { Vec::new() };
        let mut values: Vec<Option<Value>> = pos.into_iter().map(Some).collect();
        values.resize(params.len(), None);
        if let Some(r) = rest_at {
            values[r] = Some(Value::list(extra));
        }
        for (n, v) in named {
            match params.iter().position(|p| p.name.text == n && !p.rest) {
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

        let env = Env::with_layout(Some(c.env.clone()), EnvKind::Function, &c.layout);
        if let Some(this) = &c.this {
            env.define("self", this.clone());
            if let Some(owner) = c.owner.as_ref().filter(|o| o.decl.parent.is_some()) {
                env.define("super", self.super_object(owner, this)?);
            }
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
                self.declare(&env, &p.name, v, false, p.ty.clone());
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
        let tname = &t.decl.name.text;
        // With `extends`, the parents' fields come first.
        let all = self.all_fields(t)?;
        let field_of = |k: usize| &all[k].0.decl.fields[all[k].1];
        let field_list = || (0..all.len()).map(|k| field_of(k).name.text.as_str()).collect::<Vec<_>>().join(", ");
        if pos.len() > all.len() {
            return Err(self.err(
                "LIP2003",
                format!("\"{tname}\" has {} field{}, but {} values were given", all.len(), if all.len() == 1 { "" } else { "s" }, pos.len()),
                span,
                Some(format!("Its fields are: {}", field_list())),
            ));
        }
        let mut values: Vec<Option<Value>> = pos.into_iter().map(Some).collect();
        values.resize(all.len(), None);
        for (n, v) in named {
            match (0..all.len()).position(|k| field_of(k).name.text == n) {
                Some(i) => values[i] = Some(v),
                None => {
                    let hint = suggest::did_you_mean(&n, (0..all.len()).map(|k| field_of(k).name.text.as_str())).unwrap_or_else(|| format!("Its fields are: {}", field_list()));
                    return Err(self.err("LIP1007", format!("\"{tname}\" has no field \"{n}\""), span, Some(hint)));
                }
            }
        }
        let mut fields = Fields::new();
        for (i, (owner, _)) in all.iter().enumerate() {
            let f = field_of(i);
            let v = match values[i].take() {
                Some(v) => v,
                None => match &f.default {
                    Some(d) => {
                        // A default runs where its type was defined.
                        let prev = std::mem::replace(&mut self.file, owner.file.clone());
                        let r = self.eval(d, &owner.env);
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
            let value = env.get_local(&name);
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

fn int_pair(l: &Value, r: &Value) -> Option<(i64, i64)> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Some((*a, *b)),
        _ => None,
    }
}

/// Arithmetic and comparisons on two Integers (the most common case) without
/// the general dispatch. None means "take the general path", which also
/// reports overflow and division by zero.
#[inline]
fn int_op(op: BinOp, a: i64, b: i64) -> Option<Value> {
    Some(match op {
        BinOp::Add => Value::Int(a.checked_add(b)?),
        BinOp::Sub => Value::Int(a.checked_sub(b)?),
        BinOp::Mul => Value::Int(a.checked_mul(b)?),
        BinOp::Mod if b != 0 => {
            let m = a.wrapping_rem(b);
            Value::Int(if m != 0 && ((m < 0) != (b < 0)) { m + b } else { m })
        }
        BinOp::Lt => Value::Bool(a < b),
        BinOp::Gt => Value::Bool(a > b),
        BinOp::LtEq => Value::Bool(a <= b),
        BinOp::GtEq => Value::Bool(a >= b),
        BinOp::Eq => Value::Bool(a == b),
        BinOp::NotEq => Value::Bool(a != b),
        _ => return None,
    })
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

