use crate::{cache, paths};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DEFAULT_FONT: &str = "JetBrainsMono Nerd Font";

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
    pub app: String,
    pub bin: String,
    #[serde(default)]
    pub font: Option<String>,
    pub grid: [u32; 2],
    #[serde(default = "window")]
    pub frame: String,
    #[serde(default)]
    pub ground: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub typing: Option<String>,
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default)]
    pub teardown: Option<String>,
    #[serde(default)]
    pub fixtures_env: Option<String>,
    #[serde(default)]
    pub theme: Option<Value>,
    #[serde(default)]
    pub build: Option<Build>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub camera: Option<Value>,
    #[serde(default)]
    pub shots: BTreeMap<String, Shot>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Build {
    #[serde(default)]
    pub command: Option<String>,
    pub bin_dir: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Shot {
    pub kind: String,
    #[serde(default)]
    pub line: Option<String>,
    pub tape: String,
    #[serde(default)]
    pub launch: Option<String>,
    #[serde(default)]
    pub settle: Option<String>,
    #[serde(default)]
    pub closure: Option<Value>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub fixtures: Option<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub poster: Option<Poster>,
    #[serde(default)]
    pub grid: Option<[u32; 2]>,
    #[serde(default)]
    pub camera: Option<Value>,
    #[serde(default)]
    pub keys: Option<Value>,
}

pub const NO_CAMERA: &str = "prec has no camera: a terminal is drawn flat at its own grid, with no zoom, move, turn or keyframes, so camera and keys are refused";

#[derive(Clone, Debug, PartialEq)]
pub enum Closure {
    Open,
    Palindrome,
    Crossfade(f64),
}

impl Closure {
    pub fn from_value(v: &Value) -> Result<Closure, String> {
        match v {
            Value::Null => Ok(Closure::Open),
            Value::String(s) if s == "palindrome" => Ok(Closure::Palindrome),
            Value::Object(m) if m.len() == 1 && m.contains_key("crossfade") => {
                let secs = m["crossfade"]
                    .as_f64()
                    .ok_or("closure: { crossfade = <seconds> }")?;
                if secs <= 0.0 {
                    return Err("closure: the crossfade needs more than 0 seconds".into());
                }
                Ok(Closure::Crossfade(secs))
            }
            other => Err(format!(
                "closure {other}: \"palindrome\", {{ crossfade = <seconds> }}, or absent for an open clip"
            )),
        }
    }

    pub fn from_set(key: &str, value: &str) -> Result<Closure, String> {
        match (key, value) {
            ("closure", "palindrome") => Ok(Closure::Palindrome),
            ("closure", "none") | ("closure", "open") => Ok(Closure::Open),
            ("closure.crossfade", v) => {
                let secs: f64 = v
                    .parse()
                    .map_err(|_| format!("--set closure.crossfade={v}: seconds"))?;
                Closure::from_value(&serde_json::json!({ "crossfade": secs }))
            }
            (k, v) => Err(format!(
                "--set {k}={v}: closure=palindrome, closure=none or closure.crossfade=<seconds>"
            )),
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            Closure::Open => Value::Null,
            Closure::Palindrome => Value::String("palindrome".into()),
            Closure::Crossfade(s) => serde_json::json!({ "crossfade": s }),
        }
    }

    pub fn fade_frames(&self, fps: u32) -> usize {
        match self {
            Closure::Crossfade(s) => ((s * fps as f64).round() as usize).max(1),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Poster {
    First,
    BeforeClosure,
    Seconds(f64),
}

impl Poster {
    pub fn from_value(v: &Value) -> Result<Poster, String> {
        match v {
            Value::String(s) if s == "first" => Ok(Poster::First),
            Value::String(s) if s == "before-closure" => Ok(Poster::BeforeClosure),
            Value::Number(n) => match n.as_f64() {
                Some(secs) if secs.is_finite() && secs >= 0.0 => Ok(Poster::Seconds(secs)),
                _ => Err(format!(
                    "poster {v}: \"first\", \"before-closure\", or seconds of shown time"
                )),
            },
            other => Err(format!(
                "poster {other}: \"first\", \"before-closure\", or seconds of shown time"
            )),
        }
    }

    pub fn from_flag(s: &str) -> Result<Poster, String> {
        match s {
            "first" => Ok(Poster::First),
            "before-closure" => Ok(Poster::BeforeClosure),
            other => {
                let secs: f64 = other.parse().map_err(|_| {
                    format!("--poster {other}: first, before-closure, or seconds of shown time")
                })?;
                if !secs.is_finite() || secs < 0.0 {
                    return Err(format!(
                        "--poster {other}: seconds of shown time, zero or more"
                    ));
                }
                Ok(Poster::Seconds(secs))
            }
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            Poster::First => Value::String("first".into()),
            Poster::BeforeClosure => Value::String("before-closure".into()),
            Poster::Seconds(s) => serde_json::json!(s),
        }
    }
}

impl Serialize for Poster {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Poster::First => serializer.serialize_str("first"),
            Poster::BeforeClosure => serializer.serialize_str("before-closure"),
            Poster::Seconds(s) => serializer.serialize_f64(*s),
        }
    }
}

impl<'de> Deserialize<'de> for Poster {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Poster::from_value(&value).map_err(serde::de::Error::custom)
    }
}

impl Shot {
    pub fn closure(&self) -> Result<Closure, String> {
        self.closure
            .as_ref()
            .map_or(Ok(Closure::Open), Closure::from_value)
    }
}

fn window() -> String {
    "window".into()
}

pub struct Resolved {
    pub dir: PathBuf,
    pub commit: Option<String>,
    pub rev: String,
    pub tag: Option<String>,
    pub pinned: bool,
    pub terminal: Terminal,
    pub source_sha256: String,
    pub gitdir: Option<PathBuf>,
    pub key: Option<String>,
    pub clean: bool,
}

pub fn load(dir: &Path) -> Result<(Terminal, String), String> {
    let path = dir.join("render/terminal.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let t: Terminal = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if t.frame != "window" && t.frame != "bare" {
        return Err(format!("{}: frame is window or bare", path.display()));
    }
    if t.camera.is_some() {
        return Err(format!("{}: {NO_CAMERA}", path.display()));
    }
    for (name, shot) in &t.shots {
        if shot.camera.is_some() || shot.keys.is_some() {
            return Err(format!("{}: shot {name}: {NO_CAMERA}", path.display()));
        }
        shot.closure()
            .map_err(|e| format!("{}: shot {name}: {e}", path.display()))?;
        if shot.poster.is_some() && shot.kind != "clip" {
            return Err(format!(
                "{}: shot {name}: poster is for clips",
                path.display()
            ));
        }
        if shot.kind != "clip" && shot.kind != "still" {
            return Err(format!(
                "{}: shot {name} kind is clip or still",
                path.display()
            ));
        }
    }
    Ok((t, paths::hash_bytes(text.as_bytes())))
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = paths::output(Command::new("git").arg("-C").arg(dir).args(args)).ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn identity(dir: &Path) -> String {
    git(dir, &["remote", "get-url", "origin"])
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| {
            std::fs::canonicalize(dir)
                .unwrap_or_else(|_| dir.to_path_buf())
                .to_string_lossy()
                .to_string()
        })
}

fn commit_of(gitdir: &Path, rev: &str) -> Option<String> {
    let out = paths::output(Command::new("git").arg("--git-dir").arg(gitdir).args([
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{rev}^{{commit}}"),
    ]))
    .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|c| !c.is_empty())
}

