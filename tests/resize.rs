mod common;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SIZE: [u32; 2] = [320, 180];

fn case(name: &str, shot: &str, kind: &str, tape: &str) -> (PathBuf, PathBuf) {
    let dir = common::caller("resize", name);
    let app = common::demo_app(&dir, "");
    std::fs::write(app.join(format!("render/terminal/{shot}.tape")), tape).unwrap();
    let toml = app.join("render/terminal.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    text.push_str(&format!(
        "\n[shots.{shot}]\nkind = \"{kind}\"\ntape = \"render/terminal/{shot}.tape\"\ngrid = [40, 10]\n"
    ));
    std::fs::write(&toml, text).unwrap();
    (dir, app)
}

fn prec(dir: &Path, args: &[&str]) -> Output {
    let mut c = common::prec(dir, &dir.join("archive"), args);
    c.env("XDG_CACHE_HOME", dir.join("cache"));
    c.output().unwrap()
}

fn shoot(dir: &Path, app: &Path, kind: &str, shot: &str) -> Output {
    let size = format!("{}x{}", SIZE[0], SIZE[1]);
    let mut args = vec![
        kind,
        "demo",
        shot,
        "--app-dir",
        app.to_str().unwrap(),
        "--size",
        &size,
        "--set",
        "ground=#ffffff",
        "--no-archive",
        "--out",
        "tmp/out",
    ];
    if kind == "clip" {
        args.extend(["--fps", "30"]);
    }
    prec(dir, &args)
}

fn events(cast: &Path) -> (Value, Vec<Value>) {
    let text = std::fs::read_to_string(cast).unwrap();
    let mut lines = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap());
    (lines.next().unwrap(), lines.collect())
}

fn dark(p: &[u8]) -> bool {
    p.iter().all(|&c| c < 100)
}

struct Edges {
    top: u32,
    left: u32,
    right: u32,
}

fn edges(rgb: &[u8], size: [u32; 2]) -> Edges {
    let at = |x: u32, y: u32| {
        let i = ((y * size[0] + x) * 3) as usize;
        &rgb[i..i + 3]
    };
    let probe = size[0] / 4;
    let top = (0..size[1])
        .find(|&y| dark(at(probe, y)))
        .expect("no window in the frame");
    let row = top + 5;
    let left = (0..size[0]).find(|&x| dark(at(x, row))).unwrap();
    let right = (0..size[0]).rev().find(|&x| dark(at(x, row))).unwrap();
    Edges { top, left, right }
}

