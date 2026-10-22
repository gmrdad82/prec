mod common;

use common::{paint, tree, video};
use pito_codecs::h264::{Encoder, EncoderConfig, Preset, Profile};
use pito_codecs::media::{Bitstream, Frame, PixelFormat, Rate, TimeBase};
use pito_codecs::mp4::{AvcConfig, SampleEntry, TrackConfig, Writer};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};

const N: usize = 24;
const SIZE: [u32; 2] = [320, 180];
const LEVELS: [u8; 4] = [32, 96, 160, 224];

fn work(name: &str) -> PathBuf {
    common::scratch("frames", name)
}

fn run(args: &[&str], work: &Path) -> Output {
    common::run(&tree(), &work.join("archive"), args)
}

fn start(args: &[&str], work: &Path) -> Child {
    common::prec(&tree(), &work.join("archive"), args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn finish(child: Child) -> PathBuf {
    let o = child.wait_with_output().unwrap();
    common::expect_ok(&o, "prec");
    common::last_path(&o)
}

fn prec(args: &[&str], work: &Path) -> PathBuf {
    finish(start(args, work))
}

fn colour(k: usize) -> [u8; 3] {
    [LEVELS[k % 4], LEVELS[(k / 4) % 4], LEVELS[(k / 16) % 4]]
}

fn write_cast(path: &Path, shot: &str, kind: &str, fps: u32) {
    let events: Vec<(f64, String)> = (0..N)
        .map(|k| {
            let t = if k == 0 {
                0.0
            } else {
                (k as f64 - 0.5) / fps as f64
            };
            let [r, g, b] = colour(k);
            (
                t,
                format!("\x1b[?25l{}", paint(r as u32, g as u32, b as u32)),
            )
        })
        .collect();
    common::write_cast(path, "frames", shot, kind, &events, N as f64 / fps as f64);
}

fn manifest(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn role(m: &Value, dir: &Path, role: &str) -> PathBuf {
    dir.join(entry(m, role)["file"].as_str().unwrap())
}

fn entry<'a>(m: &'a Value, role: &str) -> &'a Value {
    m["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["role"] == role)
        .unwrap_or_else(|| panic!("no {role} in the manifest"))
}

fn poster_px(m: &Value, dir: &Path) -> [u8; 3] {
    let poster = image::open(role(m, dir, "poster")).unwrap().to_rgb8();
    let p = poster.get_pixel(SIZE[0] / 2, SIZE[1] / 2);
    [p[0], p[1], p[2]]
}

fn stamp(path: &Path, key: &str, value: Value) {
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut header: Value = serde_json::from_str(&lines[0]).unwrap();
    header["x_terminal_renderer"][key] = value;
    lines[0] = header.to_string();
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

fn cast_poster(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap();
    let header: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    header["x_terminal_renderer"]["poster"].clone()
}

fn codes(video: &Path, size: [u32; 2]) -> Vec<[u8; 3]> {
    video::decode(video)
        .frames
        .iter()
        .map(|f| video::codes(f, size[0] / 2, size[1] / 2))
        .collect()
}

fn centres(video: &Path, size: [u32; 2]) -> Vec<[u8; 3]> {
    codes(video, size)
        .into_iter()
        .map(video::full_range)
        .collect()
}

fn bt709(px: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = px.map(|v| v as f64 / 255.0);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    [
        16.0 + 219.0 * y,
        128.0 + 224.0 * (b - y) / 1.8556,
        128.0 + 224.0 * (r - y) / 1.5748,
    ]
    .map(|v| v.round() as u8)
}

fn on_nearest_codes(video: &Path, want: &[usize], what: &str) {
    for (slot, (got, &k)) in codes(video, SIZE).into_iter().zip(want).enumerate() {
        let near = bt709(colour(k));
        assert!(
            distance(got, near) <= 1,
            "{what}: slot {slot} {:?} reads {got:?}, more than one code from the nearest BT.709 limited-range codes {near:?}",
            colour(k)
        );
    }
}

fn probe(video: &Path) -> (u64, u64, usize) {
    let f = video::probe(video);
    (f.width as u64, f.height as u64, f.frames as usize)
}

fn distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    (0..3).map(|i| a[i].abs_diff(b[i]) as u32).max().unwrap()
}

fn nearest(px: [u8; 3]) -> usize {
    (0..N).min_by_key(|&k| distance(px, colour(k))).unwrap()
}

type Job<'a> = (&'a Path, PathBuf, u32, &'a [&'a str]);

