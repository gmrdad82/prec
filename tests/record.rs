mod common;

use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

fn case(name: &str, hooks: &str) -> (PathBuf, PathBuf) {
    let dir = common::caller("record", name);
    let app = common::demo_app(&dir, hooks);
    (dir, app)
}

fn prec(dir: &Path, args: &[&str]) -> Command {
    let mut c = common::prec(dir, &dir.join("archive"), args);
    c.env("XDG_CACHE_HOME", dir.join("cache"));
    c
}

fn record(dir: &Path, app: &Path, shot: &str) -> Output {
    prec(
        dir,
        &[
            "record",
            "demo",
            shot,
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            "tmp/out",
        ],
    )
    .output()
    .unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn cast_of(dir: &Path, shot: &str) -> PathBuf {
    dir.join(format!("tmp/out/demo/clip/demo-clip-{shot}.cast"))
}

fn refused(o: &Output) -> String {
    assert_eq!(o.status.code(), Some(1), "{}", stderr(o));
    let said = stderr(o);
    assert!(said.contains("prec: "), "{said}");
    said
}

fn gone(pid: i32) -> bool {
    common::until(Duration::from_secs(5), || {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err()
    })
}

#[test]
fn a_recorded_shot_is_an_asciicast_v2_cast_its_header_and_events_read_back() {
    let (dir, app) = case("cast", "");
    common::add_shot(
        &app,
        "short",
        "Sleep 100ms\nType \"demo tick\"\nEnter\nSleep 1.5s\n",
        "",
    );
    let o = record(&dir, &app, "short");
    common::expect_ok(&o, "prec record");
    let text = std::fs::read_to_string(cast_of(&dir, "short")).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect();
    let header = &lines[0];
    assert_eq!(header["version"], 2);
    assert_eq!(header["width"], 60);
    assert_eq!(header["height"], 16);
    let meta = &header["x_terminal_renderer"];
    assert_eq!(meta["product"], "demo");
    assert_eq!(meta["shot"], "short");
    assert_eq!(meta["kind"], "clip");
    assert_eq!(meta["grid"], serde_json::json!([60, 16]));
    let events = &lines[1..];
    assert!(!events.is_empty());
    let mut last = 0.0;
    for e in events {
        let a = e.as_array().unwrap_or_else(|| panic!("{e}"));
        assert_eq!(a.len(), 3, "{e}");
        let t = a[0].as_f64().unwrap();
        assert!(t >= last, "events out of order at {e}");
        last = t;
        assert!(matches!(a[1].as_str(), Some("o" | "i" | "m")), "{e}");
        assert!(a[2].is_string(), "{e}");
    }
    let of = |code: &str| -> Vec<&str> {
        events
            .iter()
            .filter(|e| e[1] == code)
            .map(|e| e[2].as_str().unwrap())
            .collect()
    };
    let markers = of("m");
    assert_eq!(markers.first(), Some(&"hide"));
    assert!(markers.contains(&"show"), "{markers:?}");
    assert_eq!(events.last().unwrap()[1], "m");
    assert_eq!(events.last().unwrap()[2], "end");
    let typed: String = of("i").concat();
    assert!(typed.ends_with("demo tick\r"), "{typed:?}");
    let shown: String = of("o").concat();
    assert!(shown.contains('▇'), "{shown:?}");
    assert!(
        (1.5..30.0).contains(&last),
        "a {last}s cast for a 1.6s tape"
    );
}

fn shown_pid(said: &str, label: &str) -> i32 {
    regex::Regex::new(&format!(r"{label}=(\d+)"))
        .unwrap()
        .captures_iter(said)
        .find_map(|c| c[1].parse().ok())
        .unwrap_or_else(|| panic!("no {label} pid on the screen:\n{said}"))
}

#[test]
fn a_wait_that_never_matches_fails_at_its_timeout_and_leaves_no_shell_or_app() {
    let (dir, app) = case("wait", "");
    common::add_shot(
        &app,
        "stuck",
        "Type@0ms \"echo shell=$$\"\nEnter\nType@0ms `sh -c 'trap \"\" HUP; echo app=$$; exec sleep 600'`\nEnter\nWait@3s /never-shown/\nType \"demo show\"\n",
        "",
    );
    let started = std::time::Instant::now();
    let o = record(&dir, &app, "stuck");
    let took = started.elapsed();
    let said = refused(&o);
    assert!(
        said.contains("prec: Wait /never-shown/ timed out after 3.0s; the screen showed:\n"),
        "{said}"
    );
    assert!(
        took >= Duration::from_secs(3),
        "the 3s Wait failed after {took:?}"
    );
    let shell = shown_pid(&said, "shell");
    let held = shown_pid(&said, "app");
    assert_ne!(shell, held);
    assert!(gone(shell), "the recording's shell {shell} outlived prec");
    assert!(
        gone(held),
        "the app {held} that ignores SIGHUP outlived prec's recording"
    );
    assert!(!cast_of(&dir, "stuck").exists());
    assert!(!said.contains("recorded "), "{said}");
}

#[test]
fn a_tape_that_ends_with_an_app_in_the_foreground_keeps_its_cast_and_leaves_no_app() {
    let (dir, app) = case("foreground", "");
    common::add_shot(
        &app,
        "held",
        "Type@0ms \"echo shell=$$\"\nEnter\nType@0ms `sh -c 'trap \"\" HUP; echo app=$$; exec sleep 600'`\nEnter\nWait+Screen@20s /app=[0-9]+/\n",
        "",
    );
    let o = record(&dir, &app, "held");
    common::expect_ok(&o, "prec record");
    let text = std::fs::read_to_string(cast_of(&dir, "held")).unwrap();
    assert!(text.trim_end().ends_with(r#""m","end"]"#), "{text}");
    let shown: String = text
        .lines()
        .skip(1)
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|e| e[1] == "o")
        .map(|e| e[2].as_str().unwrap().to_string())
        .collect();
    let shell = shown_pid(&shown, "shell");
    let held = shown_pid(&shown, "app");
    assert_ne!(shell, held);
    assert!(gone(shell), "the recording's shell {shell} outlived prec");
    assert!(
        gone(held),
        "the app {held} that ignores SIGHUP outlived prec's recording"
    );
}

fn cast_events(dir: &Path, shot: &str) -> Vec<Value> {
    std::fs::read_to_string(cast_of(dir, shot))
        .unwrap()
        .lines()
        .skip(1)
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

#[test]
fn launch_hide_show_screenshot_squeeze_and_idle_leave_their_marks_in_the_cast() {
    let (dir, app) = case("steps", "");
    common::add_shot(
        &app,
        "steps",
        "Type@0ms \"echo one\"\nEnter\nHide\nType@0ms \"echo hidden\"\nEnter\nShow\nScreenshot\nSqueeze 300ms\nSleep 1s\nSqueeze off\nIdle 300ms\n",
        "launch = \"echo launched-$((6*7))\"\nsettle = \"400ms\"\n",
    );
    let o = record(&dir, &app, "steps");
    common::expect_ok(&o, "prec record");
    let events = cast_events(&dir, "steps");
    let marks: Vec<(f64, &str)> = events
        .iter()
        .filter(|e| e[1] == "m" && e[2] != "cut" && e[2] != "resume")
        .map(|e| (e[0].as_f64().unwrap(), e[2].as_str().unwrap()))
        .collect();
    let names: Vec<&str> = marks.iter().map(|(_, m)| *m).collect();
    assert_eq!(
        names,
        [
            "hide",
            "show",
            "hide",
            "show",
            "screenshot",
            "squeeze 0.3",
            "hold",
            "release",
            "squeeze off",
            "end"
        ]
    );
    let at = |name: &str| marks.iter().rev().find(|(_, m)| *m == name).unwrap().0;
    assert!(at("release") - at("hold") >= 1.0, "{marks:?}");
    assert!(at("end") - at("squeeze off") >= 0.3, "{marks:?}");
    let first_show = marks[1].0;
    let launched = events
        .iter()
        .find(|e| e[1] == "o" && e[2].as_str().unwrap().contains("launched-42"))
        .unwrap_or_else(|| panic!("the launch never ran: {events:?}"));
    assert!(launched[0].as_f64().unwrap() < first_show, "{launched}");
    let typed = events
        .iter()
        .find(|e| e[1] == "i" && e[2] == "echo launched-$((6*7))\r")
        .unwrap_or_else(|| panic!("the launch was never typed: {events:?}"));
    assert!(
        first_show - typed[0].as_f64().unwrap() >= 0.4,
        "the 400ms settle after the launch was cut short: {marks:?}"
    );
}

#[test]
fn a_shot_that_needs_a_fixture_set_refuses_none_or_another_and_hands_the_right_one_to_the_session()
{
    let (dir, app) = case("fixtures", "");
    common::add_shot(
        &app,
        "fixed",
        "Type@0ms \"echo door=$PITO_DEMO_FIXTURES\"\nEnter\nWait+Screen /(?m)^door=/\n",
        "fixtures = \"alpha-set\"\n",
    );
    let with = |set: Option<&Path>| {
        let mut args = vec![
            "record".to_string(),
            "demo".into(),
            "fixed".into(),
            "--app-dir".into(),
            app.display().to_string(),
            "--out".into(),
            "tmp/out".into(),
        ];
        if let Some(set) = set {
            args.push("--fixtures".into());
            args.push(set.display().to_string());
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        prec(&dir, &args).output().unwrap()
    };
    let said = refused(&with(None));
    assert!(
        said.ends_with(
            "prec: fixed needs the alpha-set fixture set: pass --fixtures <materialized set>\n"
        ),
        "{said}"
    );
    let other = dir.join("sets/beta-set");
    std::fs::create_dir_all(&other).unwrap();
    let said = refused(&with(Some(&other)));
    assert!(
        said.ends_with("prec: fixed expects the alpha-set fixture set, not beta-set\n"),
        "{said}"
    );
    assert!(!cast_of(&dir, "fixed").exists());
    let set = dir.join("sets/alpha-set");
    std::fs::create_dir_all(set.join("deep")).unwrap();
    std::fs::write(set.join("a.txt"), "one\n").unwrap();
    std::fs::write(set.join("deep/b.txt"), "two\n").unwrap();
    common::expect_ok(&with(Some(&set)), "prec record --fixtures");
    let text = std::fs::read_to_string(cast_of(&dir, "fixed")).unwrap();
    let header: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    let fixtures = &header["x_terminal_renderer"]["fixtures"];
    assert_eq!(fixtures["path"], set.display().to_string(), "{fixtures}");
    assert_eq!(fixtures["door"], "PITO_DEMO_FIXTURES", "{fixtures}");
    let files: Vec<(&str, &str)> = fixtures["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["file"].as_str().unwrap(), f["sha256"].as_str().unwrap()))
        .collect();
    assert_eq!(
        files,
        [
            (
                "a.txt",
                "2c8b08da5ce60398e1f19af0e5dccc744df274b826abe585eaba68c525434806"
            ),
            (
                "deep/b.txt",
                "27dd8ed44a83ff94d557f9fd0412ed5a8cbca69ea04922d88c01184a07300a5a"
            )
        ]
    );
    let shown: String = cast_events(&dir, "fixed")
        .iter()
        .filter(|e| e[1] == "o")
        .map(|e| e[2].as_str().unwrap().to_string())
        .collect();
    assert!(
        shown.contains(&format!("door={}", set.display())),
        "{shown:?}"
    );
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args([
            "-c",
            "user.name=alpha",
            "-c",
            "user.email=",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgSign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", stderr(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn a_pinned_recording_builds_its_commit_once_from_the_cache_and_ignores_the_working_tree() {
    let dir = common::caller("record", "pinned");
    let app = common::demo_app(&dir, "");
    std::fs::rename(app.join("render"), dir.join("render")).unwrap();
    let toml = dir.join("render/terminal.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    let prebuilt = "[build]\nbin_dir = \"{app}/render/terminal/bin\"\n";
    assert!(text.contains(prebuilt), "{text}");
    std::fs::write(
        &toml,
        text.replace(
            prebuilt,
            "[build]\ncommand = \"mkdir -p {target}/out && cp render/terminal/bin/demo {target}/out/demo\"\nbin_dir = \"{target}/out\"\n",
        ),
    )
    .unwrap();
    let tape = "Type@0ms \"echo pinned-$((40+1))\"\nEnter\nWait+Screen /(?m)^pinned-4\\d$/\n";
    common::add_shot(&dir, "pinned", tape, "");
    git(&dir, &["add", "render"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    git(&dir, &["tag", "v9"]);
    let commit = git(&dir, &["rev-parse", "HEAD"]);
    std::fs::write(
        dir.join("render/terminal/pinned.tape"),
        tape.replace("40+1", "40+2"),
    )
    .unwrap();
    let pinned = |rev: &str| {
        prec(
            &dir,
            &["record", "demo", "pinned", "--rev", rev, "--out", "tmp/out"],
        )
        .output()
        .unwrap()
    };
    let said = refused(&pinned("nope"));
    assert!(
        said.ends_with(&format!("prec: {} has no nope\n", dir.display())),
        "{said}"
    );
    let first = pinned("v9");
    common::expect_ok(&first, "prec record --rev v9");
    assert!(
        !stderr(&first).contains("using the cached build"),
        "{}",
        stderr(&first)
    );
    let text = std::fs::read_to_string(cast_of(&dir, "pinned")).unwrap();
    let header: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    let meta = &header["x_terminal_renderer"]["app"];
    assert_eq!(meta["commit"], commit.as_str(), "{meta}");
    assert_eq!(meta["rev"], "v9", "{meta}");
    assert_eq!(meta["tag"], "v9", "{meta}");
    assert_eq!(meta["pinned"], true, "{meta}");
    let bin_dir = PathBuf::from(meta["bin_dir"].as_str().unwrap());
    assert!(bin_dir.starts_with(dir.join("cache")), "{meta}");
    assert!(bin_dir.join("demo").is_file(), "{meta}");
    let shown: String = cast_events(&dir, "pinned")
        .iter()
        .filter(|e| e[1] == "o")
        .map(|e| e[2].as_str().unwrap().to_string())
        .collect();
    assert!(shown.contains("pinned-41"), "{shown:?}");
    assert!(!shown.contains("pinned-42"), "{shown:?}");
    let again = pinned("v9");
    common::expect_ok(&again, "prec record --rev v9 again");
    assert!(
        stderr(&again).contains(&format!("demo@{}: using the cached build", &commit[..12])),
        "{}",
        stderr(&again)
    );
}

#[test]
fn an_idle_that_never_settles_fails_at_its_timeout() {
    let (dir, app) = case("idle", "");
    common::add_shot(
        &app,
        "busy",
        "Type@0ms \"demo tick\"\nEnter\nIdle@500ms 5s\n",
        "",
    );
    let o = record(&dir, &app, "busy");
    let said = refused(&o);
    assert!(
        said.contains("prec: Idle 5.0s timed out after 0.5s; the screen kept changing:\n"),
        "{said}"
    );
    assert!(!cast_of(&dir, "busy").exists());
}

#[test]
fn a_malformed_tape_is_refused_by_its_line_before_anything_runs() {
    let (dir, app) = case("tape", "setup = \"echo up > {tmp}/setup.marker\"\n");
    common::add_shot(&app, "bad", "Type \"demo show\"\nSleep never\n", "");
    let o = record(&dir, &app, "bad");
    let said = refused(&o);
    assert!(
        said.contains("bad.tape: line 2: never: a duration like 500ms, 1.5s or 2"),
        "{said}"
    );
    assert!(!dir.join("tmp/setup.marker").exists());
    assert!(!cast_of(&dir, "bad").exists());
}

#[test]
fn a_session_that_exits_non_zero_is_still_recorded() {
    let (dir, app) = case("exits", "");
    common::add_shot(
        &app,
        "quits",
        "Type@0ms \"demo show; (exit 7); echo status=$?\"\nEnter\nWait+Screen /status=7/\nType@0ms \"exit 3\"\nEnter\nSleep 500ms\n",
        "",
    );
    let o = record(&dir, &app, "quits");
    common::expect_ok(&o, "prec record");
    let text = std::fs::read_to_string(cast_of(&dir, "quits")).unwrap();
    let shown: String = text
        .lines()
        .skip(1)
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|e| e[1] == "o")
        .map(|e| e[2].as_str().unwrap().to_string())
        .collect();
    assert!(shown.contains("status=7"), "{shown:?}");
    assert!(shown.contains("truecolor"), "{shown:?}");
    assert!(text.trim_end().ends_with(r#""m","end"]"#), "{text}");
    let events: Vec<Value> = text
        .lines()
        .skip(1)
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let quit = events
        .iter()
        .filter(|e| e[1] == "i" && e[2] == "\r")
        .map(|e| e[0].as_f64().unwrap())
        .next_back()
        .unwrap();
    let end = events.last().unwrap()[0].as_f64().unwrap();
    assert!(
        end - quit >= 0.5,
        "the cast ends {end}s, before the last Sleep after the shell quit at {quit}s"
    );
    assert!(
        !events
            .iter()
            .any(|e| e[1] == "i" && e[0].as_f64().unwrap() > quit),
        "a key was sent after the shell quit"
    );
}

#[test]
fn a_step_typed_after_the_shell_quits_is_refused() {
    let (dir, app) = case("typed-late", "");
    common::add_shot(
        &app,
        "late",
        "Type@0ms \"trap 'echo gone-$((40+2))' EXIT; exit 3\"\nEnter\nWait+Screen /gone-42/\nWait+Exit\nType@0ms \"echo late\"\nEnter\n",
        "",
    );
    let o = record(&dir, &app, "late");
    let said = refused(&o);
    assert!(
        said.contains("prec: writing to the terminal: the session has ended\n"),
        "{said}"
    );
    assert!(!cast_of(&dir, "late").exists());
}

#[test]
fn a_wait_exit_holds_the_tape_until_the_shell_quits_and_keeps_its_last_output() {
    let (dir, app) = case("wait-exit", "");
    common::add_shot(
        &app,
        "bye",
        "Type@0ms \"trap 'echo bye-$((40+3))' EXIT; exit 0\"\nEnter\nWait+Exit@20s\n",
        "",
    );
    let o = record(&dir, &app, "bye");
    common::expect_ok(&o, "prec record");
    let events = cast_events(&dir, "bye");
    assert_eq!(events.last().unwrap()[2], "end");
    let shown: String = events
        .iter()
        .filter(|e| e[1] == "o")
        .map(|e| e[2].as_str().unwrap().to_string())
        .collect();
    assert!(shown.contains("bye-43"), "{shown:?}");
    let quit = events
        .iter()
        .filter(|e| e[1] == "i")
        .map(|e| e[0].as_f64().unwrap())
        .next_back()
        .unwrap();
    let bye = events
        .iter()
        .filter(|e| e[1] == "o" && e[2].as_str().unwrap().contains("bye-43"))
        .map(|e| e[0].as_f64().unwrap())
        .next()
        .unwrap();
    assert!(
        bye >= quit,
        "bye-43 at {bye}s before the exit was typed at {quit}s"
    );
}

#[test]
fn a_wait_exit_on_a_shell_that_never_quits_is_refused_at_its_timeout() {
    let (dir, app) = case("wait-exit-stuck", "");
    common::add_shot(
        &app,
        "stays",
        "Type@0ms \"echo shell=$$\"\nEnter\nWait+Screen /(?m)^shell=/\nWait+Exit@1s\nType@0ms \"echo after\"\nEnter\n",
        "",
    );
    let o = record(&dir, &app, "stays");
    let said = refused(&o);
    assert!(
        said.contains(
            "prec: Wait+Exit timed out after 1.0s; the session was still running; the screen showed:\n"
        ),
        "{said}"
    );
    let shell = shown_pid(&said, "shell");
    assert!(gone(shell), "the recording's shell {shell} outlived prec");
    assert!(!said.contains("echo after"), "{said}");
    assert!(!cast_of(&dir, "stays").exists());
}

#[test]
fn a_failing_setup_stops_the_shot_and_the_teardown_still_runs() {
    let (dir, app) = case(
        "setup",
        "setup = \"exit 4\"\nteardown = \"echo down > {tmp}/teardown.marker\"\n",
    );
    common::add_shot(
        &app,
        "short",
        "Type \"demo show\"\nEnter\nSleep 300ms\n",
        "",
    );
    let o = record(&dir, &app, "short");
    let said = refused(&o);
    assert!(said.contains("prec: exit 4 failed"), "{said}");
    assert!(!said.contains("recording demo short"), "{said}");
    assert!(dir.join("tmp/teardown.marker").exists(), "{said}");
    assert!(!cast_of(&dir, "short").exists());
}

#[test]
fn a_failing_teardown_is_reported_and_the_cast_kept() {
    let (dir, app) = case("teardown", "teardown = \"exit 5\"\n");
    common::add_shot(
        &app,
        "short",
        "Type \"demo show\"\nEnter\nSleep 300ms\n",
        "",
    );
    let o = record(&dir, &app, "short");
    common::expect_ok(&o, "prec record");
    assert!(
        stderr(&o).contains("teardown: exit 5 failed"),
        "{}",
        stderr(&o)
    );
    assert!(cast_of(&dir, "short").exists());
}

fn render(dir: &Path, cast: &Path) -> Command {
    prec(
        dir,
        &[
            "render",
            cast.to_str().unwrap(),
            "--size",
            "320x180",
            "--fps",
            "30",
            "--no-encode",
            "--out",
            "tmp/out",
        ],
    )
}

#[test]
fn a_cast_that_is_empty_truncated_or_of_another_version_is_refused() {
    let dir = common::caller("record", "casts");
    let head = r#"{"version": 2, "width": 8, "height": 4, "x_terminal_renderer": {"product": "demo", "shot": "a", "kind": "clip"}}"#;
    let cases = [
        ("empty", String::new(), "empty.cast: an empty cast".to_string()),
        (
            "truncated",
            format!("{head}\n[0.0, \"m\", \"show\"]\n[0.5, \"o\", \"ab"),
            "truncated.cast: line 3: ".to_string(),
        ),
        (
            "v1",
            r#"{"version": 1, "width": 8, "height": 4}"#.to_string() + "\n",
            "v1.cast: an asciicast v2 file".to_string(),
        ),
        (
            "v3",
            r#"{"version": 3, "term": {"cols": 8, "rows": 4}}"#.to_string() + "\n",
            "v3.cast: an asciicast v2 file".to_string(),
        ),
        (
            "garbled",
            "{not json\n".to_string(),
            "garbled.cast: header: ".to_string(),
        ),
        (
            "bare",
            format!("{head}\n"),
            "bare.cast: a cast with no events".to_string(),
        ),
        (
            "timeless",
            format!("{head}\n[0.0, \"m\", \"show\"]\n[\"soon\", \"o\", \"a\"]\n"),
            "timeless.cast: line 3: a cast event without a time".to_string(),
        ),
        (
            "backwards",
            format!("{head}\n[0.5, \"m\", \"show\"]\n[0.2, \"o\", \"a\"]\n"),
            "backwards.cast: line 3: a time of 0.2s comes before the 0.5s above it".to_string(),
        ),
        (
            "rowless",
            r#"{"version": 2, "width": 8, "height": 0, "x_terminal_renderer": {"product": "demo", "shot": "a", "kind": "clip"}}"#.to_string()
                + "\n[0.0, \"m\", \"show\"]\n[0.5, \"m\", \"end\"]\n",
            "rowless.cast: header: height 0 is not a size from 1 to 65535".to_string(),
        ),
    ];
    for (name, text, want) in cases {
        let path = dir.join(format!("casts/{name}.cast"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        let o = render(&dir, &path).output().unwrap();
        let said = refused(&o);
        assert!(said.contains(&want), "{name}: {said}");
        assert!(common::no_chromium(&dir, "demo-clip-a-replay"), "{name}");
    }
    assert!(!dir.join("tmp/out").exists());
}

fn fake_chromium(dir: &Path, body: &str) -> PathBuf {
    let bin = dir.join("fake-chromium");
    std::fs::write(&bin, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

fn a_cast(dir: &Path) -> PathBuf {
    let path = dir.join("cast/a.cast");
    common::write_cast(
        &path,
        "demo",
        "a",
        "clip",
        &[(0.1, "hello".to_string())],
        0.5,
    );
    path
}

#[test]
fn a_chromium_that_dies_at_start_fails_the_render_with_what_it_said() {
    let dir = common::caller("record", "chromium-dies");
    let bin = fake_chromium(&dir, "echo 'no display here' >&2\nexit 1");
    let mut c = render(&dir, &a_cast(&dir));
    c.env("PREC_CHROMIUM", &bin);
    let o = c.output().unwrap();
    let said = refused(&o);
    assert!(
        said.contains("did not start: chromium exited during Browser.getVersion ("),
        "{said}"
    );
    assert!(said.contains("chromium said:\nno display here"), "{said}");
}

#[test]
fn a_chromium_that_never_answers_times_out_and_is_killed() {
    let dir = common::caller("record", "chromium-silent");
    let pid_file = dir.join("fake.pid");
    let bin = fake_chromium(
        &dir,
        &format!("echo $$ > '{}'\nexec sleep 600", pid_file.display()),
    );
    let mut c = render(&dir, &a_cast(&dir));
    c.env("PREC_CHROMIUM", &bin);
    let o = c.output().unwrap();
    let said = refused(&o);
    assert!(
        said.contains("did not start: chromium did not answer Browser.getVersion within 60 s"),
        "{said}"
    );
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(gone(pid), "the silent chromium {pid} outlived prec");
}
