use std::path::PathBuf;
use std::process::Command;

pub struct Metrics {
    pub file: PathBuf,
    pub advance: f32,
    pub line: f32,
}

pub fn file(family: &str) -> Result<PathBuf, String> {
    let out =
        crate::paths::output(Command::new("fc-match").args(["-f", "%{file}\n%{family}", family]))
            .map_err(|e| format!("fc-match: {e}"))?;
    matched(family, &String::from_utf8_lossy(&out.stdout))
}

fn matched(family: &str, text: &str) -> Result<PathBuf, String> {
    let mut lines = text.lines();
    let file = lines.next().unwrap_or_default().trim().to_string();
    let matched = lines.next().unwrap_or_default().to_lowercase();
    let want = family.to_lowercase();
    if file.is_empty() || !matched.split(',').any(|f| f.trim() == want) {
        return Err(format!(
            "fontconfig has no {family:?} (it would fall back to {matched:?}); install it or pass --font"
        ));
    }
    Ok(PathBuf::from(file))
}

pub fn metrics(family: &str) -> Result<Metrics, String> {
    let file = file(family)?;
    let data = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    measured(family, file, &data)
}

fn measured(family: &str, file: PathBuf, data: &[u8]) -> Result<Metrics, String> {
    let face = ttf_parser::Face::parse(data, 0).map_err(|e| format!("{}: {e}", file.display()))?;
    let upem = face.units_per_em() as f32;
    let glyph = face
        .glyph_index('M')
        .ok_or_else(|| format!("{family} has no M"))?;
    let adv = face
        .glyph_hor_advance(glyph)
        .ok_or_else(|| format!("{family} has no advance for M"))? as f32;
    let line = (face.ascender() as f32 - face.descender() as f32 + face.line_gap() as f32) / upem;
    Ok(Metrics {
        file,
        advance: adv / upem,
        line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_be_bytes());
    }

    fn face(code: u16, advance: u16) -> Vec<u8> {
        let mut head = Vec::new();
        for v in [1u16, 0, 1, 0, 0, 0, 0x5F0F, 0x3CF5, 0, 1000] {
            be16(&mut head, v);
        }
        head.extend_from_slice(&[0; 16]);
        for v in [0u16, 0xFF38, 600, 800, 0, 8, 2, 0, 0] {
            be16(&mut head, v);
        }
        let mut hhea = Vec::new();
        for v in [1u16, 0, 800, 0xFF38, 100] {
            be16(&mut hhea, v);
        }
        hhea.extend_from_slice(&[0; 24]);
        be16(&mut hhea, 2);
        let mut maxp = Vec::new();
        for v in [0x0000u16, 0x5000, 2] {
            be16(&mut maxp, v);
        }
        let mut hmtx = Vec::new();
        for v in [500u16, 0, advance, 0] {
            be16(&mut hmtx, v);
        }
        let mut cmap = Vec::new();
        for v in [0u16, 1, 0, 3, 0, 12] {
            be16(&mut cmap, v);
        }
        let delta = 1u16.wrapping_sub(code);
        for v in [
            4u16, 32, 0, 4, 4, 1, 0, code, 0xFFFF, 0, code, 0xFFFF, delta, 1, 0, 0,
        ] {
            be16(&mut cmap, v);
        }
        let tables: [(&[u8; 4], Vec<u8>); 5] = [
            (b"cmap", cmap),
            (b"head", head),
            (b"hhea", hhea),
            (b"hmtx", hmtx),
            (b"maxp", maxp),
        ];
        let mut out = Vec::new();
        for v in [1u16, 0, 5, 64, 2, 16] {
            be16(&mut out, v);
        }
        let mut offset = 12 + 16 * tables.len();
        let mut body = Vec::new();
        for (tag, data) in &tables {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            body.extend_from_slice(data);
            while !body.len().is_multiple_of(4) {
                body.push(0);
            }
            offset = 12 + 16 * tables.len() + body.len();
        }
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn fc_match_must_name_the_family_asked_for() {
        assert_eq!(
            matched("Alpha Mono", "/fonts/alpha.ttf\nAlpha Mono").unwrap(),
            PathBuf::from("/fonts/alpha.ttf")
        );
        assert_eq!(
            matched("alpha mono", "/fonts/alpha.ttf\nAlpha Sans, ALPHA MONO ").unwrap(),
            PathBuf::from("/fonts/alpha.ttf")
        );
        let fallback = matched("Alpha Mono", "/fonts/dejavu.ttf\nDejaVu Sans").unwrap_err();
        assert_eq!(
            fallback,
            "fontconfig has no \"Alpha Mono\" (it would fall back to \"dejavu sans\"); install it or pass --font"
        );
        assert!(matched("Alpha Mono", "").is_err());
        assert!(matched("Alpha Mono", "\nAlpha Mono").is_err());
        assert!(matched("Alpha Mono", "/fonts/alpha.ttf\nAlpha Mono Nerd").is_err());
    }

    #[test]
    fn a_face_gives_the_advance_of_m_and_the_line_height_in_ems() {
        let m = measured(
            "Alpha Mono",
            PathBuf::from("alpha.ttf"),
            &face(u16::from(b'M'), 600),
        )
        .unwrap();
        assert_eq!(m.file, PathBuf::from("alpha.ttf"));
        assert_eq!(m.advance, 0.6);
        assert_eq!(m.line, 1.1);
        let wide = measured(
            "Alpha Mono",
            PathBuf::from("a.ttf"),
            &face(u16::from(b'M'), 1000),
        )
        .unwrap();
        assert_eq!(wide.advance, 1.0);
    }

    #[test]
    fn a_face_without_m_or_a_file_that_is_no_font_is_refused() {
        assert_eq!(
            measured(
                "Alpha Mono",
                PathBuf::from("a.ttf"),
                &face(u16::from(b'N'), 600)
            )
            .err()
            .unwrap(),
            "Alpha Mono has no M"
        );
        let junk = measured("Alpha Mono", PathBuf::from("junk.ttf"), b"not a font")
            .err()
            .unwrap();
        assert!(junk.starts_with("junk.ttf: "), "{junk}");
        let mut cut = face(u16::from(b'M'), 600);
        cut.truncate(40);
        assert!(measured("Alpha Mono", PathBuf::from("cut.ttf"), &cut).is_err());
    }
}
