use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Mask {
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<[u32; 4]>,
}

#[derive(Clone, Debug)]
pub struct Masks {
    rects: Vec<[u32; 4]>,
    patterns: Vec<regex::Regex>,
}

impl Mask {
    fn checked(&self, grid: [u32; 2]) -> Result<(), String> {
        match (&self.pattern, self.rect) {
            (Some(p), None) => {
                if p.is_empty() {
                    return Err("mask match: an empty pattern hides nothing".into());
                }
                regex::Regex::new(p).map_err(|e| format!("mask match {p:?}: {e}"))?;
                Ok(())
            }
            (None, Some([col, row, cols, rows])) => {
                if cols == 0 || rows == 0 {
                    return Err(format!(
                        "mask rect [{col}, {row}, {cols}, {rows}]: an empty rectangle hides nothing"
                    ));
                }
                if col.saturating_add(cols) > grid[0] || row.saturating_add(rows) > grid[1] {
                    return Err(format!(
                        "mask rect [{col}, {row}, {cols}, {rows}]: outside the {}x{} grid",
                        grid[0], grid[1]
                    ));
                }
                Ok(())
            }
            _ => Err("a mask is { match = \"regex\" } or { rect = [col, row, cols, rows] }".into()),
        }
    }
}

impl Masks {
    pub fn new(list: &[Mask], grid: [u32; 2]) -> Result<Masks, String> {
        let mut m = Masks {
            rects: vec![],
            patterns: vec![],
        };
        for mask in list {
            mask.checked(grid)?;
            if let Some(r) = mask.rect {
                m.rects.push(r);
            }
            if let Some(p) = &mask.pattern {
                m.patterns
                    .push(regex::Regex::new(p).map_err(|e| e.to_string())?);
            }
        }
        Ok(m)
    }

    pub fn of_meta(meta: &Value, grid: [u32; 2]) -> Result<Masks, String> {
        let list: Vec<Mask> = match meta.get("mask") {
            None | Some(Value::Null) => vec![],
            Some(v) => {
                serde_json::from_value(v.clone()).map_err(|e| format!("the cast's mask: {e}"))?
            }
        };
        Masks::new(&list, grid)
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty() && self.patterns.is_empty()
    }

    pub fn cells(&self, screen: &vt100::Screen) -> Vec<[u32; 4]> {
        let mut out = self.rects.clone();
        if self.patterns.is_empty() {
            return out;
        }
        let (rows, cols) = screen.size();
        for row in 0..rows {
            let mut text = String::new();
            let mut at: Vec<u32> = vec![];
            for col in 0..cols {
                let s = match screen.cell(row, col) {
                    Some(c) if c.is_wide_continuation() => continue,
                    Some(c) if c.has_contents() => c.contents().to_string(),
                    _ => " ".to_string(),
                };
                at.extend(std::iter::repeat_n(col as u32, s.len()));
                text.push_str(&s);
            }
            at.push(cols as u32);
            for re in &self.patterns {
                for m in re.find_iter(&text) {
                    if m.is_empty() {
                        continue;
                    }
                    let from = at[m.start()];
                    let last = at[m.end() - 1];
                    let wide = screen.cell(row, last as u16).is_some_and(|c| c.is_wide());
                    let to = (last + if wide { 2 } else { 1 }).min(cols as u32);
                    out.push([from, row as u32, to - from, 1]);
                }
            }
        }
        out
    }

    pub fn text(&self, screen: &vt100::Screen) -> String {
        let (rows, cols) = screen.size();
        let mut hidden = vec![vec![false; cols as usize]; rows as usize];
        for [col, row, w, h] in self.cells(screen) {
            for r in row..(row + h).min(rows as u32) {
                for c in col..(col + w).min(cols as u32) {
                    hidden[r as usize][c as usize] = true;
                }
            }
        }
        let mut out = String::new();
        for row in 0..rows {
            for col in 0..cols {
                if hidden[row as usize][col as usize] {
                    out.push(' ');
                    continue;
                }
                match screen.cell(row, col) {
                    Some(c) if c.is_wide_continuation() => {}
                    Some(c) if c.has_contents() => out.push_str(c.contents()),
                    _ => out.push(' '),
                }
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(text: &str) -> vt100::Parser {
        let mut p = vt100::Parser::new(3, 30, 0);
        p.process(text.as_bytes());
        p
    }

    #[test]
    fn a_mask_hides_its_match_and_its_rectangle_and_nothing_else() {
        let p = screen("Opus 5.5 · Claude Max\r\n/home/x/chase");
        let m = Masks::new(
            &[
                Mask {
                    pattern: Some("Claude Max".into()),
                    rect: None,
                },
                Mask {
                    pattern: None,
                    rect: Some([0, 1, 7, 1]),
                },
            ],
            [30, 3],
        )
        .unwrap();
        assert_eq!(m.cells(p.screen()), vec![[0, 1, 7, 1], [11, 0, 10, 1]]);
        let text = m.text(p.screen());
        assert!(!text.contains("Max") && !text.contains("/home/x"));
        assert!(text.starts_with("Opus 5.5 · ") && text.contains("chase"));
    }

    #[test]
    fn a_mask_is_one_match_or_one_rectangle_inside_the_grid() {
        let bad = [
            Mask {
                pattern: None,
                rect: None,
            },
            Mask {
                pattern: Some("a".into()),
                rect: Some([0, 0, 1, 1]),
            },
            Mask {
                pattern: Some("(".into()),
                rect: None,
            },
            Mask {
                pattern: None,
                rect: Some([25, 0, 6, 1]),
            },
            Mask {
                pattern: None,
                rect: Some([0, 0, 0, 1]),
            },
        ];
        for m in bad {
            assert!(
                Masks::new(std::slice::from_ref(&m), [30, 3]).is_err(),
                "{m:?}"
            );
        }
    }
}
