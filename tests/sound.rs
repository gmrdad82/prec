mod common;

use pito_codecs::aac::Decoder;
use pito_codecs::mp4::{Reader, TrackKind};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const RATE: usize = 48_000;
const FRAME: usize = 1024;
const SILENT_MASTER: &str = "d5ace16a80c988acc79afb1b4c9de85dd4cf6ca0c9839344cce642683eb1bf74";

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn wav_bytes(start: f64, seconds: f64, bursts: &[(f64, f64, f32)]) -> Vec<u8> {
    let frames = (seconds * RATE as f64) as usize;
    let mut pcm = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let t = start + i as f64 / RATE as f64;
        let mut v = 0.0f32;
        for &(from, to, amp) in bursts {
            if t >= from && t < to {
                v = amp * (2.0 * std::f64::consts::PI * 1000.0 * (t - from)).sin() as f32;
            }
        }
        let s = (v * 32767.0).round() as i16;
        pcm.extend_from_slice(&s.to_le_bytes());
        pcm.extend_from_slice(&s.to_le_bytes());
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&(RATE as u32).to_le_bytes());
    out.extend_from_slice(&(RATE as u32 * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(&pcm);
    out
}

struct Tone {
    start: f64,
    end: f64,
    bursts: Vec<(f64, f64, f32)>,
}

fn write_cast(
    dir: &Path,
    name: &str,
    markers: &[(f64, &str)],
    end: f64,
    tone: Option<&Tone>,
) -> PathBuf {
    let path = dir.join(format!("{name}.cast"));
    let mut meta = json!({
        "product": "sound", "shot": name, "kind": "clip", "grid": [8, 4],
        "app_theme": common::theme("sound"), "frame": "bare", "closure": null,
        "app": {"rev": "local"},
    });
    if let Some(t) = tone {
        let bytes = wav_bytes(t.start, t.end - t.start, &t.bursts);
        std::fs::write(dir.join(format!("{name}.wav")), &bytes).unwrap();
        meta["audio"] = json!({
            "file": format!("{name}.wav"), "sha256": hex(&bytes),
            "start": t.start, "rate": RATE, "channels": 2,
        });
    }
    let header = json!({"version": 2, "width": 8, "height": 4, "x_terminal_renderer": meta});
    let mut lines = vec![
        header.to_string(),
        json!([0.0, "o", format!("\x1b[?25l{}", common::paint(200, 40, 40))]).to_string(),
        json!([0.0, "m", "show"]).to_string(),
    ];
    for (t, m) in markers {
        lines.push(json!([t, "m", m]).to_string());
    }
    lines.push(json!([end, "m", "end"]).to_string());
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    path
}

fn render(w: &Path, cast: &Path, out: &str, extra: &[&str]) -> std::process::Output {
    let mut args = vec![
        "render",
        cast.to_str().unwrap(),
        "--size",
        "160x90",
        "--fps",
        "30",
        "--out",
        out,
    ];
    args.extend_from_slice(extra);
    common::run(w, &args)
}

fn rendered(w: &Path, cast: &Path, out: &str, extra: &[&str]) -> (PathBuf, Value) {
    let o = render(w, cast, out, extra);
    common::expect_ok(&o, "prec render");
    let manifest = common::last_path(&o);
    let value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    (manifest, value)
}

fn master_of(manifest: &Path, m: &Value) -> PathBuf {
    let f = m["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["role"] == "master")
        .unwrap();
    manifest.parent().unwrap().join(f["file"].as_str().unwrap())
}

fn heard(master: &Path) -> Vec<f32> {
    let bytes = std::fs::read(master).unwrap();
    let r = Reader::open(master).unwrap();
    let t = r
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Audio)
        .expect("an audio track");
    assert_eq!(t.edits.len(), 1);
    let mut dec = Decoder::new(&t.extradata().unwrap()).unwrap();
    let mut left = Vec::new();
    for s in &t.samples {
        let data = &bytes[s.offset as usize..s.offset as usize + s.size as usize];
        let buf = dec.decode_frame(data).unwrap();
        assert_eq!(buf.channels, 2);
        left.extend(buf.samples.as_chunks::<2>().0.iter().map(|f| f[0]));
    }
    let skip = t.edits[0].media_time as usize;
    let len = t.edits[0].segment_duration as usize;
    assert!(left.len() >= skip + len, "{} < {}", left.len(), skip + len);
    left[skip..skip + len].to_vec()
}

fn bursts(pcm: &[f32]) -> Vec<(usize, usize)> {
    let window = 48;
    let live: Vec<bool> = pcm
        .chunks(window)
        .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.08)
        .collect();
    let mut found: Vec<(usize, usize)> = Vec::new();
    let mut k = 0;
    while k < live.len() {
        if live[k] {
            let from = k;
            while k < live.len() && live[k] {
                k += 1;
            }
            match found.last_mut() {
                Some(last) if from * window - last.1 < 480 => last.1 = k * window,
                _ => found.push((from * window, k * window)),
            }
        } else {
            k += 1;
        }
    }
    found.retain(|(a, b)| b - a >= 480);
    found
}