fn renders(jobs: &[Job], work: &Path) -> Vec<(Value, PathBuf)> {
    let size = format!("{}x{}", SIZE[0], SIZE[1]);
    let children: Vec<Child> = jobs
        .iter()
        .map(|(cast, out, fps, extra)| {
            let fps = fps.to_string();
            let mut args = vec![
                "render",
                cast.to_str().unwrap(),
                "--size",
                &size,
                "--fps",
                &fps,
                "--out",
                out.to_str().unwrap(),
            ];
            args.extend(*extra);
            start(&args, work)
        })
        .collect();
    children.into_iter().map(|c| checked(&finish(c))).collect()
}

fn render(cast: &Path, out: &Path, fps: u32, extra: &[&str], work: &Path) -> (Value, PathBuf) {
    renders(&[(cast, out.to_path_buf(), fps, extra)], work).remove(0)
}

fn checked(path: &Path) -> (Value, PathBuf) {
    let m = manifest(path);
    let dir = path.parent().unwrap().to_path_buf();
    let master = role(&m, &dir, "master");
    let (w, h, frames) = probe(&master);
    assert_eq!(
        (w, h, frames),
        (
            m["files"].as_array().unwrap()[0]["size"][0]
                .as_u64()
                .unwrap(),
            m["files"].as_array().unwrap()[0]["size"][1]
                .as_u64()
                .unwrap(),
            m["frames"]["master"].as_u64().unwrap() as usize
        ),
        "{}: the decoded master and the manifest disagree",
        master.display()
    );
    assert_eq!([w as u32, h as u32], SIZE);
    (m, dir)
}

fn shows(m: &Value, dir: &Path, want: &[usize], what: &str) {
    on_nearest_codes(&role(m, dir, "master"), want, what);
    let got = centres(&role(m, dir, "master"), SIZE);
    assert_eq!(got.len(), want.len(), "{what}: frame count");
    for (slot, (&px, &k)) in got.iter().zip(want).enumerate() {
        assert_eq!(
            nearest(px),
            k,
            "{what}: slot {slot} shows {px:?}, nearest colour {}, not colour {k} {:?}",
            nearest(px),
            colour(k)
        );
        assert!(distance(px, colour(k)) <= 12, "{what}: slot {slot} {px:?}");
    }
}

#[test]
fn a_master_shows_frame_k_in_slot_k_at_every_rate() {
    let w = work("order");
    let rates = [30, 60, 120];
    let casts: Vec<PathBuf> = rates
        .iter()
        .map(|fps| {
            let cast = w.join(format!("cast/order-{fps}.cast"));
            write_cast(&cast, "order", "clip", *fps);
            cast
        })
        .collect();
    let jobs: Vec<Job> = rates
        .iter()
        .zip(&casts)
        .map(|(fps, cast)| (cast.as_path(), w.join(format!("out-{fps}")), *fps, &[][..]))
        .collect();
    for ((m, dir), fps) in renders(&jobs, &w).iter().zip(rates) {
        shows(m, dir, &(0..N).collect::<Vec<_>>(), &format!("{fps} fps"));
    }
}

