mod common;

use common::Proc;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const KILL_CHANGES: usize = 30;
const RUN: &str = "demo-clip-tour-replay";

const FLAGS: [&str; 15] = [
    "--headless=new",
    "--disable-gpu",
    "--no-first-run",
    "--no-default-browser-check",
    "--hide-scrollbars",
    "--mute-audio",
    "--disable-extensions",
    "--disable-background-timer-throttling",
    "--disable-renderer-backgrounding",
    "--disable-backgrounding-occluded-windows",
    "--disable-features=Translate,MediaRouter,OptimizationHints",
    "--force-color-profile=srgb",
    "--allow-file-access-from-files",
    "--remote-debugging-pipe",
    "about:blank",
];

const HEADLESS: [&str; 4] = [
    "--noerrdialogs",
    "--ozone-platform=headless",
    "--ozone-override-screen-size=",
    "--use-angle=swiftshader-webgl",
];

fn caller(name: &str) -> PathBuf {
    common::caller("browser", name)
}

fn cast(dir: &Path, changes: usize) -> PathBuf {
    let events: Vec<(f64, String)> = (0..changes)
        .map(|k| (k as f64 * 0.05, common::paint(32 + 6 * k as u32, 96, 160)))
        .collect();
    let path = dir.join("cast/tour.cast");
    common::write_cast(
        &path,
        "demo",
        "tour",
        "clip",
        &events,
        changes as f64 * 0.05 + 0.5,
    );
    path
}

fn profile(run: &Path) -> String {
    format!("--user-data-dir={}", run.join("chromium").display())
}

fn render(dir: &Path, changes: usize, direct: bool, size: &str, fps: &str) -> Command {
    let cast = cast(dir, changes);
    let mut c = common::prec(
        dir,
        &dir.join("archive"),
        &[
            "render",
            cast.to_str().unwrap(),
            "--size",
            size,
            "--fps",
            fps,
            "--no-encode",
            "--out",
            "tmp/out",
        ],
    );
    if !direct {
        common::confined(&mut c, dir);
    }
    c
}

fn spawn(dir: &Path, direct: bool) -> Child {
    common::spawn(render(dir, KILL_CHANGES, direct, "1920x1080", "120"), dir)
}

fn of_profile(dir: &Path) -> Vec<Proc> {
    let flags: Vec<String> = common::runs(dir, RUN).iter().map(|r| profile(r)).collect();
    common::procs()
        .into_iter()
        .filter(|p| p.args.iter().any(|a| flags.contains(a)))
        .collect()
}

fn sweep(dir: &Path) {
    for p in of_profile(dir) {
        common::signal(p.pid, "KILL");
    }
}

fn frames(dir: &Path, child: &mut Child, n: usize) {
    let reached = common::until(Duration::from_secs(180), || {
        let count = common::runs(dir, RUN)
            .iter()
            .filter_map(|r| std::fs::read_dir(r.join("frames")).ok())
            .map(|d| d.count())
            .max()
            .unwrap_or(0);
        if count < n {
            if let Ok(Some(s)) = child.try_wait() {
                panic!("prec ended ({s}) before {n} frames: {}", common::said(dir));
            }
            return false;
        }
        true
    });
    if !reached {
        let _ = child.kill();
        sweep(dir);
        panic!("no {n} frames after 180 s: {}", common::said(dir));
    }
}

fn gone_within(dir: &Path, limit: Duration) -> Vec<Proc> {
    common::gone(limit, || of_profile(dir))
}

#[test]
fn chromium_does_not_outlive_a_killed_prec_and_the_next_run_clears_its_folder() {
    let dir = caller("survivor");
    let mut child = spawn(&dir, false);
    frames(&dir, &mut child, 10);
    assert!(!of_profile(&dir).is_empty(), "no chromium found mid-replay");
    child.kill().unwrap();
    child.wait().unwrap();
    let left = gone_within(&dir, Duration::from_secs(10));
    sweep(&dir);
    assert!(
        left.is_empty(),
        "chromium outlived prec: {:?}",
        left.iter().map(|p| p.pid).collect::<Vec<_>>()
    );
    let killed = common::runs(&dir, RUN);
    assert_eq!(killed.len(), 1, "the killed run left {killed:?}");
    common::ok(render(&dir, 1, true, "640x360", "30"));
    let after = common::runs(&dir, RUN);
    assert_eq!(after.len(), 1, "{after:?}");
    assert_ne!(after, killed, "the next run kept the killed run's folder");
    assert!(!killed[0].exists());
}

#[test]
fn a_chromium_that_cannot_start_fails_at_once_with_its_stderr() {
    let dir = caller("nostart");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("chromium");
    std::fs::write(
        &fake,
        "#!/bin/sh\necho 'fake chromium: no profile for you' >&2\nexit 1\n",
    )
    .unwrap();
    Command::new("chmod")
        .args(["+x"])
        .arg(&fake)
        .status()
        .unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let started = Instant::now();
    let o = render(&dir, 1, true, "640x360", "30")
        .env("PATH", path)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "prec rendered without chromium");
    assert!(
        err.contains("fake chromium: no profile for you"),
        "prec hid chromium's stderr: {err}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "prec took {:?} to see chromium fail",
        started.elapsed()
    );
}

#[test]
fn a_caller_tree_with_a_space_and_a_hash_renders() {
    let dir = caller("my tree #1");
    common::ok(render(&dir, 3, true, "640x360", "30"));
}

#[test]
fn chromium_runs_with_prec_flags_only_inside_its_scope() {
    let dir = caller("scope");
    let mut child = spawn(&dir, false);
    frames(&dir, &mut child, 10);
    let procs = of_profile(&dir);
    child.kill().unwrap();
    child.wait().unwrap();
    let left = gone_within(&dir, Duration::from_secs(10));
    sweep(&dir);
    assert!(left.is_empty(), "chromium outlived prec");
    let data_dir = profile(&common::runs(&dir, RUN)[0]);
    let browsers: Vec<&Proc> = procs
        .iter()
        .filter(|p| !p.args.iter().any(|a| a.starts_with("--type=")))
        .collect();
    assert_eq!(browsers.len(), 1, "one browser process");
    for a in &browsers[0].args[1..] {
        assert!(
            FLAGS.contains(&a.as_str())
                || *a == data_dir
                || HEADLESS
                    .iter()
                    .any(|h| a == h || (h.ends_with('=') && a.starts_with(h))),
            "chromium carries {a}, which prec did not pass"
        );
    }
    let scope = &procs[0].cgroup;
    assert!(
        scope.contains("/build.slice/"),
        "chromium runs outside build.slice: {scope}"
    );
    for p in &procs {
        assert_eq!(
            &p.cgroup,
            scope,
            "{} left prec's scope",
            p.args.first().cloned().unwrap_or_default()
        );
    }
}