fn tag_of(gitdir: &Path, rev: &str) -> Option<String> {
    let found = paths::output(Command::new("git").arg("--git-dir").arg(gitdir).args([
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/tags/{rev}"),
    ]))
    .ok()?
    .status
    .success();
    found.then(|| rev.to_string())
}

fn checked(product: &str, dir: &Path) -> Result<(Terminal, String), String> {
    let (terminal, sha) = load(dir)?;
    if terminal.app != product {
        return Err(format!(
            "{} describes {}, not {product}",
            dir.join("render/terminal.toml").display(),
            terminal.app
        ));
    }
    Ok((terminal, sha))
}

fn common_dir(dir: &Path) -> Option<PathBuf> {
    git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .and_then(|d| std::fs::canonicalize(d).ok())
}

fn own(dir: &Path, tree: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{}: no such folder", dir.display()));
    }
    match (common_dir(tree), common_dir(dir)) {
        (Some(ours), Some(theirs)) if ours == theirs => Ok(()),
        _ => Err(format!(
            "{} is outside this repository ({}): prec renders only the caller's own app, from its own tree or from its own repository at --rev, and another product's renders come from that product's own repository",
            dir.display(),
            tree.display()
        )),
    }
}

pub fn resolve(
    product: &str,
    app_dir: Option<&Path>,
    rev: Option<&str>,
) -> Result<Resolved, String> {
    let tree = paths::caller_tree()?;
    let local = match app_dir {
        Some(d) => paths::absolute(d)?,
        None => tree.clone(),
    };
    own(&local, &tree)?;
    let Some(r) = rev else {
        let (terminal, sha) = checked(product, &local)?;
        let gitdir = git(&local, &["rev-parse", "--absolute-git-dir"]).map(PathBuf::from);
        let commit = git(&local, &["rev-parse", "HEAD"]);
        let dirty = git(&local, &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
        let rev = match &commit {
            Some(c) => format!(
                "{}{}",
                &c[..c.len().min(12)],
                if dirty { "+dirty" } else { "" }
            ),
            None => "local".into(),
        };
        let key = gitdir.as_ref().map(|_| cache::repo_key(&identity(&local)));
        return Ok(Resolved {
            dir: local,
            clean: commit.is_some() && !dirty,
            commit,
            rev,
            tag: None,
            pinned: false,
            terminal,
            source_sha256: sha,
            gitdir,
            key,
        });
    };
    let gitdir = git(&local, &["rev-parse", "--absolute-git-dir"])
        .map(PathBuf::from)
        .ok_or_else(|| format!("{} is not a git checkout", local.display()))?;
    let commit = commit_of(&gitdir, r).ok_or_else(|| format!("{} has no {r}", local.display()))?;
    let key = cache::repo_key(&identity(&local));
    let dir = cache::source(&gitdir, &key, &commit)?;
    let (terminal, sha) = checked(product, &dir)?;
    Ok(Resolved {
        dir,
        commit: Some(commit),
        rev: r.to_string(),
        tag: tag_of(&gitdir, r),
        pinned: true,
        terminal,
        source_sha256: sha,
        gitdir: Some(gitdir),
        key: Some(key),
        clean: true,
    })
}

pub fn expand(s: &str, vars: &[(&str, &Path)]) -> String {
    let mut out = s.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), &v.to_string_lossy());
    }
    out
}

