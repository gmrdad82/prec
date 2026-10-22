use crate::archive;
use crate::paths::{self, Lock};
use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const ENV: &str = "PREC_CACHE";
pub const PRUNE_DAYS: u64 = 30;
const USED: &str = ".used";
const OK: &str = ".ok";
const NAME_CAP: usize = 64;
const SECTIONS: [&str; 5] = ["git", "src", "work", "target", "bin"];

static HELD: Mutex<Vec<File>> = Mutex::new(Vec::new());

pub fn root() -> PathBuf {
    if let Some(p) = std::env::var_os(ENV).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    base().join("prec")
}

fn base() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
        })
}

pub fn normalize(identity: &str) -> String {
    let mut s = identity.trim().trim_end_matches('/').to_string();
    if let Some(stripped) = s.strip_suffix(".git") {
        s = stripped.to_string();
    }
    if let Some(rest) = s.strip_prefix("git@")
        && let Some((host, path)) = rest.split_once(':')
        && !path.starts_with('/')
    {
        s = format!("ssh://git@{host}/{path}");
    }
    s
}

pub fn repo_key(identity: &str) -> String {
    let n = normalize(identity);
    let name = n.rsplit('/').next().unwrap_or("repo");
    let mut name = paths::slug(name);
    name.truncate(NAME_CAP);
    let name = name.trim_end_matches('-');
    let name = if name.is_empty() { "repo" } else { name };
    format!("{name}-{}", &paths::hash_bytes(n.as_bytes())[..12])
}

pub fn build_key(commit: &str, command: &str, bin_dir: &str) -> String {
    let keyed = if command.contains('\n') || bin_dir.contains('\n') {
        format!("\0{}\n{command}\n{bin_dir}", command.len())
    } else {
        format!("{command}\n{bin_dir}")
    };
    let h = paths::hash_bytes(keyed.as_bytes());
    format!("{}-{}", &commit[..commit.len().min(12)], &h[..12])
}

pub fn dir(parts: &[&str]) -> Result<PathBuf, String> {
    let mut d = root();
    for p in parts {
        d.push(p);
    }
    fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    Ok(d)
}

fn lock_path(root: &Path, section: &str, name: &str) -> PathBuf {
    root.join("locks")
        .join(section)
        .join(format!("{name}.lock"))
}

fn entry_path(root: &Path, section: &str, name: &str) -> PathBuf {
    let mut p = root.join(section);
    for part in name.split('@') {
        p.push(part);
    }
    p
}

fn named(entry: &Path) -> Option<(PathBuf, String, String)> {
    let leaf = entry.file_name()?.to_string_lossy().to_string();
    let parent = entry.parent()?;
    let up = parent.file_name()?.to_string_lossy().to_string();
    let grand = parent.parent()?;
    let top = grand.file_name()?.to_string_lossy().to_string();
    if ["git", "work", "target"].contains(&up.as_str()) {
        return Some((grand.to_path_buf(), up, leaf));
    }
    (top == "src" || top == "bin")
        .then(|| Some((grand.parent()?.to_path_buf(), top, format!("{up}@{leaf}"))))
        .flatten()
}

pub struct Exclusive {
    _file: File,
}

pub fn exclusive(section: &str, name: &str) -> Result<Exclusive, String> {
    let path = lock_path(&root(), section, name);
    let f = paths::lock(
        &path,
        Lock::Exclusive,
        &format!("the cached {section} {name}"),
    )?;
    Ok(Exclusive { _file: f })
}

fn keep_held(f: File) -> Result<(), String> {
    HELD.lock().map_err(|e| e.to_string())?.push(f);
    Ok(())
}

pub fn hold(entry: &Path) -> Result<bool, String> {
    let (root, section, name) =
        named(entry).ok_or_else(|| format!("{} is no cache entry", entry.display()))?;
    let f = paths::lock(
        &lock_path(&root, &section, &name),
        Lock::Shared,
        &format!("the cached {section} {name}"),
    )?;
    if !ready(entry) {
        return Ok(false);
    }
    touch(entry);
    keep_held(f)?;
    Ok(true)
}

pub fn entry(
    section: &str,
    name: &str,
    build: impl FnMut(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    entry_in(&root(), section, name, build)
}

fn entry_in(
    root: &Path,
    section: &str,
    name: &str,
    mut build: impl FnMut(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let path = entry_path(root, section, name);
    loop {
        if hold(&path)? {
            return Ok(path);
        }
        let lock = lock_path(root, section, name);
        let _building = paths::lock(
            &lock,
            Lock::Exclusive,
            &format!("the cached {section} {name}"),
        )?;
        if !ready(&path) {
            let growth = Growth::of(&path, &format!("{section}/{name}"));
            build(&path)?;
            mark_ready(&path)?;
            growth.report();
        }
    }
}

pub fn touch(entry: &Path) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = fs::write(entry.join(USED), now.to_string());
}

pub fn last_used(entry: &Path) -> u64 {
    if let Ok(s) = fs::read_to_string(entry.join(USED))
        && let Ok(t) = s.trim().parse()
    {
        return t;
    }
    fs::metadata(entry)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn ready(entry: &Path) -> bool {
    entry.join(OK).exists()
}

pub fn mark_ready(entry: &Path) -> Result<(), String> {
    fs::write(entry.join(OK), b"").map_err(|e| format!("{}: {e}", entry.display()))
}

pub fn size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        fs::read_dir(path)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| size(&e.path())).sum())
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

pub fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

pub struct Growth {
    path: PathBuf,
    label: String,
    before: u64,
}

