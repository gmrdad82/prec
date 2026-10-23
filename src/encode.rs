use crate::fps::Fps;
use pito_codecs::filter::{ColorSpec, Order, Packed, packed_to_yuv};
use pito_codecs::h264::{Encoder, EncoderConfig, Preset, Profile};
use pito_codecs::media::{Bitstream, Frame, Packet, PixelFormat, Rate, Rgba, TimeBase};
use pito_codecs::mp4::{AvcConfig, Reader, SampleEntry, TrackConfig, TrackKind, Writer};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

pub const COLOR_709: &str = "BT.709 primaries, transfer and matrix, limited range (SDR)";
pub const LANCZOS: &str = "Lanczos3 in linear light";
pub const CODECS: &str = "2.0.0";
const THREADS: usize = 16;
const CRF: u8 = 14;
const B_FRAMES: u8 = 0;

pub fn master_what() -> String {
    format!(
        "master: H.264 High by pito-codecs {CODECS}, CRF {CRF}, 8-bit 4:2:0 BT.709 limited range"
    )
}

pub fn settings(fps: Fps) -> Vec<String> {
    vec![
        format!("pito-codecs {CODECS}"),
        "h264 High, preset quality".to_string(),
        format!("crf {CRF}"),
        format!("gop {}", fps.gop()),
        format!("bframes {B_FRAMES}"),
        format!("threads {THREADS}"),
        "BT.709 limited range, left-sited 4:2:0 chroma".to_string(),
        "mp4 faststart, no audio".to_string(),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pic<'a> {
    One(&'a Path),
    Blend(&'a Path, &'a Path, f32),
}

struct Decoded {
    png: pito_codecs::png::Decoder,
    rgba: Rgba,
    other: Rgba,
    bytes: Vec<u8>,
}

impl Decoded {
    fn load(&mut self, path: &Path, other: bool) -> Result<(), String> {
        self.bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let out = if other {
            &mut self.other
        } else {
            &mut self.rgba
        };
        self.png
            .decode_into(&self.bytes, out)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    fn frame(&mut self, pic: Pic, size: [u32; 2]) -> Result<Frame, String> {
        match pic {
            Pic::One(p) => self.load(p, false)?,
            Pic::Blend(a, b, w) => {
                self.load(a, false)?;
                self.load(b, true)?;
                if self.other.data.len() != self.rgba.data.len() {
                    return Err(format!("{}: not the size of {}", b.display(), a.display()));
                }
                for (x, y) in self.rgba.data.iter_mut().zip(&self.other.data) {
                    *x = (*x as f32 * (1.0 - w) + *y as f32 * w).round() as u8;
                }
            }
        }
        if [self.rgba.width, self.rgba.height] != size {
            return Err(format!(
                "a frame is {}x{}, the master is {}x{}",
                self.rgba.width, self.rgba.height, size[0], size[1]
            ));
        }
        let mut frame = Frame::new(
            PixelFormat::Yuv420p8,
            size[0],
            size[1],
            0,
            TimeBase::new(1, 1),
        );
        let src = Packed::new(size[0], size[1], Order::Rgbx, &self.rgba.data);
        packed_to_yuv(src, ColorSpec::BT709, PixelFormat::Yuv420p8, &mut frame)
            .map_err(|e| e.to_string())?;
        Ok(frame)
    }
}

struct Line {
    next: usize,
    wanted: usize,
    ready: BTreeMap<usize, Result<Frame, String>>,
    stop: bool,
}

pub fn master(
    pics: &[Pic],
    size: [u32; 2],
    fps: Fps,
    output: &Path,
) -> Result<Vec<String>, String> {
    if pics.is_empty() {
        return Err("the clip has no frames".into());
    }
    let fresh: Vec<usize> = (0..pics.len())
        .filter(|&i| i == 0 || pics[i] != pics[i - 1])
        .collect();
    crate::paths::confine_here();
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(2, 8);
    let line = Mutex::new(Line {
        next: 0,
        wanted: 0,
        ready: BTreeMap::new(),
        stop: false,
    });
    let moved = Condvar::new();
    let lock = || line.lock().unwrap_or_else(|e| e.into_inner());
    let work = || {
        let mut d = Decoded {
            png: pito_codecs::png::Decoder::new(),
            rgba: Rgba::new(0, 0),
            other: Rgba::new(0, 0),
            bytes: Vec::new(),
        };
        loop {
            let k = {
                let mut q = lock();
                while !q.stop && q.next < fresh.len() && q.next >= q.wanted + 2 * workers {
                    q = moved.wait(q).unwrap_or_else(|e| e.into_inner());
                }
                if q.stop || q.next >= fresh.len() {
                    return;
                }
                q.next += 1;
                q.next - 1
            };
            let made = d.frame(pics[fresh[k]], size);
            lock().ready.insert(k, made);
            moved.notify_all();
        }
    };
    let take = |k: usize| -> Result<Frame, String> {
        let mut q = lock();
        q.wanted = k;
        moved.notify_all();
        loop {
            if let Some(made) = q.ready.remove(&k) {
                q.wanted = k + 1;
                moved.notify_all();
                return made;
            }
            q = moved.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    };
    let encode = || -> Result<(), String> {
        let named = |e: &dyn std::fmt::Display| format!("{}: {e}", output.display());
        let mut enc = Encoder::new(EncoderConfig {
            width: size[0],
            height: size[1],
            fps: (fps.num, fps.den),
            profile: Profile::High,
            preset: Preset::Quality,
            rate: Rate::Crf(CRF),
            gop: fps.gop(),
            b_frames: B_FRAMES,
            threads: THREADS,
            bitstream: Bitstream::LengthPrefixed,
        })
        .map_err(|e| named(&e))?;
        let avc = AvcConfig::parse(&enc.avcc()).map_err(|e| named(&e))?;
        let mut mux = Writer::create(output).map_err(|e| named(&e))?;
        let track = mux
            .add_track(TrackConfig::new(
                SampleEntry::h264(avc, size[0] as u16, size[1] as u16),
                fps.num,
            ))
            .map_err(|e| named(&e))?;
        let mut held: Option<Packet> = None;
        let mut write = |next: Option<Packet>| -> Result<(), String> {
            if let Some(h) = held.take() {
                let duration = next
                    .as_ref()
                    .map_or(1, |n| u32::try_from((n.dts - h.dts).max(1)).unwrap_or(1));
                let den = i64::from(fps.den);
                mux.push_sample(
                    track,
                    &h.data,
                    h.pts * den,
                    h.dts * den,
                    duration * fps.den,
                    h.keyframe,
                )
                .map_err(|e| named(&e))?;
            }
            held = next;
            Ok(())
        };
        let mut current: Option<Frame> = None;
        let mut k = 0;
        for n in 0..pics.len() {
            if fresh.get(k) == Some(&n) {
                current = Some(take(k)?);
                k += 1;
            }
            let f = current.as_mut().ok_or("the first frame is missing")?;
            f.pts = n as i64;
            f.time_base = TimeBase::new(fps.den, fps.num);
            for p in enc.encode(f).map_err(|e| named(&e))? {
                write(Some(p))?;
            }
            crate::jobs::progress(n + 1);
        }
        for p in enc.flush().map_err(|e| named(&e))? {
            write(Some(p))?;
        }
        write(None)?;
        mux.finish_faststart().map_err(|e| named(&e))?;
        Ok(())
    };
    let done = std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(work);
        }
        let done = encode();
        lock().stop = true;
        moved.notify_all();
        done
    });
    if done.is_err() {
        let _ = std::fs::remove_file(output);
    }
    done.map(|()| settings(fps))
}

pub fn probe(video: &Path) -> Result<([u32; 2], usize, Option<Fps>), String> {
    let r = Reader::open(video).map_err(|e| format!("{}: {e}", video.display()))?;
    let t = r
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Video)
        .ok_or_else(|| format!("{}: no video track", video.display()))?;
    let rate = t
        .samples
        .first()
        .and_then(|s| Fps::reduced(t.timescale, s.duration));
    Ok(([t.width, t.height], t.samples.len(), rate))
}

