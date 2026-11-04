use crate::paths;
use pito_codecs::aac;
use pito_codecs::media::{AudioBuffer, Packet, wav};
use pito_codecs::mp4::SampleEntry;
use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;
pub const KBPS: u32 = 320;
pub const PRIMING: i64 = 1024;
const FADE_SECONDS: f64 = 0.005;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tune {
    pub offset: f64,
    pub gain_db: f64,
}

impl Tune {
    pub fn check(self) -> Result<(), String> {
        if !self.offset.is_finite() {
            return Err("--audio-offset takes seconds, like 0.25 or -0.1".into());
        }
        if !self.gain_db.is_finite() {
            return Err("--audio-gain takes decibels, like -6 or 3.5".into());
        }
        Ok(())
    }

    pub fn args(self) -> String {
        format!(
            "audio AAC {KBPS} kb/s, {} kHz stereo, offset {:+} s, gain {:+} dB",
            RATE / 1000,
            self.offset,
            self.gain_db
        )
    }

    pub fn to_json(self) -> Value {
        serde_json::json!({"offset": self.offset, "gain_db": self.gain_db})
    }
}

#[derive(Debug)]
pub struct Source {
    pub file: String,
    pub path: PathBuf,
    pub start: f64,
    bytes: Vec<u8>,
}

pub fn find(cast: &Path, meta: &Value) -> Result<Option<Source>, String> {
    let Some(named) = meta.get("audio").filter(|a| !a.is_null()) else {
        return Ok(None);
    };
    let file = named["file"]
        .as_str()
        .ok_or("the cast's audio names no file")?;
    if Path::new(file).file_name() != Some(OsStr::new(file)) {
        return Err(format!(
            "the cast's audio file {file:?} is not a plain file name"
        ));
    }
    let path = cast.parent().unwrap_or(Path::new(".")).join(file);
    let bytes = std::fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "{}: the cast names this audio file and it is missing",
                path.display()
            )
        } else {
            format!("{}: {e}", path.display())
        }
    })?;
    let want = named["sha256"]
        .as_str()
        .ok_or("the cast's audio names no sha256")?;
    let have = paths::hash_bytes(&bytes);
    if have != want {
        return Err(format!(
            "{}: this is not the audio the cast recorded (sha256 {have}, the cast names {want})",
            path.display()
        ));
    }
    let start = named["start"]
        .as_f64()
        .filter(|s| s.is_finite())
        .ok_or("the cast's audio names no start")?;
    Ok(Some(Source {
        file: file.to_string(),
        path,
        start,
        bytes,
    }))
}

impl Source {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn pcm(&self) -> Result<Vec<f32>, String> {
        let at = |e: &dyn std::fmt::Display| format!("{}: {e}", self.path.display());
        let buf = wav::decode(&self.bytes).map_err(|e| at(&e))?;
        if buf.rate != RATE || buf.channels as usize != CHANNELS {
            return Err(at(&format_args!(
                "the audio is {} Hz in {} channels, prec reads {RATE} Hz in {CHANNELS}",
                buf.rate, buf.channels
            )));
        }
        Ok(buf.samples)
    }
}

pub fn frames_of(count: usize, fps: crate::fps::Fps) -> usize {
    let num = u128::from(fps.num.max(1));
    let total = count as u128 * u128::from(fps.den) * u128::from(RATE);
    usize::try_from((total + num / 2) / num).unwrap_or(usize::MAX)
}

fn ease(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

fn edge(k: usize, len: usize, ramp: usize) -> f32 {
    let from_end = len - 1 - k;
    let near = k.min(from_end);
    if near >= ramp {
        return 1.0;
    }
    ease((near + 1) as f32 / (ramp + 1) as f32)
}

pub fn arrange(
    pcm: &[f32],
    start: f64,
    segments: &[(f64, f64)],
    tune: Tune,
    frames: usize,
) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * CHANNELS];
    let gain = 10f64.powf(tune.gain_db / 20.0) as f32;
    let have = (pcm.len() / CHANNELS) as i64;
    let rate = f64::from(RATE);
    let fade = (FADE_SECONDS * rate).round() as usize;
    let mut shown = 0.0;
    for &(s, e) in segments {
        let from = (shown * rate).round() as i64;
        let to = ((shown + (e - s)) * rate).round() as i64;
        shown += e - s;
        let first = ((s - start - tune.offset) * rate).round() as i64;
        let len = (to - from).max(0) as usize;
        let ramp = fade.min(len / 2);
        for k in 0..len {
            let o = from + k as i64;
            if o >= frames as i64 {
                break;
            }
            let i = first + k as i64;
            if o < 0 || i < 0 || i >= have {
                continue;
            }
            let w = gain * edge(k, len, ramp);
            for c in 0..CHANNELS {
                out[o as usize * CHANNELS + c] = pcm[i as usize * CHANNELS + c] * w;
            }
        }
    }
    for v in &mut out {
        *v = v.clamp(-1.0, 1.0);
    }
    out
}