#[test]
fn a_palindrome_and_a_crossfade_keep_their_order() {
    let w = work("closures");
    let fps = 60;
    let cast = w.join("cast/closures.cast");
    write_cast(&cast, "closures", "clip", fps);
    let mut done = renders(
        &[
            (
                &cast,
                w.join("palindrome"),
                fps,
                &["--set", "closure=palindrome"],
            ),
            (
                &cast,
                w.join("crossfade"),
                fps,
                &["--set", "closure.crossfade=0.05"],
            ),
            (
                &cast,
                w.join("palindrome-poster"),
                fps,
                &["--set", "closure=palindrome", "--poster", "before-closure"],
            ),
            (
                &cast,
                w.join("crossfade-poster"),
                fps,
                &[
                    "--set",
                    "closure.crossfade=0.05",
                    "--poster",
                    "before-closure",
                ],
            ),
        ],
        &w,
    )
    .into_iter();
    let (m, dir) = done.next().unwrap();
    let want: Vec<usize> = (0..N).chain((1..N - 1).rev()).collect();
    shows(&m, &dir, &want, "palindrome");

    let k = 3;
    let (m, dir) = done.next().unwrap();
    let got = centres(&role(&m, &dir, "master"), SIZE);
    assert_eq!(got.len(), N - k, "crossfade: frame count");
    let clean: Vec<usize> = (k..N - k).collect();
    on_nearest_codes(&role(&m, &dir, "master"), &clean, "crossfade");
    for (slot, &px) in got.iter().enumerate().take(N - 2 * k) {
        assert_eq!(nearest(px), slot + k, "crossfade: clean slot {slot} {px:?}");
        assert!(
            distance(px, colour(slot + k)) <= 12,
            "crossfade: slot {slot} {px:?}"
        );
    }
    for i in 0..k {
        let x = (i as f64 + 0.5) / k as f64;
        let s = x * x * (3.0 - 2.0 * x);
        let (a, b) = (colour(N - k + i), colour(i));
        let want: [u8; 3] =
            [0, 1, 2].map(|c| (a[c] as f64 * (1.0 - s) + b[c] as f64 * s).round() as u8);
        let px = got[N - 2 * k + i];
        assert!(
            distance(px, want) <= 12,
            "crossfade: blend {i} shows {px:?}, not {want:?}"
        );
    }
    let p = poster_px(&m, &dir);
    assert_eq!(p, colour(k), "the poster is rendered frame {k}");
    assert!(
        distance(p, got[0]) <= 12,
        "the poster {p:?} is not the master's first frame {:?}",
        got[0]
    );
    assert_eq!(m["params"]["poster"], "first");
    assert_eq!(entry(&m, "poster")["frame"].as_u64(), Some(0));

    let (m, dir) = done.next().unwrap();
    let turned = centres(&role(&m, &dir, "master"), SIZE);
    let p = poster_px(&m, &dir);
    assert_eq!(p, colour(N - 1), "before-closure is the turning frame");
    assert!(
        distance(p, turned[N - 1]) <= 12,
        "the turning poster {p:?} is not master frame {} {:?}",
        N - 1,
        turned[N - 1]
    );
    assert_eq!(m["params"]["poster"], "before-closure");
    assert_eq!(entry(&m, "poster")["frame"].as_u64(), Some((N - 1) as u64));

    let (m, dir) = done.next().unwrap();
    let blended = centres(&role(&m, &dir, "master"), SIZE);
    let last_clean = N - 2 * k - 1;
    let p = poster_px(&m, &dir);
    assert_eq!(
        p,
        colour(N - k - 1),
        "before-closure is the last clean frame"
    );
    assert!(
        distance(p, blended[last_clean]) <= 12,
        "the clean poster {p:?} is not master frame {last_clean} {:?}",
        blended[last_clean]
    );
    assert_eq!(m["params"]["poster"], "before-closure");
    assert_eq!(
        entry(&m, "poster")["frame"].as_u64(),
        Some(last_clean as u64)
    );
}

#[test]
fn a_cast_rendered_through_a_link_to_its_own_folder_keeps_its_bytes() {
    let w = work("link");
    let cast = w.join("cast/link.cast");
    write_cast(&cast, "link", "clip", 30);
    let out = w.join("out");
    let (_, dir) = render(&cast, &out, 30, &[], &w);
    let own = dir.join("frames-clip-link-320x180-30fps.cast");
    let before = std::fs::read(&own).unwrap();
    assert!(!before.is_empty());
    let link = w.join("linked");
    std::os::unix::fs::symlink(&dir, &link).unwrap();
    let (m, _) = render(
        &link.join("frames-clip-link-320x180-30fps.cast"),
        &out,
        30,
        &[],
        &w,
    );
    assert_eq!(std::fs::read(&own).unwrap(), before);
    assert_eq!(
        role(&m, &dir, "cast"),
        own,
        "the manifest lists the cast by the render's stem"
    );
}