pub fn target_dir(app: &Resolved, product: &str) -> Result<PathBuf, String> {
    match &app.key {
        Some(k) => cache::target(k),
        None => Ok(paths::caller_tmp()?
            .join("tools")
            .join(format!("{product}@{}", paths::slug(&app.rev)))
            .join("target")),
    }
}

pub const PLACEHOLDERS: [&str; 6] = ["tmp", "scratch", "app", "bin", "target", "fixtures"];

pub fn vars(
    tmp: &Path,
    scratch: &Path,
    app: &Path,
    bin: &Path,
    target: &Path,
    fixtures: &Path,
) -> Vec<(String, PathBuf)> {
    PLACEHOLDERS
        .iter()
        .zip([tmp, scratch, app, bin, target, fixtures])
        .map(|(k, v)| (k.to_string(), v.to_path_buf()))
        .collect()
}

fn run_build(cmd: &str, cwd: &Path, target: &Path, product: &str) -> Result<(), String> {
    eprintln!("building {product} with: {cmd}");
    let ok = paths::status(
        Command::new("bash")
            .args(["-c", cmd])
            .current_dir(cwd)
            .env("CARGO_TARGET_DIR", target)
            .env("CARGO_NET_GIT_FETCH_WITH_CLI", "true"),
    )
    .map_err(|e| format!("build: {e}"))?;
    if !ok.success() {
        return Err(format!("{product}'s build command failed: {cmd}"));
    }
    Ok(())
}

