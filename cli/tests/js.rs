//! JavaScript interop: tests/js/interop.lipi, built for Node, must print
//! tests/js/interop.out. (LIPI_BLESS=1 rewrites the expected output.)

use std::path::Path;
use std::process::Command;

#[test]
fn javascript_interop_in_node_builds() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node isn't installed; skipping the interop test");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out_dir = std::env::temp_dir().join(format!("lipi-js-interop-{}", std::process::id()));
    let build = Command::new(env!("CARGO_BIN_EXE_lipi"))
        .args(["build", "tests/js/interop.lipi", "--target", "node", "--out", &out_dir.to_string_lossy()])
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new("node").arg(out_dir.join("app.cjs")).current_dir(root).output().unwrap();
    let _ = std::fs::remove_dir_all(&out_dir);
    let mut actual = String::from_utf8_lossy(&run.stdout).into_owned();
    actual.push_str(&String::from_utf8_lossy(&run.stderr));
    let actual = actual.replace("\r\n", "\n");
    let expected_path = root.join("tests").join("js").join("interop.out");
    if std::env::var_os("LIPI_BLESS").is_some() {
        std::fs::write(&expected_path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(expected_path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(expected, actual);
}

#[test]
fn the_interpreter_explains_that_js_needs_a_build() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_lipi")).args(["run", "tests/js/interop.lipi"]).current_dir(root).env("NO_COLOR", "1").output().unwrap();
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(err.contains("LIP3007") && err.contains("\"js.global\" only works in JavaScript builds"), "{err}");
}
