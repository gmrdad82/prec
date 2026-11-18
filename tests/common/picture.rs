use pito_codecs::png::{Color, Decoder, Transform, info, metadata};
use std::path::Path;

pub struct Rgb<T> {
    pub width: u32,
    pub height: u32,
    pub data: Vec<T>,
}

impl<T: Copy> Rgb<T> {
    pub fn pixel(&self, x: u32, y: u32) -> [T; 3] {
        let at = (y as usize * self.width as usize + x as usize) * 3;
        [self.data[at], self.data[at + 1], self.data[at + 2]]
    }
}

fn read(path: &Path) -> pito_codecs::png::Picture {
    let bytes = std::fs::read(path).unwrap();
    Decoder::new()
        .decode_picture(&bytes, Transform::Expand)
        .unwrap()
}

pub fn rgb8(path: &Path) -> Rgb<u8> {
    let picture = read(path);
    let rgba = picture.to_rgba_rounded().unwrap();
    Rgb {
        width: rgba.width,
        height: rgba.height,
        data: rgba
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect(),
    }
}

pub fn rgb16(path: &Path) -> Rgb<u16> {
    let picture = read(path);
    assert_eq!(picture.color, Color::Rgb, "{}", path.display());
    Rgb {
        width: picture.width,
        height: picture.height,
        data: picture.samples16(),
    }
}

pub fn png_info(path: &Path) -> (u8, [u32; 2], bool) {
    let bytes = std::fs::read(path).unwrap();
    let i = info(&bytes).unwrap();
    assert_eq!(i.color, Color::Rgb, "{}", path.display());
    (
        i.depth,
        [i.width, i.height],
        metadata(&bytes).unwrap().srgb.is_some(),
    )
}

pub fn texts(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).unwrap();
    metadata(&bytes)
        .unwrap()
        .text
        .iter()
        .map(|t| format!("{}={}", t.keyword, t.text))
        .collect()
}