fn frames(video: &Path) -> Vec<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(video)
        .args([
            "-vf",
            "scale=in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let one = (SIZE[0] * SIZE[1] * 3) as usize;
    assert_eq!(out.stdout.len() % one, 0);
    out.stdout.chunks(one).map(<[u8]>::to_vec).collect()
}

#[test]
fn a_clip_resizes_its_pty_step_by_step_and_the_window_follows_on_one_canvas() {
    let (dir, app) = case(
        "clip",
        "shrink",
        "clip",
        "Sleep 300ms\nResize 20x6 1s\nType@0ms \"stty size\"\nEnter\nWait+Screen /(?m)^6 20$/\nSleep 400ms\nResize 40x10\nType@0ms \"stty size\"\nEnter\nWait+Screen /(?m)^10 40$/\nSleep 400ms\n",
    );
    let o = shoot(&dir, &app, "clip", "shrink");
    common::expect_ok(&o, "prec clip");
    let out = dir.join("tmp/out/demo/clip");
    let stem = "demo-clip-shrink-320x180-30fps";
    let (header, evs) = events(&out.join(format!("{stem}.cast")));
    assert_eq!(
        (header["width"].as_u64(), header["height"].as_u64()),
        (Some(40), Some(10))
    );
    let resizes: Vec<(f64, String)> = evs
        .iter()
        .filter(|e| e[1] == "r")
        .map(|e| (e[0].as_f64().unwrap(), e[2].as_str().unwrap().to_string()))
        .collect();
    let want = [
        "39x10", "38x10", "37x9", "36x9", "35x9", "34x9", "33x9", "32x8", "31x8", "30x8", "29x8",
        "28x8", "27x7", "26x7", "25x7", "24x7", "23x7", "22x6", "21x6", "20x6", "40x10",
    ];
    assert_eq!(
        resizes.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(),
        want
    );
    assert!(resizes.windows(2).all(|w| w[0].0 <= w[1].0), "{resizes:?}");
    let gaps: Vec<f64> = resizes[..20].windows(2).map(|w| w[1].0 - w[0].0).collect();
    let median = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let ends = median([&gaps[..2], &gaps[gaps.len() - 2..]].concat());
    let middle = median(gaps[gaps.len() / 2 - 1..gaps.len() / 2 + 2].to_vec());
    assert!(
        ends >= 1.5 * middle,
        "not eased: ends {ends}, middle {middle}, {gaps:?}"
    );
    let printed: String = evs
        .iter()
        .filter(|e| e[1] == "o")
        .map(|e| e[2].as_str().unwrap())
        .collect();
    assert!(printed.contains("6 20"), "{printed}");
    assert!(printed.contains("10 40"), "{printed}");
    let m: Value =
        serde_json::from_slice(&std::fs::read(out.join(format!("{stem}.json"))).unwrap()).unwrap();
    assert_eq!(m["grid"]["cols"], 40);
    assert_eq!(m["grid"]["rows"], 10);
    assert_eq!(m["max_grid"]["cols"], 40);
    assert_eq!(m["max_grid"]["rows"], 10);
    assert_eq!(m["max_grid"]["canvas"], m["grid"]["canvas"]);
    let all = frames(&out.join(format!("{stem}.master.mp4")));
    assert!(all.len() > 30, "{} frames", all.len());
    let seen: Vec<Edges> = all.iter().map(|f| edges(f, SIZE)).collect();
    let first = &seen[0];
    let last = &seen[seen.len() - 1];
    let (narrow, mid) = seen
        .iter()
        .enumerate()
        .min_by_key(|(_, e)| e.right)
        .unwrap();
    assert!(narrow > 0 && narrow < seen.len() - 1, "{narrow}");
    assert!(
        first.right.abs_diff(last.right) <= 2,
        "{} {}",
        first.right,
        last.right
    );
    let full = first.right - first.left;
    let small = mid.right - mid.left;
    assert!(
        small * 10 < full * 7 && small * 10 > full * 3,
        "{small} of {full}"
    );
    for e in &seen {
        assert!(
            e.left.abs_diff(first.left) <= 2,
            "left {} vs {}",
            e.left,
            first.left
        );
        assert!(
            e.top.abs_diff(first.top) <= 2,
            "top {} vs {}",
            e.top,
            first.top
        );
        assert!(
            e.right <= first.right + 2,
            "{} past {}",
            e.right,
            first.right
        );
    }
    assert!(
        (2 * first.left + full).abs_diff(SIZE[0]) <= 4,
        "{} {}",
        first.left,
        full
    );
    eprintln!(
        "frame edges: start {}..{}, narrowest {}..{} at frame {narrow}, end {}..{}",
        first.left, first.right, mid.left, mid.right, last.left, last.right
    );
}

#[test]
fn a_still_taken_after_a_resize_shows_the_smaller_window() {
    let (dir, app) = case(
        "still",
        "small",
        "still",
        "Resize 20x6\nType@0ms \"stty size\"\nEnter\nWait+Screen /(?m)^6 20$/\n",
    );
    let o = shoot(&dir, &app, "still", "small");
    common::expect_ok(&o, "prec still");
    let out = dir.join("tmp/out/demo/still");
    let stem = "demo-still-small-320x180";
    let m: Value =
        serde_json::from_slice(&std::fs::read(out.join(format!("{stem}.json"))).unwrap()).unwrap();
    let master = m["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["role"] == "master")
        .and_then(|f| f["file"].as_str())
        .unwrap();
    let img = image::open(out.join(master)).unwrap().to_rgb8();
    assert_eq!(img.dimensions(), (SIZE[0], SIZE[1]));
    let e = edges(img.as_raw(), SIZE);
    let full = SIZE[0] - 2 * e.left;
    let small = e.right - e.left;
    assert!(small * 10 < full * 7, "{small} of {full}");
    assert_eq!(m["grid"]["cols"], 40);
    assert_eq!(m["max_grid"]["cols"], 40);
    assert_eq!(m["max_grid"]["rows"], 10);
    let (_, evs) = events(&out.join(format!("{stem}.cast")));
    let sizes: Vec<&str> = evs
        .iter()
        .filter(|e| e[1] == "r")
        .map(|e| e[2].as_str().unwrap())
        .collect();
    assert_eq!(sizes, ["20x6"]);
}

#[test]
fn a_bad_resize_line_is_refused_before_anything_records() {
    let (dir, app) = case("bad", "bad", "clip", "Sleep 100ms\nResize 0x16\n");
    let o = prec(
        &dir,
        &[
            "record",
            "demo",
            "bad",
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            "tmp/out",
        ],
    );
    assert_eq!(o.status.code(), Some(1));
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(
        said.contains("prec: ")
            && said.contains(
                "line 2: Resize 0x16: each side is 2 to 1000, as in Resize <cols>x<rows> [duration]"
            ),
        "{said}"
    );
    assert!(!said.contains("recording demo"), "{said}");
    assert!(!dir.join("tmp/out/demo/clip/demo-clip-bad.cast").exists());
}
