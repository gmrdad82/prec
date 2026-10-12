use crate::encode;
use std::path::Path;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    pub window: bool,
    pub margin: u32,
    pub pad: u32,
    pub bar: u32,
    pub radius: u32,
}

impl Layout {
    pub fn new(width: u32, height: u32, window: bool) -> Layout {
        let h = height as f64;
        if window {
            Layout {
                width,
                height,
                window,
                margin: (0.055 * h).round() as u32,
                pad: (0.022 * h).round() as u32,
                bar: (0.04 * h).round() as u32,
                radius: (0.014 * h).round() as u32,
            }
        } else {
            Layout {
                width,
                height,
                window,
                margin: 0,
                pad: (0.022 * h).round() as u32,
                bar: 0,
                radius: 0,
            }
        }
    }

    pub fn avail(&self) -> [u32; 2] {
        [
            self.width.saturating_sub(2 * self.margin + 2 * self.pad),
            self.height
                .saturating_sub(2 * self.margin + 2 * self.pad + self.bar),
        ]
    }

    pub fn place(&self, canvas: [u32; 2]) -> ([u32; 4], [u32; 2]) {
        let ww = canvas[0] + 2 * self.pad;
        let wh = canvas[1] + 2 * self.pad + self.bar;
        let wx = (self.width.saturating_sub(ww)) / 2;
        let wy = (self.height.saturating_sub(wh)) / 2;
        ([wx, wy, ww, wh], [wx + self.pad, wy + self.bar + self.pad])
    }
}

pub fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] as f32 * (1.0 - t) + b[i] as f32 * t).round() as u8)
}

pub fn default_ground(bg: [u8; 3]) -> [u8; 3] {
    let lum = (0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32) / 255.0;
    if lum > 0.5 {
        mix(bg, [0, 0, 0], 0.1)
    } else {
        mix(bg, [0, 0, 0], 0.3)
    }
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn paint(c: [u8; 3]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c[0], c[1], c[2], 255));
    p.anti_alias = true;
    p
}