pub fn build(
    app: &Resolved,
    product: &str,
    bin_override: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(b) = bin_override {
        return paths::absolute(b);
    }
    let target = target_dir(app, product)?;
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let build = app.terminal.build.clone().unwrap_or(Build {
        command: None,
        bin_dir: "{app}".into(),
    });
    let tmp = paths::caller_tmp()?;
    let Some(command) = &build.command else {
        let vars = [
            ("tmp", tmp.as_path()),
            ("app", app.dir.as_path()),
            ("target", target.as_path()),
        ];
        return Ok(PathBuf::from(expand(&build.bin_dir, &vars)));
    };
    let cached = match (&app.key, &app.gitdir, &app.commit, app.clean) {
        (Some(k), Some(g), Some(c), true) => Some((k.clone(), g.clone(), c.clone())),
        _ => None,
    };
    let Some((key, gitdir, commit)) = cached else {
        let growth = app
            .key
            .as_ref()
            .map(|k| cache::Growth::of(&target, &format!("target/{k}")));
        let _guard = match &app.key {
            Some(k) => Some(cache::exclusive("work", k)?),
            None => None,
        };
        let vars = [
            ("tmp", tmp.as_path()),
            ("app", app.dir.as_path()),
            ("target", target.as_path()),
        ];
        run_build(&expand(command, &vars), &app.dir, &target, product)?;
        if let Some(g) = growth {
            g.report();
        }
        let dir = PathBuf::from(expand(&build.bin_dir, &vars));
        if !dir.join(&app.terminal.bin).exists() {
            return Err(format!(
                "{} is missing after the build",
                dir.join(&app.terminal.bin).display()
            ));
        }
        return Ok(dir);
    };
    let name = format!(
        "{key}@{}",
        cache::build_key(&commit, command, &build.bin_dir)
    );
    let mut built = false;
    let entry = cache::entry("bin", &name, |entry| {
        built = true;
        let growth = cache::Growth::of(&target, &format!("target/{key}"));
        let _work = cache::exclusive("work", &key)?;
        let tree = cache::worktree(&gitdir, &key, &commit)?;
        let vars = [
            ("tmp", tmp.as_path()),
            ("app", tree.as_path()),
            ("target", target.as_path()),
        ];
        run_build(&expand(command, &vars), &tree, &target, product)?;
        growth.report();
        let from = PathBuf::from(expand(&build.bin_dir, &vars));
        paths::fresh(entry)?;
        cache::keep(&from, entry)?;
        if !entry.join(&app.terminal.bin).exists() {
            return Err(format!(
                "{} is missing after the build",
                from.join(&app.terminal.bin).display()
            ));
        }
        Ok(())
    })?;
    if !built {
        eprintln!(
            "{product}@{}: using the cached build",
            &commit[..commit.len().min(12)]
        );
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_placeholder_expands_in_every_command_string() {
        let v = vars(
            Path::new("/t"),
            Path::new("/s"),
            Path::new("/a"),
            Path::new("/b"),
            Path::new("/g"),
            Path::new("/f"),
        );
        let vref: Vec<(&str, &Path)> = v.iter().map(|(k, p)| (k.as_str(), p.as_path())).collect();
        let all: String = PLACEHOLDERS.iter().map(|p| format!("{{{p}}} ")).collect();
        let out = expand(&all, &vref);
        assert_eq!(out, "/t /s /a /b /g /f ");
        assert_eq!(
            expand("{target}/release/examples/x && {bin}/y", &vref),
            "/g/release/examples/x && /b/y"
        );
        assert_eq!(expand("{nope}", &vref), "{nope}");
    }

    #[test]
    fn an_app_must_live_in_the_callers_own_repository() {
        let tree = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let work = tree.join("tmp/app-unit/own");
        let _ = std::fs::remove_dir_all(&work);
        let inside = work.join("inside");
        let other = work.join("other");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&other)
            .status()
            .unwrap();
        assert!(init.success());
        assert!(own(&tree, &tree).is_ok());
        assert!(own(&tree.join("examples/demo"), &tree).is_ok());
        assert!(own(&inside, &tree).is_ok());
        let refused = own(&other, &tree).unwrap_err();
        assert!(refused.contains("outside this repository"), "{refused}");
        assert!(own(&other, &other).is_ok());
        assert!(own(&tree.join("examples/demo"), &other).is_err());
        assert!(
            own(&work.join("missing"), &tree)
                .unwrap_err()
                .contains("no such folder")
        );
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn closure_reads_the_standard_spelling_and_refuses_the_old_close() {
        assert_eq!(Closure::from_value(&Value::Null).unwrap(), Closure::Open);
        assert_eq!(
            Closure::from_value(&serde_json::json!("palindrome")).unwrap(),
            Closure::Palindrome
        );
        assert_eq!(
            Closure::from_value(&serde_json::json!({"crossfade": 0.5})).unwrap(),
            Closure::Crossfade(0.5)
        );
        assert!(Closure::from_value(&serde_json::json!("crossfade:15")).is_err());
        assert!(Closure::from_set("close", "palindrome").is_err());
        assert!(
            toml::from_str::<Terminal>(
                "app = \"x\"\nbin = \"x\"\ngrid = [80, 24]\n[shots.a]\nkind = \"clip\"\ntape = \"a.tape\"\nclose = \"palindrome\"\n",
            )
            .is_err()
        );
        assert_eq!(Closure::Crossfade(0.5).fade_frames(120), 60);
        assert_eq!(
            Closure::from_set("closure.crossfade", "0.25").unwrap(),
            Closure::Crossfade(0.25)
        );
        let t: Terminal = toml::from_str(
            "app = \"x\"\nbin = \"x\"\ngrid = [80, 24]\n[shots.a]\nkind = \"clip\"\ntape = \"a.tape\"\nclosure = { crossfade = 0.5 }\n[shots.b]\nkind = \"clip\"\ntape = \"b.tape\"\nclosure = \"palindrome\"\n",
        )
        .unwrap();
        assert_eq!(t.shots["a"].closure().unwrap(), Closure::Crossfade(0.5));
        assert_eq!(t.shots["b"].closure().unwrap(), Closure::Palindrome);
    }

    #[test]
    fn a_poster_is_first_before_closure_or_seconds() {
        assert_eq!(
            Poster::from_value(&serde_json::json!("first")).unwrap(),
            Poster::First
        );
        assert_eq!(
            Poster::from_value(&serde_json::json!("before-closure")).unwrap(),
            Poster::BeforeClosure
        );
        assert_eq!(
            Poster::from_value(&serde_json::json!(1.5)).unwrap(),
            Poster::Seconds(1.5)
        );
        assert_eq!(Poster::from_flag("0.5").unwrap(), Poster::Seconds(0.5));
        assert_eq!(Poster::from_flag("first").unwrap(), Poster::First);
        assert!(Poster::from_value(&serde_json::json!("middle")).is_err());
        assert!(Poster::from_value(&serde_json::json!("1.5")).is_err());
        assert!(Poster::from_value(&serde_json::json!(-0.2)).is_err());
        assert!(Poster::from_flag("middle").is_err());
        assert!(Poster::from_flag("-1").is_err());
        let t: Terminal = toml::from_str(
            "app = \"x\"\nbin = \"x\"\ngrid = [80, 24]\n[shots.a]\nkind = \"clip\"\ntape = \"a.tape\"\nposter = \"before-closure\"\n[shots.b]\nkind = \"clip\"\ntape = \"b.tape\"\nposter = 1.5\n[shots.c]\nkind = \"clip\"\ntape = \"c.tape\"\nposter = 2\n",
        )
        .unwrap();
        assert_eq!(t.shots["a"].poster, Some(Poster::BeforeClosure));
        assert_eq!(t.shots["b"].poster, Some(Poster::Seconds(1.5)));
        assert_eq!(t.shots["c"].poster, Some(Poster::Seconds(2.0)));
        assert!(
            toml::from_str::<Terminal>(
                "app = \"x\"\nbin = \"x\"\ngrid = [80, 24]\n[shots.a]\nkind = \"clip\"\ntape = \"a.tape\"\nposter = \"middle\"\n",
            )
            .is_err()
        );
        assert_eq!(
            Poster::BeforeClosure.to_json(),
            serde_json::json!("before-closure")
        );
        assert_eq!(Poster::Seconds(1.5).to_json().as_f64(), Some(1.5));
    }
}
