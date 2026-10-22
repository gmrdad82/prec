use crate::app::{self, DEFAULT_FONT, Resolved, Terminal};
use crate::archive;
use crate::compose;
use crate::encode;
use crate::font;
use crate::fps::Fps;
use crate::home::Ns;
use crate::jobs;
use crate::paths;
use crate::private;
use crate::record::{self, Cast, Timeline};
use crate::replay::{self, Look, Take};
use crate::tape;
use crate::theme::{self, Choice};
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

pub const TOOL: &str = "prec";
pub const DEFAULT_FPS: Fps = Fps::new(120, 1);

#[derive(Clone, Debug, Default)]
pub struct Opts {
    pub made: String,
    pub product: String,
    pub shot: String,
    pub bin: Option<PathBuf>,
    pub size: Option<String>,
    pub derive: Vec<String>,
    pub fps: Option<Fps>,
    pub poster: Option<String>,
    pub out: PathBuf,
    pub seed: u64,
    pub set: Vec<String>,
    pub theme: String,
    pub font: Option<String>,
    pub frame: Option<String>,
    pub fixtures: Option<PathBuf>,
    pub youtube: bool,
    pub keep_frames: bool,
    pub no_encode: bool,
    pub web: Vec<u32>,
    pub profile: Vec<String>,
    pub no_archive: bool,
    pub allow_private: bool,
}

pub fn stages(
    kind: &str,
    build: bool,
    record: bool,
    render: bool,
    archives: bool,
) -> Vec<&'static str> {
    let mut out = Vec::new();
    if record {
        if build {
            out.push("build");
        }
        out.push("record");
    }
    if render {
        out.push("replay");
        out.push(if kind == "still" { "derive" } else { "master" });
        if archives {
            out.push("archive");
        }
    }
    out
}

pub fn archives(kind: &str, product: &str, opts: &Opts, app: &Value) -> bool {
    archive::skip_reason(
        &archive::Run {
            no_archive: opts.no_archive,
            no_encode: opts.no_encode,
            product,
            has_master: kind == "still" || !opts.no_encode,
        },
        app,
    )
    .is_none()
}

pub fn validate(kind: &str, opts: &Opts) -> Result<(), String> {
    if let Some(p) = &opts.poster {
        if kind != "clip" {
            return Err("--poster is for clip and render".into());
        }
        app::Poster::from_flag(p)?;
    }
    if kind == "clip" {
        if !opts.profile.is_empty() {
            return Err("--profile for clips belongs to pcod".into());
        }
        if opts.youtube {
            return Err("--youtube for clips belongs to pcod".into());
        }
        if !opts.derive.is_empty() {
            return Err("--derive for clips belongs to pcod".into());
        }
        if !opts.web.is_empty() {
            return Err("--web makes still web copies; use pcod for clips".into());
        }
    }
    if let Some(size) = &opts.size {
        parse_size(size, kind)?;
    }
    for d in &opts.derive {
        parse_size(d, kind)?;
    }
    Ok(())
}

pub const MAX_SIDE: u32 = 16384;

pub fn parse_size(s: &str, kind: &str) -> Result<[u32; 2], String> {
    let size = match s {
        "720p" => [1280, 720],
        "1080p" => [1920, 1080],
        "1440p" => [2560, 1440],
        "2160p" | "4k" | "4K" => [3840, 2160],
        other => {
            let bad = || format!("size {other}: N, WxH, 720p, 1080p, 1440p or 2160p");
            let (w, h) = other.split_once('x').unwrap_or((other, other));
            [w.parse().map_err(|_| bad())?, h.parse().map_err(|_| bad())?]
        }
    };
    if size[0] == 0 || size[1] == 0 {
        return Err(format!("size {s}: a width and height above 0"));
    }
    if size[0] > MAX_SIDE || size[1] > MAX_SIDE {
        return Err(format!("size {s}: at most {MAX_SIDE} on a side"));
    }
    if kind == "clip" && (size[0] % 2 == 1 || size[1] % 2 == 1) {
        return Err(format!("size {s}: even width and height, for 4:2:0 video"));
    }
    Ok(size)
}

fn checked_name<'a>(what: &str, name: &'a str) -> Result<&'a str, String> {
    if !paths::safe_name(name) {
        return Err(format!(
            "{what} {name:?}: letters, digits, '-', '_' and '.' only"
        ));
    }
    Ok(name)
}

fn checked_kind(kind: &str) -> Result<&str, String> {
    if kind != "clip" && kind != "still" {
        return Err(format!("kind {kind:?}: clip or still"));
    }
    Ok(kind)
}

struct Web {
    px: u32,
    name: String,
    width: u32,
}

fn web_targets(web: &[u32], stem: &str) -> Vec<Web> {
    web.iter()
        .flat_map(|&px| {
            [
                Web {
                    px,
                    name: format!("{stem}-web-{px}"),
                    width: px,
                },
                Web {
                    px,
                    name: format!("{stem}-web-{px}@2x"),
                    width: px * 2,
                },
            ]
        })
        .collect()
}

fn check_web(targets: &[Web], width: u32) -> Result<(), String> {
    for t in targets {
        if t.width == 0 || t.width > width {
            return Err(format!(
                "--web {}: {} needs {} px, wider than the {width} px master; raise --size",
                t.px, t.name, t.width
            ));
        }
    }
    Ok(())
}

fn sets(opts: &Opts) -> Result<Vec<(String, String)>, String> {
    let out: Vec<(String, String)> = opts
        .set
        .iter()
        .map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .ok_or_else(|| format!("--set {kv}: key=value"))
        })
        .collect::<Result<_, _>>()?;
    for (k, _) in &out {
        if k == "camera" || k.starts_with("camera.") || k == "keys" || k == "key" {
            return Err(format!("--set {k}: {}", app::NO_CAMERA));
        }
        if !SET_KEYS.contains(&k.as_str()) {
            return Err(format!("--set {k}: known keys are {}", SET_KEYS.join(", ")));
        }
    }
    Ok(out)
}

const SET_KEYS: &[&str] = &[
    "grid",
    "typing",
    "settle",
    "closure",
    "closure.crossfade",
    "launch",
    "ground",
    "inset",
];

pub fn inset_px(spec: &str, size: [u32; 2]) -> Result<[u32; 4], String> {
    let bad = || format!("inset {spec}: 1 to 4 of N% or Npx, top,right,bottom,left");
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    let [t, r, b, l] = match parts.as_slice() {
        [a] => [*a, *a, *a, *a],
        [v, h] => [*v, *h, *v, *h],
        [t, h, b] => [*t, *h, *b, *h],
        [t, r, b, l] => [*t, *r, *b, *l],
        _ => return Err(bad()),
    };
    let side = |v: &str, dim: u32| -> Result<u32, String> {
        let (n, pct) = match v.strip_suffix('%') {
            Some(n) => (n, true),
            None => (v.strip_suffix("px").unwrap_or(v), false),
        };
        let n: f64 = n.trim().parse().map_err(|_| bad())?;
        if !n.is_finite() || n < 0.0 || (pct && n >= 100.0) {
            return Err(bad());
        }
        Ok(if pct { n / 100.0 * dim as f64 } else { n }.round() as u32)
    };
    Ok([
        side(t, size[1])?,
        side(r, size[0])?,
        side(b, size[1])?,
        side(l, size[0])?,
    ])
}

fn closure_from_sets(sets: &[(String, String)]) -> Result<Option<app::Closure>, String> {
    for (k, v) in sets.iter().rev() {
        if k == "closure" || k == "closure.crossfade" {
            return app::Closure::from_set(k, v).map(Some);
        }
    }
    Ok(None)
}

fn set_value(sets: &[(String, String)], key: &str) -> Option<String> {
    sets.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

struct Hook {
    cmd: String,
    env: Vec<(String, String)>,
    passed: Vec<(String, String)>,
    path: String,
    cwd: PathBuf,
}

static TEARDOWN: Mutex<Option<Hook>> = Mutex::new(None);

const HOOK_DRAIN: Duration = Duration::from_millis(500);

fn relay(from: impl Read, mut to: impl Write, passed: &[(String, String)]) {
    let mut from = BufReader::new(from);
    let mut line = Vec::new();
    loop {
        line.clear();
        match from.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let text = private::redact(&String::from_utf8_lossy(&line), passed);
                let _ = to.write_all(text.as_bytes());
                let _ = to.flush();
            }
        }
    }
}

fn relay_apart(
    from: impl Read + Send + 'static,
    to: impl Write + Send + 'static,
    passed: Vec<(String, String)>,
    done: mpsc::Sender<()>,
) {
    std::thread::spawn(move || {
        relay(from, to, &passed);
        let _ = done.send(());
    });
}

impl Hook {
    fn command(&self) -> Command {
        let mut c = Command::new("bash");
        c.args(["-c", &self.cmd]).current_dir(&self.cwd).env_clear();
        c.envs(paths::environment(&self.env, &self.path));
        c
    }

    fn run(&self, gated: bool) -> Result<(), String> {
        self.relayed(gated, std::io::stdout(), std::io::stderr())
    }

