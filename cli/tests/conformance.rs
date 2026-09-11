//! The LiPi 1.0 conformance suite (tests/conformance).
//!
//! - Every program there must print its `.out` file under `lipi run` AND when
//!   compiled with `lipi build --target node` (LIPI_BLESS=1 rewrites the
//!   expected output from the interpreter).
//! - tests/conformance/check holds programs that must pass `lipi check` and
//!   build for the web (UI code needs a browser to run).
//! - The suite must use every syntax form in the language: adding syntax
//!   without covering it here fails `the_suite_covers_the_whole_grammar`, and
//!   the exhaustive matches below stop compiling until the new form is listed.
//! - Every error code in the source must be documented in docs/SPEC.md.

use lipi_compiler::ast::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn normalize(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(stderr));
    text.replace("\r\n", "\n").replace('\\', "/")
}

fn lipi(args: &[&str]) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_lipi")).args(args).current_dir(root()).env("NO_COLOR", "1").output().expect("failed to run lipi");
    (normalize(&out.stdout, &out.stderr), out.status.code().unwrap_or(-1))
}

fn lipi_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "lipi")).collect();
    files.sort();
    files
}

#[test]
fn conformance_programs_behave_the_same_in_both_engines() {
    let dir = root().join("tests").join("conformance");
    let bless = std::env::var_os("LIPI_BLESS").is_some();
    let has_node = Command::new("node").arg("--version").output().is_ok();
    let out_root = std::env::temp_dir().join(format!("lipi-conformance-{}", std::process::id()));
    let mut failures = Vec::new();
    let mut count = 0;
    for path in lipi_files(&dir) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with('_') {
            continue;
        }
        count += 1;
        let rel = format!("tests/conformance/{name}");
        let (actual, _) = lipi(&["run", &rel]);
        let expected_path = path.with_extension("out");
        if bless {
            std::fs::write(&expected_path, &actual).unwrap();
        }
        let expected = std::fs::read_to_string(&expected_path).unwrap_or_default().replace("\r\n", "\n");
        if expected != actual {
            failures.push(format!("--- {rel} (lipi run)\n### expected:\n{expected}\n### actual:\n{actual}"));
        }
        if has_node {
            let out_dir = out_root.join(name.trim_end_matches(".lipi"));
            let (built, code) = lipi(&["build", &rel, "--target", "node", "--out", &out_dir.to_string_lossy()]);
            let js = if code != 0 {
                built
            } else {
                let out = Command::new("node").arg(out_dir.join("app.cjs")).current_dir(root()).output().expect("failed to run node");
                normalize(&out.stdout, &out.stderr)
            };
            if expected != js {
                failures.push(format!("--- {rel} (JavaScript)\n### expected:\n{expected}\n### actual:\n{js}"));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&out_root);
    assert!(count >= 10, "conformance programs are missing");
    assert!(failures.is_empty(), "{} conformance failure(s):\n\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn ui_syntax_checks_and_builds() {
    let dir = root().join("tests").join("conformance").join("check");
    let out_dir = std::env::temp_dir().join(format!("lipi-conformance-ui-{}", std::process::id()));
    for path in lipi_files(&dir) {
        let rel = format!("tests/conformance/check/{}", path.file_name().unwrap().to_string_lossy());
        let (out, code) = lipi(&["check", &rel]);
        assert_eq!(code, 0, "{rel}: {out}");
        let (out, code) = lipi(&["build", &rel, "--out", &out_dir.to_string_lossy()]);
        assert_eq!(code, 0, "{rel}: {out}");
    }
    let _ = std::fs::remove_dir_all(&out_dir);
}

#[test]
fn conformance_files_are_formatted() {
    let dir = root().join("tests").join("conformance");
    let mut args = vec!["format".to_string(), "--check".to_string()];
    for path in lipi_files(&dir).into_iter().chain(lipi_files(&dir.join("check"))) {
        // Programs that demonstrate syntax errors can't be formatted; that's their point.
        if lipi_compiler::parse_source(&std::fs::read_to_string(&path).unwrap()).is_ok() {
            args.push(path.to_string_lossy().to_string());
        }
    }
    assert!(args.len() > 12, "{args:?}");
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (out, code) = lipi(&refs);
    assert_eq!(code, 0, "{out}");
}

// ----- grammar coverage --------------------------------------------------------

fn stmt_name(k: &StmtKind) -> &'static str {
    match k {
        StmtKind::Expr(_) => "Expr",
        StmtKind::Show(_) => "Show",
        StmtKind::Assign { .. } => "Assign",
        StmtKind::If { .. } => "If",
        StmtKind::While { .. } => "While",
        StmtKind::For { .. } => "For",
        StmtKind::Repeat { .. } => "Repeat",
        StmtKind::Func(_) => "Func",
        StmtKind::Return(_) => "Return",
        StmtKind::Break => "Break",
        StmtKind::Continue => "Continue",
        StmtKind::Throw(_) => "Throw",
        StmtKind::Try { .. } => "Try",
        StmtKind::Match { .. } => "Match",
        StmtKind::Use { .. } => "Use",
        StmtKind::Export { .. } => "Export",
        StmtKind::TypeDef(_) => "TypeDef",
        StmtKind::Test { .. } => "Test",
        StmtKind::Component(_) => "Component",
        StmtKind::State { .. } => "State",
    }
}

const STMTS: &[&str] = &[
    "Expr", "Show", "Assign", "If", "While", "For", "Repeat", "Func", "Return", "Break", "Continue", "Throw", "Try", "Match", "Use", "Export", "TypeDef", "Test",
    "Component", "State",
];

fn expr_name(k: &ExprKind) -> &'static str {
    match k {
        ExprKind::Int(_) => "Int",
        ExprKind::Decimal(_) => "Decimal",
        ExprKind::Str(_) => "Str",
        ExprKind::Template(_) => "Template",
        ExprKind::Bool(_) => "Bool",
        ExprKind::Null => "Null",
        ExprKind::Ident(_) => "Ident",
        ExprKind::List(_) => "List",
        ExprKind::Object(_) => "Object",
        ExprKind::Unary(..) => "Unary",
        ExprKind::Binary(..) => "Binary",
        ExprKind::And(..) => "And",
        ExprKind::Or(..) => "Or",
        ExprKind::Coalesce(..) => "Coalesce",
        ExprKind::IfElse { .. } => "IfElse",
        ExprKind::Range { .. } => "Range",
        ExprKind::Call { .. } => "Call",
        ExprKind::Field { .. } => "Field",
        ExprKind::Index { .. } => "Index",
        ExprKind::Lambda(_) => "Lambda",
        ExprKind::Await(_) => "Await",
    }
}

