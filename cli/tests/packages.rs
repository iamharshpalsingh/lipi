//! End-to-end test of the package manager: folder, registry and git
//! dependencies, lipi.lock, integrity checks, update and remove.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Env {
    home: PathBuf,
}

impl Env {
    fn lipi(&self, dir: &Path, args: &[&str]) -> (String, bool) {
        let out = Command::new(env!("CARGO_BIN_EXE_lipi"))
            .args(args)
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .env("LIPI_HOME", &self.home)
            .env_remove("LIPI_REGISTRY")
            .output()
            .expect("run lipi");
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (text.replace("\r\n", "\n"), out.status.success())
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap().status.success();
    assert!(ok, "git {args:?} failed");
}

#[test]
fn package_manager_end_to_end() {
    let root = std::env::temp_dir().join(format!("lipi-pkg-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let env = Env { home: root.join("home") };
    let registry = root.join("registry");
    let registry_url = format!("file:///{}", registry.to_string_lossy().replace('\\', "/").trim_start_matches('/'));

    // Two folder packages: greeter depends on shout.
    write(&root.join("libs/shout/lipi.json"), r#"{"name": "shout", "version": "1.0.0", "main": "main.lipi"}"#);
    write(&root.join("libs/shout/main.lipi"), "shout(text)\n    return text.upper() + \"!\"\n\nexport shout\n");
    write(&root.join("libs/greeter/lipi.json"), r#"{"name": "greeter", "version": "0.2.0", "main": "main.lipi", "dependencies": {"shout": "path:../shout"}}"#);
    write(&root.join("libs/greeter/main.lipi"), "use shout\n\ngreet(name)\n    return shout.shout(\"hello \" + name)\n\nexport greet\n");

    // A registry package with two versions.
    let slug = root.join("slug");
    write(&slug.join("main.lipi"), "slugify(text)\n    return text.lower().replace(\" \", \"-\")\n\nexport slugify\n");
    for version in ["1.2.0", "1.3.0"] {
        write(&slug.join("lipi.json"), &format!(r#"{{"name": "slug", "version": "{version}", "main": "main.lipi"}}"#));
        let (out, ok) = env.lipi(&slug, &["publish", "--registry", &registry_url]);
        assert!(ok, "{out}");
        assert!(out.contains(&format!("published slug@{version}")), "{out}");
    }
    let (out, ok) = env.lipi(&slug, &["publish", "--registry", &registry_url]);
    assert!(!ok && out.contains("LIP7007"), "republishing must fail: {out}");

    // A git package with a tag.
    let colors = root.join("colors");
    write(&colors.join("lipi.json"), r#"{"name": "colors", "version": "1.0.0", "main": "main.lipi"}"#);
    write(&colors.join("main.lipi"), "red = \"#D2452A\"\n\nexport red\n");
    git(&colors, &["init", "-q"]);
    git(&colors, &["add", "."]);
    git(&colors, &["commit", "-q", "-m", "colors"]);
    git(&colors, &["tag", "v1.0.0"]);

    // The app.
    let (out, ok) = env.lipi(&root, &["new", "app"]);
    assert!(ok, "{out}");
    let app = root.join("app");
    let manifest = std::fs::read_to_string(app.join("lipi.json")).unwrap();
    std::fs::write(app.join("lipi.json"), manifest.replacen("\"dependencies\"", &format!("\"registry\": \"{registry_url}\",\n  \"dependencies\""), 1)).unwrap();
    let (out, ok) = env.lipi(&app, &["install", "../libs/greeter"]);
    assert!(ok, "{out}");
    assert!(out.contains("added   greeter@0.2.0 (path)") && out.contains("added   shout@1.0.0 (path)"), "{out}");
    let (out, ok) = env.lipi(&app, &["install", "slug@~1.2"]);
    assert!(ok, "{out}");
    assert!(out.contains("slug@1.2.0 (registry)"), "~1.2 must pick 1.2.0: {out}");
    let colors_url = format!("git:file:///{}#v1.0.0", colors.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
    let (out, ok) = env.lipi(&app, &["install", &colors_url]);
    assert!(ok, "{out}");

    write(&app.join("src/main.lipi"), "use greeter\nuse slug\nuse colors\n\nshow greeter.greet(\"dezy\")\nshow slug.slugify(\"Hello World\")\nshow colors.red\n");
    let expected = "HELLO DEZY!\nhello-world\n#D2452A\n";
    let (out, ok) = env.lipi(&app, &["run"]);
    assert!(ok && out == expected, "{out}");

    let lock = std::fs::read_to_string(app.join("lipi.lock")).unwrap();
    for name in ["greeter", "shout", "slug", "colors"] {
        assert!(lock.contains(&format!("\"{name}\"")), "{lock}");
    }
    assert!(lock.contains("\"version\": \"1.2.0\"") && lock.contains("sha256-") && lock.contains("#"), "{lock}");

    // Reinstall exactly from the lock.
    std::fs::remove_dir_all(app.join("lipi_modules")).unwrap();
    let (out, ok) = env.lipi(&app, &["install", "--frozen"]);
    assert!(ok, "{out}");
    let (out, _) = env.lipi(&app, &["run"]);
    assert_eq!(out, expected);

    // A registry package whose hash doesn't match lipi.lock is refused.
    let mut parsed: serde_json::Value = serde_json::from_str(&lock).unwrap();
    parsed["packages"]["slug"]["integrity"] = serde_json::Value::String(format!("sha256-{}", "0".repeat(64)));
    std::fs::write(app.join("lipi.lock"), serde_json::to_string_pretty(&parsed).unwrap()).unwrap();
    let (out, ok) = env.lipi(&app, &["install"]);
    assert!(!ok && out.contains("LIP7001") && out.contains("slug"), "tampering must be detected: {out}");
    std::fs::write(app.join("lipi.lock"), &lock).unwrap();

    // Widening the range and updating picks the newest version.
    let manifest = std::fs::read_to_string(app.join("lipi.json")).unwrap();
    std::fs::write(app.join("lipi.json"), manifest.replace("~1.2", "^1.2")).unwrap();
    let (out, ok) = env.lipi(&app, &["update", "slug"]);
    assert!(ok && out.contains("changed slug@1.3.0"), "{out}");

    // Removing a package also removes what only it needed.
    let (out, ok) = env.lipi(&app, &["remove", "greeter"]);
    assert!(ok, "{out}");
    assert!(!app.join("lipi_modules/greeter").exists() && !app.join("lipi_modules/shout").exists());
    assert!(!std::fs::read_to_string(app.join("lipi.lock")).unwrap().contains("\"shout\""));

    // Unknown packages and impossible ranges explain themselves.
    let (out, ok) = env.lipi(&app, &["install", "nothing-here"]);
    assert!(!ok && out.contains("LIP7002"), "{out}");
    let (out, ok) = env.lipi(&app, &["install", "slug@^9"]);
    assert!(!ok && out.contains("LIP7003") && out.contains("1.3.0"), "{out}");

    let _ = std::fs::remove_dir_all(&root);
}