    fn relayed(
        &self,
        gated: bool,
        out: impl Write + Send + 'static,
        err: impl Write + Send + 'static,
    ) -> Result<(), String> {
        let mut c = self.command();
        c.stdout(Stdio::piped()).stderr(Stdio::piped());
        let started = if gated {
            paths::spawn(&mut c)
        } else {
            c.spawn()
        };
        let status = started.and_then(|mut child| {
            let (done, drained) = mpsc::channel();
            let mut relays = 0;
            let passed = private::by_line(&self.passed);
            if let Some(from) = child.stdout.take() {
                relay_apart(from, out, passed.clone(), done.clone());
                relays += 1;
            }
            if let Some(from) = child.stderr.take() {
                relay_apart(from, err, passed, done);
                relays += 1;
            }
            let status = child.wait();
            paths::reaped(child.id());
            let deadline = Instant::now() + HOOK_DRAIN;
            for _ in 0..relays {
                let left = deadline.saturating_duration_since(Instant::now());
                if drained.recv_timeout(left).is_err() {
                    break;
                }
            }
            status
        });
        self.ran(status)
    }

    fn ran(&self, status: std::io::Result<ExitStatus>) -> Result<(), String> {
        let ok = status.map_err(|e| format!("{}: {e}", self.cmd))?;
        if !ok.success() {
            return Err(format!("{} failed", self.cmd));
        }
        Ok(())
    }
}

pub fn tear_down(stop: impl FnOnce()) {
    let mut slot = TEARDOWN.lock().unwrap_or_else(|p| p.into_inner());
    stop();
    if let Some(h) = slot.take()
        && let Err(e) = h.run(false)
    {
        let _ = writeln!(
            std::io::stderr(),
            "teardown: {}",
            private::redact(&e, &h.passed)
        );
    }
}

pub fn hold(product: &str, shot: &str, what: &str) -> Result<File, String> {
    let product = checked_name("product", product)?;
    let shot = checked_name("shot", shot)?;
    paths::hold(&format!("{product}-{shot}"), what)
}

pub struct Run {
    pub dir: PathBuf,
    lock_path: PathBuf,
    _lock: File,
}

impl Run {
    fn remove(self) {
        if std::fs::remove_dir_all(&self.dir).is_ok() || !self.dir.exists() {
            let _ = std::fs::remove_file(&self.lock_path);
        }
    }
}

fn run_folder(name: &str) -> Result<Run, String> {
    let root = paths::scratch(&["run"])?;
    let id = std::process::id().to_string();
    clear_runs(&root, name, &id);
    let dir = root.join(format!("{name}-{id}"));
    let lock_path = root.join(format!("{name}-{id}.lock"));
    let lock = paths::lock(
        &lock_path,
        paths::Lock::Exclusive,
        &dir.display().to_string(),
    )?;
    paths::fresh(&dir)?;
    Ok(Run {
        dir,
        lock_path,
        _lock: lock,
    })
}

fn run_id<'a>(file: &'a str, name: &str) -> Option<&'a str> {
    let id = file.strip_prefix(name)?.strip_prefix('-')?;
    let id = id.strip_suffix(".lock").unwrap_or(id);
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

fn clear_runs(root: &Path, name: &str, mine: &str) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let ids: BTreeSet<String> = entries
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().into_string().ok()?;
            run_id(&file, name)
                .filter(|id| *id != mine)
                .map(String::from)
        })
        .collect();
    for id in ids {
        let lock = root.join(format!("{name}-{id}.lock"));
        let Ok(f) = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock)
        else {
            continue;
        };
        if f.try_lock().is_err() || !paths::same_file(&f, &lock) {
            continue;
        }
        match std::fs::remove_dir_all(root.join(format!("{name}-{id}"))) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {}
            _ => {
                let _ = std::fs::remove_file(&lock);
            }
        }
    }
}

fn hex_opt(s: Option<&str>) -> Option<[u8; 3]> {
    s.and_then(theme::hex)
}

fn hexs(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

struct Plan {
    master_size: [u32; 2],
    sizes: Vec<[u32; 2]>,
    fps: Fps,
    size_arg: String,
}

fn plan(
    kind: &str,
    size: Option<String>,
    shot_size: Option<String>,
    derive: &[String],
    fps: Option<Fps>,
    shot_fps: Option<Fps>,
) -> Result<Plan, String> {
    let size_arg = size.or(shot_size).unwrap_or_else(|| "1080p".into());
    let master_size = parse_size(&size_arg, kind)?;
    let mut sizes = vec![master_size];
    for d in derive {
        let s = parse_size(d, kind)?;
        if s[0] > master_size[0] || s[1] > master_size[1] {
            return Err(format!("--derive {d} is larger than --size {size_arg}"));
        }
        if !sizes.contains(&s) {
            sizes.push(s);
        }
    }
    let fps = fps.or(shot_fps).unwrap_or(DEFAULT_FPS);
    Ok(Plan {
        master_size,
        sizes,
        fps,
        size_arg,
    })
}

struct Prep {
    resolved: Resolved,
    terminal: Terminal,
    shot: app::Shot,
    sets: Vec<(String, String)>,
    out_root: PathBuf,
    bin_dir: PathBuf,
    family: String,
    frame: String,
    grid: [u32; 2],
    env: Vec<(String, String)>,
    session_env: Vec<(String, String)>,
    passed: Vec<(String, String)>,
    ns: Option<Ns>,
    fixture_record: Value,
    path: String,
    cwd: PathBuf,
    vars: Vec<(String, PathBuf)>,
    launch: Option<String>,
    settle: String,
    typing: String,
    prompt: String,
    tape_path: PathBuf,
}

impl Prep {
    fn vars(&self) -> Vec<(&str, &Path)> {
        self.vars
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_path()))
            .collect()
    }
}

