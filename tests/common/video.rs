use pito_codecs::h264::Decoder;
use pito_codecs::media::{Frame, Packet, PixelFormat, length_prefixed_to_annexb};
use pito_codecs::mp4::{CodecConfig, Reader, TrackKind};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    pub codec: String,
    pub profile: String,
    pub width: u32,
    pub height: u32,
    pub track_size: [u32; 2],
    pub pix_fmt: String,
    pub color: [u8; 3],
    pub r_frame_rate: String,
    pub avg_frame_rate: String,
    pub frames: u64,
    pub samples: u64,
    pub duration: f64,
    pub reordered: u32,
    pub held: u32,
}

pub struct Video {
    pub facts: Facts,
    pub frames: Vec<Frame>,
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

pub fn ratio(num: u64, den: u64) -> String {
    if den == 0 {
        return "0/0".into();
    }
    let g = gcd(num, den).max(1);
    format!("{}/{}", num / g, den / g)
}

pub fn profile_name(profile: u8, compatibility: u8) -> String {
    match profile {
        66 if compatibility & 0x40 != 0 => "Constrained Baseline".into(),
        66 => "Baseline".into(),
        77 => "Main".into(),
        88 => "Extended".into(),
        100 => "High".into(),
        110 => "High 10".into(),
        122 => "High 4:2:2".into(),
        244 => "High 4:4:4 Predictive".into(),
        other => other.to_string(),
    }
}

pub fn reorder_depth(pts: &[i64]) -> u32 {
    (0..pts.len())
        .map(|i| pts[..i].iter().filter(|&&p| p > pts[i]).count() as u32)
        .max()
        .unwrap_or(0)
}

pub fn modal(durations: &[u32]) -> u32 {
    let mut sorted = durations.to_vec();
    sorted.sort_unstable();
    let mut best = (0usize, 0u32);
    let mut i = 0;
    while i < sorted.len() {
        let run = sorted[i..].iter().take_while(|&&d| d == sorted[i]).count();
        if run > best.0 {
            best = (run, sorted[i]);
        }
        i += run;
    }
    best.1
}

pub fn decode(video: &Path) -> Video {
    let named = |e: &dyn std::fmt::Display| format!("{}: {e}", video.display());
    let r = Reader::open(video).unwrap_or_else(|e| panic!("{}", named(&e)));
    let index = r
        .tracks
        .iter()
        .position(|t| t.kind == TrackKind::Video)
        .unwrap_or_else(|| panic!("{}: no video track", video.display()));
    let t = &r.tracks[index];
    let Some(CodecConfig::Avc(avc)) = t.config() else {
        panic!("{}: the video track is not H.264", video.display());
    };
    let params = avc.parameter_sets_annexb();
    let length = avc.length_size.clamp(1, 4) as usize;
    let mut dec = Decoder::with_threads(1);
    let mut frames = Vec::new();
    let mut held = 0u32;
    for k in 0..t.samples.len() {
        let p = r
            .packet(index, k)
            .unwrap_or_else(|e| panic!("{}", named(&e)));
        let mut data = if p.keyframe { params.clone() } else { vec![] };
        length_prefixed_to_annexb(&mut data, &p.data, length);
        let packet = Packet { data, ..p };
        frames.extend(
            dec.decode(&packet)
                .unwrap_or_else(|e| panic!("{}", named(&e))),
        );
        held = held.max((k + 1).saturating_sub(frames.len()) as u32);
    }
    frames.extend(dec.flush().unwrap_or_else(|e| panic!("{}", named(&e))));
    frames.sort_by_key(|f| f.pts);
    let tags = dec.color();
    let full_range = tags.is_some_and(|c| c.full_range);
    let pix_fmt = match frames.first().map(|f| f.format) {
        Some(PixelFormat::Yuv420p8) if full_range => "yuvj420p",
        Some(PixelFormat::Yuv420p8) => "yuv420p",
        Some(PixelFormat::Yuv420p10) => "yuv420p10le",
        None => "none",
    };
    let durations: Vec<u32> = t.samples.iter().map(|s| s.duration).collect();
    let ticks: u64 = durations.iter().map(|&d| d as u64).sum();
    let pts: Vec<i64> = t.samples.iter().map(|s| s.pts).collect();
    let (width, height) = frames.first().map_or((0, 0), |f| (f.width, f.height));
    let facts = Facts {
        codec: t.config().map(CodecConfig::name).unwrap_or_default(),
        profile: profile_name(avc.profile, avc.compatibility),
        width,
        height,
        track_size: [t.width, t.height],
        pix_fmt: pix_fmt.into(),
        color: tags.map_or([2, 2, 2], |c| [c.primaries, c.transfer, c.matrix]),
        r_frame_rate: ratio(t.timescale as u64, modal(&durations) as u64),
        avg_frame_rate: ratio(t.samples.len() as u64 * t.timescale as u64, ticks),
        frames: frames.len() as u64,
        samples: t.samples.len() as u64,
        duration: t.duration_seconds(),
        reordered: reorder_depth(&pts),
        held,
    };
    Video { facts, frames }
}

pub fn probe(video: &Path) -> Facts {
    decode(video).facts
}

pub fn codes(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    let [yp, u, v] = &f.planes;
    let (x, y) = (x as usize, y as usize);
    [
        yp.data[y * yp.stride + x],
        u.data[(y / 2) * u.stride + x / 2],
        v.data[(y / 2) * v.stride + x / 2],
    ]
}

pub fn full_range(codes: [u8; 3]) -> [u8; 3] {
    let y = (codes[0] as f64 - 16.0) * 255.0 / 219.0;
    let cb = (codes[1] as f64 - 128.0) * 255.0 / 224.0;
    let cr = (codes[2] as f64 - 128.0) * 255.0 / 224.0;
    let (kr, kb) = (0.2126, 0.0722);
    let kg = 1.0 - kr - kb;
    let r = y + 2.0 * (1.0 - kr) * cr;
    let b = y + 2.0 * (1.0 - kb) * cb;
    let g = y - (2.0 * kb * (1.0 - kb) * cb + 2.0 * kr * (1.0 - kr) * cr) / kg;
    [r, g, b].map(|c| c.round().clamp(0.0, 255.0) as u8)
}

pub fn rgb_at(f: &Frame, x: u32, y: u32) -> [u8; 3] {
    full_range(codes(f, x, y))
}

pub fn rgb(f: &Frame) -> Vec<u8> {
    let mut out = Vec::with_capacity((f.width * f.height * 3) as usize);
    for y in 0..f.height {
        for x in 0..f.width {
            out.extend(rgb_at(f, x, y));
        }
    }
    out
}
