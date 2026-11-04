mod common;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const TONE_HZ: f64 = 440.0;
const TONE_SECONDS: f64 = 1.5;
const START_TOLERANCE: f64 = 0.5;

const TOOLS: [&str; 8] = [
    "pipewire",
    "wireplumber",
    "pipewire-pulse",
    "pw-cat",
    "pw-record",
    "pw-dump",
    "paplay",
    "pactl",
];

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| {
            std::fs::metadata(d.join(tool))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

fn missing_tool() -> Option<&'static str> {
    TOOLS.into_iter().find(|t| !on_path(t))
}

fn wav(samples: &[i16]) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 0x0002_0001, 48_000, 192_000, 0x0010_0004] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn tone(path: &Path) {
    let n = (48_000.0 * TONE_SECONDS) as usize;
    let samples: Vec<i16> = (0..n)
        .flat_map(|i| {
            let v = (9000.0 * (2.0 * std::f64::consts::PI * TONE_HZ * i as f64 / 48_000.0).sin())
                as i16;
            [v, v]
        })
        .collect();
    std::fs::write(path, wav(&samples)).unwrap();
}

struct Heard {
    rate: u32,
    channels: u16,
    left: Vec<i16>,
}

fn read_wav(path: &Path) -> Heard {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[..4], b"RIFF");
    assert_eq!(&b[8..16], b"WAVEfmt ");
    let channels = u16::from_le_bytes(b[22..24].try_into().unwrap());
    let rate = u32::from_le_bytes(b[24..28].try_into().unwrap());
    assert_eq!(u16::from_le_bytes(b[34..36].try_into().unwrap()), 16);
    assert_eq!(&b[36..40], b"data");
    let len = u32::from_le_bytes(b[40..44].try_into().unwrap()) as usize;
    assert_eq!(b.len(), 44 + len);
    let left = b[44..]
        .chunks_exact(2 * channels as usize)
        .map(|f| i16::from_le_bytes([f[0], f[1]]))
        .collect();
    Heard {
        rate,
        channels,
        left,
    }
}

fn hertz(h: &Heard) -> f64 {
    let from = (h.rate as f64 * 0.25) as usize;
    let to = (h.rate as f64 * 1.25) as usize;
    let window = &h.left[from..to];
    let crossings = window.windows(2).filter(|w| w[0] <= 0 && w[1] > 0).count();
    crossings as f64 * h.rate as f64 / window.len() as f64
}

