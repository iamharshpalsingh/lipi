//! The LiPi package manager: `lipi install`, `remove`, `update` and `publish`.
//!
//! Dependencies live in `lipi.json`:
//!
//! ```json
//! "dependencies": {
//!   "utils":   "path:../utils",                              // a folder
//!   "colors":  "git:https://github.com/me/lipi-colors#v1.0",  // a git repository (tag, branch or commit)
//!   "slugify": "^1.2.0"                                       // a registry version range (semver)
//! }
//! ```
//!
//! Resolution is deterministic (sorted, breadth-first, one version per name).
//! `lipi.lock` records the exact version, source and a SHA-256 integrity hash of
//! every package. Later installs reuse the lock and fail if the content changed.
//!
//! A registry is a URL (`https://...` or `file://...`) with this static layout:
//!
//! ```text
//! <registry>/<name>/index.json           {"name": ..., "versions": {"1.2.0": {"file": "...tar.gz", "integrity": "sha256-...", "dependencies": {...}}}}
//! <registry>/<name>/<name>-<version>.tar.gz
//! ```
//!
//! `lipi publish --registry file://...` writes that layout. Downloads are cached
//! in `~/.lipi/cache` (or `$LIPI_HOME/cache`) and verified on every use.

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub struct Failure {
    code: &'static str,
    message: String,
    hint: Option<String>,
}

impl Failure {
    fn new(code: &'static str, message: impl Into<String>) -> Failure {
        Failure { code, message: message.into(), hint: None }
    }

    fn hint(mut self, hint: impl Into<String>) -> Failure {
        self.hint = Some(hint.into());
        self
    }

    pub fn print(&self) {
        eprintln!("ERROR {}: {}", self.code, self.message);
        if let Some(h) = &self.hint {
            eprintln!("\nHint: {h}");
        }
    }
}

type Res<T> = Result<T, Failure>;

fn io(what: &str, e: std::io::Error) -> Failure {
    Failure::new("LIP7006", format!("{what}: {e}"))
}

// ----- files and hashing ----------------------------------------------------------

fn ignored(name: &str) -> bool {
    matches!(name, "lipi_modules" | ".git" | "target" | "node_modules" | ".lipi-tmp" | ".env" | "lipi.lock") || name.ends_with(".db")
}

/// Files of a package, sorted, as (relative path with `/`, full path).
fn package_files(root: &Path) -> Res<Vec<(String, PathBuf)>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Res<()> {
        let entries = std::fs::read_dir(dir).map_err(|e| io(&format!("couldn't read {}", dir.display()), e))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if ignored(&name) {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                out.push((rel, path));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256-{}", hex(&Sha256::digest(bytes)))
}

/// A content hash of a folder that doesn't depend on timestamps or the OS.
fn hash_dir(root: &Path) -> Res<String> {
    let mut h = Sha256::new();
    for (rel, path) in package_files(root)? {
        let data = std::fs::read(&path).map_err(|e| io(&format!("couldn't read {}", path.display()), e))?;
        h.update(rel.as_bytes());
        h.update([0]);
        h.update((data.len() as u64).to_le_bytes());
        h.update(&data);
    }
    Ok(format!("sha256-{}", hex(&h.finalize())))
}

fn copy_package(from: &Path, to: &Path) -> Res<()> {
    for (rel, path) in package_files(from)? {
        let dest = to.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io("couldn't create a folder", e))?;
        }
        std::fs::copy(&path, &dest).map_err(|e| io(&format!("couldn't copy {rel}"), e))?;
    }
    Ok(())
}

/// A deterministic .tar.gz of a package folder.
fn pack(root: &Path) -> Res<Vec<u8>> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), flate2::Compression::default()));
    for (rel, path) in package_files(root)? {
        let data = std::fs::read(&path).map_err(|e| io(&format!("couldn't read {rel}"), e))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, &rel, data.as_slice()).map_err(|e| io("couldn't build the package archive", e))?;
    }
    let gz = tar.into_inner().map_err(|e| io("couldn't build the package archive", e))?;
    gz.finish().map_err(|e| io("couldn't compress the package archive", e))
}

