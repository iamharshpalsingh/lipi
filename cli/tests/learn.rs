//! The LiPi course (docs/learn/*.md) must be correct:
//! - every ```lipi block must pass `lipi check`, unless an ```output block
//!   follows it;
//! - when an ```output block follows, running the code must print exactly that
//!   (including error messages, which is how the course teaches them).
//! Examples are run as main.lipi in an empty folder, so error locations read
//! "main.lipi:line:col".

use std::path::Path;
use std::process::Command;

struct Example {
    file: String,
    line: usize,
    code: String,
    output: Option<String>,
}

fn examples(text: &str, file: &str) -> Vec<Example> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "```lipi" {
            let start = i + 1;
            let mut j = start;
            while j < lines.len() && lines[j].trim() != "```" {
                j += 1;
            }
            let code = lines[start..j].join("\n") + "\n";
            // An ```output block right after (blank lines allowed) is the expected output.
            let mut k = j + 1;
            while k < lines.len() && lines[k].trim().is_empty() {
                k += 1;
            }
            let mut output = None;
            if k < lines.len() && lines[k].trim() == "```output" {
                let mut m = k + 1;
                while m < lines.len() && lines[m].trim() != "```" {
                    m += 1;
                }
                output = Some(lines[k + 1..m].join("\n"));
                j = m;
            }
            out.push(Example { file: file.to_string(), line: start, code, output });
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim_end().to_string()
}

#[test]
fn every_course_example_works() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dir = root.join("docs").join("learn");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "md")).collect();
    files.sort();
    let work = std::env::temp_dir().join(format!("lipi-learn-{}", std::process::id()));
    let mut failures = Vec::new();
    let mut count = 0;
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path).unwrap();
        for ex in examples(&text, &name) {
            count += 1;
            let folder = work.join(format!("{}-{}", ex.file.trim_end_matches(".md"), ex.line));
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("main.lipi"), &ex.code).unwrap();
            let args: &[&str] = if ex.output.is_some() { &["main.lipi"] } else { &["check", "main.lipi"] };
            let run = Command::new(env!("CARGO_BIN_EXE_lipi")).args(args).current_dir(&folder).env("NO_COLOR", "1").output().unwrap();
            let mut actual = String::from_utf8_lossy(&run.stdout).into_owned();
            actual.push_str(&String::from_utf8_lossy(&run.stderr));
            let actual = normalize(&actual);
            match &ex.output {
                Some(expected) if normalize(expected) != actual => {
                    failures.push(format!("--- {} line {}\n{}### expected:\n{}\n### actual:\n{}\n", ex.file, ex.line, ex.code, normalize(expected), actual))
                }
                None if !run.status.success() => failures.push(format!("--- {} line {} doesn't pass `lipi check`:\n{}{}\n", ex.file, ex.line, ex.code, actual)),
                _ => {}
            }
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    assert!(count > 50, "only {count} examples found");
    assert!(failures.is_empty(), "{} course example(s) are wrong:\n\n{}", failures.len(), failures.join("\n"));
}
