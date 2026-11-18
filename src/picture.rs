use pito_codecs::png::{self, Decoder, Transform};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgb8 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Rgb8 {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let at = (y as usize * self.width as usize + x as usize) * 3;
        [self.data[at], self.data[at + 1], self.data[at + 2]]
    }
}

pub fn decode_rgb8(bytes: &[u8]) -> Result<Rgb8, String> {
    let picture = Decoder::new()
        .decode_picture(bytes, Transform::Expand)
        .map_err(|e| e.to_string())?;
    let rgba = picture.to_rgba_rounded().map_err(|e| e.to_string())?;
    let mut data = Vec::with_capacity(rgba.data.len() / 4 * 3);
    for px in rgba.data.as_chunks::<4>().0 {
        data.extend_from_slice(&px[..3]);
    }
    Ok(Rgb8 {
        width: rgba.width,
        height: rgba.height,
        data,
    })
}

pub fn read_rgb8(path: &Path) -> Result<Rgb8, String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let bytes = std::fs::read(path).map_err(|e| at(&e))?;
    decode_rgb8(&bytes).map_err(|e| at(&e))
}

pub fn size_of_png(bytes: &[u8]) -> Result<[u32; 2], String> {
    let info = png::info(bytes).map_err(|e| e.to_string())?;
    Ok([info.width, info.height])
}

pub fn to8(v: u16) -> u8 {
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

pub struct Linear {
    width: u32,
    height: u32,
    data: Vec<f32>,
}

pub fn linear(rgb8: &Rgb8) -> Linear {
    let table: [f32; 256] = std::array::from_fn(|v| lin(v as f32 / 255.0));
    Linear {
        width: rgb8.width,
        height: rgb8.height,
        data: rgb8.data.iter().map(|&v| table[v as usize]).collect(),
    }
}

fn sinc(t: f32) -> f32 {
    let a = t * std::f32::consts::PI;
    if t == 0.0 { 1.0 } else { a.sin() / a }
}

fn lanczos3(x: f32) -> f32 {
    if x.abs() < 3.0 {
        sinc(x) * sinc(x / 3.0)
    } else {
        0.0
    }
}

const SUPPORT: f32 = 3.0;

fn weights(out: u32, from: u32, to: u32, weights: &mut Vec<f32>) -> u32 {
    let ratio = from as f32 / to as f32;
    let scale = if ratio < 1.0 { 1.0 } else { ratio };
    let support = SUPPORT * scale;
    let centre = (out as f32 + 0.5) * ratio;
    let left = ((centre - support).floor() as i64).clamp(0, from as i64 - 1) as u32;
    let right = ((centre + support).ceil() as i64).clamp(left as i64 + 1, from as i64) as u32;
    let centre = centre - 0.5;
    weights.clear();
    let mut sum = 0.0;
    for i in left..right {
        let w = lanczos3((i as f32 - centre) / scale);
        weights.push(w);
        sum += w;
    }
    for w in weights.iter_mut() {
        *w /= sum;
    }
    left
}

fn vertical(src: &Linear, height: u32) -> Vec<f32> {
    let (w, h) = (src.width as usize, src.height);
    let mut out = vec![0.0f32; w * 3 * height as usize];
    let mut ws = Vec::new();
    for y in 0..height {
        let left = weights(y, h, height, &mut ws) as usize;
        let row = &mut out[y as usize * w * 3..(y as usize + 1) * w * 3];
        for x in 0..w {
            let mut t = [0.0f32; 3];
            for (i, k) in ws.iter().enumerate() {
                let at = ((left + i) * w + x) * 3;
                t[0] += src.data[at] * k;
                t[1] += src.data[at + 1] * k;
                t[2] += src.data[at + 2] * k;
            }
            row[x * 3..x * 3 + 3].copy_from_slice(&t);
        }
    }
    out
}

fn horizontal(rows: &[f32], width: u32, height: u32, new_width: u32) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    let nw = new_width as usize;
    let mut out = vec![0.0f32; nw * h * 3];
    let mut ws = Vec::new();
    for x in 0..new_width {
        let left = weights(x, width, new_width, &mut ws) as usize;
        for y in 0..h {
            let mut t = [0.0f32; 3];
            for (i, k) in ws.iter().enumerate() {
                let at = (y * w + left + i) * 3;
                t[0] += rows[at] * k;
                t[1] += rows[at + 1] * k;
                t[2] += rows[at + 2] * k;
            }
            let at = (y * nw + x as usize) * 3;
            for c in 0..3 {
                out[at + c] = t[c].clamp(0.0, 1.0);
            }
        }
    }
    out
}

