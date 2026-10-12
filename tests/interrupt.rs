mod common;

use common::said;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SETUP: &str = "sleep 600 >/dev/null 2>&1 & echo $! > {tmp}/bg.pid";
const TEARDOWN: &str = "kill \"$(cat {tmp}/bg.pid)\"; echo down > {tmp}/teardown.marker";

fn case(name: &str) -> PathBuf {
    common::caller("interrupt", name)
}

fn app(dir: &Path, setup: &str, teardown: &str) -> PathBuf {
    let hooks = format!(
        "setup = {}\nteardown = {}\n",
        serde_json::json!(setup),
        serde_json::json!(teardown)
    );
    let app = common::demo_app(dir, &hooks);
    common::add_shot(
        &app,
        "long",
        "Sleep 200ms\nType \"demo tick\"\nEnter\nSleep 600s\n",
        "",
    );
    common::add_shot(
        &app,
        "short",
        "Sleep 200ms\nType \"demo tick\"\nEnter\nSleep 1.5s\n",
        "",
    );
    app
}

fn prec(dir: &Path, args: &[&str]) -> Command {
    common::prec(dir, &dir.join("archive"), args)
}

fn with_jobs(c: &mut Command, dir: &Path) {
    c.env_remove("PITO_JOBS")
        .env("PITO_JOBS_DIR", dir.join("jobs"));
}

fn of_run(dir: &Path) -> Vec<common::Proc> {
    let mark = format!("{}/", dir.display());
    common::procs()
        .into_iter()
        .filter(|p| p.args.iter().chain(p.env.iter()).any(|w| w.contains(&mark)))
        .collect()
}

fn none_left(dir: &Path) {
    let left = common::gone(std::time::Duration::from_secs(15), || of_run(dir));
    assert!(
        left.is_empty(),
        "processes of the run outlived prec: {:?}",
        left.iter().map(|p| (p.pid, &p.args)).collect::<Vec<_>>()
    );
}

fn job_of(dir: &Path, pid: u32) -> Value {
    let jobs = dir.join("jobs");
    for e in std::fs::read_dir(&jobs)
        .unwrap_or_else(|e| panic!("{}: {e}", jobs.display()))
        .flatten()
    {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with(&format!("{pid}-")) && name.ends_with(".json") {
            return serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
        }
    }
    panic!("no job record of prec {pid} in {}", jobs.display());
}

fn torn_down(dir: &Path) {
    let marker = dir.join("tmp/teardown.marker");
    assert!(
        marker.exists(),
        "teardown left no marker; prec said:\n{}",
        said(dir)
    );
    let bg = std::fs::read_to_string(dir.join("tmp/bg.pid")).unwrap();
    assert!(
        !common::alive(&bg),
        "setup's process {} is still running",
        bg.trim()
    );
}

#[test]
fn term_during_the_recording_runs_teardown_stops_the_run_and_fails_the_job() {
    let dir = case("term");
    let app = app(&dir, SETUP, TEARDOWN);
    let mut c = prec(
        &dir,
        &[
            "clip",
            "demo",
            "long",
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            "tmp/out",
            "--size",
            "320x180",
            "--fps",
            "30",
        ],
    );
    with_jobs(&mut c, &dir);
    let mut child = common::spawn(c, &dir);
    common::wait_for("the pty's shell", &dir, 60, || {
        of_run(&dir).iter().any(|p| {
            p.args.first().is_some_and(|b| b == "bash") && p.args.iter().any(|w| w == "-i")
        })
    });
    assert!(common::signal(child.id(), "TERM"));
    let status = common::exits(&mut child, &dir);
    assert_eq!(status.code(), Some(143), "{}", said(&dir));
    torn_down(&dir);
    none_left(&dir);
    let job = job_of(&dir, child.id());
    assert_eq!(job["state"], "failed", "{job}");
    assert_eq!(job["exit"], 143, "{job}");
}