fn app(dir: &Path, hooks: &str, tone_file: &Path) -> PathBuf {
    let tone_file = tone_file.display();
    let dump = dir.join("dump.json");
    let info = dir.join("info.txt");
    let app = common::demo_app(dir, hooks);
    let bin = app.join("render/terminal/bin/demo");
    std::fs::write(
        &bin,
        format!(
            "#!/usr/bin/env bash\ncase \"$1\" in\n  tone) printf 'tone-start\\n'; pw-cat -p '{tone_file}' ;;\n  pulse) printf 'tone-start\\n'; paplay '{tone_file}' ;;\n  quiet) printf 'quiet\\n'; sleep 1 ;;\n  later) printf 'tone-start\\n'; sleep 1; pw-cat -p '{tone_file}'; pw-cat -p '{tone_file}' ;;\n  graph) pw-dump --no-colors > '{}'; pactl info > '{}' ;;\nesac\n",
            dump.display(),
            info.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    common::add_shot(
        &app,
        "tone",
        "Sleep 200ms\nType \"demo tone\"\nEnter\nSleep 3s\n",
        "",
    );
    common::add_shot(
        &app,
        "pulse",
        "Sleep 200ms\nType \"demo pulse\"\nEnter\nSleep 3s\n",
        "",
    );
    common::add_shot(
        &app,
        "graph",
        "Sleep 200ms\nType \"demo graph\"\nEnter\nSleep 2s\n",
        "",
    );
    common::add_shot(
        &app,
        "quiet",
        "Sleep 200ms\nType \"demo quiet\"\nEnter\nSleep 2s\n",
        "",
    );
    common::add_shot(
        &app,
        "later",
        "Sleep 200ms\nType \"demo later\"\nEnter\nSleep 5s\n",
        "",
    );
    app
}

fn command(dir: &Path, app: &Path, shot: &str) -> Command {
    let mut c = common::prec(
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
    );
    c.env_remove("PREC_AUDIO")
        .env("PULSE_SERVER", "unix:/nonexistent/pulse")
        .env("PIPEWIRE_REMOTE", "/nonexistent/pipewire")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus");
    c
}

fn record(dir: &Path, app: &Path, shot: &str) -> std::process::Output {
    command(dir, app, shot).output().unwrap()
}

fn cast_header(dir: &Path, shot: &str) -> Value {
    let text =
        std::fs::read_to_string(dir.join(format!("tmp/out/demo/clip/demo-clip-{shot}.cast")))
            .unwrap();
    serde_json::from_str(text.lines().next().unwrap()).unwrap()
}

fn marker_time(dir: &Path, shot: &str, needle: &str) -> f64 {
    let text =
        std::fs::read_to_string(dir.join(format!("tmp/out/demo/clip/demo-clip-{shot}.cast")))
            .unwrap();
    text.lines()
        .skip(1)
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|e| e[1] == "o" && e[2].as_str().unwrap().contains(needle))
        .map(|e| e[0].as_f64().unwrap())
        .unwrap()
}

fn setup(name: &str) -> Option<(PathBuf, PathBuf)> {
    if let Some(tool) = missing_tool() {
        eprintln!("skipped: {tool} is not on PATH, so the private sound cannot run");
        return None;
    }
    let dir = common::caller("au", name);
    let tone_file = dir.join("t.wav");
    tone(&tone_file);
    Some((dir, tone_file))
}

#[test]
fn a_recorded_app_that_plays_a_tone_leaves_a_wav_and_a_cast_header_that_dates_it() {
    let Some((dir, tone_file)) = setup("t") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    let o = record(&dir, &app, "tone");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let header = cast_header(&dir, "tone");
    let audio = &header["x_terminal_renderer"]["audio"];
    assert_eq!(audio["file"], "demo-clip-tone.wav");
    assert_eq!(audio["rate"], 48_000);
    assert_eq!(audio["channels"], 2);
    let wav_path = dir.join("tmp/out/demo/clip/demo-clip-tone.wav");
    assert_eq!(
        audio["sha256"].as_str().unwrap(),
        format!("{:x}", Sha256::digest(std::fs::read(&wav_path).unwrap()))
    );
    let heard = read_wav(&wav_path);
    assert_eq!((heard.rate, heard.channels), (48_000, 2));
    let seconds = heard.left.len() as f64 / 48_000.0;
    assert!(
        (TONE_SECONDS - 0.2..=TONE_SECONDS + 0.2).contains(&seconds),
        "{seconds}s of sound"
    );
    let found = hertz(&heard);
    assert!((found - TONE_HZ).abs() < 2.0, "{found} Hz");
    assert!(heard.left.iter().map(|s| s.unsigned_abs()).max().unwrap() > 4000);
    let started = marker_time(&dir, "tone", "tone-start");
    let delta = audio["start"].as_f64().unwrap() - started;
    eprintln!("measured start offset after the app's first line: {delta:.3}s");
    assert!(
        (0.0..START_TOLERANCE).contains(&delta),
        "start is {delta}s after the app's first line, tolerance {START_TOLERANCE}s"
    );
    let env = serde_json::to_string(&header["env"]).unwrap();
    let meta_env = serde_json::to_string(&header["x_terminal_renderer"]["env"]).unwrap();
    let passed = serde_json::to_string(&header["x_terminal_renderer"]["pass_env"]).unwrap();
    for text in [env, meta_env, passed] {
        assert!(!text.contains("XDG_RUNTIME_DIR"), "{text}");
    }
    assert!(
        !String::from_utf8_lossy(
            &std::fs::read(dir.join("tmp/out/demo/clip/demo-clip-tone.cast")).unwrap()
        )
        .contains("/snd/x")
    );
}

#[test]
fn a_silent_app_leaves_no_wav_and_no_audio_key_and_a_stale_wav_is_removed() {
    let Some((dir, tone_file)) = setup("q") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    let out = dir.join("tmp/out/demo/clip");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("demo-clip-quiet.wav"), "old").unwrap();
    let o = record(&dir, &app, "quiet");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(
        cast_header(&dir, "quiet")["x_terminal_renderer"]
            .get("audio")
            .is_none()
    );
    assert!(!out.join("demo-clip-quiet.wav").exists());
}

#[test]
fn a_stream_opened_late_and_reopened_is_followed_into_one_wav() {
    let Some((dir, tone_file)) = setup("l") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    let o = record(&dir, &app, "later");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let header = cast_header(&dir, "later");
    let audio = &header["x_terminal_renderer"]["audio"];
    let heard = read_wav(&dir.join("tmp/out/demo/clip/demo-clip-later.wav"));
    let seconds = heard.left.len() as f64 / 48_000.0;
    assert!(
        (2.0 * TONE_SECONDS - 0.3..=2.0 * TONE_SECONDS + 0.5).contains(&seconds),
        "{seconds}s of sound"
    );
    let started = marker_time(&dir, "later", "tone-start");
    let delta = audio["start"].as_f64().unwrap() - started;
    assert!((1.0..1.0 + START_TOLERANCE).contains(&delta), "{delta}");
}

#[test]
fn a_session_with_a_home_namespace_still_reaches_the_private_sockets() {
    let Some((dir, tone_file)) = setup("n") else {
        return;
    };
    let app = app(
        &dir,
        "home = \"/home/prec-alpha\"\nhostname = \"beta-box\"\n",
        &tone_file,
    );
    let o = record(&dir, &app, "tone");
    if !o.status.success() {
        let said = String::from_utf8_lossy(&o.stderr);
        if said.contains("namespace") {
            eprintln!("skipped: user namespaces cannot be made here: {said}");
            return;
        }
        panic!("{said}");
    }
    let header = cast_header(&dir, "tone");
    let heard = read_wav(&dir.join("tmp/out/demo/clip/demo-clip-tone.wav"));
    assert!((hertz(&heard) - TONE_HZ).abs() < 2.0);
    assert_eq!(header["x_terminal_renderer"]["audio"]["rate"], 48_000);
}

