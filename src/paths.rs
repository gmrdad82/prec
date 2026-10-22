use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

pub const DIRECT: &str = "PREC_DIRECT";
const HASH_BUF: usize = 64 * 1024;

static CANCEL: LazyLock<Arc<AtomicUsize>> = LazyLock::new(|| Arc::new(AtomicUsize::new(0)));
static GATE: Mutex<Vec<Kid>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug, PartialEq)]
struct Kid {
    pid: i32,
    group: i32,
    session: bool,
}

pub fn cancel_flag() -> Arc<AtomicUsize> {
    CANCEL.clone()
}

pub fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst) != 0
}

pub fn halt() -> ! {
    loop {
        std::thread::park();
    }
}

pub fn settle() {
    if cancelled() {
        halt();
    }
}

fn gate() -> MutexGuard<'static, Vec<Kid>> {
    GATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn gated<T, E>(
    start: impl FnOnce() -> Result<T, E>,
    pid: impl FnOnce(&T) -> Option<u32>,
    session: bool,
) -> Result<T, E> {
    let mut kids = gate();
    if cancelled() {
        drop(kids);
        halt();
    }
    let started = start()?;
    if let Some(pid) = pid(&started) {
        let pid = pid as i32;
        kids.push(Kid {
            pid,
            group: pid,
            session,
        });
    }
    Ok(started)
}

pub fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    gated(|| cmd.process_group(0).spawn(), |c| Some(c.id()), false)
}

pub fn spawn_session<T, E>(
    start: impl FnOnce() -> Result<T, E>,
    pid: impl FnOnce(&T) -> Option<u32>,
) -> Result<T, E> {
    gated(start, pid, true)
}

pub fn reaped(pid: u32) {
    gate().retain(|k| k.pid != pid as i32);
}

#[cfg(test)]
pub fn quiesce() {
    drop(gate());
}

pub fn status(cmd: &mut Command) -> std::io::Result<ExitStatus> {
    let mut child = spawn(cmd)?;
    let status = child.wait();
    reaped(child.id());
    status
}

pub fn output(cmd: &mut Command) -> std::io::Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = spawn(cmd)?;
    let pid = child.id();
    let out = child.wait_with_output();
    reaped(pid);
    out
}

struct Proc {
    pid: i32,
    group: i32,
    session: i32,
    zombie: bool,
}

fn proc_stat(text: &str) -> Option<Proc> {
    let (head, tail) = text.rsplit_once(") ")?;
    let pid = head.split_once(" (")?.0.trim().parse().ok()?;
    let f: Vec<&str> = tail.split_whitespace().collect();
    if f.get(1)?.parse::<i32>().is_err() {
        return None;
    }
    Some(Proc {
        pid,
        zombie: matches!(*f.first()?, "Z" | "X"),
        group: f.get(2)?.parse().ok()?,
        session: f.get(3)?.parse().ok()?,
    })
}

fn procs() -> Vec<Proc> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return vec![];
    };
    dir.flatten()
        .filter(|e| e.file_name().to_string_lossy().parse::<u32>().is_ok())
        .filter_map(|e| std::fs::read_to_string(e.path().join("stat")).ok())
        .filter_map(|t| proc_stat(&t))
        .collect()
}

fn left(all: &[Proc], kids: &[Kid], me: Option<i32>) -> Vec<i32> {
    all.iter()
        .filter(|p| !p.zombie && Some(p.pid) != me)
        .filter(|p| {
            kids.iter()
                .any(|k| k.group == p.group || (k.session && k.pid == p.session))
        })
        .map(|p| p.pid)
        .collect()
}

