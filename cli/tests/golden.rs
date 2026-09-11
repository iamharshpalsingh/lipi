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
    assert!(out.contains("Hello, world!"), "{out}");
    let (out, code) = run_lipi(&project, &["test"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(&tmp);
}