#[test]
fn an_interrupt_stops_and_reaps_every_sound_process() {
    sound_stops_when_prec_gets("i", "TERM", Some(143));
}

#[test]
fn a_killed_prec_leaves_no_sound_process_behind() {
    sound_stops_when_prec_gets("k", "KILL", None);
}

fn sound_stops_when_prec_gets(tag: &str, signal: &str, code: Option<i32>) {
    let Some((dir, tone_file)) = setup(tag) else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    common::add_shot(
        &app,
        "long",
        "Sleep 200ms\nType \"demo later\"\nEnter\nSleep 600s\n",
        "",
    );
    let mut child = common::spawn(command(&dir, &app, "long"), &dir);
    let suffix = format!("/sockets/p{}-0/x", child.id());
    let near = dir.to_string_lossy().into_owned();
    let mine = |p: &common::Proc| {
        p.env.iter().any(|e| {
            e.strip_prefix("XDG_RUNTIME_DIR=").is_some_and(|v| {
                v.ends_with(&suffix) || (v.starts_with(&near) && v.ends_with("/x"))
            })
        })
    };
    let named =
        |p: &common::Proc, name: &str| mine(p) && p.args.first().is_some_and(|a| a.ends_with(name));
    let running = |name: &str| common::procs().iter().any(|p| named(p, name));
    common::wait_for("a capture of the app's stream", &dir, 60, || {
        running("pw-record")
    });
    for daemon in ["pipewire", "wireplumber", "pipewire-pulse", "pw-dump"] {
        assert!(running(daemon), "{daemon} is not running");
    }
    assert!(common::signal(child.id(), signal));
    let status = common::exits(&mut child, &dir);
    assert_eq!(status.code(), code, "{}", common::said(&dir));
    let left = common::gone(Duration::from_secs(15), || {
        common::procs().into_iter().filter(|p| mine(p)).collect()
    });
    assert!(
        left.is_empty(),
        "sound processes outlived prec: {:?}",
        left.iter().map(|p| (p.pid, &p.args)).collect::<Vec<_>>()
    );
}

#[test]
fn the_private_graph_has_the_null_sink_only_and_no_device() {
    let Some((dir, tone_file)) = setup("g") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    let o = record(&dir, &app, "graph");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let dump: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("dump.json")).unwrap()).unwrap();
    let objects = dump.as_array().unwrap();
    let kind = |o: &Value| o["type"].as_str().unwrap_or_default().to_string();
    assert!(
        objects.iter().all(|o| !kind(o).ends_with(":Device")),
        "a device exists"
    );
    let nodes: Vec<&Value> = objects
        .iter()
        .filter(|o| kind(o).ends_with(":Node"))
        .collect();
    fn class(n: &Value) -> &str {
        n["info"]["props"]["media.class"]
            .as_str()
            .unwrap_or_default()
    }

    let sinks: Vec<&str> = nodes
        .iter()
        .filter(|n| class(n).contains("Sink"))
        .map(|n| n["info"]["props"]["node.name"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(sinks, ["null-sink"]);
    for n in &nodes {
        assert!(
            !class(n).contains("Source") && !class(n).starts_with("Video"),
            "{}",
            class(n)
        );
        let props = n["info"]["props"].as_object().unwrap();
        assert!(
            props
                .keys()
                .all(|k| !k.starts_with("api.") && !k.starts_with("device.")),
            "{props:?}"
        );
    }
    let info = std::fs::read_to_string(dir.join("info.txt")).unwrap();
    assert!(info.contains("Default Sink: null-sink"), "{info}");
}

#[test]
fn a_pulse_client_is_heard_through_the_private_pulse_server() {
    let Some((dir, tone_file)) = setup("p") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    let o = record(&dir, &app, "pulse");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let heard = read_wav(&dir.join("tmp/out/demo/clip/demo-clip-pulse.wav"));
    assert!((hertz(&heard) - TONE_HZ).abs() < 2.0);
    assert!(heard.left.iter().map(|s| s.unsigned_abs()).max().unwrap() > 4000);
}

#[test]
fn a_daemon_that_fails_leaves_a_silent_recording_and_one_line() {
    let Some((dir, tone_file)) = setup("f") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    let stub = dir.join("stub");
    std::fs::create_dir_all(&stub).unwrap();
    std::fs::write(stub.join("pipewire"), "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(
        stub.join("pipewire"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let mut path = vec![stub];
    path.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let mut c = command(&dir, &app, "tone");
    c.env("PATH", std::env::join_paths(path).unwrap());
    let o = c.output().unwrap();
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{said}");
    assert_eq!(
        said.lines()
            .filter(|l| l.starts_with("prec: audio:"))
            .count(),
        1,
        "{said}"
    );
    assert!(
        cast_header(&dir, "tone")["x_terminal_renderer"]
            .get("audio")
            .is_none()
    );
    assert!(!dir.join("tmp/out/demo/clip/demo-clip-tone.wav").exists());
}