const EXPRS: &[&str] = &[
    "Int", "Decimal", "Str", "Template", "Bool", "Null", "Ident", "List", "Object", "Unary", "Binary", "And", "Or", "Coalesce", "IfElse", "Range", "Call", "Field",
    "Index", "Lambda", "Await",
];

const BINARY: &[BinOp] = &[
    BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::Mod, BinOp::Pow, BinOp::Eq, BinOp::NotEq, BinOp::Lt, BinOp::Gt, BinOp::LtEq, BinOp::GtEq, BinOp::In,
    BinOp::NotIn,
];

const FEATURES: &[&str] = &[
    "type: Named", "type: List", "type: Optional", "unary: Neg", "unary: Not", "assign: name", "assign: field", "assign: index", "assign: +=", "assign: -=",
    "assign: *=", "assign: /=", "template: text", "template: code", "feature: else if", "feature: else", "feature: for with two names",
    "feature: catch without a name", "feature: finally", "feature: match several patterns", "feature: match guard", "feature: match _", "feature: match range",
    "feature: match else", "feature: use", "feature: use as", "feature: from use", "feature: export definition", "feature: export names", "feature: method",
    "feature: const", "feature: async function", "feature: return type", "feature: typed parameter", "feature: default parameter", "feature: range step",
    "feature: named argument", "feature: trailing block", "feature: trailing block with", "feature: ?.",
];

#[derive(Default)]
struct Walk(BTreeSet<String>);

impl Walk {
    fn add(&mut self, s: impl Into<String>) {
        self.0.insert(s.into());
    }

    fn block(&mut self, b: &[Stmt]) {
        for s in b {
            self.stmt(s);
        }
    }

    fn ty(&mut self, t: &TypeExpr) {
        match &t.kind {
            TypeKind::Named(_) => self.add("type: Named"),
            TypeKind::List(inner) => {
                self.add("type: List");
                self.ty(inner);
            }
            TypeKind::Optional(inner) => {
                self.add("type: Optional");
                self.ty(inner);
            }
        }
    }