#[test]
fn a_failed_setup_still_runs_teardown() {
    let dir = case("setup");
    let app = app(&dir, &format!("{SETUP}; exit 1"), TEARDOWN);
    let mut c = prec(
        &dir,
        &[
            "record",
            "demo",
            "long",
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            "tmp/out",
        ],
    );
    with_jobs(&mut c, &dir);
    let mut child = common::spawn(c, &dir);
    let status = common::exits(&mut child, &dir);
    assert!(!status.success());
    assert!(said(&dir).contains("failed"), "{}", said(&dir));
    torn_down(&dir);
    none_left(&dir);
    let job = job_of(&dir, child.id());
    assert_eq!(job["state"], "failed", "{job}");
}

#[test]
fn hooks_see_only_the_ptys_environment() {
    let dir = case("hook-env");
    let app = app(&dir, "env > {tmp}/setup.env", "env > {tmp}/teardown.env");
    let mut c = prec(
        &dir,
        &[
            "record",
            "demo",
            "short",
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            "tmp/out",
        ],
    );
    c.env("PREC_LEAK_CHECK", "1")
        .env("XDG_CONFIG_HOME", dir.join("leak"));
    common::ok(c);
    let mut allowed: BTreeSet<&str> = [
        "TZ",
        "LANG",
        "LC_ALL",
        "TERM",
        "COLORTERM",
        "HOME",
        "DEMO_DATA",
        "PATH",
        "PWD",
        "SHLVL",
        "_",
    ]
    .into();
    for key in ["USER", "LOGNAME"] {
        if std::env::var_os(key).is_some() {
            allowed.insert(key);
        }
    }
    for hook in ["setup", "teardown"] {
        let text = std::fs::read_to_string(dir.join(format!("tmp/{hook}.env"))).unwrap();
        let keys: BTreeSet<&str> = text
            .lines()
            .filter_map(|l| l.split_once('=').map(|(k, _)| k))
            .collect();
        let extra: Vec<&&str> = keys.difference(&allowed).collect();
        assert!(extra.is_empty(), "{hook} saw {extra:?}");
        for key in ["HOME", "DEMO_DATA", "PATH", "TZ"] {
            assert!(keys.contains(key), "{hook} lacks {key}");
        }
        let home = text.lines().find_map(|l| l.strip_prefix("HOME=")).unwrap();
        assert!(
            home.starts_with(&format!("{}/tmp/prec/run/", dir.display())),
            "{hook}'s HOME is {home}"
        );
    }
}

#[test]
fn int_during_the_encode_leaves_no_ffmpeg_and_no_chromium() {
    let dir = case("int");
    let cast = dir.join("cast/long.cast");
    common::write_cast(
        &cast,
        "overlap",
        "long",
        "clip",
        &[(0.0, common::paint(32, 96, 160))],
        20.0,
    );
    let mut c = prec(
        &dir,
        &[
            "render",
            cast.to_str().unwrap(),
            "--size",
            "1920x1080",
            "--fps",
            "120",
            "--out",
            "tmp/out",
        ],
    );
    common::confined(&mut c, &dir);
    let mut child = common::spawn(c, &dir);
    common::wait_for("an ffmpeg of the run", &dir, 240, || {
        of_run(&dir)
            .iter()
            .any(|p| p.args.first().is_some_and(|b| b.ends_with("ffmpeg")))
    });
    assert!(common::signal(child.id(), "INT"));
    let status = common::exits(&mut child, &dir);
    assert_eq!(status.code(), Some(130), "{}", said(&dir));
    none_left(&dir);
    assert!(said(&dir).contains("SIGINT"), "{}", said(&dir));
}

fn write_cast(path: &Path) {
    let events: Vec<(f64, String)> = (0..24u32)
        .map(|k| {
            let v = 32 + 8 * k;
            (k as f64 / 30.0, common::paint(v, 255 - v, 96))
        })
        .collect();
    common::write_cast(path, "overlap", "both", "clip", &events, 0.8);
}