fn stop(kids: &[Kid], me: Option<i32>) {
    for k in kids {
        let _ = killpg(Pid::from_raw(k.group), Signal::SIGTERM);
        if k.session {
            let _ = kill(Pid::from_raw(k.pid), Signal::SIGHUP);
        }
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if left(&procs(), kids, me).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for pid in left(&procs(), kids, me) {
        let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
    }
}

pub fn stop_children() {
    let kids = gate().clone();
    stop(&kids, Some(std::process::id() as i32));
}

#[derive(Clone, Copy, PartialEq)]
pub enum Lock {
    Shared,
    Exclusive,
}

pub fn lock(path: &Path, how: Lock, what: &str) -> Result<File, String> {
    lock_with(path, how, what, &mut |said| eprintln!("{said}"))
}

fn lock_with(
    path: &Path,
    how: Lock,
    what: &str,
    waiting: &mut dyn FnMut(&str),
) -> Result<File, String> {
    loop {
        let f = lock_once(path, how, what, waiting)?;
        if same_file(&f, path) {
            return Ok(f);
        }
    }
}

fn lock_once(
    path: &Path,
    how: Lock,
    what: &str,
    waiting: &mut dyn FnMut(&str),
) -> Result<File, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let tried = match how {
        Lock::Shared => f.try_lock_shared(),
        Lock::Exclusive => f.try_lock(),
    };
    match tried {
        Ok(()) => Ok(f),
        Err(TryLockError::WouldBlock) => {
            let holder = std::fs::read_to_string(path).unwrap_or_default();
            let holder = holder.trim();
            waiting(&format!(
                "waiting for {} to finish with {what} ({})…",
                if holder.is_empty() {
                    "another prec"
                } else {
                    holder
                },
                path.display()
            ));
            match how {
                Lock::Shared => f.lock_shared(),
                Lock::Exclusive => f.lock(),
            }
            .map_err(|e| format!("lock {}: {e}", path.display()))?;
            Ok(f)
        }
        Err(TryLockError::Error(e)) => Err(format!("lock {}: {e}", path.display())),
    }
}

pub fn hold(name: &str, what: &str) -> Result<File, String> {
    let path = scratch(&["locks"])?.join(format!("{name}.lock"));
    let mut f = lock(&path, Lock::Exclusive, &format!("{name} in this tree"))?;
    let _ = f.set_len(0);
    let _ = writeln!(f, "prec {} ({what})", std::process::id());
    Ok(f)
}

pub fn same_file(f: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (f.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

pub fn direct() -> bool {
    std::env::var_os(DIRECT).is_some()
}

pub fn environment(pairs: &[(String, String)], path: &str) -> Vec<(String, String)> {
    let mut env = Vec::new();
    for key in ["USER", "LOGNAME"] {
        if let Ok(v) = std::env::var(key) {
            env.push((key.to_string(), v));
        }
    }
    for (k, v) in pairs {
        if let Some(found) = env.iter_mut().find(|(ek, _)| ek == k) {
            found.1.clone_from(v);
        } else {
            env.push((k.clone(), v.clone()));
        }
    }
    env.retain(|(k, _)| k != "PATH");
    env.push(("PATH".to_string(), path.to_string()));
    env
}

pub fn confined(bin: &str, memory_max: &str) -> Command {
    wrapped(bin, memory_max, &[bin])
}

fn wrapped(bin: &str, memory_max: &str, tail: &[&str]) -> Command {
    let confine = crate::config::get().confine.as_ref().filter(|_| !direct());
    wrap(bin, memory_max, tail, confine, build_cpus)
}

fn wrap(
    bin: &str,
    memory_max: &str,
    tail: &[&str],
    confine: Option<&crate::config::Confine>,
    cpus: impl FnOnce() -> Option<String>,
) -> Command {
    let Some(confine) = confine else {
        return Command::new(bin);
    };
    let mut c = Command::new("systemd-run");
    c.args([
        "--user",
        "--quiet",
        "--scope",
        "--collect",
        &format!(
            "--slice={}",
            confine.slice.as_deref().unwrap_or("build.slice")
        ),
        "-p",
        &format!("MemoryMax={memory_max}"),
        "-p",
        "MemorySwapMax=0",
        "--",
    ]);
    if let Some(cpus) = cpus() {
        c.args(["taskset", "-c", &cpus]);
    }
    c.args(["prlimit", "--nice=10", "nice", "-n", "10"]);
    c.args(tail);
    c
}

pub fn confine_here() {
    #[cfg(target_os = "linux")]
    if crate::config::get().confine.is_some() && !direct() {
        unsafe {
            nix::libc::setpriority(nix::libc::PRIO_PROCESS, 0, 10);
        }
        if let Some(set) = build_cpus().as_deref().and_then(cpu_set) {
            let _ = nix::sched::sched_setaffinity(nix::unistd::Pid::from_raw(0), &set);
        }
    }
}

#[cfg(target_os = "linux")]
fn cpu_set(list: &str) -> Option<nix::sched::CpuSet> {
    let mut set = nix::sched::CpuSet::new();
    let mut any = false;
    for part in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse().ok()?, b.trim().parse().ok()?),
            None => {
                let v: usize = part.parse().ok()?;
                (v, v)
            }
        };
        for cpu in a..=b {
            set.set(cpu).ok()?;
            any = true;
        }
    }
    any.then_some(set)
}