fn unpack(bytes: &[u8], to: &Path) -> Res<()> {
    std::fs::create_dir_all(to).map_err(|e| io("couldn't create a folder", e))?;
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let entries = archive.entries().map_err(|e| io("couldn't read the package archive", e))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| io("couldn't read the package archive", e))?;
        // unpack_in refuses paths that would escape the target folder.
        entry.unpack_in(to).map_err(|e| io("couldn't unpack the package archive", e))?;
    }
    Ok(())
}

fn lipi_home() -> PathBuf {
    if let Some(h) = std::env::var_os("LIPI_HOME") {
        return PathBuf::from(h);
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".lipi")
}

// ----- manifests --------------------------------------------------------------------

fn read_json(path: &Path) -> Res<Map<String, Value>> {
    let text = std::fs::read_to_string(path).map_err(|e| io(&format!("couldn't read {}", path.display()), e))?;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(Failure::new("LIP7005", format!("{} should contain a JSON object", path.display()))),
        Err(e) => Err(Failure::new("LIP7005", format!("{} isn't valid JSON: {e}", path.display()))),
    }
}

fn write_json(path: &Path, value: &Value) -> Res<()> {
    let text = serde_json::to_string_pretty(value).unwrap_or_default() + "\n";
    std::fs::write(path, text).map_err(|e| io(&format!("couldn't write {}", path.display()), e))
}

fn dependencies(m: &Map<String, Value>, file: &Path) -> Res<BTreeMap<String, String>> {
    match m.get("dependencies") {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Object(d)) => d
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => Ok((k.clone(), s.clone())),
                _ => Err(Failure::new("LIP7005", format!("in {}, the dependency \"{k}\" should be a String", file.display()))),
            })
            .collect(),
        Some(_) => Err(Failure::new("LIP7005", format!("in {}, \"dependencies\" should be an Object", file.display()))),
    }
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// The folder with lipi.json, searching upward from the current folder.
pub fn project_dir() -> Res<PathBuf> {
    let mut dir = std::env::current_dir().map_err(|e| io("couldn't find the current folder", e))?;
    loop {
        if dir.join("lipi.json").is_file() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err(Failure::new("LIP7005", "this folder isn't a LiPi project (no lipi.json found)").hint("Create one with `lipi new my-app`, or run this inside a project."));
        }
    }
}

// ----- sources ---------------------------------------------------------------------

enum Source {
    Path(PathBuf),
    Git { url: String, reference: Option<String> },
    Registry(semver::VersionReq),
}

fn parse_spec(base: &Path, spec: &str) -> Res<Source> {
    if let Some(p) = spec.strip_prefix("path:") {
        return Ok(Source::Path(base.join(p)));
    }
    if spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/') {
        return Ok(Source::Path(base.join(spec)));
    }
    if let Some(rest) = spec.strip_prefix("git:") {
        let (url, reference) = match rest.rsplit_once('#') {
            Some((u, r)) => (u.to_string(), Some(r.to_string())),
            None => (rest.to_string(), None),
        };
        return Ok(Source::Git { url, reference });
    }
    semver::VersionReq::parse(spec).map(Source::Registry).map_err(|_| {
        Failure::new("LIP7005", format!("\"{spec}\" isn't a valid dependency"))
            .hint("Use a version range like ^1.2.0, a folder like path:../utils, or a repository like git:https://github.com/me/pkg#v1.0")
    })
}

fn registry_url(project: &Map<String, Value>) -> Res<String> {
    if let Ok(url) = std::env::var("LIPI_REGISTRY") {
        return Ok(url.trim_end_matches('/').to_string());
    }
    match project.get("registry") {
        Some(Value::String(url)) => Ok(url.trim_end_matches('/').to_string()),
        _ => Err(Failure::new("LIP7002", "no package registry is configured")
            .hint("The public LiPi Registry isn't online yet. Set \"registry\" in lipi.json (or LIPI_REGISTRY) to a registry URL such as file:///C:/my-registry, or use path: and git: dependencies.")),
    }
}

