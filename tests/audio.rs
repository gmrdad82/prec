mod common;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const TONE_HZ: f64 = 440.0;
const TONE_SECONDS: f64 = 1.5;
const START_TOLERANCE: f64 = 0.5;

const TOOLS: [&str; 5] = ["pipewire", "wireplumber", "pw-cat", "pw-record", "pw-dump"];

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

struct Daemons {
    xdg: PathBuf,
    home: PathBuf,
    kids: Vec<Child>,
}

impl Daemons {
    fn env(&self, c: &mut Command) {
        c.env("XDG_RUNTIME_DIR", &self.xdg)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_DATA_HOME", self.home.join(".data"))
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove("PIPEWIRE_REMOTE")
            .env_remove("PULSE_SERVER");
    }

    fn daemon(&self, tool: &str, dir: &Path) -> Child {
        let mut c = Command::new("prlimit");
        c.args(["--core=1:1", "--", "timeout", "300", tool])
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.env(&mut c);
        c.spawn().unwrap()
    }

    fn start(dir: &Path) -> Daemons {
        let xdg = dir.join("x");
        let home = dir.join("h");
        let pw = home.join(".config/pipewire/pipewire.conf.d");
        let wp = home.join(".config/wireplumber/wireplumber.conf.d");
        for d in [&xdg, &pw, &wp] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::set_permissions(&xdg, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            pw.join("10-null.conf"),
            "context.objects = [\n  { factory = adapter\n    args = { factory.name = support.null-audio-sink node.name = null-sink media.class = Audio/Sink audio.position = [ FL FR ] object.linger = true }\n  }\n]\n",
        )
        .unwrap();
        std::fs::write(
            wp.join("10-off.conf"),
            "wireplumber.profiles = {\n  main = {\n    monitor.alsa = disabled\n    monitor.alsa-midi = disabled\n    monitor.bluez = disabled\n    monitor.bluez-midi = disabled\n    monitor.libcamera = disabled\n    monitor.v4l2 = disabled\n    support.logind = disabled\n    support.reserve-device = disabled\n  }\n}\n",
        )
        .unwrap();
        let mut d = Daemons {
            xdg,
            home,
            kids: vec![],
        };
        let first = d.daemon("pipewire", dir);
        d.kids.push(first);
        assert!(
            common::until(Duration::from_secs(30), || d
                .xdg
                .join("pipewire-0")
                .exists()),
            "pipewire made no socket"
        );
        let second = d.daemon("wireplumber", dir);
        d.kids.push(second);
        assert!(
            common::until(Duration::from_secs(30), || d
                .dump()
                .contains("default.audio.sink")),
            "wireplumber never named a default sink"
        );
        d
    }

    fn dump(&self) -> String {
        let mut c = Command::new("pw-dump");
        c.arg("--no-colors");
        self.env(&mut c);
        String::from_utf8_lossy(&c.output().unwrap().stdout).into_owned()
    }
}

impl Drop for Daemons {
    fn drop(&mut self) {
        for k in self.kids.iter_mut().rev() {
            common::signal(k.id(), "TERM");
            let _ = k.wait();
        }
    }
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
    let app = common::demo_app(dir, hooks);
    let bin = app.join("render/terminal/bin/demo");
    std::fs::write(
        &bin,
        format!(
            "#!/usr/bin/env bash\ncase \"$1\" in\n  tone) printf 'tone-start\\n'; pw-cat -p '{}' ;;\n  quiet) printf 'quiet\\n'; sleep 1 ;;\n  later) printf 'tone-start\\n'; sleep 1; pw-cat -p '{}'; pw-cat -p '{}' ;;\nesac\n",
            tone_file.display(),
            tone_file.display(),
            tone_file.display()
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

fn record(dir: &Path, app: &Path, shot: &str, daemons: &Daemons) -> std::process::Output {
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
    c.env("XDG_CACHE_HOME", dir.join("cache"));
    c.env("XDG_RUNTIME_DIR", &daemons.xdg);
    c.output().unwrap()
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

fn setup(name: &str) -> Option<(PathBuf, Daemons, PathBuf)> {
    if let Some(tool) = missing_tool() {
        eprintln!("skipped: {tool} is not on PATH, so no private audio daemon can run");
        return None;
    }
    let dir = common::caller("au", name);
    let daemons = Daemons::start(&dir);
    let tone_file = dir.join("t.wav");
    tone(&tone_file);
    Some((dir, daemons, tone_file))
}

#[test]
fn a_recorded_app_that_plays_a_tone_leaves_a_wav_and_a_cast_header_that_dates_it() {
    let Some((dir, daemons, tone_file)) = setup("t") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    let o = record(&dir, &app, "tone", &daemons);
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
        .contains(daemons.xdg.to_str().unwrap())
    );
}

#[test]
fn a_silent_app_leaves_no_wav_and_no_audio_key_and_a_stale_wav_is_removed() {
    let Some((dir, daemons, tone_file)) = setup("q") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    let out = dir.join("tmp/out/demo/clip");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("demo-clip-quiet.wav"), "old").unwrap();
    let o = record(&dir, &app, "quiet", &daemons);
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
    let Some((dir, daemons, tone_file)) = setup("l") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    let o = record(&dir, &app, "later", &daemons);
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
fn a_session_with_a_home_namespace_still_reaches_the_sockets() {
    let Some((dir, daemons, tone_file)) = setup("n") else {
        return;
    };
    let app = app(
        &dir,
        "home = \"/home/prec-alpha\"\nhostname = \"beta-box\"\n",
        &tone_file,
    );
    let o = record(&dir, &app, "tone", &daemons);
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
fn an_interrupt_stops_and_reaps_every_capture_process() {
    let Some((dir, daemons, tone_file)) = setup("i") else {
        return;
    };
    let app = app(&dir, "", &tone_file);
    common::add_shot(
        &app,
        "long",
        "Sleep 200ms\nType \"demo later\"\nEnter\nSleep 600s\n",
        "",
    );
    let mut c = common::prec(
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
    c.env("XDG_CACHE_HOME", dir.join("cache"));
    c.env("XDG_RUNTIME_DIR", &daemons.xdg);
    let mut child = common::spawn(c, &dir);
    let mine = |p: &common::Proc| {
        p.args
            .first()
            .is_some_and(|a| a.ends_with("pw-record") || a.ends_with("pw-dump"))
            && p.env
                .iter()
                .any(|e| e == &format!("XDG_RUNTIME_DIR={}", daemons.xdg.display()))
    };
    let captures = |name: &str| {
        common::procs()
            .iter()
            .any(|p| mine(p) && p.args.first().is_some_and(|a| a.ends_with(name)))
    };
    common::wait_for("a capture of the app's stream", &dir, 60, || {
        captures("pw-record")
    });
    assert!(captures("pw-dump"));
    assert!(common::signal(child.id(), "TERM"));
    let status = common::exits(&mut child, &dir);
    assert_eq!(status.code(), Some(143), "{}", common::said(&dir));
    let left = common::gone(Duration::from_secs(15), || {
        common::procs().into_iter().filter(|p| mine(p)).collect()
    });
    assert!(
        left.is_empty(),
        "capture processes outlived prec: {:?}",
        left.iter().map(|p| (p.pid, &p.args)).collect::<Vec<_>>()
    );
}