impl Growth {
    pub fn of(path: &Path, label: &str) -> Growth {
        Growth {
            path: path.to_path_buf(),
            label: label.to_string(),
            before: size(path),
        }
    }

    pub fn report(self) {
        let now = size(&self.path);
        if now > self.before {
            eprintln!(
                "cache: {} {} (+{})",
                self.label,
                human(now),
                human(now - self.before)
            );
        }
    }
}

pub fn source(gitdir: &Path, key: &str, commit: &str) -> Result<PathBuf, String> {
    source_in(&root(), gitdir, key, commit)
}

fn source_in(root: &Path, gitdir: &Path, key: &str, commit: &str) -> Result<PathBuf, String> {
    entry_in(root, "src", &format!("{key}@{commit}"), |path| {
        paths::fresh(path)?;
        let index = path.with_extension("index");
        let _ = fs::remove_file(&index);
        let checked_out = checkout(gitdir, &index, path, commit);
        let _ = fs::remove_file(&index);
        checked_out
    })
}

fn checkout(gitdir: &Path, index: &Path, tree: &Path, commit: &str) -> Result<(), String> {
    let out = paths::output(
        Command::new("git")
            .env("GIT_INDEX_FILE", index)
            .arg("--git-dir")
            .arg(gitdir)
            .arg("--work-tree")
            .arg(tree)
            .args(["read-tree", "--reset", "-u", commit]),
    )
    .map_err(|e| format!("git read-tree: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git read-tree {commit}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

pub fn worktree(gitdir: &Path, key: &str, commit: &str) -> Result<PathBuf, String> {
    worktree_in(&dir(&["work", key])?, gitdir, commit)
}

fn worktree_in(base: &Path, gitdir: &Path, commit: &str) -> Result<PathBuf, String> {
    let tree = base.join("tree");
    fs::create_dir_all(&tree).map_err(|e| format!("{}: {e}", tree.display()))?;
    checkout(gitdir, &base.join("index"), &tree, commit)?;
    touch(base);
    Ok(tree)
}

pub fn target(key: &str) -> Result<PathBuf, String> {
    target_in(&root(), key)
}

fn target_in(root: &Path, key: &str) -> Result<PathBuf, String> {
    entry_in(root, "target", key, |path| {
        fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
    })
}

fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn shared_lib(name: &str) -> bool {
    name.ends_with(".so")
        || name.contains(".so.")
        || name.ends_with(".dylib")
        || name.ends_with(".dll")
}

fn hashed(name: &str) -> bool {
    name.rsplit_once('-')
        .is_some_and(|(_, h)| h.len() == 16 && h.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn artifact(name: &str, executable: bool) -> bool {
    !name.starts_with('.')
        && !name.ends_with(".d")
        && !name.ends_with(".rlib")
        && !name.ends_with(".rmeta")
        && !hashed(name)
        && (executable || shared_lib(name))
}

pub fn keep(from: &Path, to: &Path) -> Result<u64, String> {
    let mut copied = 0;
    for sub in ["", "examples"] {
        let src = if sub.is_empty() {
            from.to_path_buf()
        } else {
            from.join(sub)
        };
        let Ok(rd) = fs::read_dir(&src) else {
            continue;
        };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !artifact(&name, executable(&p)) {
                continue;
            }
            let dest_dir = if sub.is_empty() {
                to.to_path_buf()
            } else {
                to.join(sub)
            };
            fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
            let dest = dest_dir.join(&name);
            fs::copy(&p, &dest)
                .map_err(|e| format!("{} → {}: {e}", p.display(), dest.display()))?;
            copied += 1;
        }
    }
    Ok(copied)
}

#[derive(Clone)]
pub struct Entry {
    pub section: String,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub used: u64,
}

pub fn entries_in(root: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    for section in SECTIONS {
        let Ok(rd) = fs::read_dir(root.join(section)) else {
            continue;
        };
        for repo in rd.filter_map(|e| e.ok()) {
            let rp = repo.path();
            if !rp.is_dir() {
                continue;
            }
            let rname = repo.file_name().to_string_lossy().to_string();
            if section == "src" || section == "bin" {
                let Ok(inner) = fs::read_dir(&rp) else {
                    continue;
                };
                for c in inner.filter_map(|e| e.ok()) {
                    let cp = c.path();
                    if cp.is_dir() {
                        out.push(Entry {
                            section: section.into(),
                            name: format!("{rname}@{}", c.file_name().to_string_lossy()),
                            size: size(&cp),
                            used: last_used(&cp),
                            path: cp,
                        });
                    }
                }
            } else {
                out.push(Entry {
                    section: section.into(),
                    name: rname,
                    size: size(&rp),
                    used: last_used(&rp),
                    path: rp,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        (a.section.as_str(), a.name.as_str()).cmp(&(b.section.as_str(), b.name.as_str()))
    });
    out
}

fn claim(root: &Path, entry: &Entry, create: bool) -> Option<Option<File>> {
    let path = lock_path(root, &entry.section, &entry.name);
    if create && let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    let f = match OpenOptions::new()
        .read(true)
        .write(create)
        .create(create)
        .truncate(false)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
    };
    f.try_lock().ok()?;
    Some(Some(f))
}

pub struct Gone {
    pub section: String,
    pub name: String,
    pub size: u64,
}

pub struct Pruned {
    pub removed: Vec<Gone>,
    pub kept_in_use: Vec<String>,
}

pub fn prune_in(root: &Path, older_than: Duration, all: bool, now: u64, dry_run: bool) -> Pruned {
    prune_entries(root, entries_in(root), older_than, all, now, dry_run)
}

fn prune_entries(
    root: &Path,
    entries: Vec<Entry>,
    older_than: Duration,
    all: bool,
    now: u64,
    dry_run: bool,
) -> Pruned {
    let mut removed = Vec::new();
    let mut kept_in_use = Vec::new();
    let cutoff = now.saturating_sub(older_than.as_secs());
    for e in entries {
        if !all && e.used >= cutoff {
            continue;
        }
        let Some(claimed) = claim(root, &e, !dry_run) else {
            kept_in_use.push(format!("{}/{}", e.section, e.name));
            continue;
        };
        if !dry_run {
            let _ = fs::remove_file(e.path.join(OK));
            let gone = fs::remove_dir_all(&e.path).is_ok();
            drop(claimed);
            if !gone {
                continue;
            }
        }
        removed.push(Gone {
            section: e.section,
            name: e.name,
            size: e.size,
        });
    }
    Pruned {
        removed,
        kept_in_use,
    }
}

pub fn parse_age(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: u64 = num
        .parse()
        .map_err(|_| format!("--older-than {s}: e.g. 30d, 12h or 90m"))?;
    let unit = match unit {
        "" | "d" => 86400,
        "h" => 3600,
        "m" => 60,
        "w" => 7 * 86400,
        _ => return Err(format!("--older-than {s}: e.g. 30d, 12h or 90m")),
    };
    n.checked_mul(unit)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("--older-than {s}: e.g. 30d, 12h or 90m"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn show() -> Result<(), String> {
    let r = root();
    let entries = entries_in(&r);
    let total: u64 = size(&r);
    println!("cache: {} ({})", r.display(), human(total));
    if std::env::var_os(ENV).is_some() {
        println!("       set by {ENV}");
    }
    let now = now_secs();
    for e in &entries {
        let age = now.saturating_sub(e.used);
        let ago = if age >= 86400 {
            format!("{}d ago", age / 86400)
        } else if age >= 3600 {
            format!("{}h ago", age / 3600)
        } else {
            format!("{}m ago", age / 60)
        };
        println!(
            "  {:<7} {:<48} {:>9}  used {ago}",
            e.section,
            e.name,
            human(e.size)
        );
    }
    if entries.is_empty() {
        println!("  (empty)");
    }
    Ok(())
}

fn repo_of(name: &str) -> String {
    let base = name.split_once('@').map_or(name, |(b, _)| b);
    let base = base.strip_suffix(".git").unwrap_or(base);
    match base.rsplit_once('-') {
        Some((repo, h)) if h.len() == 12 && h.chars().all(|c| c.is_ascii_hexdigit()) => {
            repo.to_string()
        }
        _ => base.to_string(),
    }
}

pub fn report_in(root: &Path, set_by: Option<&str>, tool_version: &str, now: u64) -> Value {
    let entries = entries_in(root);
    let stale = prune_entries(
        root,
        entries.clone(),
        Duration::from_secs(PRUNE_DAYS * 86400),
        false,
        now / 1000,
        true,
    );
    let (queue_entries, queue_bytes) = archive::queue_stats(root);
    let listed: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "section": e.section,
                "name": e.name,
                "repo": repo_of(&e.name),
                "path": e.path.display().to_string(),
                "bytes": e.size,
                "used": e.used * 1000,
                "in_use": claim(root, e, false).is_none(),
            })
        })
        .collect();
    json!({
        "v": 1,
        "tool": "prec",
        "tool_version": tool_version,
        "root": root.display().to_string(),
        "root_set_by": set_by,
        "bytes": size(root),
        "at": now,
        "entries": listed,
        "archive_queue": {"entries": queue_entries, "bytes": queue_bytes},
        "prune": {
            "after_days": PRUNE_DAYS,
            "stale_bytes": stale.removed.iter().map(|g| g.size).sum::<u64>(),
        },
    })
}

pub fn show_json() -> Result<(), String> {
    let r = root();
    let r = std::path::absolute(&r).unwrap_or(r);
    let set_by = std::env::var_os(ENV).filter(|p| !p.is_empty()).map(|_| ENV);
    let report = report_in(&r, set_by, env!("CARGO_PKG_VERSION"), now_ms());
    println!("{report}");
    Ok(())
}

pub fn prune_report(pruned: &Pruned) -> Value {
    json!({
        "v": 1,
        "freed": pruned.removed.iter().map(|g| g.size).sum::<u64>(),
        "removed": pruned
            .removed
            .iter()
            .map(|g| json!({"section": g.section, "name": g.name, "bytes": g.size}))
            .collect::<Vec<_>>(),
        "kept_in_use": pruned.kept_in_use.len(),
    })
}

pub fn prune(older_than: Option<&str>, all: bool, json: bool, dry_run: bool) -> Result<(), String> {
    let age = match older_than {
        Some(s) => parse_age(s)?,
        None => Duration::from_secs(PRUNE_DAYS * 86400),
    };
    let pruned = prune_in(&root(), age, all, now_secs(), dry_run);
    if json {
        println!("{}", prune_report(&pruned));
        return Ok(());
    }
    let total: u64 = pruned.removed.iter().map(|g| g.size).sum();
    for g in &pruned.removed {
        println!("removed {}/{} ({})", g.section, g.name, human(g.size));
    }
    for name in &pruned.kept_in_use {
        println!("kept    {name} (in use)");
    }
    println!(
        "freed {} from {}; cache now {}",
        human(total),
        root().display(),
        human(size(&root()))
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_root(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp")
            .join("test-cache")
            .join(name);
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn one_repo_one_key_whatever_the_spelling() {
        let a = repo_key("ssh://git@github.com/gmrdad82/pito-work.git");
        assert_eq!(a, repo_key("ssh://git@github.com/gmrdad82/pito-work"));
        assert_eq!(a, repo_key("ssh://git@github.com/gmrdad82/pito-work/"));
        assert_eq!(a, repo_key("git@github.com:gmrdad82/pito-work.git"));
        assert!(a.starts_with("pito-work-"));
        assert_ne!(a, repo_key("ssh://git@github.com/gmrdad82/pito-pigeon.git"));
        assert_ne!(a, repo_key("/work/someone/app-work"));
    }

    #[test]
    fn a_build_is_keyed_by_commit_and_its_command() {
        let c = "0123456789abcdef0123456789abcdef01234567";
        let k1 = build_key(c, "cargo build --release", "{target}/release");
        assert!(k1.starts_with("0123456789ab-"));
        assert_eq!(
            k1,
            build_key(c, "cargo build --release", "{target}/release")
        );
        assert_ne!(
            k1,
            build_key(c, "cargo build --release -p x", "{target}/release")
        );
        assert_ne!(k1, build_key(c, "cargo build --release", "{target}/debug"));
        assert_ne!(
            k1,
            build_key(
                "fedcba9876543210fedcba9876543210fedcba98",
                "cargo build --release",
                "{target}/release"
            )
        );
    }

    #[test]
    fn artifacts_are_the_binaries_and_libraries_only() {
        assert!(artifact("app", true));
        assert!(artifact("libwebgpu_dawn.so", false));
        assert!(artifact("libfoo.so.1", false));
        assert!(!artifact("app.d", true));
        assert!(!artifact("libapp.rlib", false));
        assert!(!artifact("app_fixture-0123456789abcdef", true));
        assert!(!artifact("notes.txt", false));
        assert!(!artifact(".fingerprint", true));
    }

    #[test]
    fn ages_read_in_days_hours_minutes_and_weeks() {
        assert_eq!(parse_age("7d").unwrap(), Duration::from_secs(7 * 86400));
        assert_eq!(parse_age("12h").unwrap(), Duration::from_secs(12 * 3600));
        assert_eq!(parse_age("90m").unwrap(), Duration::from_secs(90 * 60));
        assert_eq!(parse_age("2w").unwrap(), Duration::from_secs(14 * 86400));
        assert_eq!(parse_age("30").unwrap(), Duration::from_secs(30 * 86400));
        assert_eq!(parse_age(" 3h ").unwrap(), Duration::from_secs(3 * 3600));
        for bad in [
            "soon",
            "",
            "d",
            "-1d",
            "5x",
            "1.5d",
            "7 d",
            "99999999999999999999d",
            "999999999999999d",
            "99999999999999999w",
        ] {
            assert_eq!(
                parse_age(bad).unwrap_err(),
                format!("--older-than {}: e.g. 30d, 12h or 90m", bad.trim()),
                "{bad}"
            );
        }
    }

    fn populate(r: &Path, now: u64) -> PathBuf {
        let mk = |rel: &str, used: u64, bytes: usize| {
            let d = r.join(rel);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("payload"), vec![0u8; bytes]).unwrap();
            fs::write(d.join(USED), used.to_string()).unwrap();
            d
        };
        mk("src/app-aaaaaaaaaaaa/old", now - 40 * 86400, 1000);
        mk("src/app-aaaaaaaaaaaa/new", now - 86400, 500);
        let held = mk("bin/app-aaaaaaaaaaaa/old-held", now - 40 * 86400, 2000);
        mk("target/app-aaaaaaaaaaaa", now - 60 * 86400, 4000);
        mk("git/app-aaaaaaaaaaaa.git", now - 5 * 86400, 100);
        mk("work/other-bbbbbbbbbbbb", now - 1, 50);
        held
    }

    fn stale_size(r: &Path) -> u64 {
        size(&r.join("src/app-aaaaaaaaaaaa/old")) + size(&r.join("target/app-aaaaaaaaaaaa"))
    }

    fn prune_days(r: &Path, all: bool, now: u64, dry_run: bool) -> Pruned {
        prune_in(
            r,
            Duration::from_secs(PRUNE_DAYS * 86400),
            all,
            now,
            dry_run,
        )
    }

    #[test]
    fn prune_takes_the_stale_and_leaves_the_fresh_and_the_held() {
        let r = scratch_root("prune");
        let now = 10_000_000;
        let held = populate(&r, now);
        let lock = shared(&r, "bin", "app-aaaaaaaaaaaa@old-held");
        let pruned = prune_days(&r, false, now, false);
        let names: Vec<_> = pruned
            .removed
            .iter()
            .map(|g| format!("{}/{}", g.section, g.name))
            .collect();
        assert!(names.contains(&"src/app-aaaaaaaaaaaa@old".to_string()));
        assert!(names.contains(&"target/app-aaaaaaaaaaaa".to_string()));
        assert!(!names.contains(&"src/app-aaaaaaaaaaaa@new".to_string()));
        assert_eq!(
            pruned.kept_in_use,
            vec!["bin/app-aaaaaaaaaaaa@old-held".to_string()]
        );
        assert!(r.join("src/app-aaaaaaaaaaaa/new").exists());
        assert!(held.exists());
        drop(lock);
        paths::quiesce();
        let all = prune_days(&r, true, now, false);
        assert!(
            all.removed
                .iter()
                .any(|g| g.section == "src" && g.name == "app-aaaaaaaaaaaa@new")
        );
        assert!(all.kept_in_use.is_empty());
        assert!(!held.exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn the_repo_is_the_name_without_its_hash() {
        assert_eq!(repo_of("pito-app-5f514e3aeabc@f17d634909c2"), "pito-app");
        assert_eq!(repo_of("pito-app-5f514e3aeabc.git"), "pito-app");
        assert_eq!(repo_of("pito-app-5f514e3aeabc"), "pito-app");
        assert_eq!(repo_of("plain"), "plain");
        assert_eq!(repo_of("my-repo"), "my-repo");
    }

    #[test]
    fn the_report_has_the_agreed_keys_and_counts() {
        let r = scratch_root("report");
        let now = 10_000_000;
        let held = populate(&r, now);
        let q = r.join("archive-queue");
        fs::create_dir_all(q.join("one")).unwrap();
        fs::create_dir_all(q.join("two")).unwrap();
        fs::write(q.join("one/file"), vec![0u8; 300]).unwrap();
        fs::write(q.join("two/file"), vec![0u8; 200]).unwrap();
        assert!(held.exists());
        let lock = shared(&r, "bin", "app-aaaaaaaaaaaa@old-held");
        let v = report_in(&r, Some(ENV), "9.9.9", now * 1000);
        assert_eq!(v["v"], 1);
        assert_eq!(v["tool"], "prec");
        assert_eq!(v["tool_version"], "9.9.9");
        assert_eq!(v["root"], r.display().to_string());
        assert_eq!(v["root_set_by"], ENV);
        assert_eq!(v["at"], now * 1000);
        assert_eq!(v["bytes"], size(&r));
        assert_eq!(v["archive_queue"]["entries"], 2);
        assert_eq!(v["archive_queue"]["bytes"], 500);
        assert_eq!(v["prune"]["after_days"], 30);
        assert_eq!(v["prune"]["stale_bytes"].as_u64().unwrap(), stale_size(&r));
        let entries = v["entries"].as_array().unwrap();
        let keys: Vec<_> = entries
            .iter()
            .map(|e| {
                (
                    e["section"].as_str().unwrap().to_string(),
                    e["name"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert_eq!(keys.len(), 6);
        assert_eq!(keys[0].0, "bin");
        assert_eq!(keys[5].0, "work");
        for e in entries {
            let mut names: Vec<_> = e.as_object().unwrap().keys().cloned().collect();
            names.sort();
            assert_eq!(
                names,
                ["bytes", "in_use", "name", "path", "repo", "section", "used"]
            );
            assert!(e["bytes"].is_u64());
            assert!(e["used"].is_u64());
            assert!(e["in_use"].is_boolean());
            assert!(e["path"].as_str().unwrap().starts_with('/'));
        }
        let held_entry = entries
            .iter()
            .find(|e| e["name"] == "app-aaaaaaaaaaaa@old-held")
            .unwrap();
        assert_eq!(held_entry["in_use"], true);
        assert_eq!(held_entry["repo"], "app");
        assert_eq!(
            held_entry["used"].as_u64().unwrap(),
            (now - 40 * 86400) * 1000
        );
        assert_eq!(entries.iter().filter(|e| e["in_use"] == true).count(), 1);
        drop(lock);
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn the_report_is_null_about_the_root_when_nothing_moved_it() {
        let r = scratch_root("report-empty");
        let v = report_in(&r, None, "0.0.0", 1);
        assert!(v["root_set_by"].is_null());
        assert_eq!(v["entries"].as_array().unwrap().len(), 0);
        assert_eq!(v["archive_queue"]["entries"], 0);
        assert_eq!(v["archive_queue"]["bytes"], 0);
        assert_eq!(v["prune"]["stale_bytes"], 0);
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_dry_run_removes_nothing_and_reports_what_a_prune_removes() {
        let r = scratch_root("dry");
        let now = 10_000_000;
        let held = populate(&r, now);
        assert!(held.exists());
        let lock = shared(&r, "bin", "app-aaaaaaaaaaaa@old-held");
        let before = size(&r);
        let dry = prune_days(&r, false, now, true);
        assert_eq!(size(&r), before);
        assert!(r.join("src/app-aaaaaaaaaaaa/old").exists());
        assert!(r.join("target/app-aaaaaaaaaaaa").exists());
        let dry_json = prune_report(&dry);
        assert_eq!(dry_json["v"], 1);
        assert_eq!(dry_json["kept_in_use"], 1);
        assert_eq!(dry_json["freed"].as_u64().unwrap(), stale_size(&r));
        let real = prune_days(&r, false, now, false);
        assert_eq!(prune_report(&real), dry_json);
        assert!(!r.join("src/app-aaaaaaaaaaaa/old").exists());
        assert!(!r.join("target/app-aaaaaaaaaaaa").exists());
        let names: Vec<_> = dry_json["removed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["app-aaaaaaaaaaaa@old", "app-aaaaaaaaaaaa"]);
        drop(lock);
        let _ = fs::remove_dir_all(&r);
    }

    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = paths::output(
            Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                    "-C",
                ])
                .arg(dir)
                .args(args),
        )
        .unwrap();
        assert!(
            out.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn two_commits(name: &str) -> (PathBuf, PathBuf, String, String) {
        let r = scratch_root(name);
        let repo = r.join("repo");
        fs::create_dir_all(&repo).unwrap();
        git_in(&repo, &["init", "--quiet"]);
        fs::write(repo.join("a.txt"), "one").unwrap();
        git_in(&repo, &["add", "a.txt"]);
        git_in(&repo, &["commit", "--quiet", "-m", "one"]);
        let first = git_in(&repo, &["rev-parse", "HEAD"]);
        fs::write(repo.join("b.txt"), "two").unwrap();
        git_in(&repo, &["add", "b.txt"]);
        git_in(&repo, &["commit", "--quiet", "-m", "two"]);
        let second = git_in(&repo, &["rev-parse", "HEAD"]);
        (r, repo.join(".git"), first, second)
    }

    #[test]
    fn the_tree_moves_over_an_untracked_leftover() {
        let (r, gitdir, first, second) = two_commits("tree-leftover");
        let base = r.join("work/key");
        let tree = worktree_in(&base, &gitdir, &first).unwrap();
        assert!(!tree.join("b.txt").exists());
        fs::write(tree.join("b.txt"), "leftover").unwrap();
        fs::write(tree.join("unrelated.txt"), "mine").unwrap();
        let tree = worktree_in(&base, &gitdir, &second).unwrap();
        assert_eq!(fs::read_to_string(tree.join("b.txt")).unwrap(), "two");
        assert_eq!(fs::read_to_string(tree.join("a.txt")).unwrap(), "one");
        assert!(tree.join("unrelated.txt").exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn the_tree_recovers_when_its_index_is_missing() {
        let (r, gitdir, first, second) = two_commits("tree-no-index");
        let base = r.join("work/key");
        let tree = worktree_in(&base, &gitdir, &first).unwrap();
        fs::write(tree.join("b.txt"), "half written").unwrap();
        fs::remove_file(base.join("index")).unwrap();
        let tree = worktree_in(&base, &gitdir, &second).unwrap();
        assert_eq!(fs::read_to_string(tree.join("b.txt")).unwrap(), "two");
        assert_eq!(fs::read_to_string(tree.join("a.txt")).unwrap(), "one");
        assert!(base.join("index").exists());
        let tree = worktree_in(&base, &gitdir, &first).unwrap();
        assert!(!tree.join("b.txt").exists());
        let _ = fs::remove_dir_all(&r);
    }

    fn ready_entry(r: &Path, rel: &str) -> PathBuf {
        let e = r.join(rel);
        fs::create_dir_all(&e).unwrap();
        fs::write(e.join("payload"), b"x").unwrap();
        mark_ready(&e).unwrap();
        e
    }

    #[test]
    fn a_prune_between_ready_and_hold_makes_hold_report_the_entry_gone() {
        let r = scratch_root("prune-race");
        let entry = ready_entry(&r, "src/app-aaaaaaaaaaaa/abc");
        assert!(ready(&entry));
        let pruned = prune_days(&r, true, now_secs(), false);
        assert_eq!(pruned.removed.len(), 1);
        assert!(!hold(&entry).unwrap());
        assert!(!entry.exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_hold_waits_for_a_prune_that_is_deleting_and_then_sees_it_gone() {
        let r = scratch_root("prune-wait");
        let entry = ready_entry(&r, "bin/app-aaaaaaaaaaaa/abc");
        let probe = Entry {
            section: "bin".into(),
            name: "app-aaaaaaaaaaaa@abc".into(),
            path: entry.clone(),
            size: 0,
            used: 0,
        };
        let claimed = claim(&r, &probe, true).unwrap();
        let waiter = {
            let entry = entry.clone();
            std::thread::spawn(move || hold(&entry))
        };
        fs::remove_file(entry.join(OK)).unwrap();
        fs::remove_dir_all(&entry).unwrap();
        drop(claimed);
        assert!(!waiter.join().unwrap().unwrap());
        assert!(!entry.exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn an_entry_a_render_holds_survives_even_prune_all() {
        let r = scratch_root("held-entry");
        let entry = ready_entry(&r, "src/app-aaaaaaaaaaaa/abc");
        assert!(hold(&entry).unwrap());
        let pruned = prune_days(&r, true, now_secs(), false);
        assert!(pruned.removed.is_empty());
        assert_eq!(pruned.kept_in_use, vec!["src/app-aaaaaaaaaaaa@abc"]);
        assert!(entry.join("payload").exists());
        assert!(ready(&entry));
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn work_and_target_entries_a_render_uses_are_kept() {
        let r = scratch_root("held-build");
        let now = 10_000_000;
        for rel in ["work/app-aaaaaaaaaaaa", "target/app-aaaaaaaaaaaa"] {
            let d = r.join(rel);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("payload"), b"x").unwrap();
        }
        let building = paths::lock(
            &lock_path(&r, "work", "app-aaaaaaaaaaaa"),
            Lock::Exclusive,
            "test",
        )
        .unwrap();
        let running = shared(&r, "target", "app-aaaaaaaaaaaa");
        let pruned = prune_days(&r, true, now, false);
        assert!(pruned.removed.is_empty());
        assert_eq!(pruned.kept_in_use.len(), 2);
        drop(building);
        paths::quiesce();
        let pruned = prune_days(&r, true, now, false);
        assert_eq!(pruned.kept_in_use, vec!["target/app-aaaaaaaaaaaa"]);
        assert_eq!(pruned.removed.len(), 1);
        assert!(r.join("target/app-aaaaaaaaaaaa/payload").exists());
        assert!(!r.join("work/app-aaaaaaaaaaaa").exists());
        drop(running);
        let _ = fs::remove_dir_all(&r);
    }

    fn shared(r: &Path, section: &str, name: &str) -> File {
        paths::lock(&lock_path(r, section, name), Lock::Shared, "test").unwrap()
    }

    const KINDS: [(&str, &str); 3] = [
        ("src", "app-aaaaaaaaaaaa@abc"),
        ("bin", "app-aaaaaaaaaaaa@abc-def"),
        ("target", "app-aaaaaaaaaaaa"),
    ];

    fn listed(pruned: &Pruned, section: &str, name: &str) -> bool {
        pruned
            .kept_in_use
            .iter()
            .any(|k| *k == format!("{section}/{name}"))
    }

    #[test]
    fn a_prune_during_a_build_waits_for_it_and_then_for_its_user() {
        for (section, name) in KINDS {
            let r = scratch_root(&format!("prune-building-{section}"));
            let mut builds = 0;
            let entry = entry_in(&r, section, name, |p| {
                builds += 1;
                paths::fresh(p)?;
                fs::write(p.join("payload"), b"half").unwrap();
                let pruned = prune_days(&r, true, now_secs(), false);
                assert!(pruned.removed.is_empty(), "{section}");
                assert!(listed(&pruned, section, name), "{section}");
                assert!(p.join("payload").exists(), "{section}");
                Ok(())
            })
            .unwrap();
            assert_eq!(builds, 1);
            assert_eq!(entry, entry_path(&r, section, name));
            assert!(ready(&entry));
            let pruned = prune_days(&r, true, now_secs(), false);
            assert!(pruned.removed.is_empty(), "{section}");
            assert!(listed(&pruned, section, name), "{section}");
            assert!(entry.join("payload").exists());
            let _ = fs::remove_dir_all(&r);
        }
    }

    #[test]
    fn a_prune_between_a_build_and_its_use_makes_the_user_rebuild() {
        for (section, name) in KINDS {
            let r = scratch_root(&format!("prune-built-{section}"));
            let rel = entry_path(Path::new(""), section, name);
            let made = ready_entry(&r, &rel.to_string_lossy());
            let pruned = prune_days(&r, true, now_secs(), false);
            assert_eq!(pruned.removed.len(), 1, "{section}");
            assert!(!made.exists());
            assert!(lock_path(&r, section, name).exists());
            let mut builds = 0;
            let entry = entry_in(&r, section, name, |p| {
                builds += 1;
                paths::fresh(p)?;
                fs::write(p.join("payload"), b"again").map_err(|e| e.to_string())
            })
            .unwrap();
            assert_eq!(builds, 1, "{section}");
            assert_eq!(entry, made);
            let pruned = prune_days(&r, true, now_secs(), false);
            assert!(listed(&pruned, section, name), "{section}");
            assert_eq!(fs::read(entry.join("payload")).unwrap(), b"again");
            let _ = fs::remove_dir_all(&r);
        }
    }

    #[test]
    fn a_failed_build_leaves_no_ready_entry_and_the_next_user_builds_it() {
        let r = scratch_root("build-fails");
        let failed = entry_in(&r, "bin", "app-aaaaaaaaaaaa@abc-def", |p| {
            paths::fresh(p)?;
            fs::write(p.join("half"), b"x").unwrap();
            Err("the build failed".into())
        });
        assert_eq!(failed.unwrap_err(), "the build failed");
        let entry = entry_path(&r, "bin", "app-aaaaaaaaaaaa@abc-def");
        assert!(!ready(&entry));
        let entry = entry_in(&r, "bin", "app-aaaaaaaaaaaa@abc-def", |p| {
            paths::fresh(p)?;
            fs::write(p.join("whole"), b"x").map_err(|e| e.to_string())
        })
        .unwrap();
        assert!(ready(&entry));
        assert!(!entry.join("half").exists());
        assert!(entry.join("whole").exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_source_is_the_commits_tree_checked_out_once_and_held() {
        let (r, gitdir, first, second) = two_commits("source");
        let cache = r.join("cache");
        let one = source_in(&cache, &gitdir, "app-aaaaaaaaaaaa", &first).unwrap();
        assert_eq!(one, cache.join("src/app-aaaaaaaaaaaa").join(&first));
        assert_eq!(fs::read_to_string(one.join("a.txt")).unwrap(), "one");
        assert!(!one.join("b.txt").exists());
        assert!(ready(&one));
        assert!(!one.with_extension("index").exists());
        fs::write(one.join("marker"), b"kept").unwrap();
        let again = source_in(&cache, &gitdir, "app-aaaaaaaaaaaa", &first).unwrap();
        assert_eq!(again, one);
        assert!(again.join("marker").exists());
        let two = source_in(&cache, &gitdir, "app-aaaaaaaaaaaa", &second).unwrap();
        assert_eq!(fs::read_to_string(two.join("b.txt")).unwrap(), "two");
        let pruned = prune_days(&cache, true, now_secs(), false);
        assert!(pruned.removed.is_empty());
        assert_eq!(pruned.kept_in_use.len(), 2);
        assert!(source_in(&cache, &gitdir, "app-aaaaaaaaaaaa", "0123456789abcdef").is_err());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_target_comes_back_after_a_prune_with_its_lock_still_in_place() {
        let r = scratch_root("target-back");
        let lock = lock_path(&r, "target", "app-aaaaaaaaaaaa");
        fs::create_dir_all(r.join("target/app-aaaaaaaaaaaa")).unwrap();
        let pruned = prune_days(&r, true, now_secs(), false);
        assert_eq!(pruned.removed.len(), 1);
        assert!(lock.exists());
        let t = target_in(&r, "app-aaaaaaaaaaaa").unwrap();
        assert!(t.is_dir());
        let pruned = prune_days(&r, true, now_secs(), false);
        assert_eq!(pruned.kept_in_use, vec!["target/app-aaaaaaaaaaaa"]);
        assert!(t.is_dir());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn an_entry_path_and_its_name_map_both_ways() {
        let r = Path::new("/c");
        for (section, name) in KINDS.into_iter().chain([
            ("git", "app-aaaaaaaaaaaa.git"),
            ("work", "app-aaaaaaaaaaaa"),
        ]) {
            let (root, s, n) = named(&entry_path(r, section, name)).unwrap();
            assert_eq!((root.as_path(), s.as_str(), n.as_str()), (r, section, name));
        }
        assert!(named(Path::new("/c/elsewhere/x")).is_none());
    }

    fn one_safe_name(key: &str) {
        assert!(!key.is_empty());
        assert!(key.len() <= NAME_CAP + 13, "{} bytes", key.len());
        assert!(
            key.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "{key}"
        );
        assert!(!key.starts_with('-') && !key.contains("--"), "{key}");
        let r = Path::new("/c");
        for section in ["git", "work", "target"] {
            let path = entry_path(r, section, key);
            assert_eq!(path.parent(), Some(r.join(section).as_path()), "{key}");
            let (root, s, n) = named(&path).unwrap();
            assert_eq!((root.as_path(), s.as_str(), n.as_str()), (r, section, key));
        }
    }

    #[test]
    fn a_repo_key_is_one_folder_inside_its_section_whatever_the_identity() {
        let long = format!("ssh://h/{}", "x".repeat(10_000));
        let dashes = "a-".repeat(40);
        for identity in [
            "",
            "   ",
            "/",
            "//",
            ".",
            "..",
            "../../etc",
            "ssh://h/a/../..",
            "git@h:..",
            "git@h:/abs/app",
            ".git",
            "a/b/.git",
            "ÜNÏ/çödé",
            "日本",
            "a\0b",
            "C:\\Users\\x\\app",
            "app@v1",
            "-app-",
            long.as_str(),
            dashes.as_str(),
        ] {
            one_safe_name(&repo_key(identity));
        }
        assert!(repo_key("").starts_with("repo-"));
        assert!(repo_key("..").starts_with("repo-"));
        assert!(repo_key("日本").starts_with("repo-"));
        assert!(repo_key("app@v1").starts_with("app-v1-"));
        assert!(repo_key("ÜNÏ/çödé").starts_with("d-"));
    }

    #[test]
    fn repos_whose_names_slug_alike_keep_their_own_folders() {
        let keys = [
            repo_key("ssh://git@github.com/alpha/App.git"),
            repo_key("ssh://git@github.com/beta/app.git"),
            repo_key("/work/one/app"),
            repo_key("/work/one/APP"),
            repo_key("/work/one/a.p.p"),
            repo_key(""),
            repo_key(".."),
        ];
        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(keys[0].starts_with("app-") && keys[1].starts_with("app-"));
        assert_eq!(repo_key("/work/one/app/"), keys[2]);
    }

    #[test]
    fn an_over_long_repo_name_still_makes_a_folder_and_stays_apart() {
        let r = scratch_root("over-long-name");
        let stem = "n".repeat(300);
        let a = repo_key(&format!("/work/{stem}a"));
        let b = repo_key(&format!("/work/{stem}b"));
        assert_ne!(a, b);
        for key in [&a, &b] {
            one_safe_name(key);
            assert!(key.starts_with(&"n".repeat(NAME_CAP)));
            let made = entry_path(&r, "target", key);
            fs::create_dir_all(&made).unwrap();
            assert!(made.is_dir());
            fs::create_dir_all(lock_path(&r, "target", key).parent().unwrap()).unwrap();
            File::create(lock_path(&r, "target", key)).unwrap();
        }
        assert_eq!(fs::read_dir(r.join("target")).unwrap().count(), 2);
    }

    #[test]
    fn a_build_key_is_a_short_safe_name_whatever_the_command() {
        let c = "0123456789abcdef0123456789abcdef01234567";
        for (command, bin_dir) in [
            ("", ""),
            ("../../x", "/"),
            ("cargo build @ ..", "{target}/../.."),
            ("a\nb", "c"),
            (&"y".repeat(10_000)[..], "z"),
        ] {
            let k = build_key(c, command, bin_dir);
            assert_eq!(k.len(), 25, "{k}");
            assert!(k.starts_with("0123456789ab-"));
            assert!(k.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'), "{k}");
        }
        assert!(build_key("abc", "x", "y").starts_with("abc-"));
        assert_eq!(build_key("abc", "x", "y").len(), 16);
    }

    #[test]
    fn a_newline_moved_between_command_and_bin_dir_is_another_build() {
        let c = "0123456789abcdef0123456789abcdef01234567";
        assert_ne!(build_key(c, "a\nb", "c"), build_key(c, "a", "b\nc"));
        assert_ne!(build_key(c, "a\nb", "c"), build_key(c, "a\nb\nc", ""));
        assert_ne!(build_key(c, "a", "b"), build_key(c, "a\nb", ""));
        assert_ne!(build_key(c, "a", "b"), build_key(c, "", "a\nb"));
    }

    #[test]
    fn a_one_line_build_keeps_the_key_its_cached_binaries_already_have() {
        let c = "0123456789abcdef0123456789abcdef01234567";
        let old = paths::hash_bytes(b"cargo build --release\n{target}/release");
        assert_eq!(
            build_key(c, "cargo build --release", "{target}/release"),
            format!("0123456789ab-{}", &old[..12])
        );
    }

    #[test]
    fn a_source_and_a_bin_entry_nest_their_commit_under_the_repo_folder() {
        let r = Path::new("/c");
        let key = repo_key("ssh://h/../../app");
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let bin = format!("{key}@{}", build_key(commit, "make", "out"));
        for (section, name) in [("src", format!("{key}@{commit}")), ("bin", bin)] {
            let path = entry_path(r, section, &name);
            assert_eq!(
                path.parent().and_then(Path::parent),
                Some(r.join(section).as_path())
            );
            assert!(
                path.components()
                    .all(|part| !matches!(part, std::path::Component::ParentDir))
            );
            let (root, s, n) = named(&path).unwrap();
            assert_eq!((root.as_path(), s.as_str(), n), (r, section, name));
        }
    }
}