pub fn temp_of(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    path.with_file_name(name)
}

pub fn atomic(path: &Path, write: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
    let temp = temp_of(path);
    let done = write(&temp).and_then(|()| {
        std::fs::rename(&temp, path).map_err(|e| format!("{}: {e}", path.display()))
    });
    if done.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    done
}

pub fn atomic_file(
    path: &Path,
    write: impl FnOnce(&mut dyn Write) -> Result<(), String>,
) -> Result<(), String> {
    atomic(path, |temp| {
        let named = |e: std::io::Error| format!("{}: {e}", temp.display());
        let file = std::fs::File::create(temp).map_err(named)?;
        let mut buffered = std::io::BufWriter::new(file);
        write(&mut buffered)?;
        let file = buffered.into_inner().map_err(|e| named(e.into_error()))?;
        file.sync_all().map_err(named)
    })
}

pub fn poster(frame: &Path, output: &Path) -> Result<(), String> {
    let img = image::open(frame)
        .map_err(|e| format!("{}: {e}", frame.display()))?
        .to_rgb8();
    write_png8(output, [img.width(), img.height()], img.as_raw())
}

fn png_to<W: Write>(
    out: W,
    size: [u32; 2],
    depth: png::BitDepth,
    data: &[u8],
) -> Result<(), String> {
    let mut enc = png::Encoder::new(out, size[0], size[1]);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(depth);
    enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(data).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())
}

