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
pub const CODECS: &str = "1.15.1";
const THREADS: usize = 16;
const CRF: u8 = 14;
const B_FRAMES: u8 = 0;

pub fn master_what() -> String {
    format!(
        "master: H.264 High by pito-codecs {CODECS}, CRF {CRF}, 8-bit 4:2:0 BT.709 limited range"
    )
}

pub fn settings(fps: u32) -> Vec<String> {
    vec![
        format!("pito-codecs {CODECS}"),
        "h264 High, preset quality".to_string(),
        format!("crf {CRF}"),
        format!("gop {}", 2 * fps),
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
    fps: u32,
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
            fps: (fps, 1),
            profile: Profile::High,
            preset: Preset::Quality,
            rate: Rate::Crf(CRF),
            gop: 2 * fps,
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
                fps,
            ))
            .map_err(|e| named(&e))?;
        let mut held: Option<Packet> = None;
        let mut write = |next: Option<Packet>| -> Result<(), String> {
            if let Some(h) = held.take() {
                let duration = next
                    .as_ref()
                    .map_or(1, |n| u32::try_from((n.dts - h.dts).max(1)).unwrap_or(1));
                mux.push_sample(track, &h.data, h.pts, h.dts, duration, h.keyframe)
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
            f.time_base = TimeBase::new(1, fps);
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

pub fn probe(video: &Path) -> Result<([u32; 2], usize), String> {
    let r = Reader::open(video).map_err(|e| format!("{}: {e}", video.display()))?;
    let t = r
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Video)
        .ok_or_else(|| format!("{}: no video track", video.display()))?;
    Ok(([t.width, t.height], t.samples.len()))
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
        master(&[Pic::One(&missing)], [64, 64], 30, &target).unwrap_err();
        assert!(names(&dir).is_empty());
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
}
