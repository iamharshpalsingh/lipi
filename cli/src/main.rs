//! `lipi` — the command-line front door to the Lipi language.

mod repl;

use lipi_compiler::suggest;
use lipi_runtime::{Interpreter, RunError};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
Lipi — easy to start, hard to outgrow.

Usage:
  lipi <file.lipi> [args]   Run a program
  lipi run [file] [args]    Run a program (default: the project's main file)
  lipi check [file]         Find mistakes without running
  lipi test [path]          Run test blocks in *_test.lipi files
  lipi new <name>           Create a new project
  lipi repl                 Start the interactive prompt (also: just `lipi`)
  lipi doctor               Check your setup
  lipi --version            Show the version

Coming later: build, dev, format, lint, install, remove, update, publish, deploy
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
        Some(cmd @ ("build" | "dev" | "format" | "fmt" | "lint" | "install" | "remove" | "update" | "publish" | "deploy")) => {
            let when = match cmd {
                "build" | "dev" => "Lipi 0.8, together with the JavaScript target",
                "deploy" => "a later release",
                _ => "Lipi 0.5, together with the package manager and tooling",
            };
            eprintln!("`lipi {cmd}` isn't available yet. It's planned for {when}.");
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
    match it.run_file(path) {
        Ok(_) => 0,
        Err(RunError::Exit(code)) => code,
        Err(e) => {
            eprint!("{}", it.render(&e, color()));
            1
        }
    }
}

/// The project's main file: `main` in lipi.json, or main.lipi.
fn project_entry() -> Option<PathBuf> {
    if let Ok(text) = std::fs::read_to_string("lipi.json") {
        if let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(serde_json::Value::String(main)) = m.get("main") {
                return Some(PathBuf::from(main));
            }
        }
    }
    let default = PathBuf::from("main.lipi");
    default.is_file().then_some(default)
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
    let (file, _) = match file_or_entry(args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut it = Interpreter::new();
    match it.check_file(&file) {
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
        eprintln!("Test files end in _test.lipi and contain blocks like:\n    test \"adds numbers\"\n        assert_equal(1 + 1, 2)");
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
        "{{\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \"main\": \"main.lipi\",\n  \"lipi\": \"{VERSION}\",\n  \"dependencies\": {{}}\n}}\n"
    );
    let main = "\
# Welcome to Lipi! Run this with: lipi run

greet(name)
    return \"Hello, {name}!\"

show greet(\"world\")

numbers = [1, 2, 3, 4, 5]
total = numbers.sum()
show \"The total is {total}\"
";
    let test = "\
# Run the tests with: lipi test

import \"../main.lipi\"

test \"greets by name\"
    assert_equal(main.greet(\"Dezy\"), \"Hello, Dezy!\")
";
    let files: [(PathBuf, &str); 4] = [
        (dir.join("lipi.json"), &manifest),
        (dir.join("main.lipi"), main),
        (dir.join("tests").join("main_test.lipi"), test),
        (dir.join(".gitignore"), "lipi_modules/\n.env\n"),
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