    fn func(&mut self, f: &FuncDecl) {
        if f.is_async {
            self.add("feature: async function");
        }
        if let Some(t) = &f.ret {
            self.add("feature: return type");
            self.ty(t);
        }
        for p in &f.params {
            if let Some(t) = &p.ty {
                self.add("feature: typed parameter");
                self.ty(t);
            }
            if let Some(d) = &p.default {
                self.add("feature: default parameter");
                self.expr(d);
            }
        }
        self.block(&f.body);
    }

    fn stmt(&mut self, s: &Stmt) {
        self.add(format!("stmt: {}", stmt_name(&s.kind)));
        match &s.kind {
            StmtKind::Expr(e) | StmtKind::Throw(e) => self.expr(e),
            StmtKind::Show(values) => values.iter().for_each(|e| self.expr(e)),
            StmtKind::Assign { target, op, ty, value, constant } => {
                self.add(match target {
                    Target::Name(_) => "assign: name",
                    Target::Field(..) => "assign: field",
                    Target::Index(..) => "assign: index",
                });
                if let Some(op) = op {
                    self.add(format!("assign: {}=", op.symbol()));
                }
                if *constant {
                    self.add("feature: const");
                }
                if let Some(t) = ty {
                    self.ty(t);
                }
                match target {
                    Target::Field(obj, _) => self.expr(obj),
                    Target::Index(obj, index) => {
                        self.expr(obj);
                        self.expr(index);
                    }
                    Target::Name(_) => {}
                }
                self.expr(value);
            }
            StmtKind::If { branches, otherwise } => {
                if branches.len() > 1 {
                    self.add("feature: else if");
                }
                for (c, b) in branches {
                    self.expr(c);
                    self.block(b);
                }
                if let Some(b) = otherwise {
                    self.add("feature: else");
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
            StmtKind::For { second, iter, body, .. } => {
                if second.is_some() {
                    self.add("feature: for with two names");
                }
                self.expr(iter);
                self.block(body);
            }
            StmtKind::Func(f) | StmtKind::Component(f) => self.func(f),
            StmtKind::Return(value) => {
                if let Some(e) = value {
                    self.expr(e);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Try { body, catch, finally } => {
                self.block(body);
                if let Some((name, b)) = catch {
                    if name.is_none() {
                        self.add("feature: catch without a name");
                    }
                    self.block(b);
                }
                if let Some(f) = finally {
                    self.add("feature: finally");
                    self.block(f);
                }
            }
            StmtKind::Match { subject, arms, otherwise } => {
                self.expr(subject);
                for arm in arms {
                    if arm.patterns.len() > 1 {
                        self.add("feature: match several patterns");
                    }
                    for p in &arm.patterns {
                        match &p.kind {
                            ExprKind::Ident(n) if n == "_" => self.add("feature: match _"),
                            ExprKind::Range { .. } => self.add("feature: match range"),
                            _ => {}
                        }
                        self.expr(p);
                    }
                    if let Some(g) = &arm.guard {
                        self.add("feature: match guard");
                        self.expr(g);
                    }
                    self.block(&arm.body);
                }
                if let Some(b) = otherwise {
                    self.add("feature: match else");
                    self.block(b);
                }
            }
            StmtKind::Use { alias, names, .. } => self.add(match (alias, names) {
                (_, Some(_)) => "feature: from use",
                (Some(_), None) => "feature: use as",
                (None, None) => "feature: use",
            }),
            StmtKind::Export { inner, .. } => match inner {
                Some(inner) => {
                    self.add("feature: export definition");
                    self.stmt(inner);
                }
                None => self.add("feature: export names"),
            },
            StmtKind::TypeDef(t) => {
                for f in &t.fields {
                    if let Some(ty) = &f.ty {
                        self.ty(ty);
                    }
                    if let Some(d) = &f.default {
                        self.expr(d);
                    }
                }
                for m in &t.methods {
                    self.add("feature: method");
                    self.func(m);
                }
            }
            StmtKind::Test { body, .. } => self.block(body),
            StmtKind::State { ty, value, .. } => {
                if let Some(t) = ty {
                    self.ty(t);
                }
                self.expr(value);
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        self.add(format!("expr: {}", expr_name(&e.kind)));
        match &e.kind {
            ExprKind::Template(parts) => {
                for p in parts {
                    match p {
                        TemplatePart::Lit(_) => self.add("template: text"),
                        TemplatePart::Expr(x) => {
                            self.add("template: code");
                            self.expr(x);
                        }
                    }
                }
            }
            ExprKind::List(items) => items.iter().for_each(|x| self.expr(x)),
            ExprKind::Object(fields) => fields.iter().for_each(|(_, v)| self.expr(v)),
            ExprKind::Unary(op, x) => {
                self.add(format!("unary: {op:?}"));
                self.expr(x);
            }
            ExprKind::Binary(op, a, b) => {
                self.add(format!("binary: {}", op.symbol()));
                self.expr(a);
                self.expr(b);
            }
            ExprKind::And(a, b) | ExprKind::Or(a, b) | ExprKind::Coalesce(a, b) => {
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
                    self.add("feature: range step");
                    self.expr(s);
                }
            }
            ExprKind::Call { callee, args } => {
                if args.iter().any(|a| a.name.is_some()) {
                    self.add("feature: named argument");
                }
                if let Some(ExprKind::Lambda(f)) = args.last().map(|a| &a.value.kind) {
                    if f.name.text == "<block>" {
                        self.add("feature: trailing block");
                        if !f.params.is_empty() {
                            self.add("feature: trailing block with");
                        }
                    }
                }
                self.expr(callee);
                args.iter().for_each(|a| self.expr(&a.value));
            }
            ExprKind::Field { object, optional, .. } => {
                if *optional {
                    self.add("feature: ?.");
                }
                self.expr(object);
            }
            ExprKind::Index { object, index } => {
                self.expr(object);
                self.expr(index);
            }
            ExprKind::Lambda(f) => self.func(f),
            ExprKind::Await(x) => self.expr(x),
            _ => {}
        }
    }
}

#[test]
fn the_suite_covers_the_whole_grammar() {
    let dir = root().join("tests").join("conformance");
    let mut walk = Walk::default();
    for path in lipi_files(&dir).into_iter().chain(lipi_files(&dir.join("check"))) {
        let src = std::fs::read_to_string(&path).unwrap();
        // Programs that demonstrate syntax errors don't parse; that's their point.
        if let Ok(program) = lipi_compiler::parse_source(&src) {
            walk.block(&program.body);
        }
    }
    let mut required: Vec<String> = STMTS.iter().map(|s| format!("stmt: {s}")).collect();
    required.extend(EXPRS.iter().map(|e| format!("expr: {e}")));
    required.extend(BINARY.iter().map(|op| format!("binary: {}", op.symbol())));
    required.extend(FEATURES.iter().map(|f| f.to_string()));
    let missing: Vec<&String> = required.iter().filter(|r| !walk.0.contains(*r)).collect();
    assert!(missing.is_empty(), "the conformance suite doesn't use: {missing:?}");
}

// ----- error codes -------------------------------------------------------------

fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            source_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs" || e == "js") {
            out.push(p);
        }
    }
}

#[test]
fn every_error_code_is_documented() {
    let root = root();
    let spec = std::fs::read_to_string(root.join("docs").join("SPEC.md")).unwrap();
    let mut files = Vec::new();
    for dir in ["compiler/src", "runtime/src", "cli/src"] {
        source_files(&root.join(dir), &mut files);
    }
    let mut codes = BTreeSet::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let bytes = text.as_bytes();
        let mut i = 0;
        while let Some(at) = text[i..].find("\"LIP") {
            let start = i + at + 1;
            let digits = &bytes[start + 3..(start + 7).min(bytes.len())];
            if digits.len() == 4 && digits.iter().all(u8::is_ascii_digit) && bytes.get(start + 7) == Some(&b'"') {
                codes.insert(text[start..start + 7].to_string());
            }
            i = start;
        }
    }
    assert!(codes.len() > 40, "found only {} codes", codes.len());
    let undocumented: Vec<&String> = codes.iter().filter(|c| !spec.contains(c.as_str()) && !spec.contains(&format!("{} ", &c[3..]))).collect();
    assert!(undocumented.is_empty(), "error codes missing from docs/SPEC.md §16: {undocumented:?}");
}
