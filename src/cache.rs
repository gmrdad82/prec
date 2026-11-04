use crate::paths::{self, Lock};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const ENV: &str = "PREC_CACHE";
const PRUNE_DAYS: u64 = 30;
const USED: &str = ".used";
const OK: &str = ".ok";
const NAME_CAP: usize = 64;
const STAMP: &str = ".pruned";
const DAY: u64 = 86400;
const SECTIONS: [&str; 6] = ["git", "src", "work", "target", "bin", "sockets"];
const NESTED: [&str; 2] = ["src", "bin"];

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
    root.join(section).join(name)
}

fn named(entry: &Path) -> Option<(PathBuf, String, String)> {
    let leaf = entry.file_name()?.to_string_lossy().to_string();
    let parent = entry.parent()?;
    let up = parent.file_name()?.to_string_lossy().to_string();
    let grand = parent.parent()?;
    ["git", "work", "target"]
        .contains(&up.as_str())
        .then(|| (grand.to_path_buf(), up, leaf))
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
    let _ = fs::write(entry.join(USED), now_secs().to_string());
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

pub struct Entry {
    pub section: String,
    pub name: String,
    pub path: PathBuf,
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
            if NESTED.contains(&section) {
                let Ok(inner) = fs::read_dir(&rp) else {
                    continue;
                };
                for c in inner.filter_map(|e| e.ok()) {
                    let cp = c.path();
                    if cp.is_dir() {
                        out.push(Entry {
                            section: section.into(),
                            name: format!("{rname}@{}", c.file_name().to_string_lossy()),
                            used: last_used(&cp),
                            path: cp,
                        });
                    }
                }
            } else {
                out.push(Entry {
                    section: section.into(),
                    name: rname,
                    used: last_used(&rp),
                    path: rp,
                });
            }
        }
    }
    out
}

fn claim(root: &Path, entry: &Entry) -> Option<File> {
    let path = lock_path(root, &entry.section, &entry.name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .ok()?;
    f.try_lock().ok()?;
    Some(f)
}

fn writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if !meta.is_dir() {
        return;
    }
    let mode = meta.permissions().mode();
    if mode & 0o700 != 0o700 {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o700));
    }
    if let Ok(rd) = fs::read_dir(path) {
        for e in rd.filter_map(|e| e.ok()) {
            writable(&e.path());
        }
    }
}