#[test]
fn a_web_copy_too_wide_and_a_crossfade_with_no_room_fail_before_chromium() {
    let w = work("early");
    let still = w.join("cast/early-web.cast");
    write_cast(&still, "early-web", "still", 30);
    let out = run(
        &[
            "render",
            still.to_str().unwrap(),
            "--size",
            "320x180",
            "--web",
            "200",
            "--out",
            w.join("out").to_str().unwrap(),
        ],
        &w,
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--web 200"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(common::no_chromium(
        &tree(),
        "frames-still-early-web-replay"
    ));

    let clip = w.join("cast/early-fade.cast");
    write_cast(&clip, "early-fade", "clip", 60);
    let out = run(
        &[
            "render",
            clip.to_str().unwrap(),
            "--size",
            "320x180",
            "--fps",
            "60",
            "--set",
            "closure.crossfade=0.5",
            "--out",
            w.join("out").to_str().unwrap(),
        ],
        &w,
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("crossfade"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(common::no_chromium(
        &tree(),
        "frames-clip-early-fade-replay"
    ));
}

#[test]
fn a_cast_with_an_unsafe_shot_is_refused() {
    let w = work("unsafe");
    let cast = w.join("cast/unsafe.cast");
    write_cast(&cast, "../escape", "clip", 30);
    let out = run(
        &[
            "render",
            cast.to_str().unwrap(),
            "--size",
            "320x180",
            "--fps",
            "30",
            "--out",
            w.join("out").to_str().unwrap(),
        ],
        &w,
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("shot"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_recipe_size_and_fps_render_with_no_flags_and_flags_still_win() {
    let w = work("recipe");
    let cast = w.join("cast/sized.cast");
    write_cast(&cast, "sized", "clip", 30);
    stamp(&cast, "size", Value::String("480x270".into()));
    stamp(&cast, "fps", Value::from(30));
    let out = w.join("out");
    let plain = start(
        &[
            "render",
            cast.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    let flagged = start(
        &[
            "render",
            cast.to_str().unwrap(),
            "--size",
            "640x360",
            "--fps",
            "60",
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    for (child, want, size) in [
        (plain, "frames-clip-sized-480x270-30fps.json", [480, 270]),
        (flagged, "frames-clip-sized-640x360-60fps.json", [640, 360]),
    ] {
        let m = finish(child);
        assert_eq!(m.file_name().unwrap().to_str().unwrap(), want);
        assert_eq!(
            entry(&manifest(&m), "master")["size"],
            serde_json::json!(size)
        );
    }
}

#[test]
fn a_poster_is_the_named_frame_and_the_flag_wins() {
    let w = work("moment");
    let fps = 30;
    let cast = w.join("cast/moment.cast");
    write_cast(&cast, "moment", "clip", fps);
    let stamped = w.join("cast/moment-stamped.cast");
    write_cast(&stamped, "moment", "clip", fps);
    stamp(&stamped, "poster", Value::String("before-closure".into()));
    let mut done = renders(
        &[
            (&cast, w.join("default"), fps, &[]),
            (&cast, w.join("at"), fps, &["--poster", "0.5"]),
            (&cast, w.join("late"), fps, &["--poster", "100"]),
            (&stamped, w.join("shot"), fps, &[]),
            (&stamped, w.join("flag"), fps, &["--poster", "0.5"]),
        ],
        &w,
    )
    .into_iter();
    let (m, dir) = done.next().unwrap();
    assert_eq!(
        poster_px(&m, &dir),
        colour(0),
        "no key keeps the first frame"
    );
    assert_eq!(m["params"]["poster"], "first");
    assert_eq!(entry(&m, "poster")["frame"].as_u64(), Some(0));

    let (m, dir) = done.next().unwrap();
    assert_eq!(poster_px(&m, &dir), colour(15), "0.5s is frame 15");
    assert_eq!(m["params"]["poster"].as_f64(), Some(0.5));
    assert_eq!(entry(&m, "poster")["frame"].as_u64(), Some(15));

    let (m, dir) = done.next().unwrap();
    assert_eq!(
        poster_px(&m, &dir),
        colour(N - 1),
        "a time past the end clamps to the last frame"
    );
    assert_eq!(m["params"]["poster"].as_f64(), Some(100.0));
    assert_eq!(entry(&m, "poster")["frame"].as_u64(), Some((N - 1) as u64));

    let (m, dir) = done.next().unwrap();
    assert_eq!(
        poster_px(&m, &dir),
        colour(N - 1),
        "the cast's poster is the last frame of an open clip"
    );
    assert_eq!(m["params"]["poster"], "before-closure");
    assert_eq!(entry(&m, "poster")["frame"].as_u64(), Some((N - 1) as u64));

    let (m, dir) = done.next().unwrap();
    assert_eq!(
        poster_px(&m, &dir),
        colour(15),
        "--poster wins over the cast"
    );
    assert_eq!(m["params"]["poster"].as_f64(), Some(0.5));
    assert_eq!(entry(&m, "poster")["frame"].as_u64(), Some(15));
    assert_eq!(cast_poster(&stamped), "before-closure");
    assert!(cast_poster(&cast).is_null());
}

#[test]
fn a_recipe_poster_is_recorded_and_the_flag_wins() {
    let w = work("recipe-poster");
    let app = common::demo_app(&w, "");
    common::add_shot(
        &app,
        "posted",
        "Sleep 200ms\nType \"demo tick\"\nEnter\nSleep 300ms\n",
        "size = \"320x180\"\nfps = 30\nposter = \"before-closure\"\n",
    );
    let out = w.join("out");
    let m = prec(
        &[
            "clip",
            "demo",
            "posted",
            "--app-dir",
            app.to_str().unwrap(),
            "--poster",
            "first",
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    let first = manifest(&m);
    assert_eq!(first["params"]["poster"], "first");
    assert_eq!(entry(&first, "poster")["frame"].as_u64(), Some(0));
    let dir = m.parent().unwrap();
    let cast = dir.join("demo-clip-posted-320x180-30fps.cast");
    assert_eq!(cast_poster(&cast), "before-closure");
    let again = prec(
        &[
            "render",
            cast.to_str().unwrap(),
            "--out",
            w.join("again").to_str().unwrap(),
        ],
        &w,
    );
    let followed = manifest(&again);
    let frames = followed["frames"]["master"].as_u64().unwrap();
    assert!(frames > 1, "the clip has more than one frame");
    assert_eq!(followed["params"]["poster"], "before-closure");
    assert_eq!(
        entry(&followed, "poster")["frame"].as_u64(),
        Some(frames - 1)
    );
    assert_eq!(cast_poster(&cast), "before-closure");
}

#[test]
fn a_poster_on_a_still_and_an_unknown_poster_are_refused() {
    let w = work("refuse");
    let still_app = common::demo_app(&w.join("still"), "");
    let toml = still_app.join("render/terminal.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    text.push_str(
        "\n[shots.posed]\nkind = \"still\"\ntape = \"render/terminal/posed.tape\"\nposter = \"first\"\n",
    );
    std::fs::write(&toml, text).unwrap();
    let out = run(
        &["shots", "demo", "--app-dir", still_app.to_str().unwrap()],
        &w,
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("poster is for clips"), "{err}");

    let bad_app = common::demo_app(&w.join("bad"), "");
    let toml = bad_app.join("render/terminal.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    text.push_str(
        "\n[shots.badposter]\nkind = \"clip\"\ntape = \"render/terminal/badposter.tape\"\nposter = \"middle\"\n",
    );
    std::fs::write(&toml, text).unwrap();
    let out = run(
        &["shots", "demo", "--app-dir", bad_app.to_str().unwrap()],
        &w,
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("poster") && err.contains("middle"), "{err}");

    let cast = w.join("cast/badflag.cast");
    write_cast(&cast, "badflag", "clip", 30);
    let out = run(
        &[
            "render",
            cast.to_str().unwrap(),
            "--size",
            "320x180",
            "--fps",
            "30",
            "--poster",
            "nope",
            "--out",
            w.join("out").to_str().unwrap(),
        ],
        &w,
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--poster"), "{err}");
    assert!(common::no_chromium(&tree(), "frames-clip-badflag-replay"));

    let still = w.join("cast/posed.cast");
    write_cast(&still, "posed", "still", 30);
    let out = run(
        &[
            "render",
            still.to_str().unwrap(),
            "--size",
            "320x180",
            "--poster",
            "first",
            "--out",
            w.join("out").to_str().unwrap(),
        ],
        &w,
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--poster is for clip and render"), "{err}");

    let app = common::demo_app(&w.join("record"), "");
    let out = run(
        &[
            "record",
            "demo",
            "typing",
            "--app-dir",
            app.to_str().unwrap(),
            "--poster",
            "first",
            "--out",
            w.join("out").to_str().unwrap(),
        ],
        &w,
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--poster is for clip and render"), "{err}");
}

fn flat(codes: [u8; 3], size: [u32; 2], pts: i64) -> Frame {
    let mut f = Frame::new(
        PixelFormat::Yuv420p8,
        size[0],
        size[1],
        pts,
        TimeBase::new(1001, 24000),
    );
    for (plane, code) in f.planes.iter_mut().zip(codes) {
        plane.data.fill(code);
    }
    f
}

fn encoded(path: &Path, frames: &[Frame], size: [u32; 2]) {
    let mut enc = Encoder::new(EncoderConfig {
        width: size[0],
        height: size[1],
        fps: (24000, 1001),
        profile: Profile::High,
        preset: Preset::Quality,
        rate: Rate::Crf(14),
        gop: 48,
        b_frames: 0,
        threads: 1,
        bitstream: Bitstream::LengthPrefixed,
    })
    .unwrap();
    let avc = AvcConfig::parse(&enc.avcc()).unwrap();
    let mut mux = Writer::create(path).unwrap();
    let track = mux
        .add_track(TrackConfig::new(
            SampleEntry::h264(avc, size[0] as u16, size[1] as u16),
            24000,
        ))
        .unwrap();
    let mut packets = vec![];
    for f in frames {
        packets.extend(enc.encode(f).unwrap());
    }
    packets.extend(enc.flush().unwrap());
    for p in packets {
        mux.push_sample(track, &p.data, p.pts * 1001, p.dts * 1001, 1001, p.keyframe)
            .unwrap();
    }
    mux.finish_faststart().unwrap();
}

#[test]
fn limited_range_codes_convert_to_full_range_rgb_by_bt709() {
    assert_eq!(video::full_range([16, 128, 128]), [0, 0, 0]);
    assert_eq!(video::full_range([235, 128, 128]), [255, 255, 255]);
    assert_eq!(video::full_range([128, 128, 128]), [130, 130, 130]);
    assert_eq!(video::full_range([0, 128, 128]), [0, 0, 0]);
    assert_eq!(video::full_range([255, 128, 128]), [255, 255, 255]);
    assert_eq!(video::full_range([63, 102, 240]), [255, 1, 0]);
    assert_eq!(video::full_range([173, 42, 26]), [0, 255, 1]);
    assert_eq!(video::full_range([32, 240, 118]), [1, 0, 255]);
    for k in 0..N {
        let back = video::full_range(bt709(colour(k)));
        assert!(
            distance(back, colour(k)) <= 1,
            "colour {k} {:?} comes back as {back:?}",
            colour(k)
        );
        assert_eq!(nearest(back), k);
    }
}

#[test]
fn codes_read_the_luma_at_the_pixel_and_the_chroma_of_its_2x2_block() {
    let mut f = flat([16, 128, 128], [8, 4], 0);
    f.planes[0].data[2 * 8 + 5] = 200;
    f.planes[1].data[4 + 2] = 90;
    f.planes[2].data[4 + 2] = 240;
    assert_eq!(video::codes(&f, 5, 2), [200, 90, 240]);
    assert_eq!(video::codes(&f, 4, 3), [16, 90, 240]);
    assert_eq!(video::codes(&f, 3, 2), [16, 128, 128]);
    let rgb = video::rgb(&f);
    assert_eq!(rgb.len(), 8 * 4 * 3);
    let at = (2 * 8 + 5) * 3;
    assert_eq!(rgb[at..at + 3], video::full_range([200, 90, 240]));
    assert_eq!(rgb[0..3], [0, 0, 0]);
}

#[test]
fn rates_reduce_and_reordering_counts_frames_shown_out_of_decode_order() {
    assert_eq!(video::ratio(24000, 1001), "24000/1001");
    assert_eq!(video::ratio(30000, 1000), "30/1");
    assert_eq!(video::ratio(24 * 48000, 48 * 1001), "24000/1001");
    assert_eq!(video::ratio(1, 0), "0/0");
    assert_eq!(video::modal(&[1001, 1001, 1002, 1001]), 1001);
    assert_eq!(video::modal(&[]), 0);
    assert_eq!(video::reorder_depth(&[0, 1, 2, 3]), 0);
    assert_eq!(video::reorder_depth(&[0, 3, 1, 2, 6, 4, 5]), 2);
    assert_eq!(video::reorder_depth(&[0, 2, 1]), 1);
    assert_eq!(video::reorder_depth(&[]), 0);
    assert_eq!(video::profile_name(100, 0), "High");
    assert_eq!(video::profile_name(77, 0), "Main");
    assert_eq!(video::profile_name(66, 0x40), "Constrained Baseline");
    assert_eq!(video::profile_name(66, 0), "Baseline");
    assert_eq!(video::profile_name(7, 0), "7");
}

#[test]
fn the_oracle_reads_back_what_pito_codecs_wrote() {
    let w = work("oracle");
    let path = w.join("flat.mp4");
    let size = [64, 48];
    let shades: Vec<[u8; 3]> = (0..N).map(|k| bt709(colour(k))).collect();
    let frames: Vec<Frame> = shades
        .iter()
        .enumerate()
        .map(|(n, &c)| flat(c, size, n as i64))
        .collect();
    encoded(&path, &frames, size);

    let v = video::decode(&path);
    let f = &v.facts;
    assert_eq!(f.codec, "h264");
    assert_eq!(f.profile, "High");
    assert_eq!(f.pix_fmt, "yuv420p");
    assert_eq!((f.width, f.height), (64, 48));
    assert_eq!(f.track_size, [64, 48]);
    assert_eq!(f.r_frame_rate, "24000/1001");
    assert_eq!(f.avg_frame_rate, "24000/1001");
    assert_eq!((f.frames, f.samples), (N as u64, N as u64));
    assert!(
        (f.duration - N as f64 * 1001.0 / 24000.0).abs() < 1e-3,
        "{}",
        f.duration
    );
    assert_eq!((f.reordered, f.held), (0, 0));
    assert_eq!(video::probe(&path), v.facts);

    assert_eq!(v.frames.len(), N);
    for (n, (frame, &want)) in v.frames.iter().zip(&shades).enumerate() {
        let got = video::codes(frame, size[0] / 2, size[1] / 2);
        assert!(distance(got, want) <= 1, "frame {n}: {got:?} not {want:?}");
        assert_eq!(nearest(video::rgb_at(frame, 0, 0)), n, "frame {n}");
        assert_eq!(video::rgb(frame).len(), (size[0] * size[1] * 3) as usize);
    }
}

#[test]
#[should_panic(expected = "not-a-video.mp4")]
fn the_oracle_names_a_file_that_is_not_an_mp4() {
    let w = work("oracle-garbage");
    let path = w.join("not-a-video.mp4");
    std::fs::write(&path, b"plain text, not a box in sight").unwrap();
    video::decode(&path);
}
