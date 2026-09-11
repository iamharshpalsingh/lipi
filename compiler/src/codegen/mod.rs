//! `lipi build`: compiles a LiPi program (and every file it uses) into one
//! JavaScript bundle.
//!
//! The output keeps LiPi's semantics rather than borrowing JavaScript's:
//! values and operators go through the small runtime in `runtime.js`, so
//! Integers stay exact, conditions must be Booleans, `==` compares contents,
//! and errors carry the same codes, messages, hints and source excerpts as
//! `lipi run`. Each error location is a numbered "site" holding the file,
//! line, column and caret width; only the source lines that sites point to
//! are embedded.
//!
//! Variables follow LiPi's scope rule (assignment updates the nearest existing
//! variable, otherwise creates one in the current function), resolved at
//! compile time into ordinary JavaScript `let` bindings.

use crate::ast::*;
use crate::checker;
use crate::diagnostics::{Diagnostic, Severity, Span};
use crate::resolve::{self, Resolved};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The JavaScript runtime included in every bundle.
pub const RUNTIME: &str = include_str!("runtime.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Runs in a browser. Server-only modules are refused (LIP6001).
    Web,
    /// Runs with Node.js, with `fs`, `env` and `process` available.
    Node,
}

impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Target::Web => "web",
            Target::Node => "node",
        }
    }
}

/// Every standard module the interpreter has.
const STD_MODULES: &[&str] = &["math", "json", "fs", "env", "http", "time", "process", "server", "crypto", "database"];
/// Global functions the JavaScript runtime provides.
const JS_GLOBALS: &[&str] = &["toNumber", "toInteger", "toDecimal", "toString", "typeOf", "assert", "assertEqual", "sleep", "all", "timeout"];
/// LiPi UI: pages and elements (web builds only).
pub const UI_ELEMENTS: &[&str] = &[
    "page", "card", "row", "column", "section", "heading", "text", "button", "link", "image", "field", "checkbox", "element", "navigate",
];

fn js_module_available(name: &str, target: Target) -> bool {
    match target {
        Target::Web => matches!(name, "math" | "json" | "http" | "time" | "crypto"),
        Target::Node => matches!(name, "math" | "json" | "http" | "time" | "crypto" | "fs" | "env" | "process"),
    }
}

/// Why a build failed.
#[derive(Debug)]
pub struct BuildError {
    pub diags: Vec<Diagnostic>,
    pub file: String,
    pub source: String,
}

impl BuildError {
    pub fn render(&self, color: bool) -> String {
        self.diags.iter().map(|d| d.render(&self.source, &self.file, color)).collect::<Vec<_>>().join("\n")
    }
}

enum Fail {
    Diag(Diagnostic),
    Build(Box<BuildError>),
}

impl From<Diagnostic> for Fail {
    fn from(d: Diagnostic) -> Self {
        Fail::Diag(d)
    }
}

type R<T> = Result<T, Fail>;

/// Compile `entry` and everything it uses into a JavaScript bundle.
/// `builtins` are the interpreter's global names (so the checker agrees with `lipi run`).
pub fn build(entry: &Path, target: Target, builtins: &[&str]) -> Result<String, BuildError> {
    let shown = entry.to_string_lossy().to_string();
    let source = std::fs::read_to_string(entry).map_err(|e| {
        let hint = if e.kind() == std::io::ErrorKind::NotFound { "Check the file name and the folder you're in.".to_string() } else { e.to_string() };
        BuildError {
            diags: vec![Diagnostic { severity: Severity::Error, code: Some("LIP3001"), message: format!("couldn't read {shown}"), span: None, hint: Some(hint) }],
            file: shown.clone(),
            source: String::new(),
        }
    })?;
    let mut g = Gen {
        target,
        builtins: builtins.iter().map(|s| s.to_string()).collect(),
        builtin_list: builtins.to_vec(),
        root: resolve::find_project_root(entry),
        files: Vec::new(),
        sites: Vec::new(),
        modules: Vec::new(),
        ids: HashMap::new(),
        loading: Vec::new(),
        st: FileState::default(),
    };
    let canon = entry.canonicalize().unwrap_or_else(|_| entry.to_path_buf());
    g.compile_source(canon, shown, source).map_err(|e| *e)?;
    Ok(g.bundle())
}

struct FileInfo {
    name: String,
    source: String,
    /// The source lines that error sites point at.
    lines: BTreeMap<u32, String>,
}

struct Module {
    code: String,
    is_async: bool,
}

/// One JavaScript function scope (a LiPi function body, or a module's top level).
#[derive(Default)]
struct Scope {
    locals: BTreeSet<String>,
    /// Names that always hold a value (parameters, hoisted functions and types).
    no_check: HashSet<String>,
    /// JavaScript parameters, which aren't declared with `let`.
    params: HashSet<String>,
    /// Declared types, checked on every assignment.
    declared: HashMap<String, TypeExpr>,
    module: bool,
    js_async: bool,
    temps: Vec<String>,
    /// `state` variables: assigning one redraws the page.
    states: HashSet<String>,
    /// In a component: the JavaScript variable holding its instance (where its state lives).
    inst: Option<String>,
}

#[derive(Default)]
struct FileState {
    file: usize,
    scopes: Vec<Scope>,
    ind: usize,
    counter: usize,
    module_async: bool,
    /// Functions and types defined before the code around them runs.
    hoisted: HashSet<usize>,
}

struct Gen<'a> {
    target: Target,
    builtins: HashSet<String>,
    builtin_list: Vec<&'a str>,
    root: PathBuf,
    files: Vec<FileInfo>,
    sites: Vec<String>,
    modules: Vec<Option<Module>>,
    ids: HashMap<PathBuf, usize>,
    loading: Vec<PathBuf>,
    st: FileState,
}

fn var(name: &str) -> String {
    format!("v_{name}")
}

/// A JavaScript string literal.
pub fn js_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn type_js(t: &TypeExpr) -> String {
    match &t.kind {
        TypeKind::Named(n) => js_str(n),
        TypeKind::List(inner) => format!("{{list:{}}}", type_js(inner)),
        TypeKind::Optional(inner) => format!("{{opt:{}}}", type_js(inner)),
    }
}

fn line_start_offset(source: &str, line: u32) -> usize {
    if line <= 1 {
        return 0;
    }
    let mut current = 1;
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            current += 1;
            if current == line {
                return i + 1;
            }
        }
    }
    source.len()
}