fn prepare(resolved: Resolved, kind: &str, opts: &Opts, run_name: &str) -> Result<Prep, String> {
    checked_kind(kind)?;
    checked_name("product", &opts.product)?;
    checked_name("shot", &opts.shot)?;
    let sets = sets(opts)?;
    let terminal = resolved.terminal.clone();
    let passed = terminal
        .pass_env
        .iter()
        .map(|name| match std::env::var(name) {
            Ok(v) => Ok((name.clone(), v)),
            Err(std::env::VarError::NotPresent) => Err(format!("pass_env {name} is not set")),
            Err(std::env::VarError::NotUnicode(_)) => {
                Err(format!("pass_env {name} is not valid UTF-8"))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if terminal.home.is_some() && !cfg!(target_os = "linux") {
        return Err("home needs Linux user namespaces".into());
    }
    let shot = terminal
        .shots
        .get(&opts.shot)
        .ok_or_else(|| {
            format!(
                "{} has no shot {}; shots: {}",
                opts.product,
                opts.shot,
                terminal
                    .shots
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?
        .clone();
    if shot.kind != kind {
        return Err(format!(
            "{} is a {} shot; run `prec {} …`",
            opts.shot, shot.kind, shot.kind
        ));
    }
    if let Some(size) = &shot.size {
        parse_size(size, kind).map_err(|e| format!("shot {}: {e}", opts.shot))?;
    }
    if shot.fps.is_some() && kind != "clip" {
        return Err(format!("shot {}: fps is for clips", opts.shot));
    }
    let out_root = paths::inside_tmp(&opts.out)?;
    if opts.bin.is_none() {
        jobs::stage("build", None, "steps");
        jobs::note(&format!("{}@{}", opts.product, resolved.rev));
    }
    let bin_dir = app::build(&resolved, &opts.product, opts.bin.as_deref())?;
    let family = opts
        .font
        .clone()
        .or_else(|| terminal.font.clone())
        .unwrap_or_else(|| DEFAULT_FONT.to_string());
    let frame = opts.frame.clone().unwrap_or_else(|| terminal.frame.clone());
    if frame != "window" && frame != "bare" {
        return Err("--frame window or bare".into());
    }
    if let Some(spec) = set_value(&sets, "inset").or_else(|| terminal.inset.clone()) {
        inset_px(&spec, [1000, 1000])?;
    }
    let grid = match set_value(&sets, "grid") {
        Some(g) => {
            let (c, r) = g.split_once('x').ok_or("--set grid=<cols>x<rows>")?;
            [
                c.parse().map_err(|_| "--set grid=<cols>x<rows>")?,
                r.parse().map_err(|_| "--set grid=<cols>x<rows>")?,
            ]
        }
        None => shot.grid.unwrap_or(terminal.grid),
    };
    crate::mask::Masks::new(&terminal.masks(&shot), grid)?;
    let work = paths::scratch(&["run", run_name])?;
    paths::fresh(&work)?;
    let scratch_dir = work.join("scratch");
    let home = scratch_dir.join("home");
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let tmp = paths::caller_tmp()?;
    let fixtures = match &opts.fixtures {
        Some(p) => Some(paths::absolute(p)?),
        None => None,
    };
    let vars = app::vars(
        &tmp,
        &scratch_dir,
        &resolved.dir,
        &bin_dir,
        &app::target_dir(&resolved, &opts.product)?,
        &fixtures
            .clone()
            .unwrap_or_else(|| scratch_dir.join("no-fixtures")),
    );
    let vref: Vec<(&str, &Path)> = vars
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_path()))
        .collect();
    let mut env: Vec<(String, String)> = vec![
        ("TZ".into(), "UTC".into()),
        ("LANG".into(), "C.UTF-8".into()),
        ("LC_ALL".into(), "C.UTF-8".into()),
        ("TERM".into(), "xterm-256color".into()),
        ("COLORTERM".into(), "truecolor".into()),
        ("HOME".into(), home.display().to_string()),
    ];
    for (k, v) in &terminal.env {
        env.retain(|(ek, _)| ek != k);
        env.push((k.clone(), app::expand(v, &vref)));
    }
    let fixture_record = match &fixtures {
        Some(f) => {
            if let Some(want) = &shot.fixtures {
                let name = f
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if &name != want {
                    return Err(format!(
                        "{} expects the {want} fixture set, not {name}",
                        opts.shot
                    ));
                }
            }
            let door = terminal
                .fixtures_env
                .clone()
                .unwrap_or_else(|| format!("PITO_{}_FIXTURES", opts.product.to_uppercase()));
            env.retain(|(k, _)| k != &door);
            env.push((door.clone(), f.display().to_string()));
            json!({
                "path": f.display().to_string(),
                "door": door,
                "files": paths::hash_tree(f)?.into_iter().map(|(p, h)| json!({"file": p, "sha256": h})).collect::<Vec<_>>(),
            })
        }
        None => {
            if let Some(want) = &shot.fixtures {
                return Err(format!(
                    "{} needs the {want} fixture set: pass --fixtures <materialized set>",
                    opts.shot
                ));
            }
            Value::Null
        }
    };
    let system_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{system_path}", bin_dir.display());
    let mut ns = match (&terminal.home, terminal.hostname()) {
        (Some(h), Some(name)) => Some(Ns::new(h, name, &home, &work.join("stash"))?),
        _ => None,
    };
    let session_home = ns.as_ref().map_or(home.clone(), |n| n.home.clone());
    let mut session_vars = vref.clone();
    session_vars.push(("home", session_home.as_path()));
    let inside_cwd = terminal
        .cwd
        .as_ref()
        .map(|c| PathBuf::from(app::expand(c, &session_vars)))
        .unwrap_or_else(|| scratch_dir.clone());
    let cwd = ns
        .as_ref()
        .map_or(inside_cwd.clone(), |n| n.outside(&inside_cwd));
    std::fs::create_dir_all(&cwd).map_err(|e| format!("{}: {e}", cwd.display()))?;
    let mut session_env = env.clone();
    if let Some(n) = ns.as_mut() {
        n.cwd = n.inside(&cwd);
        let user = n
            .home
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        for (k, v) in [
            ("HOME", n.home.display().to_string()),
            ("USER", user.clone()),
            ("LOGNAME", user),
        ] {
            if k == "HOME" || !terminal.env.contains_key(k) {
                session_env.retain(|(ek, _)| ek != k);
                session_env.push((k.to_string(), v));
            }
        }
        n.probe(&path, &cwd)?;
    }
    let tape_path = resolved.dir.join(&shot.tape);
    let launch = set_value(&sets, "launch")
        .or_else(|| shot.launch.clone())
        .map(|l| app::expand(&l, &session_vars));
    let settle = set_value(&sets, "settle")
        .or_else(|| shot.settle.clone())
        .unwrap_or_else(|| "800ms".into());
    let typing = set_value(&sets, "typing")
        .or_else(|| terminal.typing.clone())
        .unwrap_or_else(|| "40ms".into());
    let prompt = terminal.prompt.clone().unwrap_or_else(|| "$ ".into());
    Ok(Prep {
        resolved,
        terminal,
        shot,
        sets,
        out_root,
        bin_dir,
        family,
        frame,
        grid,
        env,
        session_env,
        passed,
        ns,
        fixture_record,
        path,
        cwd,
        vars,
        launch,
        settle,
        typing,
        prompt,
        tape_path,
    })
}

fn run_name(opts: &Opts, kind: &str, label: Option<&str>) -> String {
    format!(
        "{}-{}-{}{}",
        opts.product,
        kind,
        opts.shot,
        label.map(|l| format!("-{l}")).unwrap_or_default()
    )
}

pub fn record_cast(
    resolved: Resolved,
    kind: &str,
    opts: &Opts,
    to_out: bool,
) -> Result<(PathBuf, Option<Run>), String> {
    let p = prepare(resolved, kind, opts, &run_name(opts, kind, Some("record")))?;
    record_prepared(&p, kind, opts, to_out).map_err(|e| private::redact(&e, &p.passed))
}

fn with_passed(env: &[(String, String)], passed: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = env.to_vec();
    out.extend(passed.iter().cloned());
    out
}

fn record_prepared(
    p: &Prep,
    kind: &str,
    opts: &Opts,
    to_out: bool,
) -> Result<(PathBuf, Option<Run>), String> {
    let steps = tape::load(&p.tape_path)?;
    let tape_text = std::fs::read_to_string(&p.tape_path).map_err(|e| e.to_string())?;
    let settle = tape::duration(&p.settle)?;
    let typing = tape::duration(&p.typing)?;
    let vars = p.vars();
    let hook = |cmd: &str| Hook {
        cmd: app::expand(cmd, &vars),
        env: with_passed(&p.env, &p.passed),
        passed: p.passed.clone(),
        path: p.path.clone(),
        cwd: p.cwd.clone(),
    };
    *TEARDOWN.lock().unwrap_or_else(|e| e.into_inner()) = p.terminal.teardown.as_deref().map(hook);
    let recorded = (|| -> Result<Vec<record::Event>, String> {
        if let Some(setup) = &p.terminal.setup {
            hook(setup).run(true)?;
        }
        eprintln!(
            "recording {} {} in a {}x{} pty…",
            opts.product, opts.shot, p.grid[0], p.grid[1]
        );
        jobs::stage("record", None, "seconds");
        paths::settle();
        record::record(&record::Spec {
            grid: p.grid,
            env: &with_passed(&p.session_env, &p.passed),
            path: &p.path,
            cwd: &p.cwd,
            ns: p.ns.as_ref(),
            prompt: &p.prompt,
            launch: p.launch.as_deref(),
            settle,
            typing,
            steps: &steps,
            still: kind == "still",
        })
    })();
    tear_down(|| {});
    let events = recorded?;
    let shown = Timeline::of(&events).shown();
    let closure = match closure_from_sets(&p.sets)? {
        Some(c) => c,
        None => p.shot.closure()?,
    };
    let meta = cast_meta(p, opts, kind, shown, &closure, &tape_text);
    let cast = Cast {
        cols: p.grid[0],
        rows: p.grid[1],
        meta,
        events,
    };
    let (dir, kept) = if to_out {
        (p.out_root.join(&opts.product).join(kind), None)
    } else {
        let run = run_folder(&run_name(opts, kind, Some("cast")))?;
        (run.dir.clone(), Some(run))
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}-{kind}-{}.cast", opts.product, opts.shot));
    record::write(&path, &cast)?;
    eprintln!(
        "recorded {:.2}s shown, {} events → {}",
        shown,
        cast.events.len(),
        paths::shown(&path)
    );
    if !opts.allow_private {
        let found = private::scan(&cast, &private::Needles::of_this_machine(&p.passed));
        if !to_out && let Err(refused) = private::refuse(&path, &found) {
            if let Some(run) = kept {
                run.remove();
            }
            return Err(refused);
        }
        for line in private::lines(&path, &found) {
            eprintln!("prec: {line}");
        }
    }
    Ok((path, kept))
}

fn cast_meta(
    p: &Prep,
    opts: &Opts,
    kind: &str,
    shown: f64,
    closure: &app::Closure,
    tape_text: &str,
) -> Value {
    let ground = set_value(&p.sets, "ground").or_else(|| p.terminal.ground.clone());
    let inset = set_value(&p.sets, "inset").or_else(|| p.terminal.inset.clone());
    json!({
        "tool": {"name": TOOL, "version": env!("CARGO_PKG_VERSION")},
        "product": opts.product,
        "shot": opts.shot,
        "kind": kind,
        "grid": [p.grid[0], p.grid[1]],
        "app_theme": p.terminal.theme,
        "frame": p.frame,
        "ground": ground,
        "inset": inset,
        "font": p.family,
        "closure": closure.to_json(),
        "size": p.shot.size,
        "fps": p.shot.fps,
        "poster": p.shot.poster,
        "typing": p.typing,
        "settle": p.settle,
        "launch": p.launch,
        "prompt": p.prompt,
        "shown_seconds": (shown * 1e6).round() / 1e6,
        "app": {
            "rev": p.resolved.rev,
            "tag": p.resolved.tag,
            "pinned": p.resolved.pinned && opts.bin.is_none(),
            "commit": p.resolved.commit,
            "bin_dir": p.bin_dir.display().to_string(),
            "bin_sha256": paths::hash_file(&p.bin_dir.join(&p.terminal.bin)).ok(),
            "terminal_toml_sha256": p.resolved.source_sha256,
        },
        "shot_sha256": paths::hash_bytes(serde_json::to_string(&p.shot).unwrap_or_default().as_bytes()),
        "tape_sha256": paths::hash_bytes(tape_text.as_bytes()),
        "fixtures": p.fixture_record,
        "env": p.session_env.iter().filter(|(k, _)| k != "HOME").map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
        "pass_env": p.passed.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        "home": p.ns.as_ref().map(|n| n.home.display().to_string()),
        "hostname": p.ns.as_ref().map(|n| n.hostname.as_str()),
        "mask": p.terminal.masks(&p.shot),
    })
}

fn frames_for(shown: f64, fps: Fps, closure: &app::Closure) -> usize {
    if *closure == app::Closure::Palindrome {
        return fps.frames(shown).round().max(3.0) as usize;
    }
    replay::frame_count(shown, fps, closure.fade_frames(fps))
}

fn cast_fps(meta: &Value) -> Result<Option<Fps>, String> {
    match &meta["fps"] {
        Value::Null => Ok(None),
        v => Fps::deserialize(v)
            .map(Some)
            .map_err(|e| format!("cast: {e}")),
    }
}

fn closure_of_cast(meta: &Value) -> Result<app::Closure, String> {
    app::Closure::from_value(&meta["closure"])
}

fn resolve_poster(flag: Option<&str>, shot: &Value) -> Result<app::Poster, String> {
    if let Some(flag) = flag {
        return app::Poster::from_flag(flag);
    }
    if shot.is_null() {
        return Ok(app::Poster::First);
    }
    app::Poster::from_value(shot)
}

pub fn render_cast(cast_path: &Path, opts: &Opts) -> Result<PathBuf, String> {
    let started = Instant::now();
    let cast_path = paths::absolute(cast_path)?;
    let cast = record::read(&cast_path)?;
    let cast_sha = paths::hash_file(&cast_path)?;
    let meta = cast.meta.clone();
    let product = checked_name(
        "the cast's product",
        meta["product"]
            .as_str()
            .ok_or("the cast names no product")?,
    )?
    .to_string();
    let shot =
        checked_name("the cast's shot", meta["shot"].as_str().unwrap_or("shot"))?.to_string();
    let kind = checked_kind(meta["kind"].as_str().unwrap_or("clip"))?.to_string();
    validate(&kind, opts)?;
    if !opts.allow_private {
        private::refuse(
            &cast_path,
            &private::scan(&cast, &private::Needles::of_this_machine(&[])),
        )?;
    }
    let sets = sets(opts)?;
    let out_root = paths::inside_tmp(&opts.out)?;
    let app_theme = if meta["app_theme"].is_null() {
        None
    } else {
        Some(&meta["app_theme"])
    };
    let choice: Choice = theme::resolve(&opts.theme, app_theme, &product)?;
    jobs::context(choice.label.clone());
    let theme_json = choice.json.clone();
    let family = opts
        .font
        .clone()
        .or_else(|| meta["font"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| DEFAULT_FONT.to_string());
    let metrics = font::metrics(&family)?;
    let frame = opts
        .frame
        .clone()
        .or_else(|| meta["frame"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "window".into());
    if frame != "window" && frame != "bare" {
        return Err("--frame window or bare".into());
    }
    let ground =
        hex_opt(set_value(&sets, "ground").as_deref()).or_else(|| hex_opt(meta["ground"].as_str()));
    let pl = plan(
        &kind,
        opts.size.clone(),
        meta["size"].as_str().map(String::from),
        &opts.derive,
        opts.fps,
        cast_fps(&meta)?,
    )?;
    let inset_spec = set_value(&sets, "inset").or_else(|| meta["inset"].as_str().map(String::from));
    let inset = match &inset_spec {
        Some(spec) => inset_px(spec, pl.master_size)?,
        None => [0; 4],
    };
    compose::Layout::new(pl.master_size[0], pl.master_size[1], frame == "window").boxed(inset)?;
    let label = choice.label.clone();
    let name = format!(
        "{shot}{}",
        label.as_ref().map(|l| format!("-{l}")).unwrap_or_default()
    );
    let look = Look {
        theme: theme_json,
        font: family.clone(),
        window: frame == "window",
        ground,
        inset,
        masks: crate::mask::Masks::of_meta(&meta, [cast.cols, cast.rows])?,
    };
    let timeline = Timeline::of(&cast.events);
    let shown = timeline.shown();
    let closure = match closure_from_sets(&sets)? {
        Some(c) => c,
        None => closure_of_cast(&meta)?,
    };
    let dir = out_root.join(&product).join(&kind);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem_of = |s: [u32; 2], fps: Option<Fps>| match fps {
        Some(f) => format!("{product}-{kind}-{name}-{}x{}-{}fps", s[0], s[1], f.label()),
        None => format!("{product}-{kind}-{name}-{}x{}", s[0], s[1]),
    };
    let web = web_targets(&opts.web, &stem_of(pl.master_size, None));
    let mut poster_choice = None;
    let mut poster_slot = None;
    let clip = if kind == "clip" {
        let n = frames_for(shown, pl.fps, &closure);
        let (order, fade) = compose::closure(&(0..n).collect::<Vec<_>>(), &closure, pl.fps)?;
        let choice = resolve_poster(opts.poster.as_deref(), &meta["poster"])?;
        let (src, slot) = compose::poster_frame(n, &closure, pl.fps, &choice)?;
        poster_choice = Some(choice);
        Some((n, order, fade, src, slot))
    } else {
        if opts.poster.is_some() {
            return Err("--poster is for clip and render".into());
        }
        check_web(&web, pl.master_size[0])?;
        None
    };
    let run = run_folder(&format!("{product}-{kind}-{name}-replay"))?;
    let work = &run.dir;
    let stage = work.join("out");
    std::fs::create_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    let mut files: Vec<Value> = vec![];
    let mut encodes: Vec<Value> = vec![];
    let frames_record;
    let manifest_stem;
    let rendered;
    let render_started = Instant::now();
    paths::settle();
    match clip {
        None => {
            let at = timeline.shots.last().copied().unwrap_or(timeline.end);
            eprintln!(
                "replaying {product} {shot} to {:.2}s at {}x{}…",
                at, pl.master_size[0], pl.master_size[1]
            );
            jobs::stage("replay", None, "frames");
            rendered = replay::render(
                &cast,
                pl.master_size,
                Take::Still(at),
                &look,
                (metrics.advance, metrics.line),
                work,
            )?;
            let still = rendered.frames.first().ok_or("the replay took no still")?;
            let img = image::open(still)
                .map_err(|e| format!("{}: {e}", still.display()))?
                .to_rgb8();
            manifest_stem = stem_of(pl.master_size, None);
            let master_path = stage.join(format!("{manifest_stem}.master.png"));
            encode::write_png16(&master_path, pl.master_size, img.as_raw())?;
            files.push(encode::file_entry(
                &stage,
                &master_path,
                "master: 16-bit sRGB PNG, lossless",
                json!({"role": "master"}),
            )?);
            let derived = pl.sizes.len() + web.len();
            jobs::stage("derive", Some(derived), "outputs");
            let linear = OnceCell::new();
            for (i, s) in pl.sizes.iter().enumerate() {
                let p = stage.join(format!("{}.png", stem_of(*s, None)));
                derive_png(&p, &img, &linear, *s)?;
                files.push(encode::file_entry(
                    &stage,
                    &p,
                    "delivery: 8-bit sRGB PNG with an sRGB chunk",
                    json!({"size": [s[0], s[1]]}),
                )?);
                jobs::progress(i + 1);
            }
            let (w, h) = img.dimensions();
            for t in &web {
                let height = ((h as f64 * t.width as f64 / w as f64).round() as u32).max(1);
                let p = stage.join(format!("{}.png", t.name));
                derive_png(&p, &img, &linear, [t.width, height])?;
                files.push(encode::file_entry(
                    &stage,
                    &p,
                    "web copy: 8-bit sRGB PNG with an sRGB chunk",
                    json!({"role": "web", "size": [t.width, height], "web": t.px}),
                )?);
            }
            jobs::progress(derived);
            frames_record = json!({"still_at": (at * 1e6).round() / 1e6, "exact": true});
        }
        Some((n, order, fade, src, slot)) => {
            poster_slot = Some(slot);
            eprintln!(
                "replaying {product} {shot}: {n} frames at {}x{}@{}…",
                pl.master_size[0], pl.master_size[1], pl.fps
            );
            jobs::stage("replay", Some(n), "frames");
            rendered = replay::render(
                &cast,
                pl.master_size,
                Take::Clip {
                    frames: n,
                    fps: pl.fps,
                },
                &look,
                (metrics.advance, metrics.line),
                work,
            )?;
            let paths_in: Vec<&Path> = order
                .iter()
                .map(|&i| rendered.frames[i].as_path())
                .collect();
            manifest_stem = stem_of(pl.master_size, Some(pl.fps));
            jobs::stage("master", Some(paths_in.len()), "frames");
            let count = if opts.no_encode {
                paths_in.len()
            } else {
                let output = stage.join(format!("{manifest_stem}.master.mp4"));
                let (count, used) =
                    compose::master(&paths_in, fade, pl.master_size, pl.fps, &output)?;
                let (size, frames, rate) = encode::probe(&output)?;
                if size != pl.master_size || frames != count || rate != Some(pl.fps) {
                    return Err(format!(
                        "{}: the file holds {frames} frames at {}x{} and {}, not the {count} at {}x{} and {} fps prec encoded",
                        output.display(),
                        size[0],
                        size[1],
                        rate.map_or("no rate".into(), |r| format!("{r} fps")),
                        pl.master_size[0],
                        pl.master_size[1],
                        pl.fps
                    ));
                }
                let poster = stage.join(format!("{manifest_stem}.poster.png"));
                let png = rendered
                    .frames
                    .get(src)
                    .ok_or_else(|| format!("poster: frame {src} was not rendered"))?;
                encode::poster(png, &poster)?;
                let poster_what = match &poster_choice {
                    Some(app::Poster::First) => {
                        "poster: 8-bit sRGB PNG with an sRGB chunk, the first frame".to_string()
                    }
                    _ => format!("poster: 8-bit sRGB PNG with an sRGB chunk, master frame {slot}"),
                };
                files.push(encode::file_entry(
                    &stage,
                    &output,
                    &encode::master_what(),
                    json!({"role": "master", "fps": pl.fps, "size": pl.master_size}),
                )?);
                files.push(encode::file_entry(
                    &stage,
                    &poster,
                    &poster_what,
                    json!({"role": "poster", "fps": pl.fps, "size": pl.master_size, "frame": slot}),
                )?);
                encodes.push(
                    json!({"file": output.file_name().unwrap_or_default().to_string_lossy(),
                    "role": "master", "device": "CPU", "fps": pl.fps,
                    "size": pl.master_size, "args": used}),
                );
                count
            };
            jobs::progress(count);
            frames_record = json!({
                "master": if opts.no_encode { Value::Null } else { json!(count) },
                "seconds": pl.fps.seconds(count),
                "exact": true,
                "shown_seconds": (shown * 1e6).round() / 1e6,
                "rendered": n,
                "captured": rendered.captured,
                "reused": n - rendered.captured,
                "late": 0,
                "dropped": 0,
                "closure": closure.to_json(),
                "render_seconds": (render_started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
            });
        }
    }
    let staged_cast = stage.join(format!("{manifest_stem}.cast"));
    std::fs::copy(&cast_path, &staged_cast)
        .map_err(|e| format!("{}: {e}", staged_cast.display()))?;
    files.push(encode::file_entry(
        &stage,
        &staged_cast,
        "cast: asciicast v2, the recorded session every size, fps and theme renders from",
        json!({"role": "cast"}),
    )?);
    let version = archive::version(
        meta["app"]["rev"].as_str().unwrap_or("local"),
        meta["app"]["commit"].as_str(),
        meta["app"]["tag"].as_str(),
    );
    let poster_out = match (&poster_choice, poster_slot) {
        (Some(p), Some(slot)) if !matches!(p, app::Poster::First) => {
            format!("8-bit sRGB PNG with an sRGB chunk, master frame {slot}")
        }
        _ => "8-bit sRGB PNG with an sRGB chunk, the first frame".to_string(),
    };
    let mut manifest = json!({
        "made": opts.made,
        "caller": archive::caller(std::env::var("PITO_INVOKER").ok().as_deref()),
        "tool": {"name": TOOL, "version": env!("CARGO_PKG_VERSION")},
        "product": product,
        "kind": kind,
        "version": version,
        "source": {"path": cast_path.display().to_string(), "sha256": cast_sha, "commit": meta["app"]["commit"]},
        "params": {
            "size": [pl.master_size[0], pl.master_size[1]],
            "size_arg": pl.size_arg,
            "derive": pl.sizes.iter().skip(1).map(|s| [s[0], s[1]]).collect::<Vec<_>>(),
            "fps": if kind == "clip" { json!([pl.fps]) } else { Value::Null },
            "seed": opts.seed,
            "set": opts.set,
            "frame": frame,
            "inset": inset_spec,
            "theme": opts.theme,
            "font": family,
            "youtube": opts.youtube,
            "encode": !opts.no_encode,
            "web": opts.web,
            "poster": match &poster_choice {
                Some(p) => p.to_json(),
                None => Value::Null,
            },
        },
        "device": "CPU",
        "colour": {
            "master": if kind == "clip" { encode::COLOR_709 } else { "sRGB, 16-bit RGB" },
            "delivery": if kind == "clip" { encode::COLOR_709 } else { "sRGB, 8-bit, with an sRGB chunk" },
            "terminal": "24-bit truecolor (TERM=xterm-256color, COLORTERM=truecolor), painted by xterm.js with --force-color-profile=srgb",
        },
        "outputs": {
            "delivery": if kind == "clip" { "pcod makes delivery formats from the H.264 master" } else { "8-bit sRGB PNG with an sRGB chunk" },
            "master": if kind == "clip" { if opts.no_encode { "none: frames only" } else { "H.264 High by pito-codecs, CRF 14, 8-bit 4:2:0 BT.709 limited range in MP4" } } else { "16-bit sRGB PNG, lossless" },
            "poster": poster_out,
            "resampling": format!("{} (the master is drawn at its own size, never scaled)", encode::LANCZOS),
        },
        "seconds": (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        "files": files,
        "app": meta["app"],
        "shot": shot,
        "shot_sha256": meta["shot_sha256"],
        "tape_sha256": meta["tape_sha256"],
        "cast_sha256": cast_sha,
        "fixtures": meta["fixtures"],
        "pass_env": meta["pass_env"].as_array().cloned().unwrap_or_default(),
        "private_allowed": opts.allow_private,
        "theme": {"name": choice.name, "label": label, "background": hexs(rendered.bg), "ground": hexs(rendered.ground)},
        "font": {"family": family, "file": metrics.file.display().to_string(), "size_px": rendered.font_px, "scale": 1},
        "grid": {"cols": cast.cols, "rows": cast.rows, "cell": rendered.cell, "canvas": [rendered.canvas[0], rendered.canvas[1]]},
        "xterm": {"version": replay::XTERM, "renderer": format!("canvas {}", replay::XTERM_CANVAS), "unicode": format!("11 ({})", replay::XTERM_UNICODE), "browser": format!("{} headless, --disable-gpu, in build.slice under nice", rendered.chromium)},
        "frames": frames_record,
        "encodes": encodes,
    });
    let manifest_path = dir.join(format!("{manifest_stem}.json"));
    let skip = archive::skip_reason(
        &archive::Run {
            no_archive: opts.no_archive,
            no_encode: opts.no_encode,
            product: &product,
            has_master: manifest["files"]
                .as_array()
                .is_some_and(|files| files.iter().any(|f| f["role"] == "master")),
        },
        &manifest["app"],
    );
    paths::settle();
    let fps = (kind == "clip").then_some(pl.fps);
    let mut stems: Vec<String> = pl.sizes.iter().map(|s| stem_of(*s, fps)).collect();
    stems.sort();
    stems.dedup();
    let _published = stems
        .iter()
        .map(|s| paths::hold(s, &format!("publishing {s}")))
        .collect::<Result<Vec<_>, _>>()?;
    deliver(&stage, &dir, &manifest["files"], &manifest_path)?;
    archive::finish(&manifest_path, &mut manifest, skip)?;
    let _ = std::fs::remove_dir(&stage);
    if !opts.keep_frames && !opts.no_encode {
        let _ = std::fs::remove_dir_all(work.join("frames"));
        let _ = std::fs::remove_dir_all(work.join("chromium"));
    }
    Ok(manifest_path)
}

fn derive_png(
    path: &Path,
    img: &image::RgbImage,
    linear: &OnceCell<encode::Linear>,
    size: [u32; 2],
) -> Result<(), String> {
    if size == [img.width(), img.height()] {
        return encode::write_png8(path, size, img.as_raw());
    }
    let rgb = encode::resize8(linear.get_or_init(|| encode::linear(img)), size);
    encode::write_png8(path, size, &rgb)
}

fn deliver(stage: &Path, dir: &Path, files: &Value, manifest: &Path) -> Result<(), String> {
    match std::fs::remove_file(manifest) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("{}: {e}", manifest.display()));
        }
        _ => {}
    }
    for f in files.as_array().into_iter().flatten() {
        let name = f["file"].as_str().ok_or("an output without a name")?;
        move_into(&stage.join(name), &dir.join(name))?;
    }
    Ok(())
}

fn move_into(from: &Path, to: &Path) -> Result<(), String> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            encode::atomic(to, |temp| {
                std::fs::copy(from, temp)
                    .map(|_| ())
                    .map_err(|e| format!("{}: {e}", temp.display()))
            })?;
            let _ = std::fs::remove_file(from);
            Ok(())
        }
        Err(e) => Err(format!("{}: {e}", to.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passed_value_never_reaches_the_written_cast() {
        let terminal: Terminal = toml::from_str(
            "app = \"demo\"\nbin = \"demo\"\ngrid = [20, 4]\npass_env = [\"TOKEN\"]\n[env]\nA = \"b\"\n[shots.s]\nkind = \"clip\"\ntape = \"s.tape\"\n",
        )
        .unwrap();
        let secret = "s3cret-token-value";
        let env = vec![("A".to_string(), "b".to_string())];
        let p = Prep {
            resolved: Resolved {
                dir: PathBuf::from("/a"),
                commit: None,
                rev: "local".into(),
                tag: None,
                pinned: false,
                terminal: terminal.clone(),
                source_sha256: String::new(),
                gitdir: None,
                key: None,
                clean: false,
            },
            shot: terminal.shots["s"].clone(),
            terminal,
            sets: vec![],
            out_root: PathBuf::from("/o"),
            bin_dir: PathBuf::from("/b"),
            family: DEFAULT_FONT.into(),
            frame: "bare".into(),
            grid: [20, 4],
            env: env.clone(),
            session_env: env,
            passed: vec![("TOKEN".into(), secret.into())],
            ns: None,
            fixture_record: Value::Null,
            path: "/b".into(),
            cwd: PathBuf::from("/c"),
            vars: vec![],
            launch: None,
            settle: "1ms".into(),
            typing: "1ms".into(),
            prompt: "$ ".into(),
            tape_path: PathBuf::from("/t"),
        };
        let meta = cast_meta(&p, &Opts::default(), "clip", 1.0, &app::Closure::Open, "");
        assert_eq!(meta["pass_env"], json!(["TOKEN"]));
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/unit-render-pass");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.cast");
        let cast = Cast {
            cols: 20,
            rows: 4,
            meta,
            events: vec![],
        };
        record::write(&path, &cast).unwrap();
        let bytes = std::fs::read_to_string(&path).unwrap();
        assert!(bytes.contains("\"TOKEN\""), "{bytes}");
        assert!(!bytes.contains(secret), "{bytes}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_job_declares_the_stages_its_run_goes_through() {
        assert_eq!(
            stages("clip", true, true, true, true),
            ["build", "record", "replay", "master", "archive"]
        );
        assert_eq!(
            stages("still", false, true, true, false),
            ["record", "replay", "derive"]
        );
        assert_eq!(stages("clip", true, true, false, true), ["build", "record"]);
        assert_eq!(
            stages("clip", false, false, true, true),
            ["replay", "master", "archive"]
        );
    }

    #[test]
    fn a_run_clears_earlier_folders_of_its_name_that_nobody_holds() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/test-render-runs");
        let _ = std::fs::remove_dir_all(&root);
        for dir in [
            "s-replay-11",
            "s-replay-22",
            "s-replay-33",
            "s-replay-44-replay",
            "s-replay",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let held = File::create(root.join("s-replay-11.lock")).unwrap();
        held.lock().unwrap();
        File::create(root.join("s-replay-22.lock")).unwrap();
        clear_runs(&root, "s-replay", "33");
        let mut left: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "s-replay",
                "s-replay-11",
                "s-replay-11.lock",
                "s-replay-33",
                "s-replay-44-replay"
            ]
        );
        assert_eq!(run_id("s-replay-7.lock", "s-replay"), Some("7"));
        assert_eq!(run_id("s-replay-7-x", "s-replay"), None);
        assert_eq!(run_id("s-replay-", "s-replay"), None);
        drop(held);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sizes_name_a_preset_a_square_or_wxh_and_clips_keep_them_even() {
        assert_eq!(parse_size("720p", "clip").unwrap(), [1280, 720]);
        assert_eq!(parse_size("2160p", "still").unwrap(), [3840, 2160]);
        assert_eq!(parse_size("1000x600", "clip").unwrap(), [1000, 600]);
        assert_eq!(parse_size("512", "still").unwrap(), [512, 512]);
        assert_eq!(parse_size("512", "clip").unwrap(), [512, 512]);
        assert_eq!(parse_size("1001x601", "still").unwrap(), [1001, 601]);
        assert!(parse_size("1001x600", "clip").is_err());
        assert!(parse_size("511", "clip").is_err());
        assert!(parse_size("0x600", "still").is_err());
        assert!(parse_size("huge", "still").is_err());
    }

    #[test]
    fn a_cast_names_its_product_shot_and_kind_in_safe_words() {
        assert!(checked_name("shot", "tui-tour_2.v1").is_ok());
        for bad in ["", ".", "..", "a/b", "../x", "a b", "tour#1"] {
            assert!(checked_name("shot", bad).is_err(), "{bad:?}");
        }
        assert!(checked_kind("clip").is_ok());
        assert!(checked_kind("still").is_ok());
        assert!(checked_kind("../clip").is_err());
    }

    #[test]
    fn a_web_copy_fits_inside_the_master() {
        assert!(check_web(&web_targets(&[320], "s"), 640).is_ok());
        assert!(check_web(&web_targets(&[321], "s"), 640).is_err());
        assert!(check_web(&web_targets(&[0], "s"), 640).is_err());
        let names: Vec<String> = web_targets(&[200], "s")
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["s-web-200", "s-web-200@2x"]);
    }

    #[test]
    fn a_recipe_size_and_fps_fill_in_where_no_flag_is_given() {
        let p = plan(
            "clip",
            None,
            Some("720p".into()),
            &[],
            None,
            Some(Fps::new(30, 1)),
        )
        .unwrap();
        assert_eq!((p.master_size, p.fps), ([1280, 720], Fps::new(30, 1)));
        let p = plan(
            "clip",
            Some("640x360".into()),
            Some("720p".into()),
            &[],
            Some(Fps::new(60, 1)),
            Some(Fps::new(30, 1)),
        )
        .unwrap();
        assert_eq!((p.master_size, p.fps), ([640, 360], Fps::new(60, 1)));
        assert_eq!(
            plan("clip", None, None, &[], None, None).unwrap().fps,
            DEFAULT_FPS
        );
    }

    #[test]
    fn a_clip_renders_its_shown_time_and_a_palindrome_at_least_three_frames() {
        assert_eq!(
            frames_for(1.0, Fps::new(120, 1), &app::Closure::Palindrome),
            120
        );
        assert_eq!(
            frames_for(0.0, Fps::new(30, 1), &app::Closure::Palindrome),
            3
        );
        assert_eq!(
            frames_for(10.0, Fps::new(120, 1), &app::Closure::Open),
            1200
        );
        assert_eq!(
            frames_for(1.0, Fps::new(30, 1), &app::Closure::Crossfade(0.1)),
            30
        );
        assert_eq!(DEFAULT_FPS, Fps::new(120, 1));
        assert!(
            sets(&Opts {
                set: vec!["camera.zoom=2".into()],
                ..Default::default()
            })
            .is_err()
        );
    }

    fn opts() -> Opts {
        Opts::default()
    }

    #[test]
    fn a_bad_size_or_a_clip_only_flag_is_refused_before_anything_records() {
        assert!(validate("clip", &opts()).is_ok());
        assert!(validate("still", &opts()).is_ok());
        for (kind, o, said) in [
            (
                "still",
                Opts {
                    poster: Some("first".into()),
                    ..opts()
                },
                "--poster is for clip and render",
            ),
            (
                "clip",
                Opts {
                    poster: Some("middle".into()),
                    ..opts()
                },
                "--poster middle: first, before-closure, or seconds of shown time",
            ),
            (
                "clip",
                Opts {
                    profile: vec!["web".into()],
                    ..opts()
                },
                "--profile for clips belongs to pcod",
            ),
            (
                "clip",
                Opts {
                    youtube: true,
                    ..opts()
                },
                "--youtube for clips belongs to pcod",
            ),
            (
                "clip",
                Opts {
                    derive: vec!["320x180".into()],
                    ..opts()
                },
                "--derive for clips belongs to pcod",
            ),
            (
                "clip",
                Opts {
                    web: vec![320],
                    ..opts()
                },
                "--web makes still web copies; use pcod for clips",
            ),
            (
                "still",
                Opts {
                    size: Some("0".into()),
                    ..opts()
                },
                "size 0: a width and height above 0",
            ),
            (
                "clip",
                Opts {
                    size: Some("321x180".into()),
                    ..opts()
                },
                "size 321x180: even width and height, for 4:2:0 video",
            ),
            (
                "still",
                Opts {
                    size: Some("wide".into()),
                    ..opts()
                },
                "size wide: N, WxH, 720p, 1080p, 1440p or 2160p",
            ),
            (
                "still",
                Opts {
                    size: Some("640x360".into()),
                    derive: vec!["320x0".into()],
                    ..opts()
                },
                "size 320x0: a width and height above 0",
            ),
        ] {
            assert_eq!(validate(kind, &o).unwrap_err(), said, "{kind}");
        }
        let fine = Opts {
            size: Some("321x181".into()),
            derive: vec!["160x90".into()],
            web: vec![100],
            poster: None,
            ..opts()
        };
        assert!(validate("still", &fine).is_ok());
        let clip = Opts {
            size: Some("320x180".into()),
            poster: Some("1.5".into()),
            ..opts()
        };
        assert!(validate("clip", &clip).is_ok());
    }

    #[test]
    fn a_size_is_a_preset_a_square_or_wxh_with_digits_only() {
        for (s, want) in [
            ("1080p", [1920, 1080]),
            ("1440p", [2560, 1440]),
            ("4k", [3840, 2160]),
            ("4K", [3840, 2160]),
            ("2x2", [2, 2]),
            ("1x1", [1, 1]),
        ] {
            assert_eq!(parse_size(s, "still"), Ok(want), "{s}");
        }
        for s in [
            "",
            "x",
            "1x",
            "x1",
            "-2x2",
            "2x-2",
            "1.5x2",
            "2x2x2",
            "720P",
            " 720p",
            "4294967296x2",
        ] {
            assert_eq!(
                parse_size(s, "still").unwrap_err(),
                format!("size {s}: N, WxH, 720p, 1080p, 1440p or 2160p"),
                "{s:?}"
            );
        }
        assert!(parse_size("1x1", "clip").is_err());
        assert!(parse_size("2x0", "clip").unwrap_err().contains("above 0"));
        assert_eq!(parse_size("720p", "clip"), Ok([1280, 720]));
    }

    #[test]
    fn a_size_past_the_encoder_ceiling_is_refused_and_the_edge_is_taken() {
        for kind in ["still", "clip"] {
            assert_eq!(parse_size("16384", kind), Ok([16384, 16384]), "{kind}");
            assert_eq!(parse_size("16384x2", kind), Ok([16384, 2]), "{kind}");
            assert_eq!(parse_size("2x16384", kind), Ok([2, 16384]), "{kind}");
            for s in ["16386x2", "2x16386", "16386", "60000x60000", "4294967294x2"] {
                assert_eq!(
                    parse_size(s, kind),
                    Err(format!("size {s}: at most 16384 on a side")),
                    "{kind} {s}"
                );
            }
        }
        assert_eq!(
            parse_size("16385x2", "still"),
            Err("size 16385x2: at most 16384 on a side".into())
        );
        assert!(parse_size("16385x2", "clip").is_err());
        let big = Opts {
            size: Some("320x180".into()),
            derive: vec!["16386x2".into()],
            ..opts()
        };
        assert_eq!(
            validate("still", &big),
            Err("size 16386x2: at most 16384 on a side".into())
        );
    }

    #[test]
    fn an_inset_takes_one_to_four_sides_in_percent_or_pixels() {
        let size = [1000, 500];
        assert_eq!(inset_px("10", size), Ok([10; 4]));
        assert_eq!(inset_px("10px", size), Ok([10; 4]));
        assert_eq!(inset_px("10%", size), Ok([50, 100, 50, 100]));
        assert_eq!(inset_px("10px, 5%", size), Ok([10, 50, 10, 50]));
        assert_eq!(inset_px("1,2,3", size), Ok([1, 2, 3, 2]));
        assert_eq!(inset_px("1,2,3,4", size), Ok([1, 2, 3, 4]));
        assert_eq!(inset_px("0.4,0.6,99.9%,0", size), Ok([0, 1, 500, 0]));
        let bad = |spec: &str| format!("inset {spec}: 1 to 4 of N% or Npx, top,right,bottom,left");
        for spec in [
            "",
            "1,2,3,4,5",
            "100%",
            "-1",
            "-1%",
            "a",
            "NaN",
            "inf",
            "1em",
            "%",
            "px",
            "1,,2",
        ] {
            assert_eq!(inset_px(spec, size).unwrap_err(), bad(spec), "{spec:?}");
        }
    }

    #[test]
    fn a_set_is_a_known_key_and_a_value_and_the_last_one_wins() {
        let with = |set: &[&str]| {
            sets(&Opts {
                set: set.iter().map(|s| s.to_string()).collect(),
                ..opts()
            })
        };
        assert_eq!(
            with(&[" grid = 10x4 ", "inset=5%"]).unwrap(),
            vec![
                ("grid".to_string(), "10x4".to_string()),
                ("inset".to_string(), "5%".to_string())
            ]
        );
        assert_eq!(with(&["grid"]).unwrap_err(), "--set grid: key=value");
        assert_eq!(
            with(&["zoom=2"]).unwrap_err(),
            format!("--set zoom: known keys are {}", SET_KEYS.join(", "))
        );
        for k in ["camera", "camera.x", "keys", "key"] {
            assert_eq!(
                with(&[&format!("{k}=1")]).unwrap_err(),
                format!("--set {k}: {}", app::NO_CAMERA)
            );
        }
        let s = with(&[
            "ground=#000000",
            "closure=palindrome",
            "ground=#ffffff",
            "closure.crossfade=0.5",
        ])
        .unwrap();
        assert_eq!(set_value(&s, "ground").as_deref(), Some("#ffffff"));
        assert_eq!(set_value(&s, "inset"), None);
        assert_eq!(
            closure_from_sets(&s).unwrap(),
            Some(app::Closure::Crossfade(0.5))
        );
        assert_eq!(closure_from_sets(&[]).unwrap(), None);
        let none = with(&["closure.crossfade=1", "closure=none"]).unwrap();
        assert_eq!(closure_from_sets(&none).unwrap(), Some(app::Closure::Open));
        assert!(closure_from_sets(&with(&["closure=loop"]).unwrap()).is_err());
        assert!(closure_from_sets(&with(&["closure.crossfade=0"]).unwrap()).is_err());
    }

    #[test]
    fn a_derived_size_fits_inside_the_master_and_is_listed_once() {
        let p = plan(
            "still",
            Some("640x360".into()),
            None,
            &[
                "320x180".into(),
                "640x360".into(),
                "320x180".into(),
                "160".into(),
            ],
            None,
            None,
        )
        .unwrap();
        assert_eq!(p.sizes, vec![[640, 360], [320, 180], [160, 160]]);
        assert_eq!(p.size_arg, "640x360");
        assert_eq!(
            plan(
                "still",
                Some("640x360".into()),
                None,
                &["641x10".into()],
                None,
                None
            )
            .err()
            .unwrap(),
            "--derive 641x10 is larger than --size 640x360"
        );
        assert!(
            plan(
                "still",
                Some("640x360".into()),
                None,
                &["10x361".into()],
                None,
                None
            )
            .is_err()
        );
        assert!(
            plan(
                "still",
                Some("640x360".into()),
                None,
                &["x".into()],
                None,
                None
            )
            .is_err()
        );
        let d = plan("still", None, None, &[], None, None).unwrap();
        assert_eq!(
            (d.master_size, d.size_arg.as_str()),
            ([1920, 1080], "1080p")
        );
        assert!(plan("clip", None, Some("321x180".into()), &[], None, None).is_err());
    }

    #[test]
    fn a_cast_names_its_rate_its_closure_and_its_poster_or_is_refused() {
        assert_eq!(cast_fps(&json!({})).unwrap(), None);
        assert_eq!(cast_fps(&json!({"fps": null})).unwrap(), None);
        assert_eq!(
            cast_fps(&json!({"fps": "24000/1001"})).unwrap(),
            Some(Fps::new(24000, 1001))
        );
        assert_eq!(
            cast_fps(&json!({"fps": 30})).unwrap(),
            Some(Fps::new(30, 1))
        );
        assert!(
            cast_fps(&json!({"fps": 25}))
                .unwrap_err()
                .starts_with("cast: fps 25: ")
        );
        assert!(cast_fps(&json!({"fps": [30]})).is_err());
        assert_eq!(closure_of_cast(&json!({})).unwrap(), app::Closure::Open);
        assert_eq!(
            closure_of_cast(&json!({"closure": "palindrome"})).unwrap(),
            app::Closure::Palindrome
        );
        assert!(closure_of_cast(&json!({"closure": "loop"})).is_err());
        let first = resolve_poster(None, &Value::Null).unwrap();
        assert_eq!(first.to_json(), json!("first"));
        let recipe = resolve_poster(None, &json!("before-closure")).unwrap();
        assert_eq!(recipe.to_json(), json!("before-closure"));
        let flag = resolve_poster(Some("0.5"), &json!("before-closure")).unwrap();
        assert_eq!(flag.to_json(), json!(0.5));
        assert!(resolve_poster(None, &json!(-1)).is_err());
        assert!(resolve_poster(Some("-1"), &Value::Null).is_err());
    }

    #[test]
    fn names_colours_and_the_environment_are_spelled_as_written() {
        let o = Opts {
            product: "alpha".into(),
            shot: "tour".into(),
            ..opts()
        };
        assert_eq!(run_name(&o, "clip", None), "alpha-clip-tour");
        assert_eq!(run_name(&o, "still", Some("cast")), "alpha-still-tour-cast");
        assert_eq!(hexs([0, 15, 255]), "#000fff");
        assert_eq!(hex_opt(Some("#1e1e2e")), Some([0x1e, 0x1e, 0x2e]));
        assert_eq!(hex_opt(None), None);
        let env = vec![("A".to_string(), "1".to_string())];
        let passed = vec![("T".to_string(), "x".to_string())];
        assert_eq!(
            with_passed(&env, &passed),
            vec![
                ("A".to_string(), "1".to_string()),
                ("T".to_string(), "x".to_string())
            ]
        );
        assert!(!archives(
            "clip",
            "alpha",
            &Opts {
                no_archive: true,
                ..opts()
            },
            &json!({"rev": "v1"})
        ));
    }

    fn work(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/unit-render")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[derive(Clone, Default)]
    struct Shared(std::sync::Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Shared {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).to_string()
        }
    }

    fn hook(dir: &Path, cmd: &str) -> Hook {
        Hook {
            cmd: cmd.into(),
            env: vec![
                ("TOKEN".into(), "s3cret-token-value".into()),
                ("TINY".into(), "zq".into()),
            ],
            passed: vec![
                ("TOKEN".into(), "s3cret-token-value".into()),
                ("TINY".into(), "zq".into()),
            ],
            path: std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
            cwd: dir.to_path_buf(),
        }
    }

    #[test]
    fn relayed_lines_are_masked_one_by_one_and_a_last_line_without_a_newline_is_kept() {
        let passed = vec![("TOKEN".to_string(), "s3cret-token-value".to_string())];
        let mut to = Vec::new();
        relay(
            &b"a s3cret-token-value\nplain\n\xffs3cret-token-value\nend s3cret-token-value"[..],
            &mut to,
            &passed,
        );
        assert_eq!(
            String::from_utf8(to).unwrap(),
            "a <TOKEN>\nplain\n\u{fffd}<TOKEN>\nend <TOKEN>"
        );
        let mut to = Vec::new();
        relay(&b""[..], &mut to, &passed);
        assert!(to.is_empty());
        let mut to = Vec::new();
        relay(&b"s3cret-token-value"[..], &mut to, &[]);
        assert_eq!(to, b"s3cret-token-value");
    }

    #[test]
    fn a_hook_prints_each_stream_masked_where_it_wrote_it_and_its_failure_names_it() {
        let dir = work("hook-streams");
        for gated in [true, false] {
            let (out, err) = (Shared::default(), Shared::default());
            let h = hook(
                &dir,
                "echo \"o1 $TOKEN\"; echo \"e1 $TOKEN\" >&2; echo \"o2 $TINY\"; printf 'e2 %s' \"$TOKEN\" >&2",
            );
            h.relayed(gated, out.clone(), err.clone()).unwrap();
            assert_eq!(out.text(), "o1 <TOKEN>\no2 <TINY>\n", "gated {gated}");
            assert_eq!(err.text(), "e1 <TOKEN>\ne2 <TOKEN>", "gated {gated}");
        }
        let (out, err) = (Shared::default(), Shared::default());
        let h = hook(&dir, "echo \"before $TOKEN\" >&2; exit 5");
        assert_eq!(
            h.relayed(true, out.clone(), err.clone()).unwrap_err(),
            "echo \"before $TOKEN\" >&2; exit 5 failed"
        );
        assert!(out.text().is_empty());
        assert_eq!(err.text(), "before <TOKEN>\n");
        let h = Hook {
            cwd: dir.join("absent"),
            ..hook(&dir, "true")
        };
        let said = h
            .relayed(false, Shared::default(), Shared::default())
            .unwrap_err();
        assert!(said.starts_with("true: "), "{said}");
    }

    #[test]
    fn a_hook_whose_background_child_keeps_its_output_open_does_not_hold_prec() {
        let dir = work("hook-background");
        let pid_file = dir.join("child.pid");
        let h = hook(&dir, "sleep 60 & echo $! > child.pid; echo \"up $TOKEN\"");
        let out = Shared::default();
        h.relayed(false, out.clone(), Shared::default()).unwrap();
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let pid = pid.trim();
        let alive = Command::new("kill").args(["-0", pid]).status().unwrap();
        let _ = Command::new("kill").arg(pid).status();
        assert!(
            alive.success(),
            "the background child ended before prec went on"
        );
        assert_eq!(out.text(), "up <TOKEN>\n");
    }

    #[test]
    fn a_passed_value_over_several_lines_is_masked_on_every_line_a_hook_prints() {
        let dir = work("hook-multiline");
        let key = "-----KEY-----\nline-one-x\nline-two-y\nab";
        let h = Hook {
            env: vec![("KEY".into(), key.into())],
            passed: vec![("KEY".into(), key.into())],
            ..hook(
                &dir,
                "echo \"$KEY\"; echo \"$KEY\" >&2; echo \"tail line-two-y\"",
            )
        };
        let (out, err) = (Shared::default(), Shared::default());
        h.relayed(false, out.clone(), err.clone()).unwrap();
        assert_eq!(out.text(), "<KEY>\n<KEY>\n<KEY>\nab\ntail <KEY>\n");
        assert_eq!(err.text(), "<KEY>\n<KEY>\n<KEY>\nab\n");
        for part in ["line-one-x", "line-two-y", "-----KEY-----"] {
            assert!(!out.text().contains(part) && !err.text().contains(part));
        }
    }

    #[test]
    fn a_short_passed_value_is_masked_even_inside_ordinary_words() {
        let dir = work("hook-short");
        let out = Shared::default();
        hook(&dir, "echo lazquote")
            .relayed(false, out.clone(), Shared::default())
            .unwrap();
        assert_eq!(out.text(), "la<TINY>uote\n");
    }

    #[test]
    fn removing_a_run_takes_its_folder_and_its_lock() {
        let root = work("run-remove");
        let dir = root.join("alpha-clip-s-cast-7");
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        std::fs::write(dir.join("inner/alpha-clip-s.cast"), "x").unwrap();
        let lock_path = root.join("alpha-clip-s-cast-7.lock");
        let run = Run {
            dir: dir.clone(),
            lock_path: lock_path.clone(),
            _lock: File::create(&lock_path).unwrap(),
        };
        std::fs::write(root.join("alpha-clip-s-cast-8.lock"), "").unwrap();
        run.remove();
        assert!(!dir.exists());
        assert!(!lock_path.exists());
        assert!(root.join("alpha-clip-s-cast-8.lock").exists());
        let gone = root.join("alpha-clip-s-cast-9");
        let lock_path = root.join("alpha-clip-s-cast-9.lock");
        Run {
            dir: gone,
            lock_path: lock_path.clone(),
            _lock: File::create(&lock_path).unwrap(),
        }
        .remove();
        assert!(!lock_path.exists());
    }

    fn listed(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn delivery_moves_every_listed_output_and_drops_the_old_manifest_first() {
        let w = work("deliver");
        let stage = w.join("stage");
        let dir = w.join("out");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(stage.join("a.png"), b"new a").unwrap();
        std::fs::write(stage.join("a.cast"), b"cast").unwrap();
        std::fs::write(dir.join("a.png"), b"old a").unwrap();
        let manifest = dir.join("a.json");
        std::fs::write(&manifest, b"{}").unwrap();
        let files = json!([{"file": "a.png"}, {"file": "a.cast"}]);
        deliver(&stage, &dir, &files, &manifest).unwrap();
        assert_eq!(listed(&dir), ["a.cast", "a.png"]);
        assert_eq!(std::fs::read(dir.join("a.png")).unwrap(), b"new a");
        assert!(listed(&stage).is_empty());
        deliver(&stage, &dir, &json!([]), &manifest).unwrap();
        let e = deliver(&stage, &dir, &json!([{"file": "gone.png"}]), &manifest).unwrap_err();
        assert!(e.contains("gone.png"), "{e}");
        assert_eq!(
            deliver(&stage, &dir, &json!([{"role": "x"}]), &manifest).unwrap_err(),
            "an output without a name"
        );
        std::fs::create_dir_all(dir.join("held.json/inside")).unwrap();
        assert!(deliver(&stage, &dir, &json!([]), &dir.join("held.json")).is_err());
    }

    #[test]
    fn a_derived_png_at_the_master_size_is_the_master_and_a_smaller_one_is_resampled() {
        let w = work("derive");
        let img =
            image::RgbImage::from_fn(40, 20, |x, y| image::Rgb([x as u8 * 6, y as u8 * 12, 77]));
        let linear = OnceCell::new();
        let same = w.join("same.png");
        derive_png(&same, &img, &linear, [40, 20]).unwrap();
        assert!(linear.get().is_none());
        assert_eq!(image::open(&same).unwrap().to_rgb8(), img);
        let small = w.join("small.png");
        derive_png(&small, &img, &linear, [20, 10]).unwrap();
        assert!(linear.get().is_some());
        let got = image::open(&small).unwrap().to_rgb8();
        assert_eq!(got.dimensions(), (20, 10));
        assert!(got.pixels().all(|p| p[2] == 77));
        assert!(derive_png(&w.join("gone/x.png"), &img, &linear, [20, 10]).is_err());
        assert_eq!(listed(&w), ["same.png", "small.png"]);
    }
}
