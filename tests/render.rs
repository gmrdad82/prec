mod common;

use common::{paint, tree};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

const BG: [u8; 3] = [0x1e, 0x1e, 0x2e];

fn work(name: &str) -> PathBuf {
    common::scratch("render", name)
}

fn command(args: &[&str], w: &Path) -> Command {
    let mut c = common::prec(&tree(), &w.join("archive"), args);
    c.env("XDG_CACHE_HOME", w.join("cache"));
    c
}

fn run(args: &[&str], w: &Path) -> Output {
    command(args, w).output().unwrap()
}

fn start(args: &[&str], w: &Path) -> Child {
    command(args, w)
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

fn prec(args: &[&str], w: &Path) -> PathBuf {
    finish(start(args, w))
}

fn said(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn refused(o: &Output, line: &str) {
    assert_eq!(o.status.code(), Some(1), "{}", said(o));
    assert_eq!(said(o), format!("prec: {line}\n"));
    assert!(
        o.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
}

fn json_of(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn entry<'a>(m: &'a Value, role: &str) -> &'a Value {
    m["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["role"] == role)
        .unwrap_or_else(|| panic!("no {role} in the manifest"))
}

fn sha256(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn every_file_is_what_the_manifest_says(m: &Value, dir: &Path) {
    for f in m["files"].as_array().unwrap() {
        let path = dir.join(f["file"].as_str().unwrap());
        let bytes = std::fs::metadata(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .len();
        assert_eq!(f["bytes"].as_u64(), Some(bytes), "{}", path.display());
        assert_eq!(
            f["sha256"].as_str().unwrap(),
            sha256(&path),
            "{}",
            path.display()
        );
    }
    let mut left: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".partial"))
        .collect();
    left.sort();
    assert!(left.is_empty(), "{left:?}");
}

fn png_info(path: &Path) -> (png::BitDepth, [u32; 2], bool) {
    let bytes = std::fs::read(path).unwrap();
    let r = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let i = r.info();
    (i.bit_depth, [i.width, i.height], i.srgb.is_some())
}

fn pixel(path: &Path, x: u32, y: u32) -> [u8; 3] {
    let img = image::open(path).unwrap().to_rgb8();
    let p = img.get_pixel(x, y);
    [p[0], p[1], p[2]]
}

fn hex(s: &str) -> [u8; 3] {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap();
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn distance(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap()
}

fn stamp(path: &Path, key: &str, value: Value) {
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut header: Value = serde_json::from_str(&lines[0]).unwrap();
    header["x_terminal_renderer"][key] = value;
    lines[0] = header.to_string();
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

fn painted_cast(path: &Path, shot: &str, kind: &str, rgb: [u32; 3], end: f64) {
    let events = vec![(0.0, format!("\x1b[?25l{}", paint(rgb[0], rgb[1], rgb[2])))];
    common::write_cast(path, "render", shot, kind, &events, end);
}

fn ffprobe(video: &Path) -> Value {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=codec_name,profile,width,height,pix_fmt,r_frame_rate,avg_frame_rate,nb_read_frames,duration,has_b_frames",
            "-of",
            "json",
        ])
        .arg(video)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    v["streams"][0].clone()
}

fn demo_with(w: &Path, shot: &str, kind: &str, extra: &str) -> PathBuf {
    let app = common::demo_app(w, "");
    std::fs::write(
        app.join(format!("render/terminal/{shot}.tape")),
        "Type \"demo show\"\nEnter\nSleep 300ms\n",
    )
    .unwrap();
    let toml = app.join("render/terminal.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    text.push_str(&format!(
        "\n[shots.{shot}]\nkind = \"{kind}\"\ntape = \"render/terminal/{shot}.tape\"\n{extra}"
    ));
    std::fs::write(&toml, text).unwrap();
    app
}

fn no_run(name: &str) -> bool {
    !tree().join("tmp/prec/run").join(name).exists()
}

#[test]
fn a_still_writes_a_16_bit_master_and_8_bit_deliveries_on_the_theme_background() {
    let w = work("still");
    let app = demo_with(&w, "covstill", "still", "");
    let out = w.join("out");
    let m = prec(
        &[
            "still",
            "demo",
            "covstill",
            "--app-dir",
            app.to_str().unwrap(),
            "--size",
            "320x180",
            "--derive",
            "160x90,320x180",
            "--frame",
            "bare",
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    let dir = out.join("demo/still");
    assert_eq!(m, dir.join("demo-still-covstill-320x180.json"));
    let manifest = json_of(&m);
    assert_eq!(manifest["kind"], "still");
    assert_eq!(manifest["params"]["size"], json!([320, 180]));
    assert_eq!(manifest["params"]["derive"], json!([[160, 90]]));
    assert_eq!(manifest["params"]["fps"], Value::Null);
    assert_eq!(manifest["theme"]["background"], "#1e1e2e");
    assert_eq!(manifest["archive"]["state"], "skipped");
    every_file_is_what_the_manifest_says(&manifest, &dir);

    let master = dir.join(entry(&manifest, "master")["file"].as_str().unwrap());
    assert_eq!(master, dir.join("demo-still-covstill-320x180.master.png"));
    assert_eq!(
        png_info(&master),
        (png::BitDepth::Sixteen, [320, 180], true)
    );
    let wide = image::open(&master).unwrap().to_rgb16();
    let corner = wide.get_pixel(0, 0);
    assert_eq!(
        [corner[0], corner[1], corner[2]],
        BG.map(|v| v as u16 * 257)
    );
    for (name, size) in [
        ("demo-still-covstill-320x180.png", [320, 180]),
        ("demo-still-covstill-160x90.png", [160, 90]),
    ] {
        let p = dir.join(name);
        assert_eq!(png_info(&p), (png::BitDepth::Eight, size, true), "{name}");
        for (x, y) in [(0, 0), (size[0] - 1, size[1] - 1)] {
            assert!(distance(pixel(&p, x, y), BG) <= 1, "{name} ({x}, {y})");
        }
        let listed = manifest["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["file"] == name)
            .unwrap();
        assert_eq!(listed["size"], json!(size));
    }
    assert_eq!(
        pixel(&dir.join("demo-still-covstill-320x180.png"), 0, 0),
        BG
    );
    assert_eq!(
        image::open(dir.join("demo-still-covstill-320x180.png"))
            .unwrap()
            .to_rgb8()
            .into_raw(),
        image::open(&master).unwrap().to_rgb8().into_raw()
    );
    let cast = dir.join(entry(&manifest, "cast")["file"].as_str().unwrap());
    assert_eq!(cast, dir.join("demo-still-covstill-320x180.cast"));
}

#[test]
fn a_bad_size_is_refused_before_the_app_records() {
    let w = work("bad-size");
    let app = demo_with(&w, "covbad", "still", "");
    let clip_app = demo_with(&w.join("clip"), "covbadclip", "clip", "");
    let out = w.join("out");
    for (kind, shot, app, size, line) in [
        (
            "still",
            "covbad",
            &app,
            "0",
            "size 0: a width and height above 0",
        ),
        (
            "still",
            "covbad",
            &app,
            "320x0",
            "size 320x0: a width and height above 0",
        ),
        (
            "still",
            "covbad",
            &app,
            "big",
            "size big: N, WxH, 720p, 1080p, 1440p or 2160p",
        ),
        (
            "clip",
            "covbadclip",
            &clip_app,
            "321x180",
            "size 321x180: even width and height, for 4:2:0 video",
        ),
        (
            "clip",
            "covbadclip",
            &clip_app,
            "320x181",
            "size 320x181: even width and height, for 4:2:0 video",
        ),
        (
            "still",
            "covbad",
            &app,
            "16385x180",
            "size 16385x180: at most 16384 on a side",
        ),
        (
            "clip",
            "covbadclip",
            &clip_app,
            "320x16386",
            "size 320x16386: at most 16384 on a side",
        ),
    ] {
        let record = format!("demo-{kind}-{shot}-record");
        let _ = std::fs::remove_dir_all(tree().join("tmp/prec/run").join(&record));
        let o = run(
            &[
                kind,
                "demo",
                shot,
                "--app-dir",
                app.to_str().unwrap(),
                "--size",
                size,
                "--out",
                out.to_str().unwrap(),
            ],
            &w,
        );
        refused(&o, line);
        assert!(no_run(&record), "{kind} {size} recorded");
    }
    let o = run(
        &[
            "still",
            "demo",
            "covbad",
            "--app-dir",
            app.to_str().unwrap(),
            "--size",
            "320x180",
            "--derive",
            "0x90",
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    refused(&o, "size 0x90: a width and height above 0");
    assert!(!out.exists());
    let jobs = w.join("jobs");
    for (kind, shot, app) in [("still", "covbad", &app), ("clip", "covbadclip", &clip_app)] {
        let o = command(
            &[
                kind,
                "demo",
                shot,
                "--app-dir",
                app.to_str().unwrap(),
                "--size",
                "16386x180",
                "--out",
                out.to_str().unwrap(),
            ],
            &w,
        )
        .env_remove("PITO_JOBS")
        .env("PITO_JOBS_DIR", &jobs)
        .output()
        .unwrap();
        refused(&o, "size 16386x180: at most 16384 on a side");
        assert!(!jobs.exists(), "a refused {kind} size opened a job record");
    }
}

#[test]
fn a_wrong_kind_a_still_with_a_rate_a_bad_frame_or_grid_and_an_unset_pass_env_are_refused() {
    let w = work("prepare");
    let app = demo_with(&w, "covprep", "still", "fps = 30\n");
    common::add_shot(
        &app,
        "covplain",
        "Type \"demo show\"\nEnter\nSleep 300ms\n",
        "",
    );
    let out = w.join("out");
    let shoot = |app: &Path, args: &[&str]| {
        let mut all = args.to_vec();
        all.extend([
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        command(&all, &w)
            .env_remove("ALPHA_UNSET")
            .output()
            .unwrap()
    };
    for (args, line) in [
        (
            &["clip", "demo", "covprep"][..],
            "covprep is a still shot; run `prec still …`",
        ),
        (
            &["still", "demo", "covplain"][..],
            "covplain is a clip shot; run `prec clip …`",
        ),
        (
            &["still", "demo", "covprep"][..],
            "shot covprep: fps is for clips",
        ),
        (
            &["clip", "demo", "covplain", "--frame", "round"][..],
            "--frame window or bare",
        ),
        (
            &["clip", "demo", "covplain", "--set", "grid=8"][..],
            "--set grid=<cols>x<rows>",
        ),
        (
            &["clip", "demo", "covplain", "--set", "grid=8xq"][..],
            "--set grid=<cols>x<rows>",
        ),
    ] {
        refused(&shoot(&app, args), line);
    }
    let needy = demo_with(&w.join("env"), "covenv", "clip", "");
    let toml = needy.join("render/terminal.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(&toml, format!("pass_env = [\"ALPHA_UNSET\"]\n{text}")).unwrap();
    refused(
        &shoot(&needy, &["clip", "demo", "covenv"]),
        "pass_env ALPHA_UNSET is not set",
    );
    assert!(!out.exists());
}

#[test]
fn a_bad_size_in_the_shot_itself_is_refused_before_the_app_records() {
    let w = work("bad-shot-size");
    let out = w.join("out");
    for (kind, shot, size, line) in [
        (
            "clip",
            "covoddshot",
            "321x180",
            "shot covoddshot: size 321x180: even width and height, for 4:2:0 video",
        ),
        (
            "still",
            "covzeroshot",
            "0x90",
            "shot covzeroshot: size 0x90: a width and height above 0",
        ),
        (
            "still",
            "covhugeshot",
            "16385",
            "shot covhugeshot: size 16385: at most 16384 on a side",
        ),
    ] {
        let app = demo_with(&w.join(shot), shot, kind, &format!("size = \"{size}\"\n"));
        let record = format!("demo-{kind}-{shot}-record");
        let _ = std::fs::remove_dir_all(tree().join("tmp/prec/run").join(&record));
        let o = run(
            &[
                kind,
                "demo",
                shot,
                "--app-dir",
                app.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
            ],
            &w,
        );
        refused(&o, line);
        assert!(no_run(&record), "{kind} {shot} recorded");
    }
    assert!(!out.exists());
}

#[test]
fn prec_record_and_prec_render_refuse_a_bad_size_before_any_work() {
    let w = work("bad-size-verbs");
    let out = w.join("out");
    let app = demo_with(&w, "covrecsize", "clip", "");
    for (size, line) in [
        (
            "321x180",
            "size 321x180: even width and height, for 4:2:0 video",
        ),
        ("0", "size 0: a width and height above 0"),
        ("16386x180", "size 16386x180: at most 16384 on a side"),
    ] {
        let record = "demo-clip-covrecsize-record";
        let _ = std::fs::remove_dir_all(tree().join("tmp/prec/run").join(record));
        let o = run(
            &[
                "record",
                "demo",
                "covrecsize",
                "--app-dir",
                app.to_str().unwrap(),
                "--size",
                size,
                "--out",
                out.to_str().unwrap(),
            ],
            &w,
        );
        refused(&o, line);
        assert!(no_run(record), "record {size} recorded");
    }
    assert!(!out.exists());

    let cast = w.join("cast/sized.cast");
    painted_cast(&cast, "sized", "clip", [1, 2, 3], 0.2);
    let still = w.join("cast/sizedstill.cast");
    painted_cast(&still, "sizedstill", "still", [1, 2, 3], 0.2);
    for (cast, size, line) in [
        (
            &cast,
            "321x180",
            "size 321x180: even width and height, for 4:2:0 video",
        ),
        (
            &cast,
            "16386x180",
            "size 16386x180: at most 16384 on a side",
        ),
        (&still, "16385", "size 16385: at most 16384 on a side"),
        (&still, "0x90", "size 0x90: a width and height above 0"),
    ] {
        let o = run(
            &[
                "render",
                cast.to_str().unwrap(),
                "--size",
                size,
                "--out",
                out.to_str().unwrap(),
            ],
            &w,
        );
        refused(&o, line);
    }
    assert!(common::no_chromium(&tree(), "render-clip-sized-replay"));
    assert!(common::no_chromium(
        &tree(),
        "render-still-sizedstill-replay"
    ));
    assert!(!out.exists());
}

#[test]
fn a_clip_writes_its_master_poster_and_manifest_at_a_film_rate() {
    let w = work("clip");
    let app = demo_with(&w, "covclip", "clip", "");
    let out = w.join("out");
    let m = prec(
        &[
            "clip",
            "demo",
            "covclip",
            "--app-dir",
            app.to_str().unwrap(),
            "--size",
            "320x180",
            "--fps",
            "23.976",
            "--frame",
            "bare",
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    let dir = out.join("demo/clip");
    let stem = "demo-clip-covclip-320x180-23.976fps";
    assert_eq!(m, dir.join(format!("{stem}.json")));
    let manifest = json_of(&m);
    let mut keys: Vec<&str> = manifest
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "app",
            "archive",
            "caller",
            "cast_sha256",
            "colour",
            "device",
            "encodes",
            "files",
            "fixtures",
            "font",
            "frames",
            "grid",
            "kind",
            "made",
            "outputs",
            "params",
            "pass_env",
            "private_allowed",
            "product",
            "seconds",
            "shot",
            "shot_sha256",
            "source",
            "tape_sha256",
            "theme",
            "tool",
            "version",
            "xterm",
        ]
    );
    assert_eq!(manifest["tool"]["name"], "prec");
    assert_eq!(manifest["params"]["fps"], json!(["24000/1001"]));
    assert_eq!(manifest["frames"]["exact"], true);
    every_file_is_what_the_manifest_says(&manifest, &dir);
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    on_disk.sort();
    assert_eq!(
        on_disk,
        [
            format!("{stem}.cast"),
            format!("{stem}.json"),
            format!("{stem}.master.mp4"),
            format!("{stem}.poster.png"),
        ]
    );

    let master = entry(&manifest, "master");
    assert_eq!(master["fps"], "24000/1001");
    assert_eq!(master["size"], json!([320, 180]));
    let count = manifest["frames"]["master"].as_u64().unwrap();
    let shown = manifest["frames"]["shown_seconds"].as_f64().unwrap();
    assert_eq!(count, (shown * 24000.0 / 1001.0).round().max(1.0) as u64);
    assert!(
        (manifest["frames"]["seconds"].as_f64().unwrap() - count as f64 * 1001.0 / 24000.0).abs()
            < 1e-9
    );
    let s = ffprobe(&dir.join(format!("{stem}.master.mp4")));
    assert_eq!(s["codec_name"], "h264");
    assert_eq!(s["profile"], "High");
    assert_eq!(s["pix_fmt"], "yuv420p");
    assert_eq!(
        (s["width"].as_u64(), s["height"].as_u64()),
        (Some(320), Some(180))
    );
    assert_eq!(s["r_frame_rate"], "24000/1001");
    assert_eq!(s["avg_frame_rate"], "24000/1001");
    assert_eq!(s["has_b_frames"].as_u64(), Some(0));
    assert_eq!(
        s["nb_read_frames"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        count
    );
    let duration: f64 = s["duration"].as_str().unwrap().parse().unwrap();
    assert!(
        (duration - count as f64 * 1001.0 / 24000.0).abs() < 1e-3,
        "{duration}"
    );

    let poster = dir.join(format!("{stem}.poster.png"));
    assert_eq!(png_info(&poster), (png::BitDepth::Eight, [320, 180], true));
    assert_eq!(pixel(&poster, 0, 0), BG);
    assert_eq!(entry(&manifest, "poster")["frame"].as_u64(), Some(0));
    let encodes = manifest["encodes"].as_array().unwrap();
    assert_eq!(encodes.len(), 1);
    assert!(
        encodes[0]["args"]
            .as_array()
            .unwrap()
            .contains(&json!("bframes 0"))
    );
    assert!(
        encodes[0]["args"]
            .as_array()
            .unwrap()
            .contains(&json!("gop 48"))
    );
}

#[test]
fn a_cast_renders_at_every_exact_rate_with_its_frames_and_duration() {
    let w = work("rates");
    let cast = w.join("cast/rates.cast");
    painted_cast(&cast, "rates", "clip", [200, 40, 40], 0.5);
    let rates = [
        ("24", "24", 24, 1),
        ("30", "30", 30, 1),
        ("48", "48", 48, 1),
        ("60", "60", 60, 1),
        ("120", "120", 120, 1),
        ("24000/1001", "23.976", 24000, 1001),
        ("30000/1001", "29.97", 30000, 1001),
        ("48000/1001", "47.952", 48000, 1001),
        ("60000/1001", "59.94", 60000, 1001),
    ];
    for batch in rates.chunks(3) {
        let children: Vec<Child> = batch
            .iter()
            .map(|(flag, ..)| {
                start(
                    &[
                        "render",
                        cast.to_str().unwrap(),
                        "--size",
                        "160x90",
                        "--fps",
                        flag,
                        "--out",
                        w.join(format!("out-{}", flag.replace('/', "-")))
                            .to_str()
                            .unwrap(),
                    ],
                    &w,
                )
            })
            .collect();
        for (child, (flag, label, num, den)) in children.into_iter().zip(batch) {
            let m = finish(child);
            assert_eq!(
                m.file_name().unwrap().to_string_lossy(),
                format!("render-clip-rates-160x90-{label}fps.json"),
                "{flag}"
            );
            let manifest = json_of(&m);
            let shown = manifest["frames"]["shown_seconds"].as_f64().unwrap();
            assert_eq!(shown, 0.5, "{flag}");
            let want = (0.5 * *num as f64 / *den as f64).round() as u64;
            assert_eq!(manifest["frames"]["master"].as_u64(), Some(want), "{flag}");
            let dir = m.parent().unwrap();
            every_file_is_what_the_manifest_says(&manifest, dir);
            let s = ffprobe(&dir.join(format!("render-clip-rates-160x90-{label}fps.master.mp4")));
            let rate = if *den == 1 {
                format!("{num}/1")
            } else {
                format!("{num}/{den}")
            };
            assert_eq!(s["r_frame_rate"], json!(rate), "{flag}");
            assert_eq!(s["has_b_frames"].as_u64(), Some(0), "{flag}");
            assert_eq!(
                s["nb_read_frames"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap(),
                want,
                "{flag}"
            );
            let duration: f64 = s["duration"].as_str().unwrap().parse().unwrap();
            let exact = want as f64 * *den as f64 / *num as f64;
            assert!(
                (duration - exact).abs() < 1e-3,
                "{flag}: {duration} vs {exact}"
            );
        }
    }
}

#[test]
fn a_rate_clap_cannot_take_gets_the_short_answer() {
    let w = work("bad-rate");
    for (fps, why) in [
        ("29.9", "not an exact rate; use 30000/1001"),
        ("23.9", "not an exact rate; use 24000/1001"),
        ("59.9", "not an exact rate; use 60000/1001"),
        (
            "0",
            "24, 30, 48, 60, 120, 24000/1001 (23.976 or 23.98), 30000/1001 (29.97), 48000/1001 (47.952) or 60000/1001 (59.94)",
        ),
        (
            "fast",
            "24, 30, 48, 60, 120, 24000/1001 (23.976 or 23.98), 30000/1001 (29.97), 48000/1001 (47.952) or 60000/1001 (59.94)",
        ),
        (
            "29.97.1",
            "24, 30, 48, 60, 120, 24000/1001 (23.976 or 23.98), 30000/1001 (29.97), 48000/1001 (47.952) or 60000/1001 (59.94)",
        ),
    ] {
        for verb in [&["clip", "demo", "x"][..], &["render", "a.cast"][..]] {
            let mut args = verb.to_vec();
            args.extend(["--fps", fps]);
            let o = run(&args, &w);
            assert_eq!(o.status.code(), Some(2), "{args:?}");
            assert!(o.stdout.is_empty());
            assert_eq!(
                said(&o),
                format!(
                    "error: invalid value '{fps}' for '--fps <FPS>': {why}\n\nFor more information, try '--help'.\n"
                ),
                "{args:?}"
            );
        }
    }
    for (verb, usage) in [
        (
            &["clip", "demo", "x"][..],
            "prec clip [OPTIONS] <PRODUCT> <SHOT>",
        ),
        (&["render", "a.cast"][..], "prec render [OPTIONS] <CAST>"),
    ] {
        let mut args = verb.to_vec();
        args.extend(["--fps", "-1"]);
        let o = run(&args, &w);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(o.stdout.is_empty());
        assert_eq!(
            said(&o),
            format!(
                "error: unexpected argument '-1' found\n\n  tip: to pass '-1' as a value, use '-- -1'\n\nUsage: {usage}\n\nFor more information, try '--help'.\n"
            ),
            "{args:?}"
        );
    }
    let o = run(&["render", "a.cast", "--fps"], &w);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        said(&o).starts_with("error: a value is required for '--fps <FPS>'"),
        "{}",
        said(&o)
    );
}

#[test]
fn a_recorded_cast_renders_at_another_size_rate_and_theme_without_the_app() {
    let w = work("again");
    let app = demo_with(&w, "covagain", "clip", "");
    let out = w.join("out");
    let cast = prec(
        &[
            "record",
            "demo",
            "covagain",
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    assert_eq!(cast, out.join("demo/clip/demo-clip-covagain.cast"));
    let before = std::fs::read(&cast).unwrap();
    std::fs::remove_dir_all(&app).unwrap();
    let m = prec(
        &[
            "render",
            cast.to_str().unwrap(),
            "--size",
            "160x90",
            "--fps",
            "30",
            "--theme",
            "Catppuccin Latte",
            "--frame",
            "bare",
            "--out",
            w.join("again").to_str().unwrap(),
        ],
        &w,
    );
    let stem = "demo-clip-covagain-catppuccin-latte-160x90-30fps";
    assert_eq!(
        m.file_name().unwrap().to_string_lossy(),
        format!("{stem}.json")
    );
    let manifest = json_of(&m);
    assert_eq!(manifest["theme"]["name"], "Catppuccin Latte");
    assert_eq!(manifest["theme"]["label"], "catppuccin-latte");
    let bg = hex(manifest["theme"]["background"].as_str().unwrap());
    assert_ne!(bg, BG);
    assert_eq!(manifest["params"]["size"], json!([160, 90]));
    assert_eq!(manifest["params"]["fps"], json!([30]));
    let dir = m.parent().unwrap();
    every_file_is_what_the_manifest_says(&manifest, dir);
    assert_eq!(pixel(&dir.join(format!("{stem}.poster.png")), 0, 0), bg);
    let s = ffprobe(&dir.join(format!("{stem}.master.mp4")));
    assert_eq!(s["r_frame_rate"], "30/1");
    assert_eq!(
        (s["width"].as_u64(), s["height"].as_u64()),
        (Some(160), Some(90))
    );
    assert_eq!(std::fs::read(&cast).unwrap(), before);
    assert_eq!(
        std::fs::read(dir.join(format!("{stem}.cast"))).unwrap(),
        before
    );
}

#[test]
fn a_missing_cast_a_cast_of_another_format_and_an_unknown_theme_are_refused() {
    let w = work("refused");
    let out = w.join("out");
    let gone = w.join("cast/gone.cast");
    let o = run(
        &[
            "render",
            gone.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    assert_eq!(o.status.code(), Some(1));
    let e = said(&o);
    assert!(e.starts_with(&format!("prec: {}: ", gone.display())), "{e}");

    let newer = w.join("cast/newer.cast");
    painted_cast(&newer, "newer", "clip", [1, 2, 3], 0.2);
    let text = std::fs::read_to_string(&newer).unwrap();
    std::fs::write(&newer, text.replacen("\"version\":2", "\"version\":3", 1)).unwrap();
    let o = run(
        &[
            "render",
            newer.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    refused(&o, &format!("{}: an asciicast v2 file", newer.display()));

    let empty = w.join("cast/empty.cast");
    std::fs::write(&empty, "").unwrap();
    let o = run(
        &[
            "render",
            empty.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    refused(&o, &format!("{}: an empty cast", empty.display()));

    let themed = w.join("cast/themed.cast");
    painted_cast(&themed, "themed", "clip", [1, 2, 3], 0.2);
    let o = run(
        &[
            "render",
            themed.to_str().unwrap(),
            "--size",
            "160x90",
            "--fps",
            "30",
            "--theme",
            "No Such Theme",
            "--out",
            out.to_str().unwrap(),
        ],
        &w,
    );
    refused(&o, "no theme \"No Such Theme\"; see `prec themes`");
    assert!(common::no_chromium(&tree(), "render-clip-themed-replay"));
    assert!(!out.exists());
}

#[test]
fn a_mask_paints_its_cells_in_the_theme_background_and_nothing_else() {
    let w = work("mask");
    let red = [200u8, 30, 30];
    let plain = w.join("cast/plain.cast");
    painted_cast(&plain, "plain", "still", red.map(u32::from), 0.3);
    let masked = w.join("cast/masked.cast");
    painted_cast(&masked, "masked", "still", red.map(u32::from), 0.3);
    stamp(&masked, "mask", json!([{"rect": [0, 0, 4, 4]}]));
    let children: Vec<Child> = [&plain, &masked]
        .iter()
        .map(|cast| {
            start(
                &[
                    "render",
                    cast.to_str().unwrap(),
                    "--size",
                    "320x180",
                    "--out",
                    w.join("out").to_str().unwrap(),
                ],
                &w,
            )
        })
        .collect();
    let mut found = vec![];
    for child in children {
        let m = finish(child);
        let manifest = json_of(&m);
        let dir = m.parent().unwrap().to_path_buf();
        let master = dir.join(entry(&manifest, "master")["file"].as_str().unwrap());
        let canvas = manifest["grid"]["canvas"].as_array().unwrap().clone();
        let quarter = canvas[0].as_u64().unwrap() as u32 / 4;
        found.push((
            pixel(&master, 160 - quarter, 90),
            pixel(&master, 160 + quarter, 90),
            pixel(&master, 0, 0),
        ));
    }
    let (left, right, corner) = found[0];
    assert!(distance(left, red) <= 2, "unmasked left {left:?}");
    assert!(distance(right, red) <= 2, "unmasked right {right:?}");
    assert!(
        distance(corner, red) <= 2,
        "a bare frame keeps the app's ground to the edge, not {corner:?}"
    );
    let (left, right, _) = found[1];
    assert!(distance(left, BG) <= 2, "masked left {left:?}");
    assert!(distance(right, red) <= 2, "masked right {right:?}");

    let outside = w.join("cast/outside.cast");
    painted_cast(&outside, "outside", "still", red.map(u32::from), 0.3);
    stamp(&outside, "mask", json!([{"rect": [6, 0, 4, 1]}]));
    let o = run(
        &[
            "render",
            outside.to_str().unwrap(),
            "--size",
            "320x180",
            "--out",
            w.join("out").to_str().unwrap(),
        ],
        &w,
    );
    refused(&o, "mask rect [6, 0, 4, 1]: outside the 8x4 grid");
    assert!(common::no_chromium(&tree(), "render-still-outside-replay"));
}