fn render(dir: &Path, cast: &Path, size: &str, out: &str, extra: &[&str]) -> Command {
    let mut c = prec(dir, &["render"]);
    c.arg(cast)
        .args(["--size", size, "--fps", "30", "--out", out])
        .args(extra);
    c
}

fn outputs(dir: &Path, out: &str, size: &str) -> (Vec<u8>, Vec<u8>) {
    let stem = dir
        .join(out)
        .join(format!("overlap/clip/overlap-clip-both-{size}-30fps"));
    let read = |ext: &str| {
        let p = stem.with_extension(ext);
        std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    };
    (read("master.mp4"), read("poster.png"))
}

fn at_once(renders: Vec<Command>) -> Vec<String> {
    let children: Vec<_> = renders
        .into_iter()
        .map(|mut c| {
            c.stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    children
        .into_iter()
        .map(|child| {
            let o = child.wait_with_output().unwrap();
            common::expect_ok(&o, "a render of two");
            String::from_utf8_lossy(&o.stderr).to_string()
        })
        .collect()
}

#[test]
fn two_renders_of_one_shot_at_different_sizes_run_at_once_and_match_their_lone_bytes() {
    let dir = case("overlap");
    let cast = dir.join("cast/both.cast");
    write_cast(&cast);
    let sizes = ["320x180", "640x360"];
    for size in sizes {
        common::ok(render(&dir, &cast, size, "tmp/alone", &[]));
    }
    let said = at_once(
        sizes
            .iter()
            .map(|size| render(&dir, &cast, size, "tmp/together", &[]))
            .collect(),
    );
    for err in said {
        assert!(!err.contains("waiting for"), "a render waited: {err}");
    }
    for size in sizes {
        assert!(
            outputs(&dir, "tmp/alone", size) == outputs(&dir, "tmp/together", size),
            "{size}: the overlapped render differs from the lone one"
        );
    }
}

#[test]
fn two_renders_of_one_stem_at_once_leave_one_of_them_whole() {
    let dir = case("same-stem");
    let cast = dir.join("cast/both.cast");
    write_cast(&cast);
    let size = "320x180";
    let looks: [&[&str]; 2] = [
        &["--poster", "first"],
        &["--set", "closure=palindrome", "--poster", "0.5"],
    ];
    let alone: Vec<_> = looks
        .iter()
        .enumerate()
        .map(|(i, extra)| {
            let out = format!("tmp/alone-{i}");
            common::ok(render(&dir, &cast, size, &out, extra));
            outputs(&dir, &out, size)
        })
        .collect();
    assert!(alone[0].0 != alone[1].0 && alone[0].1 != alone[1].1);
    at_once(
        looks
            .iter()
            .map(|extra| render(&dir, &cast, size, "tmp/together", extra))
            .collect(),
    );
    let (master, poster) = outputs(&dir, "tmp/together", size);
    let whole = alone.iter().position(|(m, _)| *m == master);
    let whole = whole.unwrap_or_else(|| panic!("the master is neither lone render's"));
    assert!(
        poster == alone[whole].1,
        "the master is render {whole}'s but the poster is not"
    );
}

#[test]
fn a_clip_leaves_only_its_stems_cast_in_the_out_folder() {
    let dir = case("out");
    let app = app(&dir, "true", "true");
    common::add_shot(
        &app,
        "brief",
        "Type \"demo tick\"\nEnter\n",
        "settle = \"100ms\"\n",
    );
    common::ok(prec(
        &dir,
        &[
            "clip",
            "demo",
            "brief",
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            "tmp/out",
            "--size",
            "320x180",
            "--fps",
            "30",
        ],
    ));
    let clips = dir.join("tmp/out/demo/clip");
    let casts: BTreeSet<String> = std::fs::read_dir(&clips)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".cast"))
        .collect();
    assert_eq!(
        casts,
        BTreeSet::from(["demo-clip-brief-320x180-30fps.cast".to_string()])
    );
}
