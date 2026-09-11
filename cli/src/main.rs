//! `lipi` — the command-line front door to the Lipi language.

mod dev;
mod lsp;
mod pkg;
mod repl;

use lipi_compiler::suggest;
use lipi_runtime::{Interpreter, RunError};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

const HELP: &str = "\
LiPi — Unified Development Language. Easy to start. Hard to outgrow.

Usage:
  lipi <file.lipi> [args]   Run a program
  lipi run [file] [args]    Run a program (default: the project's main file)
  lipi check [file] [--json]  Find mistakes without running (--json for editors and CI)
  lipi test [path]          Run test blocks in *_test.lipi files
  lipi format [paths] [--check]  Rewrite files in the canonical LiPi style (--check: only report)
  lipi lint [paths] [--strict]   Errors plus warnings (unused names, shadowing, dead code)
  lipi build [file] [--target web|node] [--out dist]  Compile to JavaScript for the browser or Node.js
  lipi dev [file] [--port 3000]  Serve the web app and rebuild + reload it on every save
  lipi install [spec...]    Install dependencies (e.g. ../utils, git:URL#v1, slugify@^1.2)
  lipi remove <name...>     Remove dependencies
  lipi update [name...]     Upgrade dependencies within their version ranges
  lipi publish [--registry URL]  Publish this package to a registry
  lipi new <name>           Create a new project
  lipi repl                 Start the interactive prompt (also: just `lipi`)
  lipi lsp                  Start the language server (used by editors such as VS Code)
  lipi doctor               Check your setup
  lipi --version            Show the version

Coming later: deploy
";

fn main() {
    enable_ansi();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The interpreter recurses on the Rust stack, so give it plenty of room.
    let worker = std::thread::Builder::new().stack_size(512 * 1024 * 1024).spawn(move || run(args));
    let code = match worker {
        Ok(handle) => handle.join().unwrap_or(101),
        Err(e) => {
            eprintln!("lipi: couldn't start: {e}");
            101
        }
    };
    std::process::exit(code);
}

fn color() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn run(args: Vec<String>) -> i32 {
    let rest = || args.get(1..).unwrap_or(&[]).to_vec();
    match args.first().map(String::as_str) {
        None => {
            if std::io::stdin().is_terminal() {
                repl::start()
            } else {
                print!("{HELP}");
                0
            }
        }
        Some("run") => cmd_run(&rest()),
        Some("check") => cmd_check(&rest()),
        Some("test") => cmd_test(&rest()),
        Some("format" | "fmt") => cmd_format(&rest()),
        Some("lint") => cmd_lint(&rest()),
        Some("lsp") => lsp::run(),
        Some(cmd @ ("install" | "i" | "remove" | "update" | "publish")) => {
            let result = match cmd {
                "install" | "i" => pkg::install(&rest()),
                "remove" => pkg::remove(&rest()),
                "update" => pkg::update(&rest()),
                _ => pkg::publish(&rest()),
            };
            match result {
                Ok(()) => 0,
                Err(e) => {
                    e.print();
                    1
                }
            }
        }
        Some("new") => cmd_new(&rest()),
        Some("repl") => repl::start(),
        Some("doctor") => cmd_doctor(),
        Some("tokens") => cmd_debug(&rest(), false),
        Some("ast") => cmd_debug(&rest(), true),
        Some("--version" | "-v" | "-V" | "version") => {
            println!("lipi {VERSION}");
            0
        }
        Some("help" | "--help" | "-h") => {
            print!("{HELP}");
            0
        }
        Some("build") => cmd_build(&rest()),
        Some("dev") => dev::run(&rest()),
        Some("deploy") => {
            eprintln!("`lipi deploy` isn't available yet. It's planned for a later release.");
            2
        }
        Some(file) if file.ends_with(".lipi") || Path::new(file).is_file() => run_program(Path::new(file), &rest()),
        Some(other) => {
            const COMMANDS: &[&str] = &["run", "check", "test", "new", "repl", "doctor", "help", "version"];
            eprintln!("lipi: unknown command `{other}`");
            if let Some(c) = suggest::closest(other, COMMANDS.iter().copied()) {
                eprintln!("Did you mean `lipi {c}`?");
            } else {
                eprintln!("Run `lipi help` to see what lipi can do.");
            }
            2
        }
    }
}

fn run_program(path: &Path, script_args: &[String]) -> i32 {
    let mut it = Interpreter::new();
    it.set_script_args(script_args);
    match it.run_file(path).and_then(|_| it.serve(color())) {
        Ok(()) => 0,
        Err(RunError::Exit(code)) => code,
        Err(e) => {
            eprint!("{}", it.render(&e, color()));
            1
        }
    }
}

/// The project's main file: `main` in lipi.json, else src/main.lipi or main.lipi.
fn project_entry() -> Option<PathBuf> {
    if let Ok(text) = std::fs::read_to_string("lipi.json") {
        if let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(serde_json::Value::String(main)) = m.get("main") {
                return Some(PathBuf::from(main));
            }
        }
    }
    ["src/main.lipi", "main.lipi"].into_iter().map(PathBuf::from).find(|p| p.is_file())
}

fn file_or_entry(args: &[String]) -> Result<(PathBuf, Vec<String>), i32> {
    match args.first() {
        Some(f) if f.ends_with(".lipi") || Path::new(f).is_file() => Ok((PathBuf::from(f), args[1..].to_vec())),
        _ => match project_entry() {
            Some(p) => Ok((p, args.to_vec())),
            None => {
                eprintln!("lipi: which file should I run?");
                eprintln!("Pass one, like `lipi run hello.lipi`, or run it inside a project made with `lipi new`.");
                Err(2)
            }
        },
    }
}

fn cmd_run(args: &[String]) -> i32 {
    match file_or_entry(args) {
        Ok((file, rest)) => run_program(&file, &rest),
        Err(code) => code,
    }
}

fn cmd_check(args: &[String]) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--json").cloned().collect();
    let (file, _) = match file_or_entry(&rest) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut it = Interpreter::new();
    let result = it.check_file(&file);
    if json {
        let diags: Vec<(lipi_compiler::Diagnostic, String)> = match &result {
            Err(RunError::Syntax(d, f)) => vec![(d.clone(), f.to_string())],
            Err(RunError::Check(ds, f)) => ds.iter().map(|d| (d.clone(), f.to_string())).collect(),
            _ => Vec::new(),
        };
        let items: Vec<serde_json::Value> = diags
            .iter()
            .map(|(d, f)| {
                serde_json::json!({
                    "code": d.code,
                    "severity": if d.severity == lipi_compiler::Severity::Error { "error" } else { "warning" },
                    "category": d.category(),
                    "message": d.message,
                    "file": f,
                    "line": d.span.map(|s| s.line),
                    "column": d.span.map(|s| s.col),
                    "length": d.span.map(|s| s.end - s.start),
                    "hint": d.hint,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into()));
        return if items.is_empty() { 0 } else { 1 };
    }
    match result {
        Ok(()) => {
            println!("No problems found in {}", file.display());
            0
        }
        Err(e) => {
            eprint!("{}", it.render(&e, color()));
            1
        }
    }
}

/// All `.lipi` files under `dir`, skipping dependencies, build output and hidden folders.
fn find_lipi_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if p.is_dir() {
            if !name.starts_with('.') && !matches!(name.as_str(), "lipi_modules" | "target" | "node_modules") {
                find_lipi_files(&p, out);
            }
        } else if name.ends_with(".lipi") {
            out.push(p);
        }
    }
}