/// A local path for `file://` URLs and bare paths.
fn local_path(url: &str) -> Option<PathBuf> {
    if let Some(rest) = url.strip_prefix("file://") {
        let rest = rest.strip_prefix('/').filter(|r| r.as_bytes().get(1) == Some(&b':')).unwrap_or(rest);
        return Some(PathBuf::from(rest));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        None
    } else {
        Some(PathBuf::from(url))
    }
}

fn download(url: &str) -> Res<Vec<u8>> {
    if let Some(path) = local_path(url) {
        return std::fs::read(&path).map_err(|e| Failure::new("LIP7006", format!("couldn't read {}: {e}", path.display())));
    }
    let out = Command::new("curl")
        .args(["-sSfL", "--max-time", "120", url])
        .output()
        .map_err(|e| Failure::new("LIP7006", format!("couldn't start curl to download {url}: {e}")))?;
    if !out.status.success() {
        return Err(Failure::new("LIP7006", format!("couldn't download {url}: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    Ok(out.stdout)
}

fn fetch_index(registry: &str, name: &str) -> Res<Map<String, Value>> {
    let bytes = download(&format!("{registry}/{name}/index.json")).map_err(|_| {
        Failure::new("LIP7002", format!("package \"{name}\" wasn't found in the registry {registry}")).hint("Check the name, or publish it with `lipi publish`.")
    })?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(m)) => Ok(m),
        _ => Err(Failure::new("LIP7006", format!("the registry's index for \"{name}\" isn't valid JSON"))),
    }
}

// ----- resolution -------------------------------------------------------------------

#[derive(Clone)]
struct Locked {
    version: String,
    source: String,
    integrity: String,
}

struct Resolved {
    version: String,
    source: String,
    integrity: String,
    dir: PathBuf,
    deps: BTreeMap<String, String>,
    spec: String,
    required_by: String,
}

struct Ctx {
    project: PathBuf,
    manifest: Map<String, Value>,
    tmp: PathBuf,
    counter: usize,
}

impl Ctx {
    fn temp(&mut self) -> Res<PathBuf> {
        self.counter += 1;
        let dir = self.tmp.join(self.counter.to_string());
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| io("couldn't create a temporary folder", e))?;
        Ok(dir)
    }
}

fn package_info(dir: &Path) -> Res<(Option<String>, String, BTreeMap<String, String>)> {
    let file = dir.join("lipi.json");
    if !file.is_file() {
        return Ok((None, "0.0.0".into(), BTreeMap::new()));
    }
    let m = read_json(&file)?;
    let name = m.get("name").and_then(Value::as_str).map(String::from);
    let version = m.get("version").and_then(Value::as_str).unwrap_or("0.0.0").to_string();
    Ok((name, version, dependencies(&m, &file)?))
}

fn git(args: &[&str], dir: Option<&Path>) -> Res<String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.arg("-C").arg(d);
    }
    let out = cmd.args(args).output().map_err(|e| Failure::new("LIP7006", format!("couldn't run git: {e}")).hint("Install Git to use git: dependencies."))?;
    if !out.status.success() {
        return Err(Failure::new("LIP7006", format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim())));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn mismatch(name: &str) -> Failure {
    Failure::new("LIP7001", format!("dependency checksum mismatch for \"{name}\": its content differs from lipi.lock"))
        .hint("Someone changed the published package, or the download was corrupted. If the change is expected, run `lipi update` for it.")
}

