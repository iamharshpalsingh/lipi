//! Runs tests/postgres/database.lipi against a real PostgreSQL server.
//! Skipped unless LIPI_TEST_POSTGRES_URL is set, for example:
//!     LIPI_TEST_POSTGRES_URL=postgres://postgres@localhost:5432/postgres cargo test -p lipi --test postgres
//! Use LIPI_BLESS=1 to accept new output.

use std::path::Path;
use std::process::Command;

#[test]
fn postgres_database_layer() {
    let Ok(url) = std::env::var("LIPI_TEST_POSTGRES_URL") else {
        eprintln!("skipped: set LIPI_TEST_POSTGRES_URL to run the PostgreSQL test");
        return;
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lipi"))
        .args(["run", "tests/postgres/database.lipi"])
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("LIPI_TEST_POSTGRES_URL", url)
        .output()
        .expect("failed to run lipi");
    let mut actual = String::from_utf8_lossy(&out.stdout).into_owned();
    actual.push_str(&String::from_utf8_lossy(&out.stderr));
    let actual = actual.replace("\r\n", "\n");
    let expected_path = root.join("tests/postgres/database.out");
    if std::env::var_os("LIPI_BLESS").is_some() {
        std::fs::write(&expected_path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(expected, actual);
}