fn cmd_format(args: &[String]) -> i32 {
    let check = args.iter().any(|a| a == "--check");
    let targets: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    let targets = if targets.is_empty() { vec![PathBuf::from(".")] } else { targets };
    let mut files = Vec::new();
    for t in &targets {
        if t.is_file() {
            files.push(t.clone());
        } else if t.is_dir() {
            find_lipi_files(t, &mut files);
        } else {
            eprintln!("lipi: there's no file or folder at {}", t.display());
            return 2;
        }
    }
    let (mut changed, mut failed) = (0, 0);
    for file in &files {
        let Ok(src) = std::fs::read_to_string(file) else {
            eprintln!("lipi: couldn't read {}", file.display());
            failed += 1;
            continue;
        };
        // Keep each file's own line endings and byte-order mark (editors on
        // Windows often save CRLF, and some add a UTF-8 BOM).
        let crlf = src.contains("\r\n");
        let bom = if src.starts_with('\u{feff}') { "\u{feff}" } else { "" };
        let formatted = lipi_compiler::format::format_source(src.trim_start_matches('\u{feff}')).map(|out| {
            let out = if crlf { out.replace('\n', "\r\n") } else { out };
            format!("{bom}{out}")
        });
        match formatted {
            Ok(out) if out != src => {
                changed += 1;
                if check {
                    println!("would reformat {}", file.display());
                } else if let Err(e) = std::fs::write(file, out) {
                    eprintln!("lipi: couldn't write {}: {e}", file.display());
                    failed += 1;
                } else {
                    println!("formatted {}", file.display());
                }
            }
            Ok(_) => {}
            Err(d) => {
                failed += 1;
                eprint!("{}", d.render(&src, &file.to_string_lossy(), color()));
            }
        }
    }
    let verb = if check { "need formatting" } else { "reformatted" };
    println!("{} file{} checked, {changed} {verb}{}", files.len(), if files.len() == 1 { "" } else { "s" }, if failed > 0 { format!(", {failed} with errors") } else { String::new() });
    if failed > 0 || (check && changed > 0) {
        1
    } else {
        0
    }
}

