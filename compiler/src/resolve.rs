//! Finding the file behind a `use`. Shared by the interpreter and `lipi build`,
//! so both see exactly the same modules.

use std::path::{Path, PathBuf};

pub enum Resolved {
    /// A LiPi file (the path is shown in messages as-is).
    File(PathBuf),
    /// A standard module such as `math`.
    Std(String),
}

pub struct UseError {
    pub code: &'static str,
    pub message: String,
    pub hint: String,
}

/// Resolve `use <source>` written in `current_file`.
///
/// - A path (`"./helpers.lipi"`, `"../lib/x"`) is relative to the current file.
/// - A name (`math`, `utils.strings`) is looked up, in order, as a project file
///   next to the current file or in `src/`, then as a package in
///   `lipi_modules/`, then as a standard module. A name that matches both a
///   project file and a package is an error.
pub fn resolve_use(source: &str, current_file: &str, project_root: &Path, std_modules: &[&str]) -> Result<Resolved, UseError> {
    let is_path = source.starts_with('.') || source.starts_with('/') || source.ends_with(".lipi") || source.contains(['/', '\\']);
    let base = Path::new(current_file).parent().map(Path::to_path_buf).unwrap_or_default();
    if is_path {
        let mut path = base.join(source);
        if path.extension().is_none_or(|e| e != "lipi") {
            path = PathBuf::from(format!("{}.lipi", path.to_string_lossy()));
        }
        if !path.is_file() {
            return Err(UseError {
                code: "LIP3001",
                message: format!("module not found: \"{}\"", path.to_string_lossy()),
                hint: format!("Paths in `use` are relative to the file that uses them ({current_file})."),
            });
        }
        return Ok(Resolved::File(path));
    }
    let rel = format!("{}.lipi", source.replace('.', "/"));
    let local = [base.join(&rel), project_root.join("src").join(&rel)].into_iter().find(|p| p.is_file());
    let package = package_entry(&project_root.join("lipi_modules"), source);
    match (local, package) {
        (Some(l), Some(p)) => Err(UseError {
            code: "LIP3004",
            message: format!("\"{source}\" matches both a project file and a package"),
            hint: format!("Found {} and {}. Rename your file, or use it by path: use \"./{rel}\"", l.display(), p.display()),
        }),
        (Some(path), None) | (None, Some(path)) => Ok(Resolved::File(path)),
        (None, None) if std_modules.contains(&source) => Ok(Resolved::Std(source.to_string())),
        (None, None) => Err(UseError {
            code: "LIP3001",
            message: format!("module not found: \"{source}\""),
            hint: format!("Looked for {rel} next to this file and in src/, and for a package in lipi_modules/. Install packages with `lipi install`."),
        }),
    }
}

/// The entry file of an installed package: `main` from its lipi.json, else
/// main.lipi or src/main.lipi (or a single-file package `lipi_modules/<name>.lipi`).
pub fn package_entry(modules: &Path, name: &str) -> Option<PathBuf> {
    let dir = modules.join(name);
    if let Ok(text) = std::fs::read_to_string(dir.join("lipi.json")) {
        if let Some(main) = json_string_field(&text, "main") {
            let file = dir.join(main);
            if file.is_file() {
                return Some(file);
            }
        }
    }
    [dir.join("main.lipi"), dir.join("src").join("main.lipi"), modules.join(format!("{name}.lipi"))].into_iter().find(|p| p.is_file())
}

/// A hint for a program file that doesn't exist. A common slip is running
/// `lipi main.lipi` in a new project, where the file is in src/.
pub fn missing_file_hint(path: &Path) -> String {
    if let Some(name) = path.file_name() {
        let in_src = Path::new("src").join(name);
        if !path.starts_with("src") && in_src.is_file() {
            return format!(
                "There's a {} in the src folder. Run it with `lipi {}`, or run the whole project with `lipi run`.",
                name.to_string_lossy(),
                in_src.display()
            );
        }
    }
    "Check the file name and the folder you're in.".to_string()
}

/// The folder containing `lipi.json`, searching upward from the main file.
pub fn find_project_root(main: &Path) -> PathBuf {
    // The file may not exist yet (canonicalize fails); still start from an absolute path.
    let start = main
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().map(|d| d.join(main)).unwrap_or_else(|_| main.to_path_buf()));
    let mut dir = start.parent();
    while let Some(d) = dir {
        if d.join("lipi.json").is_file() {
            return d.to_path_buf();
        }
        dir = d.parent();
    }
    start.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

/// A top-level `"key": "value"` from a small JSON document such as lipi.json.
/// (The compiler has no JSON dependency; manifests are simple.)
fn json_string_field(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\"");
    let at = text.find(&pattern)? + pattern.len();
    let rest = text[at..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_string_fields_from_manifests() {
        let text = "{\n  \"name\": \"demo\",\n  \"main\" : \"src/app.lipi\"\n}";
        assert_eq!(json_string_field(text, "main").as_deref(), Some("src/app.lipi"));
        assert_eq!(json_string_field(text, "version"), None);
    }
}