pub fn prune_in(root: &Path, now: u64) -> Vec<String> {
    let cutoff = now.saturating_sub(PRUNE_DAYS * DAY);
    let mut removed = Vec::new();
    for e in entries_in(root) {
        if e.used >= cutoff {
            continue;
        }
        let Some(claimed) = claim(root, &e) else {
            continue;
        };
        let _ = fs::remove_file(e.path.join(OK));
        writable(&e.path);
        let gone = fs::remove_dir_all(&e.path).is_ok();
        drop(claimed);
        if gone {
            removed.push(format!("{}/{}", e.section, e.name));
        }
    }
    removed
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn tidy_in(root: &Path, now: u64) -> Option<Vec<String>> {
    if !root.is_dir() {
        return None;
    }
    let stamp = root.join(STAMP);
    if let Ok(s) = fs::read_to_string(&stamp)
        && let Ok(t) = s.trim().parse::<u64>()
        && t <= now
        && now - t < DAY
    {
        return None;
    }
    fs::write(&stamp, now.to_string()).ok()?;
    Some(prune_in(root, now))
}

pub fn tidy() {
    tidy_in(&root(), now_secs());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const NOW: u64 = 10_000_000;

    fn scratch_root(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp")
            .join("test-cache")
            .join(name);
        writable(&d);
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn mk(r: &Path, rel: &str, used: u64) -> PathBuf {
        let d = r.join(rel);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("payload"), b"x").unwrap();
        fs::write(d.join(USED), used.to_string()).unwrap();
        d
    }

    #[test]
    fn a_socket_folder_a_killed_run_left_is_pruned_by_the_same_rule() {
        let r = scratch_root("sockets");
        let stale = mk(&r, "sockets/p1-0", NOW - 31 * DAY);
        let fresh = mk(&r, "sockets/p2-0", NOW);
        assert_eq!(prune_in(&r, NOW), ["sockets/p1-0"]);
        assert!(!stale.exists() && fresh.exists());
    }

    fn shared(r: &Path, section: &str, name: &str) -> File {
        paths::lock(&lock_path(r, section, name), Lock::Shared, "test").unwrap()
    }

    fn ready_entry(r: &Path, rel: &str) -> PathBuf {
        let e = r.join(rel);
        fs::create_dir_all(&e).unwrap();
        fs::write(e.join("payload"), b"x").unwrap();
        mark_ready(&e).unwrap();
        e
    }

    fn far(r: &Path) -> Vec<String> {
        prune_in(r, now_secs() + (PRUNE_DAYS + 1) * DAY)
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
    fn a_stale_entry_goes_and_a_fresh_one_stays() {
        let r = scratch_root("prune-age");
        let stale = mk(&r, "target/app-aaaaaaaaaaaa", NOW - 31 * DAY);
        let edge = mk(&r, "work/app-bbbbbbbbbbbb", NOW - 30 * DAY);
        let fresh = mk(&r, "target/other-cccccccccccc", NOW - DAY);
        let removed = prune_in(&r, NOW);
        assert_eq!(removed, ["target/app-aaaaaaaaaaaa"]);
        assert!(!stale.exists());
        assert!(edge.exists());
        assert!(fresh.exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_stale_entry_a_run_holds_stays_until_the_run_lets_go() {
        let r = scratch_root("prune-held");
        let held = mk(&r, "target/app-aaaaaaaaaaaa", NOW - 60 * DAY);
        let lock = shared(&r, "target", "app-aaaaaaaaaaaa");
        assert!(prune_in(&r, NOW).is_empty());
        assert!(held.join("payload").exists());
        drop(lock);
        paths::quiesce();
        assert_eq!(prune_in(&r, NOW), ["target/app-aaaaaaaaaaaa"]);
        assert!(!held.exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_build_tree_a_build_holds_stays() {
        let r = scratch_root("prune-building");
        let tree = mk(&r, "work/app-aaaaaaaaaaaa", NOW - 60 * DAY);
        let building = paths::lock(
            &lock_path(&r, "work", "app-aaaaaaaaaaaa"),
            Lock::Exclusive,
            "test",
        )
        .unwrap();
        assert!(prune_in(&r, NOW).is_empty());
        assert!(tree.exists());
        drop(building);
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn entries_an_older_prec_left_are_found_and_pruned_by_the_same_rule() {
        let r = scratch_root("prune-retired");
        let old_src = mk(&r, "src/app-aaaaaaaaaaaa/abc", NOW - 40 * DAY);
        let new_src = mk(&r, "src/app-aaaaaaaaaaaa/def", NOW - DAY);
        let old_bin = mk(&r, "bin/app-aaaaaaaaaaaa/abc-123", NOW - 40 * DAY);
        let old_git = mk(&r, "git/app-aaaaaaaaaaaa.git", NOW - 40 * DAY);
        let held = mk(&r, "bin/app-aaaaaaaaaaaa/held", NOW - 40 * DAY);
        let lock = shared(&r, "bin", "app-aaaaaaaaaaaa@held");
        let mut removed = prune_in(&r, NOW);
        removed.sort();
        assert_eq!(
            removed,
            [
                "bin/app-aaaaaaaaaaaa@abc-123",
                "git/app-aaaaaaaaaaaa.git",
                "src/app-aaaaaaaaaaaa@abc"
            ]
        );
        assert!(!old_src.exists() && !old_bin.exists() && !old_git.exists());
        assert!(new_src.exists());
        assert!(held.exists());
        drop(lock);
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_read_only_source_tree_is_removed_too() {
        let r = scratch_root("prune-read-only");
        let tree = mk(&r, "src/app-aaaaaaaaaaaa/abc", NOW - 40 * DAY);
        let inner = tree.join("sub/deeper");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("file"), b"x").unwrap();
        for d in [&inner, &tree.join("sub"), &tree] {
            fs::set_permissions(d, fs::Permissions::from_mode(0o555)).unwrap();
        }
        assert_eq!(prune_in(&r, NOW), ["src/app-aaaaaaaaaaaa@abc"]);
        assert!(!tree.exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn the_prune_runs_once_a_day() {
        let r = scratch_root("tidy-daily");
        let first = mk(&r, "target/app-aaaaaaaaaaaa", NOW - 40 * DAY);
        assert_eq!(tidy_in(&r, NOW).unwrap(), ["target/app-aaaaaaaaaaaa"]);
        assert!(!first.exists());
        assert_eq!(fs::read_to_string(r.join(STAMP)).unwrap(), NOW.to_string());
        let second = mk(&r, "target/beta-dddddddddddd", NOW - 40 * DAY);
        assert!(tidy_in(&r, NOW + DAY - 1).is_none());
        assert!(second.exists());
        assert_eq!(
            tidy_in(&r, NOW + DAY).unwrap(),
            ["target/beta-dddddddddddd"]
        );
        assert!(!second.exists());
        assert_eq!(
            fs::read_to_string(r.join(STAMP)).unwrap(),
            (NOW + DAY).to_string()
        );
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_stamp_from_the_future_or_garbage_does_not_hold_the_prune_back() {
        let r = scratch_root("tidy-stamp");
        for stamp in [(NOW + 5 * DAY).to_string(), "soon".to_string()] {
            let stale = mk(&r, "target/app-aaaaaaaaaaaa", NOW - 40 * DAY);
            fs::write(r.join(STAMP), stamp).unwrap();
            assert_eq!(tidy_in(&r, NOW).unwrap().len(), 1);
            assert!(!stale.exists());
        }
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_prune_that_cannot_run_fails_nothing() {
        let r = scratch_root("tidy-failing");
        assert!(tidy_in(&r.join("absent"), NOW).is_none());
        fs::write(r.join("file"), b"x").unwrap();
        assert!(tidy_in(&r.join("file"), NOW).is_none());
        let stale = mk(&r, "target/app-aaaaaaaaaaaa", NOW - 40 * DAY);
        fs::create_dir(r.join(STAMP)).unwrap();
        assert!(tidy_in(&r, NOW).is_none());
        assert!(stale.exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_prune_between_ready_and_hold_makes_hold_report_the_entry_gone() {
        let r = scratch_root("prune-race");
        let entry = ready_entry(&r, "work/app-aaaaaaaaaaaa");
        assert!(ready(&entry));
        assert_eq!(far(&r).len(), 1);
        assert!(!hold(&entry).unwrap());
        assert!(!entry.exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_hold_waits_for_a_prune_that_is_deleting_and_then_sees_it_gone() {
        let r = scratch_root("prune-wait");
        let entry = ready_entry(&r, "target/app-aaaaaaaaaaaa");
        let probe = Entry {
            section: "target".into(),
            name: "app-aaaaaaaaaaaa".into(),
            path: entry.clone(),
            used: 0,
        };
        let claimed = claim(&r, &probe).unwrap();
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
    fn an_entry_a_run_holds_survives_a_prune() {
        let r = scratch_root("held-entry");
        let entry = ready_entry(&r, "target/app-aaaaaaaaaaaa");
        assert!(hold(&entry).unwrap());
        assert!(far(&r).is_empty());
        assert!(entry.join("payload").exists());
        assert!(ready(&entry));
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_prune_during_a_build_waits_for_it_and_then_for_its_user() {
        let r = scratch_root("prune-building-target");
        let mut builds = 0;
        let entry = entry_in(&r, "target", "app-aaaaaaaaaaaa", |p| {
            builds += 1;
            paths::fresh(p)?;
            fs::write(p.join("payload"), b"half").unwrap();
            assert!(far(&r).is_empty());
            assert!(p.join("payload").exists());
            Ok(())
        })
        .unwrap();
        assert_eq!(builds, 1);
        assert_eq!(entry, entry_path(&r, "target", "app-aaaaaaaaaaaa"));
        assert!(ready(&entry));
        assert!(far(&r).is_empty());
        assert!(entry.join("payload").exists());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_prune_between_a_build_and_its_use_makes_the_user_rebuild() {
        let r = scratch_root("prune-built");
        let made = ready_entry(&r, "target/app-aaaaaaaaaaaa");
        assert_eq!(far(&r).len(), 1);
        assert!(!made.exists());
        assert!(lock_path(&r, "target", "app-aaaaaaaaaaaa").exists());
        let mut builds = 0;
        let entry = entry_in(&r, "target", "app-aaaaaaaaaaaa", |p| {
            builds += 1;
            paths::fresh(p)?;
            fs::write(p.join("payload"), b"again").map_err(|e| e.to_string())
        })
        .unwrap();
        assert_eq!(builds, 1);
        assert_eq!(entry, made);
        assert!(far(&r).is_empty());
        assert_eq!(fs::read(entry.join("payload")).unwrap(), b"again");
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn a_failed_build_leaves_no_ready_entry_and_the_next_user_builds_it() {
        let r = scratch_root("build-fails");
        let failed = entry_in(&r, "target", "app-aaaaaaaaaaaa", |p| {
            paths::fresh(p)?;
            fs::write(p.join("half"), b"x").unwrap();
            Err("the build failed".into())
        });
        assert_eq!(failed.unwrap_err(), "the build failed");
        let entry = entry_path(&r, "target", "app-aaaaaaaaaaaa");
        assert!(!ready(&entry));
        let entry = entry_in(&r, "target", "app-aaaaaaaaaaaa", |p| {
            paths::fresh(p)?;
            fs::write(p.join("whole"), b"x").map_err(|e| e.to_string())
        })
        .unwrap();
        assert!(ready(&entry));
        assert!(!entry.join("half").exists());
        assert!(entry.join("whole").exists());
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

    #[test]
    fn a_target_comes_back_after_a_prune_with_its_lock_still_in_place() {
        let r = scratch_root("target-back");
        let lock = lock_path(&r, "target", "app-aaaaaaaaaaaa");
        fs::create_dir_all(r.join("target/app-aaaaaaaaaaaa")).unwrap();
        assert_eq!(far(&r).len(), 1);
        assert!(lock.exists());
        let t = target_in(&r, "app-aaaaaaaaaaaa").unwrap();
        assert!(t.is_dir());
        assert!(far(&r).is_empty());
        assert!(t.is_dir());
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn an_entry_path_and_its_name_map_both_ways() {
        let r = Path::new("/c");
        for (section, name) in [
            ("git", "app-aaaaaaaaaaaa.git"),
            ("work", "app-aaaaaaaaaaaa"),
            ("target", "app-aaaaaaaaaaaa"),
        ] {
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
        for section in ["work", "target"] {
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
}