fn cmd_lint(args: &[String]) -> i32 {
    let strict = args.iter().any(|a| a == "--strict");
    let targets: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    let targets = if targets.is_empty() { vec![PathBuf::from(".")] } else { targets };
    let mut files = Vec::new();
    for t in &targets {
        if t.is_file() {
            files.push(t.clone());
        } else {
            find_lipi_files(t, &mut files);
        }
    }
    let names = Interpreter::new().builtin_names();
    let builtins: Vec<&str> = names.iter().map(String::as_str).collect();
    let (mut errors, mut warnings) = (0, 0);
    for file in &files {
        let Ok(src) = std::fs::read_to_string(file) else { continue };
        let shown = file.to_string_lossy();
        let program = match lipi_compiler::parse_source(&src) {
            Ok(p) => p,
            Err(d) => {
                errors += 1;
                eprint!("{}", d.render(&src, &shown, color()));
                continue;
            }
        };
        let mut diags = lipi_compiler::checker::check(&program, &builtins);
        diags.extend(lipi_compiler::lint::lint(&program));
        for d in diags {
            if d.severity == lipi_compiler::Severity::Error {
                errors += 1;
            } else {
                warnings += 1;
            }
            eprint!("{}\n", d.render(&src, &shown, color()));
        }
    }
    println!("{} file{} linted: {errors} error{}, {warnings} warning{}", files.len(), if files.len() == 1 { "" } else { "s" }, if errors == 1 { "" } else { "s" }, if warnings == 1 { "" } else { "s" });
    if errors > 0 || (strict && warnings > 0) {
        1
    } else {
        0
    }
}

fn find_test_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if p.is_dir() {
            if !name.starts_with('.') && name != "lipi_modules" && name != "target" {
                find_test_files(&p, out);
            }
        } else if name.ends_with("_test.lipi") {
            out.push(p);
        }
    }
}

fn cmd_test(args: &[String]) -> i32 {
    let target = PathBuf::from(args.first().map(String::as_str).unwrap_or("."));
    let mut files = Vec::new();
    if target.is_file() {
        files.push(target.clone());
    } else {
        find_test_files(&target, &mut files);
    }
    if files.is_empty() {
        eprintln!("No test files found in {}.", target.display());
        eprintln!("Test files end in _test.lipi and contain blocks like:\n    test \"adds numbers\"\n        assertEqual(1 + 1, 2)");
        return 2;
    }
    let color = color() && std::io::stdout().is_terminal();
    let (green, red, dim, reset) = if color { ("\x1b[32m", "\x1b[31m", "\x1b[2m", "\x1b[0m") } else { ("", "", "", "") };
    let (mut passed, mut failed) = (0, 0);
    for file in &files {
        println!("{dim}{}{reset}", file.display());
        let mut it = Interpreter::new();
        match it.run_tests(file, color) {
            Ok(results) => {
                if results.is_empty() {
                    println!("  {dim}(no tests){reset}");
                }
                for r in results {
                    match r.error {
                        None => {
                            passed += 1;
                            println!("  {green}✓{reset} {}", r.name);
                        }
                        Some(err) => {
                            failed += 1;
                            println!("  {red}✗ {}{reset}", r.name);
                            for line in err.lines() {
                                println!("      {line}");
                            }
                        }
                    }
                }
            }
            Err(e) => {
                failed += 1;
                println!("  {red}✗ the file couldn't run{reset}");
                for line in it.render(&e, color).lines() {
                    println!("      {line}");
                }
            }
        }
    }
    println!();
    if failed == 0 {
        println!("{green}{passed} passed{reset}");
        0
    } else {
        println!("{green}{passed} passed{reset}, {red}{failed} failed{reset}");
        1
    }
}