static BUILD_CPUS: OnceLock<Option<String>> = OnceLock::new();

fn build_cpus() -> Option<String> {
    BUILD_CPUS
        .get_or_init(|| cpus_of(crate::config::get().confine.as_ref()?))
        .clone()
}

fn cpus_of(confine: &crate::config::Confine) -> Option<String> {
    let (cpus, warning) = cpus_said(confine);
    if let Some(w) = warning {
        eprintln!("{w}");
    }
    cpus
}

fn cpus_said(confine: &crate::config::Confine) -> (Option<String>, Option<String>) {
    if let Some(cpus) = confine.cpus.as_deref() {
        return (Some(cpus.to_string()).filter(|v| !v.trim().is_empty()), None);
    }
    let Some(from) = confine.cpus_from.as_deref() else {
        return (None, None);
    };
    let key = confine.cpus_key.as_deref().unwrap_or("BUILD_CPUS");
    let text = match std::fs::read_to_string(from) {
        Ok(text) => text,
        Err(e) => {
            return (
                None,
                Some(format!("prec: {}: {e}; running unpinned", from.display())),
            );
        }
    };
    match file_cpus(Some(&text), key) {
        Some(cpus) => (Some(cpus), None),
        None => (
            None,
            Some(format!(
                "prec: {} names no {key}; running unpinned",
                from.display()
            )),
        ),
    }
}

fn file_cpus(text: Option<&str>, key: &str) -> Option<String> {
    text.and_then(|text| env_value(text, key))
        .filter(|v| !v.trim().is_empty())
}

fn env_value(text: &str, key: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
    })
}

static CALLER_TREE: OnceLock<Result<PathBuf, String>> = OnceLock::new();

pub fn caller_tree() -> Result<PathBuf, String> {
    CALLER_TREE
        .get_or_init(|| -> Result<PathBuf, String> {
            let out = output(Command::new("git").args(["rev-parse", "--show-toplevel"]))
                .map_err(|e| format!("git: {e}"))?;
            if !out.status.success() {
                return Err(
                    "prec runs inside a git tree; its outputs land in that tree's tmp/".into(),
                );
            }
            Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
        })
        .clone()
}

pub fn caller_tmp() -> Result<PathBuf, String> {
    let tmp = caller_tree()?.join("tmp");
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    Ok(tmp)
}

pub fn printed(p: &Path) -> String {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() {
        shown(p)
    } else {
        p.display().to_string()
    }
}

pub fn shown(p: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| shown_from(p, &cwd))
        .unwrap_or_else(|| p.display().to_string())
}

fn shown_from(p: &Path, cwd: &Path) -> Option<String> {
    let rel = p.strip_prefix(cwd).ok()?;
    (!rel.as_os_str().is_empty()).then(|| rel.display().to_string())
}

pub fn absolute(p: &Path) -> Result<PathBuf, String> {
    let base = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map_err(|e| e.to_string())?.join(p)
    };
    let mut out = PathBuf::new();
    for c in base.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    Ok(out)
}

pub fn inside_tmp(p: &Path) -> Result<PathBuf, String> {
    let tmp = caller_tmp()?;
    let abs = absolute(p)?;
    if !resolved(&abs).starts_with(resolved(&tmp)) {
        return Err(format!(
            "{} is outside the caller's tmp/ ({}); prec writes nowhere else",
            abs.display(),
            tmp.display()
        ));
    }
    Ok(abs)
}

fn resolved(abs: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut at = abs;
    loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            return rest.iter().rev().fold(real, |p, c| p.join(c));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                at = parent;
            }
            _ => return abs.to_path_buf(),
        }
    }
}

pub fn scratch(parts: &[&str]) -> Result<PathBuf, String> {
    let mut dir = caller_tmp()?.join("prec");
    for p in parts {
        dir.push(p);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

pub fn fresh(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn hash_file(p: &Path) -> Result<String, String> {
    let mut f = File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_BUF];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        if n == 0 {
            return Ok(format!("{:x}", hasher.finalize()));
        }
        hasher.update(&buf[..n]);
    }
}

pub fn hash_tree(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, hash_file(&path)?));
        }
    }
    Ok(())
}

pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proc_stat_line_gives_the_group_and_session_whatever_the_name_holds() {
        let p = proc_stat("4242 (a) b (c)) S 7 4243 4244 34816 0").unwrap();
        assert_eq!(
            (p.pid, p.group, p.session, p.zombie),
            (4242, 4243, 4244, false)
        );
        assert!(proc_stat("4242 (a) S").is_none());
        assert!(proc_stat("9 (x) Z 1 9 9 0").unwrap().zombie);
        assert!(proc_stat("garbage").is_none());
    }

    #[test]
    fn what_is_left_is_the_recorded_groups_and_the_ptys_session() {
        let p = |pid, group, session, zombie| Proc {
            pid,
            group,
            session,
            zombie,
        };
        let kid = |pid, session| Kid {
            pid,
            group: pid,
            session,
        };
        let all = [
            p(10, 10, 5, false),
            p(11, 11, 5, false),
            p(21, 20, 5, false),
            p(22, 20, 5, true),
            p(31, 31, 30, false),
            p(32, 32, 32, false),
            p(41, 41, 20, false),
        ];
        let kids = [kid(20, false), kid(30, true)];
        assert_eq!(left(&all, &kids, Some(10)), vec![21, 31]);
        assert_eq!(left(&all, &kids, None), vec![21, 31]);
        assert!(left(&all, &[], Some(10)).is_empty());
    }

    #[test]
    fn a_childs_environment_is_the_identity_the_pairs_and_path() {
        let pairs = [
            ("ONE".into(), "a".into()),
            ("TWO".into(), "b".into()),
            ("ONE".into(), "c".into()),
            ("PATH".into(), "ignored".into()),
            ("USER".into(), "other".into()),
        ];
        let env = environment(&pairs, "/only");
        let mut keys = Vec::new();
        if std::env::var_os("USER").is_some() {
            keys.push("USER");
        }
        if std::env::var_os("LOGNAME").is_some() {
            keys.push("LOGNAME");
        }
        keys.extend(["ONE", "TWO"]);
        if std::env::var_os("USER").is_none() {
            keys.push("USER");
        }
        keys.push("PATH");
        assert_eq!(
            env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            keys
        );
        let value = |key: &str| {
            env.iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(value("ONE"), vec!["c"]);
        assert_eq!(value("TWO"), vec!["b"]);
        assert_eq!(value("USER"), vec!["other"]);
        assert_eq!(value("PATH"), vec!["/only"]);
        if let Ok(logname) = std::env::var("LOGNAME") {
            assert_eq!(value("LOGNAME"), vec![logname.as_str()]);
        }
        for (k, v) in std::env::vars() {
            if k == "USER" || k == "LOGNAME" {
                continue;
            }
            assert!(
                env.iter().all(|(ek, ev)| ek != &k || ev != &v),
                "{k} leaked from prec"
            );
        }
    }

    #[test]
    fn the_caller_tree_is_computed_once() {
        let first = caller_tree().unwrap();
        let second = caller_tree().unwrap();
        assert_eq!(first, second);
        assert!(first.join("Cargo.toml").is_file());
    }

    #[cfg(target_os = "linux")]
    fn gone(pid: i32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|t| proc_stat(&t))
            .is_none_or(|p| p.zombie)
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_cancel_stops_a_spawned_childs_own_children() {
        use std::io::BufRead;
        use std::os::unix::process::ExitStatusExt;
        let mut child = spawn(
            Command::new("bash")
                .args(["-c", "sleep 30 & echo $!; wait"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped()),
        )
        .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let sleep: i32 = line.trim().parse().unwrap();
        let kid = *gate().iter().find(|k| k.pid == child.id() as i32).unwrap();
        assert_eq!(kid.group, kid.pid);
        assert!(!gone(sleep));
        stop(&[kid], None);
        assert_eq!(child.wait().unwrap().signal(), Some(15));
        reaped(child.id());
        assert!(gone(sleep));
        assert!(!gate().iter().any(|k| k.pid == kid.pid));
    }

    #[test]
    fn a_file_hashes_the_same_streamed_as_whole() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/test-paths");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hashed");
        let bytes: Vec<u8> = (0..HASH_BUF * 2 + 77)
            .map(|i| (i * 31 % 251) as u8)
            .collect();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(hash_file(&path).unwrap(), hash_bytes(&bytes));
        assert!(hash_file(&dir.join("absent")).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_path_under_the_working_folder_is_shown_relative_to_it() {
        let cwd = Path::new("/work/app");
        assert_eq!(
            shown_from(Path::new("/work/app/tmp/renders/x.json"), cwd).as_deref(),
            Some("tmp/renders/x.json")
        );
        assert_eq!(shown_from(Path::new("/elsewhere/x.json"), cwd), None);
        assert_eq!(shown_from(cwd, cwd), None);
    }

    #[test]
    fn the_cpus_come_from_the_named_key_of_an_env_file() {
        let text = "# machine\nBUILD_CPUS=2-3,6-7\nQUEUE_SLOTS=3\nNO_COMPRESS=\"a b\"\n";
        assert_eq!(
            file_cpus(Some(text), "BUILD_CPUS").as_deref(),
            Some("2-3,6-7")
        );
        assert_eq!(env_value(text, "NO_COMPRESS").as_deref(), Some("a b"));
        assert_eq!(
            file_cpus(
                Some("BUILD_CPUS=0-3\nexport BUILD_CPUS=4-7\n"),
                "BUILD_CPUS"
            )
            .as_deref(),
            Some("4-7")
        );
        assert_eq!(file_cpus(Some("OTHER=1\n"), "BUILD_CPUS"), None);
        assert_eq!(file_cpus(Some("BUILD_CPUS=\n"), "BUILD_CPUS"), None);
        assert_eq!(file_cpus(None, "BUILD_CPUS"), None);
    }

    fn test_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/test-paths")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn argv(c: &Command) -> Vec<String> {
        std::iter::once(c.get_program())
            .chain(c.get_args())
            .map(|a| a.to_string_lossy().to_string())
            .collect()
    }

    fn confine(slice: Option<&str>) -> crate::config::Confine {
        crate::config::Confine {
            slice: slice.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn an_unconfined_child_runs_bare_and_never_asks_for_cpus() {
        let c = wrap("ffmpeg", "4G", &["ffmpeg"], None, || {
            panic!("the cpus are read only to confine")
        });
        assert_eq!(argv(&c), ["ffmpeg"]);
    }

    #[test]
    fn a_confined_child_runs_in_the_slice_with_its_memory_cap_pinned_and_niced() {
        let c = wrap("ffmpeg", "4G", &["ffmpeg"], Some(&confine(None)), || {
            Some("2-3,6".into())
        });
        assert_eq!(
            argv(&c),
            [
                "systemd-run",
                "--user",
                "--quiet",
                "--scope",
                "--collect",
                "--slice=build.slice",
                "-p",
                "MemoryMax=4G",
                "-p",
                "MemorySwapMax=0",
                "--",
                "taskset",
                "-c",
                "2-3,6",
                "prlimit",
                "--nice=10",
                "nice",
                "-n",
                "10",
                "ffmpeg",
            ]
        );
        let unpinned = wrap(
            "chromium",
            "8G",
            &["chromium"],
            Some(&confine(Some("render.slice"))),
            || None,
        );
        let got = argv(&unpinned);
        assert!(got.contains(&"--slice=render.slice".to_string()), "{got:?}");
        assert!(got.contains(&"MemoryMax=8G".to_string()), "{got:?}");
        assert!(!got.contains(&"taskset".to_string()), "{got:?}");
        assert_eq!(got.last().map(String::as_str), Some("chromium"));
    }

    #[test]
    fn the_cpus_come_from_the_config_or_its_env_file_and_a_file_without_them_runs_unpinned() {
        let dir = test_dir("cpus-of");
        let named = |cpus: &str| crate::config::Confine {
            cpus: Some(cpus.into()),
            ..Default::default()
        };
        assert_eq!(cpus_of(&named("4-7")).as_deref(), Some("4-7"));
        assert_eq!(cpus_of(&named("  ")), None);
        let from = |file: &Path, key: Option<&str>| crate::config::Confine {
            cpus_from: Some(file.to_path_buf()),
            cpus_key: key.map(str::to_string),
            ..Default::default()
        };
        let env = dir.join("machine.env");
        std::fs::write(&env, "QUEUE=2\nBUILD_CPUS=\"8-15\"\nRENDER_CPUS=16-23\n").unwrap();
        assert_eq!(cpus_of(&from(&env, None)).as_deref(), Some("8-15"));
        assert_eq!(
            cpus_of(&from(&env, Some("RENDER_CPUS"))).as_deref(),
            Some("16-23")
        );
        assert_eq!(cpus_of(&from(&env, Some("OTHER_CPUS"))), None);
        let blank = dir.join("blank.env");
        std::fs::write(&blank, "BUILD_CPUS=\n").unwrap();
        assert_eq!(cpus_of(&from(&blank, None)), None);
        assert_eq!(cpus_of(&from(&dir.join("absent.env"), None)), None);
        assert_eq!(cpus_of(&crate::config::Confine::default()), None);
    }

    #[test]
    fn a_cpus_file_missing_its_key_or_missing_itself_says_which_and_runs_unpinned() {
        let dir = test_dir("cpus-said");
        let from = |file: &Path, key: Option<&str>| crate::config::Confine {
            cpus_from: Some(file.to_path_buf()),
            cpus_key: key.map(str::to_string),
            ..Default::default()
        };
        let env = dir.join("machine.env");
        std::fs::write(&env, "BUILD_CPUS=8-15\nRENDER_CPUS=\n").unwrap();
        assert_eq!(cpus_said(&from(&env, None)), (Some("8-15".into()), None));
        for key in ["OTHER_CPUS", "RENDER_CPUS"] {
            assert_eq!(
                cpus_said(&from(&env, Some(key))),
                (
                    None,
                    Some(format!(
                        "prec: {} names no {key}; running unpinned",
                        env.display()
                    ))
                )
            );
        }
        let absent = dir.join("absent.env");
        let (cpus, said) = cpus_said(&from(&absent, None));
        assert_eq!(cpus, None);
        let said = said.unwrap();
        assert!(
            said.starts_with(&format!("prec: {}: ", absent.display())),
            "{said}"
        );
        assert!(said.ends_with("; running unpinned"), "{said}");
        assert!(!said.contains("names no"), "{said}");
        assert_eq!(cpus_said(&crate::config::Confine::default()), (None, None));
        let pinned = crate::config::Confine {
            cpus: Some("1-2".into()),
            cpus_from: Some(absent.clone()),
            ..Default::default()
        };
        assert_eq!(cpus_said(&pinned), (Some("1-2".into()), None));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_cpu_list_becomes_the_set_it_names() {
        let set = cpu_set("0-1, 3").unwrap();
        assert!(set.is_set(0).unwrap() && set.is_set(1).unwrap() && set.is_set(3).unwrap());
        assert!(!set.is_set(2).unwrap());
        for bad in ["", " , ", "a", "1-b", "3-1"] {
            assert!(cpu_set(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_path_is_made_absolute_and_its_dots_resolved_without_touching_the_disk() {
        assert_eq!(
            absolute(Path::new("/a/./b/../c")).unwrap(),
            PathBuf::from("/a/c")
        );
        assert_eq!(absolute(Path::new("/..")).unwrap(), PathBuf::from("/"));
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            absolute(Path::new("x/../y/./z")).unwrap(),
            cwd.join("y/z")
        );
    }

    #[test]
    fn an_out_folder_is_taken_only_inside_the_callers_tmp() {
        let tree = caller_tree().unwrap();
        let tmp = caller_tmp().unwrap();
        assert_eq!(tmp, tree.join("tmp"));
        assert!(tmp.is_dir());
        assert_eq!(
            inside_tmp(&tmp.join("a/../b")).unwrap(),
            tmp.join("b")
        );
        assert_eq!(inside_tmp(&tmp).unwrap(), tmp);
        for outside in [tmp.join("../src"), tree.join("tmpx/out"), tree.clone()] {
            let e = inside_tmp(&outside).unwrap_err();
            assert!(
                e.ends_with(&format!(
                    "is outside the caller's tmp/ ({}); prec writes nowhere else",
                    tmp.display()
                )),
                "{e}"
            );
        }
    }

    #[test]
    fn an_out_folder_through_a_link_is_judged_by_where_the_link_leads() {
        let dir = test_dir("inside-link");
        let away = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR"))
            .unwrap()
            .join("src");
        std::os::unix::fs::symlink(&away, dir.join("out")).unwrap();
        std::fs::create_dir_all(dir.join("real")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("near")).unwrap();
        let tmp = caller_tmp().unwrap();
        for escaping in [dir.join("out/x"), dir.join("out"), dir.join("out/a/b")] {
            let e = inside_tmp(&escaping).unwrap_err();
            assert_eq!(
                e,
                format!(
                    "{} is outside the caller's tmp/ ({}); prec writes nowhere else",
                    escaping.display(),
                    tmp.display()
                )
            );
        }
        assert!(!away.join("x").exists() && !away.join("a").exists());
        assert_eq!(
            inside_tmp(&dir.join("near/x")).unwrap(),
            dir.join("near/x")
        );
        assert_eq!(
            inside_tmp(&dir.join("not-yet/deep/x")).unwrap(),
            dir.join("not-yet/deep/x")
        );
    }

    #[test]
    fn a_path_resolves_through_its_deepest_existing_part() {
        let dir = std::fs::canonicalize(test_dir("resolved")).unwrap();
        std::fs::create_dir_all(dir.join("real/inner")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
        assert_eq!(resolved(&dir.join("link/inner/new/x")), dir.join("real/inner/new/x"));
        assert_eq!(resolved(&dir.join("link")), dir.join("real"));
        assert_eq!(resolved(&dir.join("absent/y")), dir.join("absent/y"));
        assert_eq!(resolved(Path::new("/")), PathBuf::from("/"));
    }

    #[test]
    fn scratch_folders_live_under_the_callers_tmp_prec() {
        let dir = scratch(&["test-paths-scratch", "a", "b"]).unwrap();
        assert_eq!(
            dir,
            caller_tmp().unwrap().join("prec/test-paths-scratch/a/b")
        );
        assert!(dir.is_dir());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_fresh_folder_is_empty_whether_it_existed_or_not() {
        let dir = test_dir("fresh");
        let old = dir.join("old");
        std::fs::create_dir_all(old.join("deep")).unwrap();
        std::fs::write(old.join("deep/file"), "x").unwrap();
        fresh(&old).unwrap();
        assert!(old.is_dir());
        assert_eq!(std::fs::read_dir(&old).unwrap().count(), 0);
        let new = dir.join("new/nested");
        fresh(&new).unwrap();
        assert!(new.is_dir());
        std::fs::write(dir.join("file"), "x").unwrap();
        assert!(fresh(&dir.join("file/below")).is_err());
    }

    #[test]
    fn a_tree_hashes_to_its_sorted_relative_files() {
        let dir = test_dir("hash-tree");
        std::fs::create_dir_all(dir.join("b/c")).unwrap();
        std::fs::create_dir_all(dir.join("empty")).unwrap();
        std::fs::write(dir.join("z"), "zed").unwrap();
        std::fs::write(dir.join("b/c/a"), "deep").unwrap();
        std::fs::write(dir.join("a"), "").unwrap();
        assert_eq!(
            hash_tree(&dir).unwrap(),
            vec![
                ("a".to_string(), hash_bytes(b"")),
                ("b/c/a".to_string(), hash_bytes(b"deep")),
                ("z".to_string(), hash_bytes(b"zed")),
            ]
        );
        assert_eq!(
            hash_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(hash_tree(&dir.join("empty")).unwrap().is_empty());
        assert!(hash_tree(&dir.join("absent")).is_err());
    }

    #[test]
    fn a_safe_name_is_one_plain_path_part() {
        for good in ["demo", "a-b_c.1", "v1.2.3", "..x"] {
            assert!(safe_name(good), "{good:?}");
        }
        for bad in ["", ".", "..", "a/b", "a b", "a\\b", "é", "a\0b", "~"] {
            assert!(!safe_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_slug_is_lowercase_words_joined_by_single_dashes() {
        assert_eq!(slug("Hello, World!"), "hello-world");
        assert_eq!(slug("--a__b--"), "a-b");
        assert_eq!(slug("v1.2+dirty"), "v1-2-dirty");
        assert_eq!(slug("Été 3"), "t-3");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn a_lock_makes_its_folder_and_two_shared_holders_share_it() {
        let dir = test_dir("lock");
        let path = dir.join("deep/a.lock");
        let first = lock(&path, Lock::Shared, "a test").unwrap();
        let second = lock(&path, Lock::Shared, "a test").unwrap();
        assert!(same_file(&first, &path) && same_file(&second, &path));
        std::fs::remove_file(&path).unwrap();
        assert!(!same_file(&first, &path));
        std::fs::write(dir.join("file"), "x").unwrap();
        let e = lock(&dir.join("file/b.lock"), Lock::Exclusive, "a test").unwrap_err();
        assert!(e.starts_with(&dir.join("file").display().to_string()), "{e}");
    }

    #[test]
    fn a_taken_lock_says_who_holds_it_and_is_had_once_they_let_go() {
        let dir = test_dir("lock-wait");
        let path = dir.join("a.lock");
        let mut held = lock(&path, Lock::Exclusive, "a test").unwrap();
        writeln!(held, "prec 1 (record demo s)").unwrap();
        let (said_tx, said_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let p = path.clone();
        let waiter = std::thread::spawn(move || {
            lock_with(&p, Lock::Shared, "demo in this tree", &mut |said| {
                said_tx.send(said.to_string()).unwrap();
                go_rx.recv().unwrap();
            })
        });
        let said = said_rx.recv().unwrap();
        assert_eq!(
            said,
            format!(
                "waiting for prec 1 (record demo s) to finish with demo in this tree ({})…",
                path.display()
            )
        );
        drop(held);
        go_tx.send(()).unwrap();
        let got = waiter.join().unwrap().unwrap();
        assert!(same_file(&got, &path));
        assert!(said_rx.try_recv().is_err());
    }

    #[test]
    fn a_waiter_with_no_named_holder_waits_for_another_prec() {
        let dir = test_dir("lock-anon");
        let path = dir.join("a.lock");
        let held = lock(&path, Lock::Shared, "a test").unwrap();
        let (said_tx, said_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let p = path.clone();
        let waiter = std::thread::spawn(move || {
            lock_with(&p, Lock::Exclusive, "x", &mut |said| {
                said_tx.send(said.to_string()).unwrap();
                go_rx.recv().unwrap();
            })
        });
        assert_eq!(
            said_rx.recv().unwrap(),
            format!("waiting for another prec to finish with x ({})…", path.display())
        );
        drop(held);
        go_tx.send(()).unwrap();
        assert!(same_file(&waiter.join().unwrap().unwrap(), &path));
    }

    #[test]
    fn a_lock_file_replaced_while_waiting_is_locked_again_by_its_new_self() {
        let dir = test_dir("lock-replaced");
        let path = dir.join("a.lock");
        let held = lock(&path, Lock::Exclusive, "a test").unwrap();
        let (said_tx, said_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let p = path.clone();
        let waiter = std::thread::spawn(move || {
            let mut times = 0;
            let f = lock_with(&p, Lock::Exclusive, "x", &mut |said| {
                times += 1;
                said_tx.send(said.to_string()).unwrap();
                go_rx.recv().unwrap();
            });
            (f, times)
        });
        said_rx.recv().unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "").unwrap();
        let replaced = std::fs::File::open(&path).unwrap();
        assert!(!same_file(&held, &path));
        drop(held);
        go_tx.send(()).unwrap();
        let (got, times) = waiter.join().unwrap();
        let got = got.unwrap();
        assert_eq!(times, 1);
        assert!(same_file(&got, &path));
        assert!(same_file(&replaced, &path));
        assert!(matches!(
            replaced.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
    }

    #[test]
    fn a_held_lock_names_this_prec_and_what_it_does() {
        let f = hold("test-paths-hold", "record demo s").unwrap();
        let path = scratch(&["locks"]).unwrap().join("test-paths-hold.lock");
        assert!(same_file(&f, &path));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("prec {} (record demo s)\n", std::process::id())
        );
        drop(f);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_waited_child_gives_its_output_and_status_and_a_missing_one_is_an_error() {
        let out = output(Command::new("sh").args(["-c", "echo out; echo err >&2; exit 3"])).unwrap();
        assert_eq!(out.stdout, b"out\n");
        assert_eq!(out.stderr, b"err\n");
        assert_eq!(out.status.code(), Some(3));
        let st = status(Command::new("sh").args(["-c", "exit 0"]).stdout(Stdio::null())).unwrap();
        assert!(st.success());
        let e = spawn(&mut Command::new("prec-test-no-such-program")).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
    }
}
