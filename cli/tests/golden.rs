//! Golden tests: every `tests/language/*.lipi` program is run with the real
//! `lipi` binary and its output compared with the `.out` file next to it.
//!
//! To update the expected output after an intentional change:
//!     LIPI_BLESS=1 cargo test -p lipi --test golden

use std::path::Path;
use std::process::Command;

fn run_lipi(root: &Path, args: &[&str]) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_lipi"))
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run lipi");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (text.replace("\r\n", "\n").replace('\\', "/"), out.status.code().unwrap_or(-1))
}

#[test]
fn language_programs_match_expected_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dir = root.join("tests").join("language");
    let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    let bless = std::env::var_os("LIPI_BLESS").is_some();
    let mut failures = Vec::new();
    let mut count = 0;
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !name.ends_with(".lipi") || name.ends_with("_test.lipi") || name.starts_with('_') {
            continue;
        }
        count += 1;
        let rel = format!("tests/language/{name}");
        let (actual, _) = run_lipi(root, &["run", &rel]);
        let expected_path = path.with_extension("out");
        if bless {
            std::fs::write(&expected_path, &actual).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&expected_path).unwrap_or_default().replace("\r\n", "\n");
        if expected != actual {
            failures.push(format!("--- {rel}\n### expected:\n{expected}\n### actual:\n{actual}"));
        }
    }
    assert!(count > 0, "no golden programs found");
    assert!(failures.is_empty(), "{} golden test(s) failed:\n\n{}", failures.len(), failures.join("\n"));
}

/// The same golden programs, compiled with `lipi build --target node` and run
/// with Node.js, must print exactly what `lipi run` prints.
#[test]
fn javascript_builds_match_expected_output() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node isn't installed; skipping the JavaScript golden tests");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dir = root.join("tests").join("language");
    let out_root = std::env::temp_dir().join(format!("lipi-js-golden-{}", std::process::id()));
    let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    let mut failures = Vec::new();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        // The database module is server-only; it isn't part of JavaScript builds.
        if !name.ends_with(".lipi") || name.ends_with("_test.lipi") || name.starts_with('_') || name == "database.lipi" {
            continue;
        }
        let rel = format!("tests/language/{name}");
        let out_dir = out_root.join(name.trim_end_matches(".lipi"));
        let (built, code) = run_lipi(root, &["build", &rel, "--target", "node", "--out", &out_dir.to_string_lossy()]);
        let actual = if code != 0 {
            built
        } else {
            let out = Command::new("node").arg(out_dir.join("app.cjs")).current_dir(root).output().expect("failed to run node");
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            text.replace("\r\n", "\n").replace('\\', "/")
        };
        let expected = std::fs::read_to_string(path.with_extension("out")).unwrap_or_default().replace("\r\n", "\n");
        if expected != actual {
            failures.push(format!("--- {rel}\n### expected:\n{expected}\n### actual (JavaScript):\n{actual}"));
        }
    }
    let _ = std::fs::remove_dir_all(&out_root);
    assert!(failures.is_empty(), "{} JavaScript golden test(s) failed:\n\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn web_builds_refuse_server_only_modules() {
    let tmp = std::env::temp_dir().join(format!("lipi-web-build-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("app.lipi"), "secret = env.get(\"API_KEY\")\nshow secret\n").unwrap();
    let (out, code) = run_lipi(&tmp, &["build", "app.lipi"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("LIP6001"), "{out}");
    assert!(out.contains("\"env\" only works on the server"), "{out}");
    std::fs::write(tmp.join("app.lipi"), "show \"Hello from the browser\"\n").unwrap();
    let (out, code) = run_lipi(&tmp, &["build", "app.lipi", "--out", "site"]);
    assert_eq!(code, 0, "{out}");
    let html = std::fs::read_to_string(tmp.join("site").join("index.html")).unwrap();
    assert!(html.contains("<script src=\"app.js\"></script>"), "{html}");
    assert!(tmp.join("site").join("app.js").is_file());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn lipi_test_command_runs_test_blocks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let (out, code) = run_lipi(root, &["test", "tests/language/sample_test.lipi"]);
    assert!(out.contains("✓ adds numbers"), "{out}");
    assert!(out.contains("✗ deliberately fails"), "{out}");
    assert!(out.contains("expected 3, but got 4"), "{out}");
    assert!(out.contains("2 passed, 1 failed"), "{out}");
    assert_eq!(code, 1);
}

#[test]
fn lipi_new_creates_a_runnable_project() {
    let tmp = std::env::temp_dir().join(format!("lipi-new-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let (out, code) = run_lipi(&tmp, &["new", "demo"]);
    assert_eq!(code, 0, "{out}");
    let project = tmp.join("demo");
    let (out, code) = run_lipi(&project, &["run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Hello, LiPi!"), "{out}");
    let (out, code) = run_lipi(&project, &["test"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(&tmp);
}