fn fetch(ctx: &mut Ctx, name: &str, spec: &str, base: &Path, locked: Option<&Locked>) -> Res<Resolved> {
    let source = parse_spec(base, spec)?;
    let resolved = match source {
        Source::Path(path) => {
            if !path.is_dir() {
                return Err(Failure::new("LIP7002", format!("the folder for \"{name}\" wasn't found: {}", path.display())));
            }
            let (_, version, deps) = package_info(&path)?;
            // Local folders are expected to change, so their hash is recorded but not enforced.
            Resolved { version, source: format!("path:{}", spec.trim_start_matches("path:")), integrity: hash_dir(&path)?, dir: path, deps, spec: spec.into(), required_by: String::new() }
        }
        Source::Git { url, reference } => {
            let dir = ctx.temp()?;
            let dir_s = dir.to_string_lossy().to_string();
            let locked_commit = locked.and_then(|l| l.source.rsplit_once('#')).filter(|(u, _)| *u == format!("git:{url}")).map(|(_, c)| c.to_string());
            match (&locked_commit, &reference) {
                (Some(commit), _) => {
                    git(&["clone", "--quiet", &url, &dir_s], None)?;
                    git(&["checkout", "--quiet", commit], Some(&dir))?;
                }
                (None, Some(r)) => {
                    git(&["clone", "--quiet", "--depth", "1", "--branch", r, &url, &dir_s], None)?;
                }
                (None, None) => {
                    git(&["clone", "--quiet", "--depth", "1", &url, &dir_s], None)?;
                }
            }
            let commit = git(&["rev-parse", "HEAD"], Some(&dir))?;
            let integrity = hash_dir(&dir)?;
            if let Some(l) = locked {
                if locked_commit.as_deref() == Some(commit.as_str()) && l.integrity != integrity {
                    return Err(mismatch(name));
                }
            }
            let (_, version, deps) = package_info(&dir)?;
            Resolved { version, source: format!("git:{url}#{commit}"), integrity, dir, deps, spec: spec.into(), required_by: String::new() }
        }
        Source::Registry(req) => {
            let registry = registry_url(&ctx.manifest)?;
            let index = fetch_index(&registry, name)?;
            let versions = index.get("versions").and_then(Value::as_object).cloned().unwrap_or_default();
            let mut candidates: Vec<semver::Version> = versions.keys().filter_map(|v| semver::Version::parse(v).ok()).filter(|v| req.matches(v)).collect();
            candidates.sort();
            let chosen = match locked.and_then(|l| semver::Version::parse(&l.version).ok()).filter(|v| candidates.contains(v)) {
                Some(v) => v,
                None => candidates.pop().ok_or_else(|| {
                    let mut all: Vec<&String> = versions.keys().collect();
                    all.sort();
                    Failure::new("LIP7003", format!("no version of \"{name}\" matches {spec}"))
                        .hint(format!("Available: {}", all.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")))
                })?,
            };
            let entry = &versions[&chosen.to_string()];
            let expected = entry.get("integrity").and_then(Value::as_str).unwrap_or_default().to_string();
            let file = entry.get("file").and_then(Value::as_str).unwrap_or_default();
            let cache = lipi_home().join("cache").join("registry").join(format!("{}.tar.gz", expected.trim_start_matches("sha256-")));
            let bytes = match std::fs::read(&cache) {
                Ok(b) if sha256_bytes(&b) == expected => b,
                _ => {
                    let url = if file.contains("://") { file.to_string() } else { format!("{registry}/{name}/{file}") };
                    let b = download(&url)?;
                    if sha256_bytes(&b) != expected {
                        return Err(Failure::new("LIP7001", format!("dependency checksum mismatch for \"{name}\" {chosen}: the download doesn't match the registry's hash"))
                            .hint("The package file was changed or corrupted. Try again later, or contact the package's publisher."));
                    }
                    if let Some(parent) = cache.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(&cache, &b);
                    b
                }
            };
            if let Some(l) = locked {
                if l.version == chosen.to_string() && l.integrity != expected {
                    return Err(mismatch(name));
                }
            }
            let dir = ctx.temp()?;
            unpack(&bytes, &dir)?;
            let (_, _, deps) = package_info(&dir)?;
            Resolved { version: chosen.to_string(), source: format!("registry:{registry}"), integrity: expected, dir, deps, spec: spec.into(), required_by: String::new() }
        }
    };
    Ok(resolved)
}

fn read_lock(project: &Path) -> Res<BTreeMap<String, Locked>> {
    let file = project.join("lipi.lock");
    if !file.is_file() {
        return Ok(BTreeMap::new());
    }
    let m = read_json(&file)?;
    let packages = m.get("packages").and_then(Value::as_object).cloned().unwrap_or_default();
    Ok(packages
        .into_iter()
        .filter_map(|(name, v)| {
            Some((
                name,
                Locked {
                    version: v.get("version")?.as_str()?.to_string(),
                    source: v.get("source")?.as_str()?.to_string(),
                    integrity: v.get("integrity")?.as_str()?.to_string(),
                },
            ))
        })
        .collect())
}

fn resolve(ctx: &mut Ctx, lock: &BTreeMap<String, Locked>, refresh: &BTreeSet<String>, refresh_all: bool) -> Res<BTreeMap<String, Resolved>> {
    let file = ctx.project.join("lipi.json");
    let root = dependencies(&ctx.manifest, &file)?;
    let mut queue: VecDeque<(String, String, PathBuf, String)> = root.into_iter().map(|(n, s)| (n, s, ctx.project.clone(), "your project".to_string())).collect();
    let mut resolved: BTreeMap<String, Resolved> = BTreeMap::new();
    while let Some((name, spec, base, by)) = queue.pop_front() {
        if !valid_name(&name) {
            return Err(Failure::new("LIP7005", format!("\"{name}\" isn't a valid package name")).hint("Package names use lowercase letters, digits, - _ and ."));
        }
        if let Some(existing) = resolved.get(&name) {
            let compatible = match parse_spec(&base, &spec)? {
                Source::Registry(req) => semver::Version::parse(&existing.version).is_ok_and(|v| req.matches(&v)) && existing.source.starts_with("registry:"),
                _ => existing.spec == spec,
            };
            if !compatible {
                return Err(Failure::new("LIP7004", format!(
                    "version conflict for \"{name}\": {by} needs {spec}, but {} needs {} (resolved to {})",
                    existing.required_by, existing.spec, existing.version
                ))
                .hint("Change one of the requirements so they agree. LiPi installs one version of each package."));
            }
            continue;
        }
        let locked = if refresh_all || refresh.contains(&name) { None } else { lock.get(&name) };
        let mut r = fetch(ctx, &name, &spec, &base, locked)?;
        r.required_by = by;
        for (dn, ds) in &r.deps {
            queue.push_back((dn.clone(), ds.clone(), r.dir.clone(), format!("\"{name}\"")));
        }
        resolved.insert(name, r);
    }
    Ok(resolved)
}

fn write_modules(ctx: &Ctx, resolved: &BTreeMap<String, Resolved>) -> Res<()> {
    let modules = ctx.project.join("lipi_modules");
    std::fs::create_dir_all(&modules).map_err(|e| io("couldn't create lipi_modules", e))?;
    if let Ok(entries) = std::fs::read_dir(&modules) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !resolved.contains_key(&name) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    for (name, r) in resolved {
        let dest = modules.join(name);
        let _ = std::fs::remove_dir_all(&dest);
        copy_package(&r.dir, &dest)?;
    }
    let packages: Map<String, Value> = resolved
        .iter()
        .map(|(name, r)| (name.clone(), json!({"version": r.version, "source": r.source, "integrity": r.integrity})))
        .collect();
    write_json(&ctx.project.join("lipi.lock"), &json!({"lockfileVersion": 1, "packages": packages}))
}

fn context(project: PathBuf) -> Res<Ctx> {
    let manifest = read_json(&project.join("lipi.json"))?;
    let tmp = project.join(".lipi-tmp");
    Ok(Ctx { project, manifest, tmp, counter: 0 })
}

fn finish(ctx: &Ctx, resolved: &BTreeMap<String, Resolved>, before: &BTreeMap<String, Locked>) {
    let _ = std::fs::remove_dir_all(&ctx.tmp);
    if resolved.is_empty() {
        println!("No dependencies to install.");
        return;
    }
    for (name, r) in resolved {
        let label = match r.source.split_once(':').map(|(k, _)| k) {
            Some("git") => format!("git {}", &r.source.rsplit_once('#').map(|(_, c)| c.get(..7).unwrap_or(c)).unwrap_or("")),
            Some(kind) => kind.to_string(),
            None => String::new(),
        };
        let change = match before.get(name) {
            None => "added",
            Some(l) if l.version != r.version || l.source != r.source => "changed",
            Some(_) => "ok",
        };
        println!("  {change:<7} {name}@{} ({label})", r.version);
    }
    println!("{} package{} installed into lipi_modules/, lipi.lock updated", resolved.len(), if resolved.len() == 1 { "" } else { "s" });
}

fn run_install(ctx: &mut Ctx, refresh: &BTreeSet<String>, refresh_all: bool) -> Res<()> {
    let lock = read_lock(&ctx.project)?;
    let result = resolve(ctx, &lock, refresh, refresh_all).and_then(|resolved| write_modules(ctx, &resolved).map(|_| resolved));
    match result {
        Ok(resolved) => {
            finish(ctx, &resolved, &lock);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&ctx.tmp);
            Err(e)
        }
    }
}

// ----- commands ---------------------------------------------------------------------

/// `lipi install` or `lipi install <spec>...`
pub fn install(args: &[String]) -> Res<()> {
    let project = project_dir()?;
    let mut ctx = context(project.clone())?;
    let frozen = args.iter().any(|a| a == "--frozen");
    let specs: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if frozen {
        let lock = read_lock(&project)?;
        let deps = dependencies(&ctx.manifest, &project.join("lipi.json"))?;
        if let Some(missing) = deps.keys().find(|d| !lock.contains_key(*d)) {
            return Err(Failure::new("LIP7005", format!("lipi.lock doesn't include \"{missing}\"")).hint("Run `lipi install` without --frozen and commit the updated lipi.lock."));
        }
    }
    let mut added = BTreeSet::new();
    for spec in specs {
        let (name, dep_spec) = new_dependency(&mut ctx, spec)?;
        let deps = ctx.manifest.entry("dependencies").or_insert_with(|| json!({}));
        if let Value::Object(d) = deps {
            d.insert(name.clone(), Value::String(dep_spec));
        }
        added.insert(name);
    }
    if !added.is_empty() {
        write_json(&project.join("lipi.json"), &Value::Object(ctx.manifest.clone()))?;
    }
    run_install(&mut ctx, &added, false)
}

/// Work out the name and lipi.json entry for `lipi install <arg>`.
fn new_dependency(ctx: &mut Ctx, arg: &str) -> Res<(String, String)> {
    let is_path = arg.starts_with("path:") || arg.starts_with("./") || arg.starts_with("../") || Path::new(arg).is_dir();
    if is_path || arg.starts_with("git:") {
        let spec = if is_path && !arg.starts_with("path:") { format!("path:{}", arg.replace('\\', "/")) } else { arg.to_string() };
        let fetched = fetch(ctx, "new-package", &spec, &ctx.project.clone(), None)?;
        let fallback = match parse_spec(&ctx.project, &spec)? {
            Source::Path(p) => p.canonicalize().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase())).unwrap_or_default(),
            Source::Git { url, .. } => url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or("").to_lowercase(),
            Source::Registry(_) => String::new(),
        };
        let name = package_info(&fetched.dir)?.0.unwrap_or(fallback);
        if !valid_name(&name) {
            return Err(Failure::new("LIP7005", format!("couldn't work out a valid package name for {arg}")).hint("Give the package a \"name\" in its lipi.json."));
        }
        return Ok((name, spec));
    }
    let (name, req) = match arg.split_once('@') {
        Some((n, r)) => (n.to_string(), Some(r.to_string())),
        None => (arg.to_string(), None),
    };
    if !valid_name(&name) {
        return Err(Failure::new("LIP7005", format!("\"{name}\" isn't a valid package name")));
    }
    let req = match req {
        Some(r) => {
            semver::VersionReq::parse(&r).map_err(|_| Failure::new("LIP7005", format!("\"{r}\" isn't a valid version range")).hint("For example: ^1.2.0, ~1.2 or 1.2.3"))?;
            r
        }
        None => {
            let registry = registry_url(&ctx.manifest)?;
            let index = fetch_index(&registry, &name)?;
            let latest = index
                .get("versions")
                .and_then(Value::as_object)
                .and_then(|v| v.keys().filter_map(|k| semver::Version::parse(k).ok()).filter(|v| v.pre.is_empty()).max())
                .ok_or_else(|| Failure::new("LIP7003", format!("\"{name}\" has no published versions")))?;
            format!("^{latest}")
        }
    };
    Ok((name, req))
}

