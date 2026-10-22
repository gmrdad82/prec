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
        let p = screen("Opus 5.5 · Claude Max\r\n/srv/x/chase");
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
        assert!(!text.contains("Max") && !text.contains("/srv/x"));
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

    fn rect(r: [u32; 4]) -> Mask {
        Mask {
            pattern: None,
            rect: Some(r),
        }
    }

    fn matching(p: &str) -> Mask {
        Mask {
            pattern: Some(p.into()),
            rect: None,
        }
    }

    #[test]
    fn no_mask_hides_nothing() {
        let p = screen("alpha beta\r\ngamma");
        let m = Masks::new(&[], [30, 3]).unwrap();
        assert!(m.is_empty());
        assert!(m.cells(p.screen()).is_empty());
        let text = m.text(p.screen());
        assert_eq!(text.lines().count(), 3);
        assert!(text.lines().all(|l| l.chars().count() == 30));
        assert!(text.starts_with("alpha beta") && text.contains("\ngamma"));
        assert!(
            !Masks::new(&[rect([0, 0, 1, 1])], [30, 3])
                .unwrap()
                .is_empty()
        );
        assert!(!Masks::new(&[matching("a")], [30, 3]).unwrap().is_empty());
    }

    #[test]
    fn a_rectangle_reaching_the_grid_edge_fits_and_one_past_it_is_refused() {
        assert!(Masks::new(&[rect([0, 0, 30, 3])], [30, 3]).is_ok());
        assert!(Masks::new(&[rect([29, 2, 1, 1])], [30, 3]).is_ok());
        for r in [
            [0, 0, 31, 3],
            [0, 0, 30, 4],
            [30, 0, 1, 1],
            [0, 3, 1, 1],
            [u32::MAX, 0, 2, 1],
            [0, u32::MAX, 1, 2],
            [0, 0, 1, 0],
        ] {
            let e = Masks::new(&[rect(r)], [30, 3]).unwrap_err();
            assert!(e.starts_with("mask rect ["), "{r:?}: {e}");
        }
        assert_eq!(
            Masks::new(&[rect([25, 0, 6, 1])], [30, 3]).unwrap_err(),
            "mask rect [25, 0, 6, 1]: outside the 30x3 grid"
        );
        assert_eq!(
            Masks::new(&[matching("")], [30, 3]).unwrap_err(),
            "mask match: an empty pattern hides nothing"
        );
    }

    #[test]
    fn overlapping_masks_hide_their_union_once() {
        let p = screen("0123456789\r\nabcdefghij\r\nKLMNOPQRST");
        let m = Masks::new(
            &[
                rect([2, 0, 4, 2]),
                rect([4, 1, 4, 2]),
                matching("4567"),
                matching("56"),
            ],
            [30, 3],
        )
        .unwrap();
        let text = m.text(p.screen());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(&lines[0][..10], "01      89");
        assert_eq!(&lines[1][..10], "ab      ij");
        assert_eq!(&lines[2][..10], "KLMN    ST");
        assert_eq!(
            m.cells(p.screen()),
            vec![[2, 0, 4, 2], [4, 1, 4, 2], [4, 0, 4, 1], [5, 0, 2, 1]]
        );
    }

    #[test]
    fn a_match_on_a_wide_character_covers_both_its_cells_and_stops_at_the_edge() {
        let p = screen("ab三cd\r\n");
        let m = Masks::new(&[matching("三")], [30, 3]).unwrap();
        assert_eq!(m.cells(p.screen()), vec![[2, 0, 2, 1]]);
        let text = m.text(p.screen());
        assert!(text.starts_with("ab  cd"), "{text:?}");
        let mut edge = vt100::Parser::new(1, 4, 0);
        edge.process("ab三".as_bytes());
        let m = Masks::new(&[matching("三")], [4, 1]).unwrap();
        assert_eq!(m.cells(edge.screen()), vec![[2, 0, 2, 1]]);
        assert_eq!(m.text(edge.screen()), "ab  \n");
    }

    #[test]
    fn a_pattern_that_can_match_nothing_hides_only_what_it_matches() {
        let p = screen("xx yy\r\n");
        let m = Masks::new(&[matching("y*")], [30, 3]).unwrap();
        assert_eq!(m.cells(p.screen()), vec![[3, 0, 2, 1]]);
        assert!(m.text(p.screen()).starts_with("xx   "));
    }

    #[test]
    fn a_cast_carries_its_masks_strictly() {
        let grid = [30, 3];
        assert!(
            Masks::of_meta(&serde_json::json!({}), grid)
                .unwrap()
                .is_empty()
        );
        assert!(
            Masks::of_meta(&serde_json::json!({"mask": null}), grid)
                .unwrap()
                .is_empty()
        );
        let m = Masks::of_meta(
            &serde_json::json!({"mask": [{"rect": [0, 0, 2, 1]}, {"match": "b"}]}),
            grid,
        )
        .unwrap();
        let p = screen("abc");
        assert_eq!(m.cells(p.screen()), vec![[0, 0, 2, 1], [1, 0, 1, 1]]);
        for bad in [
            serde_json::json!({"mask": [{"rect": [0, 0, 2, 1], "colour": "red"}]}),
            serde_json::json!({"mask": {"rect": [0, 0, 2, 1]}}),
            serde_json::json!({"mask": [{"rect": [0, 0, 2]}]}),
            serde_json::json!({"mask": "abc"}),
        ] {
            let e = Masks::of_meta(&bad, grid).unwrap_err();
            assert!(e.starts_with("the cast's mask: "), "{bad}: {e}");
        }
        assert!(
            Masks::of_meta(
                &serde_json::json!({"mask": [{"rect": [0, 0, 31, 1]}]}),
                grid
            )
            .unwrap_err()
            .contains("outside the 30x3 grid")
        );
    }

    #[test]
    fn a_mask_writes_back_as_the_toml_it_was_read_from() {
        let m: Vec<Mask> =
            serde_json::from_value(serde_json::json!([{"match": "a+"}, {"rect": [1, 2, 3, 4]}]))
                .unwrap();
        assert_eq!(m, vec![matching("a+"), rect([1, 2, 3, 4])]);
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            serde_json::json!([{"match": "a+"}, {"rect": [1, 2, 3, 4]}])
        );
    }
}