pub fn write_png8(path: &Path, size: [u32; 2], rgb: &[u8]) -> Result<(), String> {
    atomic_file(path, |w| png_to(w, size, png::BitDepth::Eight, rgb))
}

pub fn write_png16(path: &Path, size: [u32; 2], rgb8: &[u8]) -> Result<(), String> {
    let wide: Vec<u8> = rgb8.iter().flat_map(|&v| [v, v]).collect();
    atomic_file(path, |w| png_to(w, size, png::BitDepth::Sixteen, &wide))
}

fn to8(v: u16) -> u8 {
    ((v as u32 * 255 + 32767) / 65535) as u8
}

fn lin(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn delin(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

pub type Linear = image::ImageBuffer<image::Rgb<f32>, Vec<f32>>;

pub fn linear(rgb8: &image::RgbImage) -> Linear {
    let table: [f32; 256] = std::array::from_fn(|v| lin(v as f32 / 255.0));
    let (w, h) = rgb8.dimensions();
    let data = rgb8.as_raw().iter().map(|&v| table[v as usize]).collect();
    Linear::from_raw(w, h, data).expect("linear image")
}

pub fn resize8(linear: &Linear, size: [u32; 2]) -> Vec<u8> {
    image::imageops::resize(
        linear,
        size[0],
        size[1],
        image::imageops::FilterType::Lanczos3,
    )
    .into_raw()
    .into_iter()
    .map(|v| to8((delin(v) * 65535.0).round() as u16))
    .collect()
}

pub fn file_entry(dir: &Path, path: &Path, what: &str, extra: Value) -> Result<Value, String> {
    let bytes = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    let file = path
        .strip_prefix(dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let mut v = json!({
        "file": file,
        "sha256": crate::paths::hash_file(path)?,
        "bytes": bytes,
        "what": what,
    });
    if let Value::Object(m) = extra {
        for (k, x) in m {
            v[k] = x;
        }
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/unit-encode")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct Short(usize);

    impl Write for Short {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::Error::other("disk full"));
            }
            let n = buf.len().min(self.0);
            self.0 -= n;
            Ok(n)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn a_png_that_fails_halfway_leaves_the_previous_file_and_no_temporary() {
        let dir = work("png");
        let target = dir.join("a.png");
        std::fs::write(&target, b"previous").unwrap();
        let rgb: Vec<u8> = (0..64 * 64 * 3).map(|i| (i * 7 % 251) as u8).collect();
        let rgb16: Vec<u16> = rgb.iter().map(|&v| v as u16 * 257).collect();
        assert!(png_to(Short(100), [64, 64], png::BitDepth::Eight, &rgb).is_err());
        atomic_file(&target, |w| {
            w.write_all(b"half").map_err(|e| e.to_string())?;
            Err("disk full".to_string())
        })
        .unwrap_err();
        assert_eq!(std::fs::read(&target).unwrap(), b"previous");
        assert_eq!(names(&dir), ["a.png"]);
        write_png8(&target, [64, 64], &rgb).unwrap();
        assert_ne!(std::fs::read(&target).unwrap(), b"previous");
        assert_eq!(image::open(&target).unwrap().to_rgb8().into_raw(), rgb);
        let big = dir.join("b.png");
        write_png16(&big, [64, 64], &rgb).unwrap();
        assert_eq!(image::open(&big).unwrap().to_rgb16().into_raw(), rgb16);
        assert_eq!(names(&dir), ["a.png", "b.png"]);
    }

    #[test]
    fn a_png_into_a_missing_folder_leaves_nothing() {
        let dir = work("png-missing");
        let target = dir.join("gone/a.png");
        write_png8(&target, [1, 1], &[0, 0, 0]).unwrap_err();
        write_png16(&target, [1, 1], &[0, 0, 0]).unwrap_err();
        assert!(names(&dir).is_empty());
    }

    #[test]
    fn a_master_that_fails_leaves_no_output() {
        let dir = work("master");
        let target = dir.join("a.master.mp4");
        std::fs::write(&target, b"half").unwrap();
        let missing = dir.join("missing.png");
        master(&[Pic::One(&missing)], [64, 64], Fps::new(30, 1), &target).unwrap_err();
        assert!(names(&dir).is_empty());
    }

    #[test]
    fn a_film_rate_master_reads_back_at_its_exact_rate_and_frame_times() {
        let dir = work("film");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        write_png8(&a, [64, 64], &[40; 64 * 64 * 3]).unwrap();
        write_png8(&b, [64, 64], &[200; 64 * 64 * 3]).unwrap();
        let pics: Vec<Pic> = (0..60)
            .map(|i| Pic::One(if i % 7 < 3 { &a } else { &b }))
            .collect();
        let film = Fps::new(24000, 1001);
        let out = dir.join("a.master.mp4");
        assert!(
            master(&pics, [64, 64], film, &out)
                .unwrap()
                .contains(&"gop 48".to_string())
        );
        assert_eq!(probe(&out).unwrap(), ([64, 64], 60, Some(film)));
        let r = Reader::open(&out).unwrap();
        let t = &r.tracks[0];
        assert_eq!(t.timescale, 24000);
        for (n, s) in t.samples.iter().enumerate() {
            assert_eq!((s.pts, s.duration), (n as i64 * 1001, 1001));
        }
    }

    #[test]
    fn a_temporary_name_never_looks_like_an_output() {
        let temp = temp_of(Path::new("/x/a.master.mp4"));
        assert_eq!(temp, Path::new("/x/a.master.mp4.partial"));
        let target = work("rename").join("a.cast");
        atomic(&target, |t| {
            assert_eq!(t, temp_of(&target));
            std::fs::write(t, b"whole").map_err(|e| e.to_string())
        })
        .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"whole");
        assert!(!temp_of(&target).exists());
    }

    fn png_info(path: &Path) -> (png::BitDepth, [u32; 2], bool) {
        let bytes = std::fs::read(path).unwrap();
        let r = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let i = r.info();
        (i.bit_depth, [i.width, i.height], i.srgb.is_some())
    }

    #[test]
    fn every_rate_keys_a_frame_every_two_seconds() {
        for (f, gop) in [
            (Fps::new(24, 1), 48),
            (Fps::new(30, 1), 60),
            (Fps::new(48, 1), 96),
            (Fps::new(60, 1), 120),
            (Fps::new(120, 1), 240),
            (Fps::new(24000, 1001), 48),
            (Fps::new(30000, 1001), 60),
            (Fps::new(48000, 1001), 96),
            (Fps::new(60000, 1001), 120),
        ] {
            assert_eq!(f.gop(), gop, "{f}");
            assert!(settings(f).contains(&format!("gop {gop}")), "{f}");
        }
    }

    #[test]
    fn a_master_with_no_pictures_is_refused_and_writes_nothing() {
        let dir = work("empty");
        let out = dir.join("a.master.mp4");
        assert_eq!(
            master(&[], [64, 64], Fps::new(30, 1), &out).unwrap_err(),
            "the clip has no frames"
        );
        assert!(names(&dir).is_empty());
    }

    #[test]
    fn a_frame_of_the_wrong_size_fails_the_master_and_leaves_no_file() {
        let dir = work("wrong-size");
        let a = dir.join("a.png");
        write_png8(&a, [32, 32], &[9; 32 * 32 * 3]).unwrap();
        let out = dir.join("a.master.mp4");
        let e = master(&[Pic::One(&a)], [64, 64], Fps::new(30, 1), &out).unwrap_err();
        assert_eq!(e, "a frame is 32x32, the master is 64x64");
        assert_eq!(names(&dir), ["a.png"]);
    }

    #[test]
    fn a_blend_of_two_sizes_fails_the_master_and_leaves_no_file() {
        let dir = work("blend-size");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        write_png8(&a, [64, 64], &[9; 64 * 64 * 3]).unwrap();
        write_png8(&b, [32, 32], &[9; 32 * 32 * 3]).unwrap();
        let out = dir.join("a.master.mp4");
        let e = master(&[Pic::Blend(&a, &b, 0.5)], [64, 64], Fps::new(30, 1), &out).unwrap_err();
        assert_eq!(
            e,
            format!("{}: not the size of {}", b.display(), a.display())
        );
        assert_eq!(names(&dir), ["a.png", "b.png"]);
    }

    #[test]
    fn a_master_that_fails_late_removes_what_it_had_written() {
        let dir = work("late");
        let a = dir.join("a.png");
        write_png8(&a, [64, 64], &[90; 64 * 64 * 3]).unwrap();
        let bad = dir.join("bad.png");
        std::fs::write(&bad, b"not a png").unwrap();
        let b = dir.join("b.png");
        write_png8(&b, [64, 64], &[160; 64 * 64 * 3]).unwrap();
        let mut pics: Vec<Pic> = (0..200)
            .map(|i| Pic::One(if i % 2 == 0 { &a } else { &b }))
            .collect();
        pics.push(Pic::One(&bad));
        pics.push(Pic::One(&a));
        let fresh = pics
            .iter()
            .enumerate()
            .filter(|&(i, p)| i == 0 || *p != pics[i - 1])
            .count();
        assert_eq!(fresh, pics.len());
        let out = dir.join("a.master.mp4");
        let e = master(&pics, [64, 64], Fps::new(30, 1), &out).unwrap_err();
        assert!(e.contains("bad.png"), "{e}");
        assert!(!out.exists());
        assert!(!temp_of(&out).exists());
        assert_eq!(names(&dir), ["a.png", "b.png", "bad.png"]);
    }

    #[test]
    fn a_blend_lands_between_its_two_pictures() {
        let dir = work("blend");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        write_png8(&a, [64, 64], &[40; 64 * 64 * 3]).unwrap();
        write_png8(&b, [64, 64], &[200; 64 * 64 * 3]).unwrap();
        let mut d = Decoded {
            png: pito_codecs::png::Decoder::new(),
            rgba: Rgba::new(0, 0),
            other: Rgba::new(0, 0),
            bytes: Vec::new(),
        };
        d.frame(Pic::Blend(&a, &b, 0.25), [64, 64]).unwrap();
        let rgb: Vec<u8> = d
            .rgba
            .data
            .chunks(4)
            .take(3)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        assert_eq!(rgb, [80; 9]);
        d.frame(Pic::One(&a), [64, 64]).unwrap();
        assert_eq!(&d.rgba.data[..3], [40, 40, 40]);
        let gone = dir.join("gone.png");
        let e = d.frame(Pic::One(&gone), [64, 64]).unwrap_err();
        assert!(e.starts_with(&gone.display().to_string()), "{e}");
    }

    #[test]
    fn a_file_that_is_not_an_mp4_cannot_be_probed() {
        let dir = work("probe");
        let not = dir.join("a.master.mp4");
        std::fs::write(&not, b"text").unwrap();
        let e = probe(&not).unwrap_err();
        assert!(e.starts_with(&not.display().to_string()), "{e}");
        assert!(probe(&dir.join("missing.mp4")).is_err());
    }

    #[test]
    fn a_rename_onto_a_folder_fails_and_leaves_no_temporary() {
        let dir = work("onto-dir");
        let target = dir.join("a.png");
        std::fs::create_dir_all(target.join("inside")).unwrap();
        let e = atomic(&target, |t| {
            std::fs::write(t, b"whole").map_err(|e| e.to_string())
        })
        .unwrap_err();
        assert!(e.starts_with(&target.display().to_string()), "{e}");
        assert!(!temp_of(&target).exists());
        assert!(target.join("inside").is_dir());
        assert_eq!(names(&dir), ["a.png"]);
    }

    #[test]
    fn a_still_master_is_16_bit_and_its_deliveries_and_poster_8_bit_srgb() {
        let dir = work("depths");
        let rgb: Vec<u8> = (0..16 * 8 * 3).map(|i| (i * 13 % 256) as u8).collect();
        let master = dir.join("m.png");
        write_png16(&master, [16, 8], &rgb).unwrap();
        assert_eq!(png_info(&master), (png::BitDepth::Sixteen, [16, 8], true));
        let delivery = dir.join("d.png");
        write_png8(&delivery, [16, 8], &rgb).unwrap();
        assert_eq!(png_info(&delivery), (png::BitDepth::Eight, [16, 8], true));
        let poster_png = dir.join("p.png");
        poster(&master, &poster_png).unwrap();
        assert_eq!(png_info(&poster_png), (png::BitDepth::Eight, [16, 8], true));
        assert_eq!(image::open(&poster_png).unwrap().to_rgb8().into_raw(), rgb);
        let e = poster(&dir.join("gone.png"), &dir.join("q.png")).unwrap_err();
        assert!(e.contains("gone.png"), "{e}");
        assert!(!dir.join("q.png").exists());
    }

    #[test]
    fn a_resize_in_linear_light_keeps_a_flat_colour_and_the_size_asked() {
        for v in [0u8, 1, 30, 128, 254, 255] {
            let img = image::RgbImage::from_pixel(20, 10, image::Rgb([v, v / 2, 255 - v]));
            let out = resize8(&linear(&img), [7, 3]);
            assert_eq!(out.len(), 7 * 3 * 3);
            for px in out.chunks(3) {
                assert_eq!(px, [v, v / 2, 255 - v], "{v}");
            }
        }
        assert_eq!(to8(0), 0);
        assert_eq!(to8(65535), 255);
        assert_eq!(to8(257 * 100), 100);
        assert_eq!(to8(257 * 100 + 128), 100);
        assert_eq!(to8(257 * 100 + 129), 101);
    }

    #[test]
    fn a_file_entry_names_the_file_from_its_folder_with_its_hash_and_size() {
        let dir = work("entry");
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let f = sub.join("a.cast");
        std::fs::write(&f, b"abc").unwrap();
        let e = file_entry(&dir, &f, "cast", json!({"role": "cast", "what": "kept"})).unwrap();
        assert_eq!(
            e,
            json!({
                "file": "sub/a.cast",
                "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                "bytes": 3,
                "what": "kept",
                "role": "cast",
            })
        );
        let outside = file_entry(&sub, &dir.join("sub/a.cast"), "x", Value::Null).unwrap();
        assert_eq!(outside["file"], "a.cast");
        let elsewhere = file_entry(Path::new("/nowhere"), &f, "x", json!([1])).unwrap();
        assert_eq!(elsewhere["file"], f.display().to_string());
        assert_eq!(elsewhere.as_object().unwrap().len(), 4);
        assert!(file_entry(&dir, &dir.join("gone"), "x", Value::Null).is_err());
    }
}
