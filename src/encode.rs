use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const LIFT: &str = "format=gbrp,zscale,format=gbrp16le";
const TO_709: &str = "format=rgb48le,zscale=matrix=709:range=limited:filter=bicubic:chromal=left,format=yuv420p,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv";
pub const COLOR_709: &str = "BT.709 primaries, transfer and matrix, limited range (SDR)";
pub const LANCZOS: &str = "Lanczos3 in linear light";
pub const X264_THREADS: &str = "24";

pub fn ffmpeg(args: &[String], cwd: &Path) -> Result<(), String> {
    let out = crate::paths::output(
        crate::paths::confined("ffmpeg", "16G")
            .current_dir(cwd)
            .args(["-hide_banner", "-v", "error", "-y"])
            .args(args),
    )
    .map_err(|e| format!("ffmpeg: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ffmpeg failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

pub fn x264_args(fps: u32) -> Vec<String> {
    let mut args = vec!["-vf".to_string(), format!("{LIFT},{TO_709}")];
    args.extend(x264(fps));
    args
}

fn x264(fps: u32) -> Vec<String> {
    [
        "-c:v",
        "libx264",
        "-threads",
        X264_THREADS,
        "-preset",
        "slow",
        "-profile:v",
        "high",
        "-tune",
        "animation",
        "-pix_fmt",
        "yuv420p",
        "-crf",
        "14",
        "-g",
        &format!("{}", 2 * fps),
        "-keyint_min",
        &format!("{}", 2 * fps),
        "-sc_threshold",
        "0",
        "-color_primaries",
        "bt709",
        "-color_trc",
        "bt709",
        "-colorspace",
        "bt709",
        "-color_range",
        "tv",
        "-movflags",
        "+faststart+write_colr",
        "-an",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
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

pub fn master(
    work: &Path,
    inputs: &[String],
    graph: Option<&str>,
    fps: u32,
    output: &Path,
) -> Result<Vec<String>, String> {
    let used = x264_args(fps);
    let mut args: Vec<String> = ["-filter_threads", "1", "-filter_complex_threads", "1"]
        .map(String::from)
        .to_vec();
    for input in inputs {
        args.extend(["-f", "image2", "-framerate"].map(String::from));
        args.push(fps.to_string());
        args.extend(["-start_number", "0", "-i"].map(String::from));
        args.push(input.clone());
    }
    match graph {
        None => args.extend(used.iter().cloned()),
        Some(g) => {
            args.extend([
                "-filter_complex".to_string(),
                format!("{g},{TO_709}[out]"),
                "-map".to_string(),
                "[out]".to_string(),
            ]);
            args.extend(x264(fps));
        }
    }
    args.push(output.display().to_string());
    ffmpeg(&args, work).inspect_err(|_| {
        let _ = std::fs::remove_file(output);
    })?;
    Ok(used)
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

    #[test]
    fn x264_master_args_match_the_convention() {
        assert_eq!(
            x264_args(60).join(" "),
            "-vf format=gbrp,zscale,format=gbrp16le,format=rgb48le,zscale=matrix=709:range=limited:filter=bicubic:chromal=left,format=yuv420p,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv -c:v libx264 -threads 24 -preset slow -profile:v high -tune animation -pix_fmt yuv420p -crf 14 -g 120 -keyint_min 120 -sc_threshold 0 -color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range tv -movflags +faststart+write_colr -an"
        );
    }

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
        master(&dir, &["missing/%06d.png".into()], None, 30, &target).unwrap_err();
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