pub fn background(
    layout: &Layout,
    canvas: [u32; 2],
    bg: [u8; 3],
    ground: [u8; 3],
) -> Result<Pixmap, String> {
    let mut pm = Pixmap::new(layout.width, layout.height).ok_or("bad frame size")?;
    if !layout.window {
        pm.fill(Color::from_rgba8(bg[0], bg[1], bg[2], 255));
        return Ok(pm);
    }
    pm.fill(Color::from_rgba8(ground[0], ground[1], ground[2], 255));
    let (win, _) = layout.place(canvas);
    let [wx, wy, ww, wh] = win.map(|v| v as f32);
    let shadow = mix(ground, [0, 0, 0], 0.35);
    for i in 0..6 {
        let spread = (layout.height as f32 * 0.002) * (6 - i) as f32;
        let mut p = paint(shadow);
        p.set_color(Color::from_rgba8(shadow[0], shadow[1], shadow[2], 18));
        if let Some(path) = rounded(
            wx - spread,
            wy - spread + layout.height as f32 * 0.006,
            ww + 2.0 * spread,
            wh + 2.0 * spread,
            layout.radius as f32 + spread,
        ) {
            pm.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
        }
    }
    if let Some(path) = rounded(wx, wy, ww, wh, layout.radius as f32) {
        pm.fill_path(
            &path,
            &paint(bg),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    let r = layout.bar as f32 * 0.16;
    let cy = wy + layout.bar as f32 * 0.55;
    for (i, c) in [[0xff, 0x5f, 0x57], [0xfe, 0xbc, 0x2e], [0x28, 0xc8, 0x40]]
        .iter()
        .enumerate()
    {
        let cx = wx + layout.pad as f32 + r + i as f32 * r * 3.2;
        if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
            pm.fill_path(
                &path,
                &paint(*c),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    Ok(pm)
}

pub fn closure(
    picks: &[usize],
    closure: &crate::app::Closure,
    fps: u32,
) -> Result<(Vec<usize>, Option<usize>), String> {
    use crate::app::Closure;
    match closure {
        Closure::Open => Ok((picks.to_vec(), None)),
        Closure::Palindrome => {
            let mut out = picks.to_vec();
            if picks.len() > 2 {
                out.extend(picks[1..picks.len() - 1].iter().rev());
            }
            Ok((out, None))
        }
        Closure::Crossfade(secs) => {
            let k = closure.fade_frames(fps);
            if picks.len() <= 2 * k {
                return Err(format!(
                    "closure {{ crossfade = {secs} }}: the clip needs more than {} frames at {fps} fps",
                    2 * k
                ));
            }
            let mut out = picks.to_vec();
            out.rotate_left(k);
            Ok((out, Some(k)))
        }
    }
}

pub fn poster_frame(
    n: usize,
    closure: &crate::app::Closure,
    fps: u32,
    poster: &crate::app::Poster,
) -> Result<(usize, usize), String> {
    use crate::app::{Closure, Poster};
    if n == 0 {
        return Err("poster: the clip has no frames".into());
    }
    let picks: Vec<usize> = (0..n).collect();
    let (order, fade) = self::closure(&picks, closure, fps)?;
    let src = match poster {
        Poster::First => *order.first().ok_or("poster: the clip has no frames")?,
        Poster::BeforeClosure => match closure {
            Closure::Crossfade(_) => {
                let k = fade.unwrap_or(0);
                let body = order.len().saturating_sub(2 * k);
                if body == 0 {
                    return Err("poster: the crossfade has no clean frame".into());
                }
                order[body - 1]
            }
            Closure::Open | Closure::Palindrome => n - 1,
        },
        Poster::Seconds(t) => frame_at(*t, fps, n),
    };
    Ok((src, master_slot(&order, fade.unwrap_or(0), src)?))
}

fn frame_at(seconds: f64, fps: u32, n: usize) -> usize {
    let scaled = seconds * fps as f64;
    if !scaled.is_finite() {
        return n - 1;
    }
    let nearest = scaled.round();
    let k = if (scaled - nearest).abs() < 1e-4 {
        nearest
    } else {
        scaled.floor()
    };
    let k = if k <= 0.0 { 0.0 } else { k };
    (k as usize).min(n - 1)
}

fn master_slot(order: &[usize], fade: usize, src: usize) -> Result<usize, String> {
    let pos = order
        .iter()
        .position(|&r| r == src)
        .ok_or_else(|| format!("poster: frame {src} is not in the clip"))?;
    if fade == 0 || pos < order.len().saturating_sub(2 * fade) {
        return Ok(pos);
    }
    let master_len = order.len() - fade;
    if pos < master_len {
        return Ok(pos);
    }
    let body = order.len() - 2 * fade;
    Ok(body + (pos - master_len))
}

fn sequence(dir: &Path, files: &[&Path]) -> Result<(), String> {
    crate::paths::fresh(dir)?;
    for (i, f) in files.iter().enumerate() {
        let link = dir.join(format!("{i:06}.png"));
        std::os::unix::fs::symlink(f, &link).map_err(|e| format!("{}: {e}", link.display()))?;
    }
    Ok(())
}

fn crossfade(k: usize, fps: u32) -> String {
    let expr = format!("st(0,(T*{fps}+0.5)/{k});st(1,ld(0)*ld(0)*(3-2*ld(0)));A*(1-ld(1))+B*ld(1)");
    format!(
        "[0:v]{lift}[body];[1:v]{lift}[tail];[2:v]{lift}[head];[tail][head]blend=all_expr='{expr}'[x];[body][x]concat=n=2:v=1:a=0",
        lift = encode::LIFT
    )
}

pub fn master(
    work: &Path,
    files: &[&Path],
    fade: Option<usize>,
    fps: u32,
    output: &Path,
) -> Result<(usize, Vec<String>), String> {
    let parts: Vec<&[&Path]> = match fade {
        None => vec![files],
        Some(k) => {
            let (body, ends) = files.split_at(files.len() - 2 * k);
            let (tail, head) = ends.split_at(k);
            vec![body, tail, head]
        }
    };
    let seq = work.join("seq");
    let mut inputs = vec![];
    for (i, part) in parts.iter().enumerate() {
        sequence(&seq.join(i.to_string()), part)?;
        inputs.push(format!("seq/{i}/%06d.png"));
    }
    let graph = fade.map(|k| crossfade(k, fps));
    let used = encode::master(work, &inputs, graph.as_deref(), fps, output);
    let _ = std::fs::remove_dir_all(&seq);
    Ok((files.len() - fade.unwrap_or(0), used?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_palindrome_never_repeats_its_ends_and_a_crossfade_starts_where_the_master_does() {
        use crate::app::Closure;
        let (p, f) = closure(&[0, 1, 2, 3], &Closure::Palindrome, 30).unwrap();
        assert_eq!(p, vec![0, 1, 2, 3, 2, 1]);
        assert!(f.is_none());
        assert_eq!(
            closure(&[0, 1, 2, 3, 4, 5, 6], &Closure::Crossfade(0.1), 30).unwrap(),
            (vec![3, 4, 5, 6, 0, 1, 2], Some(3))
        );
        assert!(closure(&[0, 1, 2, 3], &Closure::Crossfade(2.0 / 30.0), 30).is_err());
        assert_eq!(closure(&[0, 1], &Closure::Open, 30).unwrap().0, vec![0, 1]);
    }

    #[test]
    fn a_poster_picks_the_first_frame_the_turn_or_the_moment() {
        use crate::app::{Closure, Poster};
        let n = 24;
        let open = Closure::Open;
        let pal = Closure::Palindrome;
        let fade = Closure::Crossfade(0.1);
        assert_eq!(fade.fade_frames(30), 3);
        assert_eq!(poster_frame(n, &open, 30, &Poster::First).unwrap(), (0, 0));
        assert_eq!(
            poster_frame(n, &open, 30, &Poster::BeforeClosure).unwrap(),
            (23, 23)
        );
        assert_eq!(
            poster_frame(n, &open, 30, &Poster::Seconds(0.5)).unwrap(),
            (15, 15)
        );
        assert_eq!(
            poster_frame(n, &open, 30, &Poster::Seconds(0.35)).unwrap(),
            (10, 10)
        );
        assert_eq!(
            poster_frame(n, &open, 30, &Poster::Seconds(100.0)).unwrap(),
            (23, 23)
        );
        assert_eq!(
            poster_frame(n, &open, 60, &Poster::Seconds(0.5)).unwrap(),
            (23, 23)
        );
        assert_eq!(
            poster_frame(n, &pal, 30, &Poster::BeforeClosure).unwrap(),
            (23, 23)
        );
        assert_eq!(poster_frame(n, &pal, 30, &Poster::First).unwrap(), (0, 0));
        assert_eq!(
            poster_frame(n, &pal, 30, &Poster::Seconds(1.0 / 30.0)).unwrap(),
            (1, 1)
        );
        assert_eq!(poster_frame(n, &fade, 30, &Poster::First).unwrap(), (3, 0));
        assert_eq!(
            poster_frame(n, &fade, 30, &Poster::BeforeClosure).unwrap(),
            (20, 17)
        );
        assert_eq!(
            poster_frame(n, &fade, 30, &Poster::Seconds(0.0)).unwrap(),
            (0, 18)
        );
        assert_eq!(
            poster_frame(n, &fade, 30, &Poster::Seconds(20.0 / 30.0)).unwrap(),
            (20, 17)
        );
        assert_eq!(
            poster_frame(n, &fade, 30, &Poster::Seconds(23.0 / 30.0)).unwrap(),
            (23, 20)
        );
    }

    #[test]
    fn a_window_leaves_margin_pad_and_bar_and_centres_the_canvas() {
        let l = Layout::new(1920, 1080, true);
        assert_eq!(
            l.avail(),
            [1920 - 2 * 59 - 2 * 24, 1080 - 2 * 59 - 2 * 24 - 43]
        );
        let (win, at) = l.place([1500, 864]);
        assert_eq!(win[2], 1500 + 48);
        assert_eq!(at[0], win[0] + 24);
        let bare = Layout::new(1920, 1080, false);
        assert_eq!(bare.bar, 0);
        assert_eq!(bare.margin, 0);
    }
}