pub fn resize8(linear: &Linear, size: [u32; 2]) -> Vec<u8> {
    let [new_width, new_height] = size;
    let samples = if linear.width == 0 || linear.height == 0 {
        vec![0.0; new_width as usize * new_height as usize * 3]
    } else if [linear.width, linear.height] == size {
        linear.data.clone()
    } else {
        let rows = vertical(linear, new_height);
        horizontal(&rows, linear.width, new_height, new_width)
    };
    samples
        .into_iter()
        .map(|v| to8((delin(v) * 65535.0).round() as u16))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pito_codecs::png::Color;

    fn gradient(width: u32, height: u32) -> Rgb8 {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.push((x * 255 / width.max(1)) as u8);
                data.push((y * 255 / height.max(1)) as u8);
                data.push(((x * 7 + y * 13) % 256) as u8);
            }
        }
        Rgb8 {
            width,
            height,
            data,
        }
    }

    #[test]
    fn a_resize_in_linear_light_keeps_a_flat_colour_and_the_size_asked() {
        for v in [0u8, 1, 30, 128, 254, 255] {
            let img = Rgb8 {
                width: 20,
                height: 10,
                data: std::iter::repeat_n([v, v / 2, 255 - v], 200)
                    .flatten()
                    .collect(),
            };
            let out = resize8(&linear(&img), [7, 3]);
            assert_eq!(out.len(), 7 * 3 * 3);
            for px in out.chunks(3) {
                assert_eq!(px, [v, v / 2, 255 - v], "{v}");
            }
        }
    }

    #[test]
    fn a_resize_gives_the_samples_recorded_from_image_0_25_10_lanczos3() {
        let down: [u8; 63] = [
            13, 27, 25, 48, 27, 47, 85, 27, 71, 121, 27, 94, 158, 27, 117, 195, 27, 139, 230, 27,
            160, 13, 116, 73, 48, 116, 96, 85, 116, 119, 121, 116, 142, 158, 116, 164, 195, 116,
            192, 230, 116, 218, 13, 201, 121, 48, 201, 144, 85, 201, 167, 121, 201, 189, 158, 201,
            215, 195, 201, 221, 230, 201, 170,
        ];
        assert_eq!(resize8(&linear(&gradient(23, 11)), [7, 3]), down);
        let up: [u8; 60] = [
            1, 0, 0, 0, 0, 0, 85, 0, 4, 151, 0, 9, 176, 0, 11, 1, 63, 3, 0, 63, 5, 85, 63, 11, 151,
            63, 16, 176, 63, 18, 1, 112, 10, 0, 112, 12, 85, 112, 18, 151, 112, 23, 176, 112, 25,
            1, 137, 14, 0, 137, 16, 85, 137, 22, 151, 137, 27, 176, 137, 29,
        ];
        assert_eq!(resize8(&linear(&gradient(3, 2)), [5, 4]), up);
    }

    #[test]
    fn a_resize_to_the_same_size_returns_the_same_samples() {
        let img = gradient(23, 11);
        assert_eq!(resize8(&linear(&img), [23, 11]), img.data);
    }

    #[test]
    fn a_resize_covers_both_directions_and_an_empty_picture() {
        let img = gradient(9, 5);
        assert_eq!(resize8(&linear(&img), [31, 17]).len(), 31 * 17 * 3);
        assert_eq!(resize8(&linear(&img), [1, 1]).len(), 3);
        let empty = Rgb8 {
            width: 0,
            height: 0,
            data: Vec::new(),
        };
        assert_eq!(resize8(&linear(&empty), [2, 2]), vec![0; 12]);
    }

    #[test]
    fn to8_rounds_the_sixteen_bit_range_to_the_nearest_byte() {
        assert_eq!(to8(0), 0);
        assert_eq!(to8(65535), 255);
        assert_eq!(to8(257 * 100), 100);
        assert_eq!(to8(257 * 100 + 128), 100);
        assert_eq!(to8(257 * 100 + 129), 101);
    }

    #[test]
    fn a_picture_reads_back_the_samples_that_were_written_at_eight_and_sixteen_bits() {
        let img = gradient(16, 8);
        for (depth, data) in [
            (8u8, img.data.clone()),
            (16, img.data.iter().flat_map(|&v| [v, v]).collect()),
        ] {
            let picture = png::Picture {
                width: 16,
                height: 8,
                color: Color::Rgb,
                depth,
                data,
            };
            let bytes = png::encode_picture(&picture, &png::Options::default()).unwrap();
            assert_eq!(decode_rgb8(&bytes).unwrap(), img, "{depth}");
            assert_eq!(size_of_png(&bytes).unwrap(), [16, 8]);
            assert_eq!(png::info(&bytes).unwrap().color, Color::Rgb);
        }
        assert!(decode_rgb8(b"not a png").is_err());
        assert!(size_of_png(b"not a png").is_err());
    }

    #[test]
    fn a_grey_and_an_alpha_picture_read_as_rgb_with_the_alpha_dropped() {
        let grey = png::Picture {
            width: 2,
            height: 1,
            color: Color::Grey,
            depth: 8,
            data: vec![10, 200],
        };
        let bytes = png::encode_picture(&grey, &png::Options::default()).unwrap();
        assert_eq!(
            decode_rgb8(&bytes).unwrap().data,
            [10, 10, 10, 200, 200, 200]
        );
        assert_eq!(png::info(&bytes).unwrap().color, Color::Grey);
        let alpha = png::Picture {
            width: 1,
            height: 1,
            color: Color::Rgba,
            depth: 8,
            data: vec![1, 2, 3, 4],
        };
        let bytes = png::encode_picture(&alpha, &png::Options::default()).unwrap();
        let got = decode_rgb8(&bytes).unwrap();
        assert_eq!(got.data, [1, 2, 3]);
        assert_eq!(got.pixel(0, 0), [1, 2, 3]);
    }
}