/// `lipi remove <name>...`
pub fn remove(args: &[String]) -> Res<()> {
    if args.is_empty() {
        return Err(Failure::new("LIP7005", "which package should I remove?").hint("For example: lipi remove utils"));
    }
    let project = project_dir()?;
    let mut ctx = context(project.clone())?;
    let deps = ctx.manifest.entry("dependencies").or_insert_with(|| json!({}));
    for name in args {
        let removed = deps.as_object_mut().and_then(|d| d.remove(name.as_str()));
        if removed.is_none() {
            return Err(Failure::new("LIP7002", format!("\"{name}\" isn't a dependency of this project")));
        }
        println!("  removed {name}");
    }
    write_json(&project.join("lipi.json"), &Value::Object(ctx.manifest.clone()))?;
    run_install(&mut ctx, &BTreeSet::new(), false)
}

/// `lipi update [name...]`: re-resolve (ignoring lipi.lock) for the named packages, or all.
pub fn update(args: &[String]) -> Res<()> {
    let project = project_dir()?;
    let mut ctx = context(project)?;
    let names: BTreeSet<String> = args.iter().cloned().collect();
    run_install(&mut ctx, &names, names.is_empty())
}

/// `lipi publish [--registry <url>]`
pub fn publish(args: &[String]) -> Res<()> {
    let project = project_dir()?;
    let manifest = read_json(&project.join("lipi.json"))?;
    let name = manifest.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
    let version = manifest.get("version").and_then(Value::as_str).unwrap_or_default().to_string();
    if !valid_name(&name) {
        return Err(Failure::new("LIP7005", format!("\"{name}\" isn't a valid package name")).hint("Set \"name\" in lipi.json using lowercase letters, digits, - _ and ."));
    }
    let parsed = semver::Version::parse(&version).map_err(|_| Failure::new("LIP7005", format!("\"{version}\" isn't a semantic version")).hint("Set \"version\" in lipi.json, like 1.0.0"))?;
    let registry = match args.iter().position(|a| a == "--registry") {
        Some(i) => args.get(i + 1).cloned().ok_or_else(|| Failure::new("LIP7005", "--registry needs a URL"))?,
        None => registry_url(&manifest)?,
    };
    let Some(root) = local_path(&registry) else {
        return Err(Failure::new("LIP7006", "publishing to an online registry isn't available yet")
            .hint("Publish to a folder registry (--registry file:///path/to/registry), or share the package through git."));
    };
    let bytes = pack(&project)?;
    let integrity = sha256_bytes(&bytes);
    let dir = root.join(&name);
    std::fs::create_dir_all(&dir).map_err(|e| io("couldn't create the registry folder", e))?;
    let index_file = dir.join("index.json");
    let mut index = if index_file.is_file() { read_json(&index_file)? } else { Map::new() };
    index.insert("name".into(), json!(name));
    let versions = index.entry("versions").or_insert_with(|| json!({}));
    if versions.get(parsed.to_string()).is_some() {
        return Err(Failure::new("LIP7007", format!("{name} {parsed} is already published")).hint("Published versions never change. Bump \"version\" in lipi.json and publish again."));
    }
    let file = format!("{name}-{parsed}.tar.gz");
    std::fs::write(dir.join(&file), &bytes).map_err(|e| io("couldn't write the package archive", e))?;
    let deps = dependencies(&manifest, &project.join("lipi.json"))?;
    if let Value::Object(v) = versions {
        v.insert(parsed.to_string(), json!({"file": file, "integrity": integrity, "dependencies": deps}));
    }
    write_json(&index_file, &Value::Object(index))?;
    println!("published {name}@{parsed} to {registry} ({} KB, {integrity})", bytes.len().div_ceil(1024));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_and_names() {
        assert!(matches!(parse_spec(Path::new("."), "^1.2").unwrap(), Source::Registry(_)));
        assert!(matches!(parse_spec(Path::new("."), "path:../x").unwrap(), Source::Path(_)));
        assert!(matches!(parse_spec(Path::new("."), "git:https://x/y.git#v1").unwrap(), Source::Git { reference: Some(_), .. }));
        assert!(valid_name("payments.razorpay") && !valid_name("Bad Name"));
        assert_eq!(local_path("file:///C:/reg"), Some(PathBuf::from("C:/reg")));
        assert_eq!(local_path("file:///srv/reg"), Some(PathBuf::from("/srv/reg")));
    }
}
