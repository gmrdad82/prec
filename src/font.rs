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
    let text = String::from_utf8_lossy(&out.stdout).to_string();
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
    let face = ttf_parser::Face::parse(&data, 0).map_err(|e| format!("{}: {e}", file.display()))?;
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