fn peak(pcm: &[f32]) -> f32 {
    pcm.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

fn near(got: usize, seconds: f64, what: &str) {
    let want = seconds * RATE as f64;
    assert!(
        (got as f64 - want).abs() <= FRAME as f64,
        "{what}: sample {got}, wanted about {want}"
    );
}

fn every_file_is_what_the_manifest_says(manifest: &Path, m: &Value) {
    for f in m["files"].as_array().unwrap() {
        let bytes =
            std::fs::read(manifest.parent().unwrap().join(f["file"].as_str().unwrap())).unwrap();
        assert_eq!(f["sha256"].as_str().unwrap(), hex(&bytes), "{}", f["file"]);
        assert_eq!(f["bytes"].as_u64().unwrap(), bytes.len() as u64);
    }
}

fn cut_cast(w: &Path, name: &str) -> PathBuf {
    let tone = Tone {
        start: 0.25,
        end: 3.0,
        bursts: vec![
            (0.45, 0.55, 0.5),
            (0.8, 0.95, 0.5),
            (1.3, 1.5, 0.5),
            (2.5, 2.7, 0.5),
        ],
    };
    write_cast(
        w,
        name,
        &[(0.4, "cut"), (0.6, "resume"), (1.0, "hide"), (2.0, "show")],
        3.0,
        Some(&tone),
    )
}

#[test]
fn the_sound_lands_at_its_shown_time_across_a_hidden_stretch_and_a_squeeze() {
    let w = common::scratch("sound", "cuts");
    let cast = cut_cast(&w, "cuts");
    let (manifest, m) = rendered(&w, &cast, "out", &[]);
    assert_eq!(m["frames"]["shown_seconds"], json!(1.8));
    assert_eq!(m["frames"]["master"], json!(54));
    let pcm = heard(&master_of(&manifest, &m));
    assert_eq!(pcm.len(), 54 * 1600);
    let found = bursts(&pcm);
    assert_eq!(found.len(), 2, "{found:?}");
    near(found[0].0, 0.6, "the burst before the hidden stretch");
    near(found[1].0, 1.3, "the burst after it");
    near(found[0].1 - found[0].0, 0.15, "the first burst's length");
    near(found[1].1 - found[1].0, 0.2, "the second burst's length");
    assert!(peak(&pcm[found[0].0..found[0].1]) > 0.4);
    every_file_is_what_the_manifest_says(&manifest, &m);
}

#[test]
fn an_offset_moves_the_sound_and_a_gain_scales_it() {
    let w = common::scratch("sound", "tune");
    let tone = Tone {
        start: 0.0,
        end: 2.0,
        bursts: vec![(0.5, 0.8, 0.5)],
    };
    let cast = write_cast(&w, "tune", &[], 2.0, Some(&tone));
    let at = |name: &str, extra: &[&str]| {
        let (manifest, m) = rendered(&w, &cast, name, extra);
        let pcm = heard(&master_of(&manifest, &m));
        let found = bursts(&pcm);
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        (found[0], peak(&pcm), m)
    };
    let (plain, plain_peak, _) = at("plain", &[]);
    near(plain.0, 0.5, "plain");
    assert!(plain_peak > 0.4 && plain_peak < 0.6, "{plain_peak}");
    let (late, _, m) = at("late", &["--audio-offset", "0.25"]);
    near(late.0, 0.75, "later");
    near(late.1 - late.0, 0.3, "later length");
    let args = m["encodes"][0]["args"].as_array().unwrap();
    let last = args.last().unwrap().as_str().unwrap();
    assert_eq!(
        last,
        "audio AAC 320 kb/s, 48 kHz stereo, offset +0.25 s, gain +0 dB"
    );
    assert!(
        !args
            .iter()
            .any(|a| a.as_str().unwrap().contains("no audio"))
    );
    assert_eq!(
        m["params"]["audio"],
        json!({"offset": 0.25, "gain_db": 0.0})
    );
    let (early, _, _) = at("early", &["--audio-offset=-0.2"]);
    near(early.0, 0.3, "earlier");
    let (_, quiet, m) = at("quiet", &["--audio-gain", "-6"]);
    let ratio = quiet / plain_peak;
    assert!((ratio - 0.501).abs() < 0.03, "{ratio}");
    let last = m["encodes"][0]["args"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    assert!(last.ends_with("offset +0 s, gain -6 dB"), "{last}");
    let (_, loud, _) = at("loud", &["--audio-gain", "6"]);
    let ratio = loud / plain_peak;
    assert!((ratio - 1.995).abs() < 0.08, "{ratio}");
}

#[test]
fn the_same_cast_wav_and_arguments_give_a_byte_identical_master() {
    let w = common::scratch("sound", "same");
    let cast = cut_cast(&w, "same");
    let args = ["--audio-offset", "0.1", "--audio-gain", "-3"];
    let (ma, a) = rendered(&w, &cast, "one", &args);
    let (mb, b) = rendered(&w, &cast, "two", &args);
    assert_eq!(
        std::fs::read(master_of(&ma, &a)).unwrap(),
        std::fs::read(master_of(&mb, &b)).unwrap()
    );
}

#[test]
fn a_missing_or_changed_wav_is_refused_naming_the_file() {
    let w = common::scratch("sound", "refused");
    let cast = cut_cast(&w, "gone");
    let wav = w.join("gone.wav");
    let kept = std::fs::read(&wav).unwrap();
    std::fs::remove_file(&wav).unwrap();
    let o = render(&w, &cast, "out", &[]);
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains(&wav.display().to_string()), "{said}");
    assert!(said.contains("missing"), "{said}");
    let mut changed = kept.clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    std::fs::write(&wav, &changed).unwrap();
    let o = render(&w, &cast, "out", &[]);
    assert!(!o.status.success());
    let said = String::from_utf8_lossy(&o.stderr);
    assert!(said.contains(&wav.display().to_string()), "{said}");
    assert!(said.contains("not the audio the cast recorded"), "{said}");
    assert!(!w.join("out").exists());
    std::fs::write(&wav, &kept).unwrap();
    common::expect_ok(&render(&w, &cast, "out", &[]), "prec render");
}

#[test]
fn the_delivered_cast_brings_its_wav_and_renders_again_from_there() {
    let w = common::scratch("sound", "deliver");
    let cast = cut_cast(&w, "deliver");
    let (manifest, m) = rendered(&w, &cast, "out", &[]);
    let dir = manifest.parent().unwrap();
    let entry = m["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["role"] == "audio")
        .expect("an audio output");
    assert_eq!(entry["file"], "deliver.wav");
    assert_eq!(
        entry["sha256"].as_str().unwrap(),
        hex(&std::fs::read(w.join("deliver.wav")).unwrap())
    );
    assert!(dir.join("deliver.wav").is_file());
    let copied = m["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["role"] == "cast")
        .unwrap();
    let again = dir.join(copied["file"].as_str().unwrap());
    let (mb, b) = rendered(&w, &again, "again", &[]);
    assert_eq!(
        std::fs::read(master_of(&manifest, &m)).unwrap(),
        std::fs::read(master_of(&mb, &b)).unwrap()
    );
    assert!(
        m["outputs"]["master"]
            .as_str()
            .unwrap()
            .contains("AAC 320 kb/s")
    );
}

#[test]
fn a_silent_cast_keeps_today_master_with_no_audio_track() {
    let w = common::scratch("sound", "silent");
    let cast = write_cast(&w, "quiet", &[], 1.0, None);
    let (manifest, m) = rendered(&w, &cast, "out", &[]);
    let master = master_of(&manifest, &m);
    assert!(
        Reader::open(&master)
            .unwrap()
            .tracks
            .iter()
            .all(|t| t.kind != TrackKind::Audio)
    );
    assert_eq!(hex(&std::fs::read(&master).unwrap()), SILENT_MASTER);
    assert!(
        m["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["role"] != "audio")
    );
    assert!(m["params"].get("audio").is_none());
    let last = m["encodes"][0]["args"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last, "mp4 faststart, no audio");
    let (mb, b) = rendered(&w, &cast, "flagged", &["--audio-gain", "-3"]);
    assert_eq!(
        std::fs::read(&master).unwrap(),
        std::fs::read(master_of(&mb, &b)).unwrap()
    );
}

#[test]
fn the_audio_flags_are_for_clips_and_must_be_numbers() {
    let w = common::scratch("sound", "flags");
    let still = {
        let path = w.join("shot.cast");
        let text = std::fs::read_to_string(write_cast(&w, "shot", &[], 1.0, None)).unwrap();
        std::fs::write(
            &path,
            text.replace("\"kind\":\"clip\"", "\"kind\":\"still\""),
        )
        .unwrap();
        path
    };
    let o = render(&w, &still, "out", &["--audio-offset", "1"]);
    assert!(!o.status.success());
    assert!(
        String::from_utf8_lossy(&o.stderr)
            .contains("--audio-offset and --audio-gain are for clip and render")
    );
    let cast = write_cast(&w, "quiet", &[], 1.0, None);
    for bad in ["nan", "inf"] {
        let o = render(&w, &cast, "out", &["--audio-gain", bad]);
        assert!(!o.status.success(), "{bad}");
    }
    let o = common::run(&w, &["render", "--help"]);
    let help = String::from_utf8_lossy(&o.stdout);
    assert!(help.contains("--audio-offset"), "{help}");
    assert!(help.contains("--audio-gain"), "{help}");
}