// ----- names each function defines --------------------------------------------------

#[derive(Default)]
struct Names {
    /// Always local: parameters of `for`, `catch` and `use`, functions, types, typed and constant variables.
    defined: Vec<(String, Option<TypeExpr>)>,
    /// Plain assignments: local unless an enclosing scope has the name.
    assigned: Vec<String>,
    /// Functions and types at the top of the body (defined before it runs).
    hoisted: Vec<String>,
    /// `state` variables.
    states: Vec<String>,
}

fn collect_block(body: &[Stmt], n: &mut Names, top: bool) {
    for stmt in body {
        collect_stmt(stmt, n, top);
    }
}

fn collect_stmt(stmt: &Stmt, n: &mut Names, top: bool) {
    match &stmt.kind {
        StmtKind::Assign { target: Target_::Name(name), ty, constant, .. } => {
            if *constant || ty.is_some() {
                n.defined.push((name.text.clone(), ty.clone()));
            } else {
                n.assigned.push(name.text.clone());
            }
        }
        StmtKind::If { branches, otherwise } => {
            for (_, b) in branches {
                collect_block(b, n, false);
            }
            if let Some(b) = otherwise {
                collect_block(b, n, false);
            }
        }
        StmtKind::While { body, .. } | StmtKind::Repeat { body, .. } => collect_block(body, n, false),
        StmtKind::For { first, second, body, .. } => {
            n.defined.push((first.text.clone(), None));
            if let Some(s) = second {
                n.defined.push((s.text.clone(), None));
            }
            collect_block(body, n, false);
        }
        StmtKind::Func(f) | StmtKind::Component(f) => {
            n.defined.push((f.name.text.clone(), None));
            if top {
                n.hoisted.push(f.name.text.clone());
            }
        }
        StmtKind::State { name, ty, .. } => {
            n.defined.push((name.text.clone(), ty.clone()));
            n.states.push(name.text.clone());
        }
        StmtKind::TypeDef(t) => {
            n.defined.push((t.name.text.clone(), None));
            if top {
                n.hoisted.push(t.name.text.clone());
            }
        }
        StmtKind::Try { body, catch, finally } => {
            collect_block(body, n, false);
            if let Some((name, handler)) = catch {
                if let Some(name) = name {
                    n.defined.push((name.text.clone(), None));
                }
                collect_block(handler, n, false);
            }
            if let Some(f) = finally {
                collect_block(f, n, false);
            }
        }
        StmtKind::Match { arms, otherwise, .. } => {
            for arm in arms {
                collect_block(&arm.body, n, false);
            }
            if let Some(b) = otherwise {
                collect_block(b, n, false);
            }
        }
        StmtKind::Use { source, alias, names } => match names {
            Some(names) => n.defined.extend(names.iter().map(|x| (x.text.clone(), None))),
            None => n.defined.push((alias.as_ref().map(|a| a.text.clone()).unwrap_or_else(|| checker::module_binding_name(source)), None)),
        },
        StmtKind::Export { inner: Some(inner), .. } => collect_stmt(inner, n, top),
        _ => {}
    }
}

use crate::ast::Target as Target_;

// ----- does a body use `await` directly? ----------------------------------------------

fn block_awaits(body: &[Stmt]) -> bool {
    body.iter().any(stmt_awaits)
}

fn stmt_awaits(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::Return(Some(e)) => expr_awaits(e),
        StmtKind::Show(values) => values.iter().any(expr_awaits),
        StmtKind::Assign { target, value, .. } => {
            expr_awaits(value)
                || match target {
                    Target_::Name(_) => false,
                    Target_::Field(o, _) => expr_awaits(o),
                    Target_::Index(o, i) => expr_awaits(o) || expr_awaits(i),
                }
        }
        StmtKind::If { branches, otherwise } => branches.iter().any(|(c, b)| expr_awaits(c) || block_awaits(b)) || otherwise.as_deref().is_some_and(block_awaits),
        StmtKind::While { cond, body } => expr_awaits(cond) || block_awaits(body),
        StmtKind::Repeat { count, body } => expr_awaits(count) || block_awaits(body),
        StmtKind::For { iter, body, .. } => expr_awaits(iter) || block_awaits(body),
        StmtKind::Try { body, catch, finally } => block_awaits(body) || catch.as_ref().is_some_and(|(_, b)| block_awaits(b)) || finally.as_deref().is_some_and(block_awaits),
        StmtKind::Match { subject, arms, otherwise } => {
            expr_awaits(subject)
                || arms.iter().any(|a| a.patterns.iter().any(expr_awaits) || a.guard.as_ref().is_some_and(expr_awaits) || block_awaits(&a.body))
                || otherwise.as_deref().is_some_and(block_awaits)
        }
        StmtKind::Export { inner: Some(inner), .. } => stmt_awaits(inner),
        _ => false,
    }
}

fn expr_awaits(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Await(_) => true,
        ExprKind::Lambda(_) => false,
        ExprKind::Template(parts) => parts.iter().any(|p| matches!(p, TemplatePart::Expr(x) if expr_awaits(x))),
        ExprKind::List(items) => items.iter().any(expr_awaits),
        ExprKind::Object(fields) => fields.iter().any(|(_, v)| expr_awaits(v)),
        ExprKind::Unary(_, x) => expr_awaits(x),
        ExprKind::Binary(_, a, b) | ExprKind::And(a, b) | ExprKind::Or(a, b) | ExprKind::Coalesce(a, b) => expr_awaits(a) || expr_awaits(b),
        ExprKind::IfElse { cond, then, otherwise } => expr_awaits(cond) || expr_awaits(then) || expr_awaits(otherwise),
        ExprKind::Range { start, end, step } => expr_awaits(start) || expr_awaits(end) || step.as_deref().is_some_and(expr_awaits),
        ExprKind::Call { callee, args } => expr_awaits(callee) || args.iter().any(|a| expr_awaits(&a.value)),
        ExprKind::Field { object, .. } => expr_awaits(object),
        ExprKind::Index { object, index } => expr_awaits(object) || expr_awaits(index),
        _ => false,
    }
}

/// Comparisons and logic always produce Booleans (or throw), so conditions made
/// of them don't need a runtime check.
fn always_bool(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Bool(_) | ExprKind::And(..) | ExprKind::Or(..) | ExprKind::Unary(UnaryOp::Not, _) => true,
        ExprKind::Binary(op, ..) => !op.is_arithmetic(),
        _ => false,
    }
}

