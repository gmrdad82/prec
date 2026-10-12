#![allow(dead_code)]

use serde_json::{Value, json};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

pub struct Proc {
    pub pid: u32,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub cgroup: String,
}

pub fn tree() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn scratch(group: &str, name: &str) -> PathBuf {
    let dir = tree().join("tmp").join(group).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn caller(group: &str, name: &str) -> PathBuf {
    let dir = scratch(group, name);
    let ok = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git init {}", dir.display());
    std::fs::canonicalize(&dir).unwrap()
}

pub fn runs(tree: &Path, name: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(tree.join("tmp/prec/run")) else {
        return vec![];
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            e.path().is_dir()
                && e.file_name()
                    .to_string_lossy()
                    .strip_prefix(name)
                    .and_then(|rest| rest.strip_prefix('-'))
                    .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

pub fn no_chromium(tree: &Path, name: &str) -> bool {
    runs(tree, name)
        .iter()
        .all(|r| !r.join("chromium").exists())
}

pub fn confine(c: &mut Command, cwd: &Path, archive: &Path) {
    c.current_dir(cwd)
        .env("PITO_ARCHIVE", archive)
        .env("PITO_JOBS", "off")
        .env("PREC_CONFIG", "/dev/null")
        .env("PREC_DIRECT", "1");
}

pub fn confined(c: &mut Command, dir: &Path) {
    let config = dir.join("confine.toml");
    std::fs::write(&config, "[confine]\n").unwrap();
    c.env_remove("PREC_DIRECT").env("PREC_CONFIG", config);
}

pub fn prec(cwd: &Path, archive: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_prec"));
    c.args(args);
    confine(&mut c, cwd, archive);
    c
}

pub fn run(cwd: &Path, archive: &Path, args: &[&str]) -> Output {
    prec(cwd, archive, args).output().unwrap()
}

pub fn expect_ok(o: &Output, what: &str) {
    assert!(
        o.status.success(),
        "{what}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

pub fn ok(mut c: Command) -> Output {
    let o = c.output().unwrap();
    let args: Vec<String> = c
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    expect_ok(&o, &format!("prec {}", args.join(" ")));
    o
}

pub fn last_path(o: &Output) -> PathBuf {
    PathBuf::from(
        String::from_utf8_lossy(&o.stdout)
            .trim()
            .lines()
            .last()
            .unwrap(),
    )
}

pub fn spawn(mut c: Command, dir: &Path) -> Child {
    let err = File::create(dir.join("prec.err")).unwrap();
    c.stdout(Stdio::null())
        .stderr(Stdio::from(err))
        .spawn()
        .unwrap()
}

pub fn said(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("prec.err")).unwrap_or_default()
}

pub fn until(limit: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    loop {
        if ready() {
            return true;
        }
        if started.elapsed() > limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn wait_for(what: &str, dir: &Path, secs: u64, ready: impl FnMut() -> bool) {
    assert!(
        until(Duration::from_secs(secs), ready),
        "{what} never happened; prec said:\n{}",
        said(dir)
    );
}

pub fn try_exit(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let mut status = None;
    until(limit, || {
        status = child.try_wait().unwrap();
        status.is_some()
    });
    status
}

pub fn exits(child: &mut Child, dir: &Path) -> ExitStatus {
    try_exit(child, Duration::from_secs(60)).unwrap_or_else(|| {
        let _ = child.kill();
        panic!("prec never exited; it said:\n{}", said(dir))
    })
}

pub fn signal(pid: u32, name: &str) -> bool {
    Command::new("kill")
        .args([format!("-{name}"), pid.to_string()])
        .status()
        .unwrap()
        .success()
}

fn words(raw: &[u8], split_spaces: bool) -> Vec<String> {
    raw.split(|&b| b == 0 || (split_spaces && b == b' '))
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).to_string())
        .collect()
}

fn state_of(stat: &str) -> Option<char> {
    stat.rsplit_once(") ").and_then(|(_, t)| t.chars().next())
}

pub fn procs() -> Vec<Proc> {
    let me = std::process::id();
    let mut found = vec![];
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if pid == me {
            continue;
        }
        let stat = std::fs::read_to_string(e.path().join("stat")).unwrap_or_default();
        if matches!(state_of(&stat), None | Some('Z') | Some('X')) {
            continue;
        }
        let read = |file: &str| std::fs::read(e.path().join(file)).unwrap_or_default();
        found.push(Proc {
            pid,
            args: words(&read("cmdline"), true),
            env: words(&read("environ"), false),
            cgroup: String::from_utf8_lossy(&read("cgroup")).trim().to_string(),
        });
    }
    found.sort_by_key(|p| p.pid);
    found
}

pub fn alive(pid: &str) -> bool {
    std::fs::read_to_string(format!("/proc/{}/stat", pid.trim()))
        .ok()
        .and_then(|s| state_of(&s))
        .is_some_and(|state| state != 'Z' && state != 'X')
}

pub fn gone(limit: Duration, find: impl Fn() -> Vec<Proc>) -> Vec<Proc> {
    let mut left = find();
    until(limit, || {
        left = find();
        left.is_empty()
    });
    left
}

pub fn theme(name: &str) -> Value {
    json!({
        "name": name, "background": "#1e1e2e", "foreground": "#cdd6f4",
        "selection": "#45475a", "cursor": "#f5e0dc",
        "black": "#45475a", "red": "#f38ba8", "green": "#a6e3a1", "yellow": "#f9e2af",
        "blue": "#89b4fa", "magenta": "#f5c2e7", "cyan": "#94e2d5", "white": "#bac2de",
        "brightBlack": "#585b70", "brightRed": "#f38ba8", "brightGreen": "#a6e3a1",
        "brightYellow": "#f9e2af", "brightBlue": "#89b4fa", "brightMagenta": "#f5c2e7",
        "brightCyan": "#94e2d5", "brightWhite": "#a6adc8",
    })
}

pub fn paint(r: u32, g: u32, b: u32) -> String {
    format!("\x1b[48;2;{r};{g};{b}m\x1b[2J")
}

pub fn write_cast(
    path: &Path,
    product: &str,
    shot: &str,
    kind: &str,
    events: &[(f64, String)],
    end: f64,
) {
    let header = json!({
        "version": 2, "width": 8, "height": 4,
        "x_terminal_renderer": {
            "product": product, "shot": shot, "kind": kind, "grid": [8, 4],
            "app_theme": theme(product), "frame": "bare", "closure": null,
            "app": {"rev": "local"},
        },
    });
    let mut lines = vec![header.to_string(), json!([0.0, "m", "show"]).to_string()];
    for (t, text) in events {
        lines.push(json!([t, "o", text]).to_string());
    }
    lines.push(json!([end, "m", "end"]).to_string());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

pub fn demo_app(dir: &Path, hooks: &str) -> PathBuf {
    let app = dir.join("app");
    std::fs::create_dir_all(&app).unwrap();
    let copied = Command::new("cp")
        .arg("-r")
        .arg(tree().join("examples/demo/render"))
        .arg(app.join("render"))
        .status()
        .unwrap();
    assert!(copied.success());
    let toml = app.join("render/terminal.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(&toml, format!("{hooks}{text}")).unwrap();
    app
}

pub fn add_shot(app: &Path, name: &str, tape: &str, extra: &str) {
    std::fs::write(app.join(format!("render/terminal/{name}.tape")), tape).unwrap();
    let toml = app.join("render/terminal.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    text.push_str(&format!(
        "\n[shots.{name}]\nkind = \"clip\"\ntape = \"render/terminal/{name}.tape\"\n{extra}"
    ));
    std::fs::write(&toml, text).unwrap();
}