fn cmd_build(args: &[String]) -> i32 {
    use lipi_compiler::codegen::{self, Target};
    let mut target = Target::Web;
    let mut out_dir = PathBuf::from("dist");
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a, None),
        };
        match flag {
            "--target" | "-t" | "--out" | "-o" => {
                let value = match inline {
                    Some(v) => Some(v),
                    None => {
                        i += 1;
                        args.get(i).cloned()
                    }
                };
                let Some(value) = value else {
                    eprintln!("lipi: {flag} needs a value");
                    return 2;
                };
                if matches!(flag, "--out" | "-o") {
                    out_dir = PathBuf::from(value);
                } else {
                    target = match value.as_str() {
                        "web" | "browser" => Target::Web,
                        "node" => Target::Node,
                        other => {
                            eprintln!("lipi: unknown target `{other}`. Use --target web or --target node.");
                            return 2;
                        }
                    };
                }
            }
            _ => files.push(a.to_string()),
        }
        i += 1;
    }
    let (file, _) = match file_or_entry(&files) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let names = Interpreter::new().builtin_names();
    let builtins: Vec<&str> = names.iter().map(String::as_str).collect();
    let js = match codegen::build(&file, target, &builtins) {
        Ok(js) => js,
        Err(e) => {
            eprint!("{}", e.render(color()));
            return 1;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("lipi: couldn't create {}: {e}", out_dir.display());
        return 1;
    }
    let write = |name: &str, content: &str| -> bool {
        let path = out_dir.join(name);
        match std::fs::write(&path, content) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("lipi: couldn't write {}: {e}", path.display());
                false
            }
        }
    };
    match target {
        Target::Node => {
            if !write("app.cjs", &js) {
                return 1;
            }
            println!("Built {} from {}", out_dir.join("app.cjs").display(), file.display());
            println!("Run it with: node {}", out_dir.join("app.cjs").display());
        }
        Target::Web => {
            let title = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "LiPi app".into());
            if !write("app.js", &js) || !write("index.html", &web_page(&title, false)) {
                return 1;
            }
            // Images, styles and JavaScript files the app uses go in public/.
            let public = lipi_compiler::resolve::find_project_root(&file).join("public");
            if public.is_dir() {
                match copy_dir(&public, &out_dir) {
                    Ok(n) => println!("Copied {n} file{} from {}", if n == 1 { "" } else { "s" }, public.display()),
                    Err(e) => {
                        eprintln!("lipi: couldn't copy {}: {e}", public.display());
                        return 1;
                    }
                }
            }
            println!("Built {} and {} from {}", out_dir.join("index.html").display(), out_dir.join("app.js").display(), file.display());
            println!("Open index.html in a browser, or serve the folder with any static file server.");
        }
    }
    0
}

/// Copy a folder's contents into another folder. Returns how many files were copied.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<usize> {
    let mut count = 0;
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            count += copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
            count += 1;
        }
    }
    Ok(count)
}

/// The script `lipi dev` adds to the page: reload after each rebuild, show build errors.
const DEV_SCRIPT: &str = r#"<script>
(() => {
  let version;
  const events = new EventSource("/__lipi/events");
  events.addEventListener("version", (e) => {
    if (version !== undefined && e.data !== version) location.reload();
    version = e.data;
  });
  events.addEventListener("build-error", (e) => {
    let box = document.getElementById("lipi-dev-error");
    if (!box) {
      box = document.createElement("pre");
      box.id = "lipi-dev-error";
      box.className = "lipi-error";
      document.body.prepend(box);
    }
    box.textContent = "The build failed. Fix this and save:\n\n" + e.data;
  });
})();
</script>
"#;