pub struct Bed<'a> {
    pcm: &'a [f32],
    enc: aac::Encoder,
    fed: usize,
}

impl<'a> Bed<'a> {
    pub fn new(pcm: &'a [f32]) -> Result<Bed<'a>, String> {
        let enc = aac::Encoder::new(aac::EncoderConfig {
            rate: RATE,
            channels: CHANNELS as u16,
            bitrate: KBPS * 1000,
        })
        .map_err(|e| format!("aac: {e}"))?;
        Ok(Bed { pcm, enc, fed: 0 })
    }

    pub fn frames(&self) -> usize {
        self.pcm.len() / CHANNELS
    }

    pub fn entry(&self) -> SampleEntry {
        SampleEntry::aac(self.enc.extradata(), CHANNELS as u32, RATE, KBPS * 1000)
    }

    pub fn through(&mut self, upto: usize) -> Result<Vec<Packet>, String> {
        let upto = upto.min(self.frames());
        if upto <= self.fed {
            return Ok(Vec::new());
        }
        let chunk = AudioBuffer {
            samples: self.pcm[self.fed * CHANNELS..upto * CHANNELS].to_vec(),
            rate: RATE,
            channels: CHANNELS as u16,
        };
        self.fed = upto;
        self.enc.encode(&chunk).map_err(|e| format!("aac: {e}"))
    }

    pub fn finish(&mut self) -> Result<Vec<Packet>, String> {
        let mut packets = self.through(self.frames())?;
        packets.extend(self.enc.flush());
        Ok(packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fps::Fps;

    fn ramp_pcm(frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| [(i + 1) as f32 / 1e6, -((i + 1) as f32) / 1e6])
            .collect()
    }

    fn flat(frames: usize, level: f32) -> Vec<f32> {
        vec![level; frames * CHANNELS]
    }

    fn at(out: &[f32], frame: usize) -> f32 {
        out[frame * CHANNELS]
    }

    #[test]
    fn a_shown_stretch_plays_the_samples_of_its_cast_time_and_a_hidden_one_is_cut() {
        let pcm = flat(48_000 * 4, 0.5);
        let segments = [(0.5, 1.0), (2.0, 2.5)];
        let out = arrange(&pcm, 0.0, &segments, Tune::default(), 48_000 * 2);
        assert_eq!(out.len(), 48_000 * 2 * CHANNELS);
        assert_eq!(at(&out, 12_000), 0.5);
        assert_eq!(at(&out, 23_000), 0.5);
        assert_eq!(at(&out, 24_500), 0.5);
        assert_eq!(at(&out, 47_000), 0.5);
        assert_eq!(at(&out, 48_000), 0.0);
        assert_eq!(at(&out, 90_000), 0.0);
    }

    #[test]
    fn the_samples_follow_the_cast_clock_from_the_recordings_start() {
        let pcm = ramp_pcm(48_000);
        let out = arrange(&pcm, 0.25, &[(0.5, 0.75)], Tune::default(), 12_000);
        assert!((at(&out, 6_000) - (12_000.0 + 6_000.0 + 1.0) / 1e6).abs() < 1e-9);
        assert_eq!(out[6_000 * 2 + 1], -at(&out, 6_000));
    }

    #[test]
    fn a_positive_offset_plays_the_sound_later_and_a_negative_one_earlier() {
        let pcm = ramp_pcm(48_000);
        let seg = [(0.0, 0.5)];
        let late = arrange(
            &pcm,
            0.0,
            &seg,
            Tune {
                offset: 0.1,
                gain_db: 0.0,
            },
            24_000,
        );
        assert_eq!(at(&late, 100), 0.0);
        assert!((at(&late, 4_800 + 1_000) - 1_001.0 / 1e6).abs() < 1e-9);
        let early = arrange(
            &pcm,
            0.0,
            &seg,
            Tune {
                offset: -0.1,
                gain_db: 0.0,
            },
            24_000,
        );
        assert!((at(&early, 1_000) - (4_800.0 + 1_000.0 + 1.0) / 1e6).abs() < 1e-9);
    }

    #[test]
    fn gain_scales_by_decibels_and_never_leaves_full_scale() {
        let pcm = flat(48_000, 0.25);
        let seg = [(0.0, 1.0)];
        let louder = arrange(
            &pcm,
            0.0,
            &seg,
            Tune {
                offset: 0.0,
                gain_db: 6.0,
            },
            48_000,
        );
        assert!((at(&louder, 20_000) - 0.25 * 10f32.powf(0.3)).abs() < 1e-6);
        let hot = arrange(
            &pcm,
            0.0,
            &seg,
            Tune {
                offset: 0.0,
                gain_db: 40.0,
            },
            48_000,
        );
        assert_eq!(at(&hot, 20_000), 1.0);
        let quiet = arrange(
            &pcm,
            0.0,
            &seg,
            Tune {
                offset: 0.0,
                gain_db: -20.0,
            },
            48_000,
        );
        assert!((at(&quiet, 20_000) - 0.025).abs() < 1e-6);
    }

    #[test]
    fn every_cut_eases_in_and_out_over_five_milliseconds() {
        let pcm = flat(48_000, 1.0);
        let out = arrange(
            &pcm,
            0.0,
            &[(0.0, 0.5), (0.6, 0.9)],
            Tune::default(),
            48_000,
        );
        let fade = 240;
        let mut last = 0.0;
        for k in 0..fade {
            let v = at(&out, k);
            assert!(v > last && v < 1.0, "{k} {v}");
            last = v;
        }
        assert_eq!(at(&out, fade), 1.0);
        let mut last = 0.0;
        for k in 0..fade {
            let v = at(&out, 24_000 - 1 - k);
            assert!(v > last && v < 1.0, "{k} {v}");
            last = v;
        }
        assert_eq!(at(&out, 24_000 - 1 - fade), 1.0);
        let join = 24_000;
        assert!(at(&out, join) < 0.01);
        assert_eq!(at(&out, join + 1_000), 1.0);
    }

    #[test]
    fn the_result_is_padded_with_silence_or_trimmed_to_the_length_asked() {
        let pcm = flat(48_000, 0.5);
        let seg = [(0.0, 1.0)];
        let long = arrange(&pcm, 0.0, &seg, Tune::default(), 96_000);
        assert_eq!(long.len(), 96_000 * CHANNELS);
        assert_eq!(at(&long, 40_000), 0.5);
        assert_eq!(at(&long, 60_000), 0.0);
        let short = arrange(&pcm, 0.0, &seg, Tune::default(), 10_000);
        assert_eq!(short.len(), 10_000 * CHANNELS);
        assert_eq!(at(&short, 9_000), 0.5);
        assert!(
            arrange(&pcm, 0.0, &[], Tune::default(), 5)
                .iter()
                .all(|v| *v == 0.0)
        );
    }

    #[test]
    fn audio_before_or_after_the_recording_is_silence() {
        let pcm = flat(4_800, 0.5);
        let out = arrange(&pcm, 1.0, &[(0.0, 2.0)], Tune::default(), 96_000);
        assert_eq!(at(&out, 10_000), 0.0);
        assert_eq!(at(&out, 48_000 + 2_000), 0.5);
        assert_eq!(at(&out, 48_000 + 6_000), 0.0);
    }

    #[test]
    fn the_audio_of_a_master_is_as_long_as_its_frames_at_any_rate() {
        assert_eq!(frames_of(30, Fps::new(30, 1)), 48_000);
        assert_eq!(frames_of(1, Fps::new(30, 1)), 1_600);
        assert_eq!(frames_of(30000, Fps::new(30000, 1001)), 48_048_000);
        assert_eq!(frames_of(24, Fps::new(24000, 1001)), 48_048);
    }

    #[test]
    fn a_tune_must_be_finite_and_says_itself_in_the_encode_args() {
        assert!(Tune::default().check().is_ok());
        assert!(
            Tune {
                offset: f64::NAN,
                gain_db: 0.0
            }
            .check()
            .is_err()
        );
        assert!(
            Tune {
                offset: 0.0,
                gain_db: f64::INFINITY
            }
            .check()
            .is_err()
        );
        assert_eq!(
            Tune {
                offset: 0.25,
                gain_db: -3.0
            }
            .args(),
            "audio AAC 320 kb/s, 48 kHz stereo, offset +0.25 s, gain -3 dB"
        );
    }

    #[test]
    fn a_bed_encodes_the_same_packets_however_it_is_fed() {
        let pcm: Vec<f32> = (0..48_000 * 2)
            .map(|i| ((i / 2) as f32 * 0.05).sin() * 0.4)
            .collect();
        let whole = Bed::new(&pcm).unwrap().finish().unwrap();
        let mut bed = Bed::new(&pcm).unwrap();
        let mut parts = Vec::new();
        for k in 1..=30 {
            parts.extend(bed.through(k * 1_600).unwrap());
        }
        parts.extend(bed.finish().unwrap());
        assert_eq!(whole.len(), parts.len());
        assert!(whole.iter().zip(&parts).all(|(a, b)| a.data == b.data));
        assert_eq!(whole.len(), 48_000usize.div_ceil(1024) + 1);
    }
}