impl Gen<'_> {
    // ----- files and modules ----------------------------------------------------------

    fn compile_source(&mut self, canon: PathBuf, shown: String, source: String) -> Result<usize, Box<BuildError>> {
        let fail = |diags: Vec<Diagnostic>| Box::new(BuildError { diags, file: shown.clone(), source: source.clone() });
        let program = crate::parse_source(&source).map_err(|d| fail(vec![d]))?;
        let mut errors: Vec<Diagnostic> = checker::check(&program, &self.builtin_list).into_iter().filter(|d| d.severity == Severity::Error).collect();
        if !errors.is_empty() {
            // Like `lipi run`: every error in the main file, the first one in a module.
            if !self.files.is_empty() {
                errors.truncate(1);
            }
            return Err(fail(errors));
        }
        let id = self.files.len();
        self.files.push(FileInfo { name: shown.clone(), source: source.clone(), lines: BTreeMap::new() });
        self.modules.push(None);
        self.ids.insert(canon.clone(), id);
        self.loading.push(canon);
        let saved = std::mem::replace(&mut self.st, FileState { file: id, ..FileState::default() });
        let result = self.module(&program, &shown);
        self.st = saved;
        self.loading.pop();
        match result {
            Ok(m) => {
                self.modules[id] = Some(m);
                Ok(id)
            }
            Err(Fail::Diag(d)) => Err(fail(vec![d])),
            Err(Fail::Build(b)) => Err(b),
        }
    }

    fn module(&mut self, program: &Program, shown: &str) -> R<Module> {
        let scope = self.new_scope(&program.body, &[], false, true, true);
        self.st.scopes.push(scope);
        self.st.ind = 2;
        let mut body = String::new();
        self.hoist(&program.body, &mut body)?;
        self.block(&program.body, &mut body)?;
        let scope = self.st.scopes.pop().unwrap_or_default();
        let mut exports: Vec<String> = Vec::new();
        for stmt in &program.body {
            if let StmtKind::Export { names, .. } = &stmt.kind {
                for n in names {
                    if !exports.contains(&n.text) {
                        exports.push(n.text.clone());
                    }
                }
            }
        }
        let stem = Path::new(shown).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let is_async = self.st.module_async;
        let mut code = format!("$rt.mods[{}] = {}function () {{\n", self.st.file, if is_async { "async " } else { "" });
        code.push_str(&self.prologue(&scope, 1));
        code.push_str("  $body: {\n");
        code.push_str(&body);
        code.push_str("  }\n");
        let pairs: Vec<String> = exports.iter().map(|n| format!("[{}, {}]", js_str(n), var(n))).collect();
        code.push_str(&format!("  return $module({}, [{}]);\n}};\n", js_str(&stem), pairs.join(", ")));
        Ok(Module { code, is_async })
    }

    fn prologue(&self, scope: &Scope, ind: usize) -> String {
        let mut names: Vec<String> = scope.locals.iter().filter(|n| !scope.params.contains(*n)).map(|n| var(n)).collect();
        names.extend(scope.temps.iter().cloned());
        if names.is_empty() {
            String::new()
        } else {
            format!("{}let {};\n", "  ".repeat(ind), names.join(", "))
        }
    }

    fn bundle(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("// Built by `lipi build` (target: {}). Edit the .lipi files, not this one.\n", self.target.name()));
        out.push_str("(function () {\n");
        out.push_str(RUNTIME);
        out.push_str(&format!("\n$rt.target = {};\n", js_str(self.target.name())));
        if self.target == Target::Node {
            out.push_str("installNode();\n");
        }
        out.push_str("$rt.files = [\n");
        for f in &self.files {
            let lines: Vec<String> = f.lines.iter().map(|(n, l)| format!("{n}: {}", js_str(l))).collect();
            out.push_str(&format!("  {{name: {}, lines: {{{}}}}},\n", js_str(&f.name), lines.join(", ")));
        }
        out.push_str("];\n$rt.sites = [\n");
        for chunk in self.sites.chunks(8) {
            out.push_str("  ");
            out.push_str(&chunk.join(", "));
            out.push_str(",\n");
        }
        out.push_str("];\n");
        for m in self.modules.iter().flatten() {
            out.push_str(&m.code);
        }
        out.push_str("$start(0);\n})();\n");
        out
    }

    // ----- scopes and names ----------------------------------------------------------------

    fn new_scope(&mut self, body: &[Stmt], params: &[Param], method: bool, module: bool, js_async: bool) -> Scope {
        let mut names = Names::default();
        collect_block(body, &mut names, true);
        let mut s = Scope { module, js_async, ..Scope::default() };
        let add_param = |s: &mut Scope, name: &str| {
            s.locals.insert(name.to_string());
            s.no_check.insert(name.to_string());
            s.params.insert(name.to_string());
        };
        if method {
            add_param(&mut s, "self");
        }
        for p in params {
            add_param(&mut s, &p.name.text);
            if let Some(t) = &p.ty {
                s.declared.insert(p.name.text.clone(), t.clone());
            }
        }
        for (name, ty) in names.defined {
            s.locals.insert(name.clone());
            if let Some(t) = ty {
                s.declared.insert(name, t);
            }
        }
        for name in names.hoisted {
            s.no_check.insert(name);
        }
        s.states.extend(names.states);
        for name in names.assigned {
            if !s.locals.contains(&name) && !self.visible(&name) {
                s.locals.insert(name);
            }
        }
        s
    }

    fn visible(&self, name: &str) -> bool {
        self.st.scopes.iter().any(|s| s.locals.contains(name))
    }

    fn scope(&mut self) -> &mut Scope {
        self.st.scopes.last_mut().expect("inside a scope")
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.st.counter += 1;
        format!("{prefix}{}", self.st.counter)
    }

    fn temp(&mut self) -> String {
        let t = self.fresh("$t");
        self.scope().temps.push(t.clone());
        t
    }

    fn declared_type(&self, name: &str) -> Option<TypeExpr> {
        let scope = self.st.scopes.iter().rev().find(|s| s.locals.contains(name))?;
        scope.declared.get(name).cloned()
    }

    fn read_name(&mut self, name: &str, span: Span) -> R<String> {
        let found = self.st.scopes.iter().rev().find(|s| s.locals.contains(name)).map(|s| s.no_check.contains(name));
        match found {
            Some(true) => Ok(var(name)),
            Some(false) => {
                let s = self.undefined_site(name, span);
                let v = var(name);
                Ok(format!("({v} === undefined ? $u({s}) : {v})"))
            }
            None if self.builtins.contains(name) => self.global(name, span),
            None => Ok(format!("$u({})", self.undefined_site(name, span))),
        }
    }

    fn undefined_site(&mut self, name: &str, span: Span) -> usize {
        let mut candidates: Vec<String> = self.st.scopes.iter().flat_map(|s| s.locals.iter().cloned()).collect();
        candidates.extend(self.builtin_list.iter().map(|s| s.to_string()));
        let d = checker::unknown_name(name, span, candidates.iter().map(String::as_str));
        let extra = format!(",m:{},h:{}", js_str(&d.message), js_str(d.hint.as_deref().unwrap_or("")));
        self.site(span, &extra)
    }

    /// After assigning a `state` variable: the statement that tells the page to redraw.
    fn state_notify(&self, name: &str) -> Option<String> {
        let scope = self.st.scopes.iter().rev().find(|s| s.locals.contains(name))?;
        if !scope.states.contains(name) {
            return None;
        }
        Some(match &scope.inst {
            Some(i) => format!("{i}.put({}, {});", js_str(name), var(name)),
            None => "$rt.changed();".to_string(),
        })
    }

    fn global(&mut self, name: &str, span: Span) -> R<String> {
        if UI_ELEMENTS.contains(&name) {
            return match self.target {
                Target::Web => Ok(format!("$g.{name}")),
                Target::Node => Err(self.unavailable(name, span).into()),
            };
        }
        if JS_GLOBALS.contains(&name) || (STD_MODULES.contains(&name) && js_module_available(name, self.target)) {
            return Ok(format!("$g.{name}"));
        }
        Err(self.unavailable(name, span).into())
    }

    fn unavailable(&self, name: &str, span: Span) -> Diagnostic {
        match self.target {
            Target::Web => Diagnostic::error(format!("\"{name}\" only works on the server, not in browser code"), span).with_code("LIP6001").with_hint(
                "Browser code can't reach files, environment variables, databases or the server, so secrets stay on the server. \
                 Do this in server code (lipi run) and fetch the result with http.",
            ),
            Target::Node if UI_ELEMENTS.contains(&name) => Diagnostic::error(format!("\"{name}\" draws web pages, so it only works in web builds"), span)
                .with_code("LIP3006")
                .with_hint("Build without --target node; the web target is the default."),
            Target::Node => Diagnostic::error(format!("\"{name}\" isn't available in JavaScript builds yet"), span)
                .with_code("LIP3006")
                .with_hint("Run this program with `lipi run` instead."),
        }
    }

    // ----- sites -------------------------------------------------------------------------

    /// Record an error location and return its number. `extra` adds fields such as `,n:"x"`.
    fn site(&mut self, span: Span, extra: &str) -> usize {
        let file = self.st.file;
        let f = &mut self.files[file];
        let src = &f.source;
        let line_start = line_start_offset(src, span.line).min(src.len());
        let line_text = src[line_start..].split('\n').next().unwrap_or("").trim_end_matches('\r');
        let line_end = line_start + line_text.len();
        let from = span.start.clamp(line_start, line_end);
        let to = span.end.clamp(from, line_end);
        let p = src[line_start..from].replace('\t', "    ").chars().count();
        let w = src[from..to].chars().count().max(1);
        if !f.lines.contains_key(&span.line) {
            let text = line_text.to_string();
            f.lines.insert(span.line, text);
        }
        self.sites.push(format!("{{f:{file},l:{},c:{},p:{p},w:{w}{extra}}}", span.line, span.col));
        self.sites.len() - 1
    }

    fn site_expr(&mut self, e: &Expr) -> usize {
        let extra = match &e.kind {
            ExprKind::Ident(n) => format!(",n:{}", js_str(n)),
            _ => String::new(),
        };
        self.site(e.span, &extra)
    }

    /// For "cannot read .x of null" hints: the name of the object, if it's a variable.
    fn obj_hint(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Ident(n) => format!(",o:{}", js_str(n)),
            _ => String::new(),
        }
    }

    /// A binary operation's site: the whole expression, plus its operands.
    fn bin_site(&mut self, left: Span, left_extra: &str, right: &Expr) -> usize {
        let l = self.site(left, left_extra);
        let r = self.site_expr(right);
        self.site(left.to(right.span), &format!(",L:{l},R:{r}"))
    }

    // ----- statements --------------------------------------------------------------------

    fn line(&self, out: &mut String, text: &str) {
        for _ in 0..self.st.ind {
            out.push_str("  ");
        }
        out.push_str(text);
        out.push('\n');
    }

    fn nested(&mut self, body: &[Stmt], out: &mut String) -> R<()> {
        self.st.ind += 1;
        let r = self.block(body, out);
        self.st.ind -= 1;
        r
    }

    fn block(&mut self, body: &[Stmt], out: &mut String) -> R<()> {
        for stmt in body {
            self.stmt(stmt, out)?;
        }
        Ok(())
    }

    /// Define a body's functions and types before the rest of it runs.
    fn hoist(&mut self, body: &[Stmt], out: &mut String) -> R<()> {
        for stmt in body {
            let stmt = match &stmt.kind {
                StmtKind::Export { inner: Some(inner), .. } => inner,
                _ => stmt,
            };
            match &stmt.kind {
                StmtKind::Func(f) | StmtKind::Component(f) => {
                    self.st.hoisted.insert(Rc::as_ptr(f) as usize);
                    let js = self.func(f, false, matches!(stmt.kind, StmtKind::Component(_)))?;
                    self.line(out, &format!("{} = {js};", var(&f.name.text)));
                }
                StmtKind::TypeDef(t) => {
                    self.st.hoisted.insert(Rc::as_ptr(t) as usize);
                    let js = self.type_def(t)?;
                    self.line(out, &format!("{} = {js};", var(&t.name.text)));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn cond(&mut self, e: &Expr) -> R<String> {
        let js = self.expr(e)?;
        if always_bool(e) {
            return Ok(js);
        }
        let s = self.site(e.span, "");
        Ok(format!("$bool({js}, {s})"))
    }

    fn stmt(&mut self, stmt: &Stmt, out: &mut String) -> R<()> {
        match &stmt.kind {
            StmtKind::Expr(e) => {
                let js = self.expr(e)?;
                self.line(out, &format!("{js};"));
            }
            StmtKind::Show(values) => {
                // Each value becomes text as soon as it's evaluated (like `lipi run`),
                // so a later argument that changes an Array doesn't change what's shown.
                let parts = values.iter().map(|v| self.expr(v).map(|js| format!("display({js})"))).collect::<R<Vec<_>>>()?;
                self.line(out, &format!("$show([{}]);", parts.join(", ")));
            }
            StmtKind::Assign { target, op, ty, value, constant } => self.assign(target, *op, ty.as_ref(), value, *constant, out)?,
            StmtKind::If { branches, otherwise } => {
                for (i, (c, body)) in branches.iter().enumerate() {
                    let cond = self.cond(c)?;
                    self.line(out, &format!("{}if ({cond}) {{", if i == 0 { "" } else { "} else " }));
                    self.nested(body, out)?;
                }
                if let Some(body) = otherwise {
                    self.line(out, "} else {");
                    self.nested(body, out)?;
                }
                self.line(out, "}");
            }
            StmtKind::While { cond, body } => {
                let c = self.cond(cond)?;
                self.line(out, &format!("while ({c}) {{"));
                self.nested(body, out)?;
                self.line(out, "}");
            }
            StmtKind::Repeat { count, body } => {
                let c = self.expr(count)?;
                let s = self.site_expr(count);
                let r = self.fresh("$r");
                self.line(out, &format!("for (let {r} = $count({c}, {s}); {r} > 0; {r}--) {{"));
                self.nested(body, out)?;
                self.line(out, "}");
            }
            StmtKind::For { first, second, iter, body } => self.for_loop(first, second.as_ref(), iter, body, out)?,
            StmtKind::Func(f) | StmtKind::Component(f) => {
                if !self.st.hoisted.contains(&(Rc::as_ptr(f) as usize)) {
                    let js = self.func(f, false, matches!(stmt.kind, StmtKind::Component(_)))?;
                    self.line(out, &format!("{} = {js};", var(&f.name.text)));
                }
            }
            StmtKind::State { name, ty, value } => {
                let mut v = self.expr(value)?;
                if let Some(t) = ty {
                    let s = self.site(value.span, "");
                    v = format!("$chk({v}, {}, {}, {s})", type_js(t), js_str(&name.text));
                }
                let target = var(&name.text);
                match self.scope().inst.clone() {
                    // Component state: created on the first draw, then kept.
                    Some(i) => self.line(out, &format!("{target} = {i}.init({}, () => {v});", js_str(&name.text))),
                    None => self.line(out, &format!("{target} = {v};")),
                }
            }
            StmtKind::TypeDef(t) => {
                if !self.st.hoisted.contains(&(Rc::as_ptr(t) as usize)) {
                    let js = self.type_def(t)?;
                    self.line(out, &format!("{} = {js};", var(&t.name.text)));
                }
            }
            StmtKind::Return(value) => {
                let module = self.scope().module;
                match (value, module) {
                    (Some(v), false) => {
                        let js = self.expr(v)?;
                        self.line(out, &format!("return {js};"));
                    }
                    (None, false) => self.line(out, "return null;"),
                    (Some(v), true) => {
                        let js = self.expr(v)?;
                        self.line(out, &format!("{js};"));
                        self.line(out, "break $body;");
                    }
                    (None, true) => self.line(out, "break $body;"),
                }
            }
            StmtKind::Break => self.line(out, "break;"),
            StmtKind::Continue => self.line(out, "continue;"),
            StmtKind::Throw(e) => {
                let js = self.expr(e)?;
                let s = self.site(stmt.span, "");
                self.line(out, &format!("throw $throw({js}, {s});"));
            }
            StmtKind::Try { body, catch, finally } => {
                self.line(out, "try {");
                self.nested(body, out)?;
                if let Some((name, handler)) = catch {
                    let e = self.fresh("$e");
                    self.line(out, &format!("}} catch ({e}) {{"));
                    self.st.ind += 1;
                    match name {
                        Some(n) => self.line(out, &format!("{} = $caught({e});", var(&n.text))),
                        None => self.line(out, &format!("$caught({e});")),
                    }
                    self.st.ind -= 1;
                    self.nested(handler, out)?;
                }
                if let Some(f) = finally {
                    self.line(out, "} finally {");
                    self.nested(f, out)?;
                } else if catch.is_none() {
                    self.line(out, "} finally {");
                }
                self.line(out, "}");
            }
            StmtKind::Match { subject, arms, otherwise } => {
                let subj = self.expr(subject)?;
                let m = self.fresh("$m");
                self.line(out, "{");
                self.st.ind += 1;
                self.line(out, &format!("const {m} = {subj};"));
                for (i, arm) in arms.iter().enumerate() {
                    let mut pats = Vec::new();
                    for p in &arm.patterns {
                        pats.push(match &p.kind {
                            ExprKind::Ident(n) if n == "_" => "true".to_string(),
                            ExprKind::Range { start, end, step: None } => {
                                let (a, b) = (self.expr(start)?, self.expr(end)?);
                                format!("$inr({m}, {a}, {b})")
                            }
                            _ => format!("eq({m}, {})", self.expr(p)?),
                        });
                    }
                    let mut cond = if pats.len() == 1 { pats.remove(0) } else { format!("({})", pats.join(" || ")) };
                    if let Some(g) = &arm.guard {
                        cond = format!("{cond} && {}", self.cond(g)?);
                    }
                    self.line(out, &format!("{}if ({cond}) {{", if i == 0 { "" } else { "} else " }));
                    self.nested(&arm.body, out)?;
                }
                if let Some(body) = otherwise {
                    if arms.is_empty() {
                        self.line(out, "{");
                    } else {
                        self.line(out, "} else {");
                    }
                    self.nested(body, out)?;
                }
                if !arms.is_empty() || otherwise.is_some() {
                    self.line(out, "}");
                }
                self.st.ind -= 1;
                self.line(out, "}");
            }
            StmtKind::Use { source, alias, names } => self.use_stmt(source, alias.as_ref(), names.as_deref(), stmt.span, out)?,
            StmtKind::Export { inner, .. } => {
                if let Some(inner) = inner {
                    self.stmt(inner, out)?;
                }
            }
            StmtKind::Test { .. } => {}
        }
        Ok(())
    }

    fn for_loop(&mut self, first: &Name, second: Option<&Name>, iter: &Expr, body: &[Stmt], out: &mut String) -> R<()> {
        let (k, v) = (self.fresh("$k"), self.fresh("$v"));
        self.line(out, "{");
        self.st.ind += 1;
        if let ExprKind::Range { start, end, step } = &iter.kind {
            let (a, b) = (self.expr(start)?, self.expr(end)?);
            let st = match step {
                Some(s) => self.expr(s)?,
                None => "null".into(),
            };
            let (sa, sb) = (self.site_expr(start), self.site_expr(end));
            let ss = match step {
                Some(s) => self.site_expr(s).to_string(),
                None => "null".into(),
            };
            let (from, to, by) = (self.fresh("$a"), self.fresh("$b"), self.fresh("$s"));
            self.line(out, &format!("const [{from}, {to}, {by}] = $rangeParts({a}, {b}, {st}, {sa}, {sb}, {ss});"));
            self.line(out, &format!("for (let {v} = {from}, {k} = 0; {by} > 0 ? {v} <= {to} : {v} >= {to}; {v} += {by}, {k}++) {{"));
            self.st.ind += 1;
            match second {
                Some(s) => {
                    self.line(out, &format!("{} = {k};", var(&first.text)));
                    self.line(out, &format!("{} = {v};", var(&s.text)));
                }
                None => self.line(out, &format!("{} = {v};", var(&first.text))),
            }
        } else {
            let c = self.expr(iter)?;
            let s = self.site(iter.span, "");
            let p = self.fresh("$p");
            self.line(out, &format!("const {p} = $pairs({c}, {s});"));
            self.line(out, &format!("for (const [{k}, {v}] of {p}.items) {{"));
            self.st.ind += 1;
            match second {
                Some(s) => {
                    self.line(out, &format!("{} = {k};", var(&first.text)));
                    self.line(out, &format!("{} = {v};", var(&s.text)));
                }
                None => self.line(out, &format!("{} = {p}.keyed ? {k} : {v};", var(&first.text))),
            }
        }
        self.block(body, out)?;
        self.st.ind -= 1;
        self.line(out, "}");
        self.st.ind -= 1;
        self.line(out, "}");
        Ok(())
    }

    fn assign(&mut self, target: &Target_, op: Option<BinOp>, ty: Option<&TypeExpr>, value: &Expr, constant: bool, out: &mut String) -> R<()> {
        match target {
            Target_::Name(n) => {
                let mut v = self.expr(value)?;
                if let Some(op) = op {
                    let current = self.read_name(&n.text, n.span)?;
                    let s = self.bin_site(n.span, &format!(",n:{}", js_str(&n.text)), value);
                    v = format!("$bin({}, {current}, {v}, {s})", js_str(op.symbol()));
                }
                let declared = if constant || ty.is_some() { ty.cloned() } else { self.declared_type(&n.text) };
                if let Some(t) = declared {
                    let s = self.site(value.span, "");
                    v = format!("$chk({v}, {}, {}, {s})", type_js(&t), js_str(&n.text));
                }
                self.line(out, &format!("{} = {v};", var(&n.text)));
                if let Some(notify) = self.state_notify(&n.text) {
                    self.line(out, &notify);
                }
            }
            Target_::Field(obj, name) => {
                let o = self.expr(obj)?;
                let vs = self.site(value.span, "");
                let s = self.site(name.span, &format!("{},V:{vs}", Self::obj_hint(obj)));
                let key = js_str(&name.text);
                match op {
                    None => {
                        let v = self.expr(value)?;
                        self.line(out, &format!("$set({o}, {key}, {v}, {s});"));
                    }
                    Some(op) => {
                        let t = self.temp();
                        let gs = self.site(name.span, &Self::obj_hint(obj));
                        let bs = self.bin_site(name.span, &format!(",n:{key}"), value);
                        let v = self.expr(value)?;
                        self.line(out, &format!("{t} = {o};"));
                        self.line(out, &format!("$set({t}, {key}, $bin({}, $get({t}, {key}, {gs}, false), {v}, {bs}), {s});", js_str(op.symbol())));
                    }
                }
            }
            Target_::Index(obj, index) => {
                let o = self.expr(obj)?;
                let i = self.expr(index)?;
                let extra = match &index.kind {
                    ExprKind::Ident(n) => format!(",n:{}", js_str(n)),
                    _ => String::new(),
                };
                let s = self.site(index.span, &format!("{extra}{}", Self::obj_hint(obj)));
                match op {
                    None => {
                        let v = self.expr(value)?;
                        self.line(out, &format!("$seti({o}, {i}, {v}, {s});"));
                    }
                    Some(op) => {
                        let (t1, t2) = (self.temp(), self.temp());
                        let bs = self.bin_site(index.span, &extra, value);
                        let v = self.expr(value)?;
                        self.line(out, &format!("{t1} = {o};"));
                        self.line(out, &format!("{t2} = {i};"));
                        self.line(out, &format!("$seti({t1}, {t2}, $bin({}, $idx({t1}, {t2}, {s}), {v}, {bs}), {s});", js_str(op.symbol())));
                    }
                }
            }
        }
        Ok(())
    }

    fn use_stmt(&mut self, source: &str, alias: Option<&Name>, names: Option<&[Name]>, span: Span, out: &mut String) -> R<()> {
        let current = self.files[self.st.file].name.clone();
        let module = match resolve::resolve_use(source, &current, &self.root, STD_MODULES) {
            Ok(Resolved::Std(name)) => {
                if !js_module_available(&name, self.target) {
                    return Err(self.unavailable(&name, span).into());
                }
                format!("$g.{name}")
            }
            Ok(Resolved::File(path)) => {
                let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
                let id = match self.ids.get(&canon) {
                    Some(&id) if self.modules[id].is_some() => id,
                    _ if self.loading.contains(&canon) => {
                        return Err(Diagnostic::error(format!("circular use: \"{}\" is already being loaded", path.to_string_lossy()), span)
                            .with_code("LIP3002")
                            .with_hint("Two files use each other. Move the shared code into a third file that both use.")
                            .into())
                    }
                    _ => {
                        let Ok(text) = std::fs::read_to_string(&path) else {
                            return Err(Diagnostic::error(format!("couldn't read \"{}\"", path.to_string_lossy()), span).with_code("LIP3001").into());
                        };
                        let saved_ind = self.st.ind;
                        let r = self.compile_source(canon, path.to_string_lossy().to_string(), text);
                        self.st.ind = saved_ind;
                        r.map_err(Fail::Build)?
                    }
                };
                let is_async = self.modules[id].as_ref().is_some_and(|m| m.is_async);
                if is_async {
                    self.note_await(span, "this module uses `await` at its top level, so it can only be used at the top level of a file or inside an async function")?;
                    format!("(await $use({id}))")
                } else {
                    format!("$use({id})")
                }
            }
            Err(e) => return Err(Diagnostic::error(e.message, span).with_code(e.code).with_hint(e.hint).into()),
        };
        match names {
            Some(names) => {
                let m = self.fresh("$m");
                self.line(out, "{");
                self.st.ind += 1;
                self.line(out, &format!("const {m} = {module};"));
                for n in names {
                    let s = self.site(n.span, "");
                    self.line(out, &format!("{} = $exp({m}, {}, {}, {s});", var(&n.text), js_str(&n.text), js_str(source)));
                }
                self.st.ind -= 1;
                self.line(out, "}");
            }
            None => {
                let bound = alias.map(|a| a.text.clone()).unwrap_or_else(|| checker::module_binding_name(source));
                self.line(out, &format!("{} = {module};", var(&bound)));
            }
        }
        Ok(())
    }

    /// `await` is about to be emitted: make sure the surrounding JavaScript function is async.
    fn note_await(&mut self, span: Span, why: &str) -> R<()> {
        let scope = self.scope();
        if scope.module {
            self.st.module_async = true;
            return Ok(());
        }
        if scope.js_async {
            return Ok(());
        }
        Err(Diagnostic::error(why, span).with_code("LIP4001").with_hint("Mark the function with `async`.").into())
    }

    // ----- functions and types -----------------------------------------------------------

    fn func(&mut self, f: &FuncDecl, method: bool, component: bool) -> R<String> {
        let js_async = f.is_async || (f.is_lambda && block_awaits(&f.body));
        if component && (f.is_async || block_awaits(&f.body)) {
            return Err(Diagnostic::error("components draw right away, so they can't use `await`", f.name.span)
                .with_code("LIP4001")
                .with_hint("Load data in top-level code or in a button's block, keep it in `state`, and draw it here.")
                .into());
        }
        let mut scope = self.new_scope(&f.body, &f.params, method, false, js_async);
        let inst = if component {
            let i = self.fresh("$inst");
            scope.inst = Some(i.clone());
            Some(i)
        } else {
            None
        };
        let saved_hoisted = std::mem::take(&mut self.st.hoisted);
        self.st.scopes.push(scope);
        self.st.ind += 1;
        let mut body = String::new();
        let result = (|| -> R<()> {
            for p in &f.params {
                if let Some(d) = &p.default {
                    let v = var(&p.name.text);
                    let mut js = self.expr(d)?;
                    if let Some(t) = &p.ty {
                        let s = self.site(d.span, "");
                        js = format!("$chk({js}, {}, {}, {s})", type_js(t), js_str(&p.name.text));
                    }
                    self.line(&mut body, &format!("if ({v} === undefined) {v} = {js};"));
                }
            }
            self.hoist(&f.body, &mut body)?;
            self.block(&f.body, &mut body)?;
            self.line(&mut body, "return null;");
            Ok(())
        })();
        self.st.ind -= 1;
        let scope = self.st.scopes.pop().unwrap_or_default();
        self.st.hoisted = saved_hoisted;
        result?;

        let params: Vec<String> = f.params.iter().map(|p| format!("[{},{},{}]", js_str(&p.name.text), p.default.is_some(), p.ty.as_ref().map_or("null".into(), type_js))).collect();
        let (ret, ret_site) = match &f.ret {
            Some(t) => (type_js(t), self.site(t.span, "").to_string()),
            None => ("null".into(), "null".into()),
        };
        let meta = format!("{{n:{},p:[{}],r:{ret},rs:{ret_site},a:{js_async},l:{}}}", js_str(&f.name.text), params.join(","), f.is_lambda);
        let mut js_params: Vec<String> = Vec::new();
        if method {
            js_params.push(var("self"));
        }
        js_params.extend(f.params.iter().map(|p| var(&p.name.text)));
        let mut code = format!("$fn({meta}, {}({}) => {{\n", if js_async { "async " } else { "" }, js_params.join(", "));
        code.push_str(&self.prologue(&scope, self.st.ind + 1));
        let pad = "  ".repeat(self.st.ind + 1);
        match inst {
            // A component draws inside its own instance, which keeps its state between draws.
            Some(i) => {
                let s = self.site(f.name.span, "");
                code.push_str(&format!("{pad}const {i} = $ui.enter({}, {s});\n{pad}try {{\n", js_str(&f.name.text)));
                code.push_str(&body);
                code.push_str(&format!("{pad}}} finally {{\n{pad}  $ui.leave();\n{pad}}}\n"));
            }
            None => code.push_str(&body),
        }
        code.push_str(&"  ".repeat(self.st.ind));
        code.push_str("})");
        Ok(code)
    }

    fn type_def(&mut self, t: &TypeDecl) -> R<String> {
        let mut fields = Vec::new();
        for f in &t.fields {
            let default = match &f.default {
                Some(d) => format!("() => {}", self.expr(d)?),
                None => "null".into(),
            };
            let optional = matches!(f.ty.as_ref().map(|t| &t.kind), Some(TypeKind::Optional(_)));
            fields.push(format!("{{n:{},t:{},d:{default},o:{optional}}}", js_str(&f.name.text), f.ty.as_ref().map_or("null".into(), type_js)));
        }
        let mut methods = Vec::new();
        for m in &t.methods {
            methods.push(format!("{}: {}", js_str(&m.name.text), self.func(m, true, false)?));
        }
        Ok(format!("$type({}, [{}], {{{}}})", js_str(&t.name.text), fields.join(", "), methods.join(", ")))
    }

    // ----- expressions -------------------------------------------------------------------

    fn args(&mut self, args: &[Arg]) -> R<(String, String)> {
        let mut pos = Vec::new();
        let mut named = Vec::new();
        for a in args {
            let v = self.expr(&a.value)?;
            match &a.name {
                Some(n) => named.push(format!("{}: {v}", js_str(&n.text))),
                None => pos.push(v),
            }
        }
        let named = if named.is_empty() { "null".to_string() } else { format!("{{{}}}", named.join(", ")) };
        Ok((format!("[{}]", pos.join(", ")), named))
    }

    /// The value before `?.`: a field missing from a plain Object gives null.
    fn lenient(&mut self, e: &Expr) -> R<String> {
        if let ExprKind::Field { object, name, optional } = &e.kind {
            let o = if *optional { self.lenient(object)? } else { self.expr(object)? };
            let s = self.site(name.span, &Self::obj_hint(object));
            return Ok(format!("$getl({o}, {}, {s}, {optional})", js_str(&name.text)));
        }
        self.expr(e)
    }

    fn expr(&mut self, e: &Expr) -> R<String> {
        Ok(match &e.kind {
            ExprKind::Int(n) => {
                if n.unsigned_abs() > 9_007_199_254_740_991 {
                    format!("{n}n")
                } else {
                    n.to_string()
                }
            }
            ExprKind::Decimal(n) => format!("$d({n:?})"),
            ExprKind::Str(s) => js_str(s),
            ExprKind::Template(parts) => {
                let mut items = Vec::new();
                for p in parts {
                    items.push(match p {
                        TemplatePart::Lit(s) => js_str(s),
                        TemplatePart::Expr(x) => self.expr(x)?,
                    });
                }
                format!("$tpl([{}])", items.join(", "))
            }
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Null => "null".into(),
            ExprKind::Ident(name) => self.read_name(name, e.span)?,
            ExprKind::List(items) => {
                let items = items.iter().map(|x| self.expr(x)).collect::<R<Vec<_>>>()?;
                format!("[{}]", items.join(", "))
            }
            ExprKind::Object(fields) => {
                let mut pairs = Vec::new();
                for (k, v) in fields {
                    pairs.push(format!("[{}, {}]", js_str(&k.text), self.expr(v)?));
                }
                format!("$obj([{}])", pairs.join(", "))
            }
            ExprKind::Unary(UnaryOp::Neg, inner) => {
                let v = self.expr(inner)?;
                let s = self.site_expr(inner);
                format!("$neg({v}, {s})")
            }
            ExprKind::Unary(UnaryOp::Not, inner) => format!("!{}", self.cond(inner)?),
            ExprKind::Binary(op, l, r) => {
                let (a, b) = (self.expr(l)?, self.expr(r)?);
                match op {
                    BinOp::Eq => format!("eq({a}, {b})"),
                    BinOp::NotEq => format!("!eq({a}, {b})"),
                    _ => {
                        let extra = match &l.kind {
                            ExprKind::Ident(n) => format!(",n:{}", js_str(n)),
                            _ => String::new(),
                        };
                        let s = self.bin_site(l.span, &extra, r);
                        format!("$bin({}, {a}, {b}, {s})", js_str(op.symbol()))
                    }
                }
            }
            ExprKind::And(l, r) => format!("({} && {})", self.cond(l)?, self.cond(r)?),
            ExprKind::Or(l, r) => format!("({} || {})", self.cond(l)?, self.cond(r)?),
            ExprKind::Coalesce(l, r) => format!("({} ?? {})", self.expr(l)?, self.expr(r)?),
            ExprKind::IfElse { cond, then, otherwise } => format!("({} ? {} : {})", self.cond(cond)?, self.expr(then)?, self.expr(otherwise)?),
            ExprKind::Range { start, end, step } => {
                let (a, b) = (self.expr(start)?, self.expr(end)?);
                let st = match step {
                    Some(s) => self.expr(s)?,
                    None => "null".into(),
                };
                let (sa, sb) = (self.site_expr(start), self.site_expr(end));
                let ss = match step {
                    Some(s) => self.site_expr(s).to_string(),
                    None => "null".into(),
                };
                let s = self.site(e.span, "");
                format!("$range({a}, {b}, {st}, {sa}, {sb}, {ss}, {s})")
            }
            ExprKind::Call { callee, args } => {
                if let ExprKind::Field { object, name, optional } = &callee.kind {
                    let o = if *optional { self.lenient(object)? } else { self.expr(object)? };
                    let (pos, named) = self.args(args)?;
                    let n = self.site(name.span, &Self::obj_hint(object));
                    let s = self.site(e.span, &format!(",N:{n}"));
                    let key = js_str(&name.text);
                    if *optional {
                        let t = self.temp();
                        return Ok(format!("(({t} = {o}) === null ? null : $mc({t}, {key}, {pos}, {named}, {s}, false))"));
                    }
                    return Ok(format!("$mc({o}, {key}, {pos}, {named}, {s}, false)"));
                }
                let f = self.expr(callee)?;
                let (pos, named) = self.args(args)?;
                let c = self.site_expr(callee);
                let extra = match &callee.kind {
                    ExprKind::Ident(n) => format!(",n:{},C:{c}", js_str(n)),
                    _ => format!(",C:{c}"),
                };
                let s = self.site(e.span, &extra);
                format!("$call({f}, {pos}, {named}, {s})")
            }
            ExprKind::Field { object, name, optional } => {
                let o = if *optional { self.lenient(object)? } else { self.expr(object)? };
                let s = self.site(name.span, &Self::obj_hint(object));
                format!("$get({o}, {}, {s}, {optional})", js_str(&name.text))
            }
            ExprKind::Index { object, index } => {
                let (o, i) = (self.expr(object)?, self.expr(index)?);
                let extra = match &index.kind {
                    ExprKind::Ident(n) => format!(",n:{}", js_str(n)),
                    _ => String::new(),
                };
                let s = self.site(index.span, &format!("{extra}{}", Self::obj_hint(object)));
                format!("$idx({o}, {i}, {s})")
            }
            ExprKind::Lambda(f) => self.func(f, false, false)?,
            ExprKind::Await(inner) => {
                let v = self.expr(inner)?;
                let s = self.site(e.span, "");
                self.note_await(e.span, "`await` can only be used inside an async function")?;
                format!("(await $aw({v}, {s}))")
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_escaped_for_javascript() {
        assert_eq!(js_str("a\"b\\c\nd\u{2028}"), "\"a\\\"b\\\\c\\nd\\u2028\"");
    }

    #[test]
    fn scopes_follow_the_nearest_binding_rule() {
        let src = "count = 0\nbump()\n    count = count + 1\n    local = 5\n    return local\n";
        let dir = std::env::temp_dir().join(format!("lipi-codegen-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("scope.lipi");
        std::fs::write(&file, src).unwrap();
        let js = build(&file, Target::Node, &[]).map_err(|e| e.render(false)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        // `count` belongs to the module; `local` to the function.
        assert!(js.contains("let v_bump, v_count;"), "{js}");
        assert!(js.contains("let v_local;"), "{js}");
    }
}