/// The page that loads a web build.
fn web_page(title: &str, dev: bool) -> String {
    let reload = if dev { DEV_SCRIPT } else { "" };
    const LOGO: &str = include_str!("../../assets/lipi-mark.svg");
    let mut icon = String::from("data:image/svg+xml,");
    for c in LOGO.trim().chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | ' ' | '-' | '.' | '/' | ':' | '=' | '\'' | ',' | '(' | ')' => icon.push(c),
            '"' => icon.push('\''),
            '\r' | '\n' | '\t' => icon.push(' '),
            c => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    icon.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    let title = title.replace('&', "&amp;").replace('<', "&lt;");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="icon" href="{icon}">
<style>
  body {{ font-family: system-ui, sans-serif; margin: 2rem; color: #17120E; background: #FBF8F3; }}
  #lipi-output {{ font: 15px/1.5 ui-monospace, Consolas, monospace; white-space: pre-wrap; }}
  .lipi-error {{ font: 14px/1.5 ui-monospace, Consolas, monospace; white-space: pre-wrap; color: #8A1C0C;
    background: #FDECE8; border-left: 4px solid #D2452A; padding: 1rem; }}
</style>
</head>
<body>
<div id="app"></div>
<pre id="lipi-output" hidden></pre>
<script src="app.js"></script>
{reload}</body>
</html>
"#
    )
}

fn cmd_new(args: &[String]) -> i32 {
    let Some(name) = args.first() else {
        eprintln!("lipi: give your project a name, like `lipi new my-app`");
        return 2;
    };
    let valid = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    if !valid {
        eprintln!("lipi: project names can use letters, numbers, - and _ (got `{name}`)");
        return 2;
    }
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("lipi: `{name}` already exists. Pick another name or remove it first.");
        return 1;
    }
    let manifest = format!(
        "{{\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \"main\": \"src/main.lipi\",\n  \"lipi\": \"{VERSION}\",\n  \"dependencies\": {{}}\n}}\n"
    );
    let main = "\
# Welcome to LiPi! Run this project with: lipi run

greet(name)
    return \"Hello, \" + name + \"!\"

export greet

show greet(\"LiPi\")

numbers = [1, 2, 3, 4, 5]
total = numbers.sum()
show \"The total is {total}\"
";
    let test = "\
# Run the tests with: lipi test

use \"../src/main.lipi\" as app

test \"greets by name\"
    assertEqual(app.greet(\"Dezy\"), \"Hello, Dezy!\")
";
    let files: [(PathBuf, &str); 4] = [
        (dir.join("lipi.json"), &manifest),
        (dir.join("src").join("main.lipi"), main),
        (dir.join("tests").join("main_test.lipi"), test),
        (dir.join(".gitignore"), "lipi_modules/\n.lipi-tmp/\n.env\n*.db\n"),
    ];
    for (path, content) in files {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("lipi: couldn't create {}: {e}", parent.display());
                return 1;
            }
        }
        if let Err(e) = std::fs::write(&path, content) {
            eprintln!("lipi: couldn't write {}: {e}", path.display());
            return 1;
        }
    }
    println!("Created {name}/\n\nNext steps:\n  cd {name}\n  lipi run\n  lipi test");
    0
}

fn cmd_doctor() -> i32 {
    println!("lipi {VERSION}");
    println!("platform: {} ({})", std::env::consts::OS, std::env::consts::ARCH);
    match std::process::Command::new("curl").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let first = String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("").to_string();
            println!("http: ok ({})", first.split_whitespace().take(2).collect::<Vec<_>>().join(" "));
        }
        _ => println!("http: curl was not found. The http module needs it."),
    }
    match project_entry() {
        Some(p) => println!("project: main file is {}", p.display()),
        None => println!("project: none in this folder (create one with `lipi new <name>`)"),
    }
    0
}

fn cmd_debug(args: &[String], ast: bool) -> i32 {
    let Some(file) = args.first() else {
        eprintln!("lipi: pass a file");
        return 2;
    };
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lipi: couldn't read {file}: {e}");
            return 1;
        }
    };
    let result = if ast {
        lipi_compiler::parse_source(&source).map(|p| {
            for stmt in &p.body {
                println!("{stmt:#?}");
            }
        })
    } else {
        lipi_compiler::lexer::Lexer::new(&source).tokenize().map(|toks| {
            for t in toks {
                println!("{}:{}  {:?}", t.span.line, t.span.col, t.tok);
            }
        })
    };
    match result {
        Ok(()) => 0,
        Err(d) => {
            eprint!("{}", d.render(&source, file, color()));
            1
        }
    }
}

#[cfg(windows)]
fn enable_ansi() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(n: u32) -> *mut c_void;
        fn GetConsoleMode(h: *mut c_void, mode: *mut u32) -> i32;
        fn SetConsoleMode(h: *mut c_void, mode: u32) -> i32;
    }
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    // STD_OUTPUT_HANDLE = -11, STD_ERROR_HANDLE = -12
    for n in [-11i32 as u32, -12i32 as u32] {
        // SAFETY: plain Win32 console calls on the process's own standard handles.
        unsafe {
            let h = GetStdHandle(n);
            let mut mode = 0;
            if !h.is_null() && GetConsoleMode(h, &mut mode) != 0 {
                SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }
}

#[cfg(not(windows))]
fn enable_ansi() {}
