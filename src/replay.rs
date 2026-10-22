use crate::cdp::Browser;
use crate::compose::{self, Layout};
use crate::fps::Fps;
use crate::paths;
use crate::record::{self, Cast, Event, Kind, Timeline};
use base64::Engine;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const XTERM: &str = "5.5.0";
pub const XTERM_CANVAS: &str = "0.7.0";
pub const XTERM_UNICODE: &str = "0.8.0";

const ASSETS: [(&str, &str); 4] = [
    ("xterm.js", include_str!("../assets/xterm/xterm.js")),
    ("xterm.css", include_str!("../assets/xterm/xterm.css")),
    (
        "addon-canvas.js",
        include_str!("../assets/xterm/addon-canvas.js"),
    ),
    (
        "addon-unicode11.js",
        include_str!("../assets/xterm/addon-unicode11.js"),
    ),
];

pub fn assets_ok() -> Result<String, String> {
    for (name, text) in ASSETS {
        if text.len() < 1000 {
            return Err(format!("the vendored {name} is empty"));
        }
    }
    Ok(format!(
        "xterm.js {XTERM}, addon-canvas {XTERM_CANVAS}, addon-unicode11 {XTERM_UNICODE}, embedded"
    ))
}

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8">
<link rel="stylesheet" href="xterm.css">
<style>
html,body{margin:0;padding:0;overflow:hidden;width:100%;height:100%}
#bg{position:absolute;left:0;top:0;display:block}
#term{position:absolute;left:0;top:0}
.xterm-viewport{overflow:hidden!important}
.xterm .xterm-helpers{display:none}
</style>
<script src="xterm.js"></script>
<script src="addon-canvas.js"></script>
<script src="addon-unicode11.js"></script>
</head><body><img id="bg" alt=""><div id="term"></div>
<script>
let term = null;
const frame2 = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => r(true))));
window.boot = async (o) => {
  if (term) { term.dispose(); term = null; }
  document.body.style.background = o.page;
  const bg = document.getElementById('bg');
  bg.style.width = o.vw + 'px';
  bg.style.height = o.vh + 'px';
  for (const f of o.fonts) { try { await document.fonts.load(o.size + 'px "' + f + '"', '三Mg▒'); } catch (e) {} }
  await document.fonts.ready;
  term = new Terminal({
    cols: o.cols, rows: o.rows, fontFamily: o.family, fontSize: o.size,
    lineHeight: 1, letterSpacing: 0, theme: o.theme, cursorBlink: false,
    cursorStyle: 'block', cursorInactiveStyle: 'block', scrollback: 0,
    disableStdin: true, allowProposedApi: true, customGlyphs: true,
    minimumContrastRatio: 1, allowTransparency: false, drawBoldTextInBrightColors: true
  });
  term.open(document.getElementById('term'));
  let reflow = null;
  try {
    const nb = term._core.buffers.normal;
    Object.defineProperty(Object.getPrototypeOf(nb), '_isReflowEnabled', { get: () => false, configurable: true });
    reflow = nb._isReflowEnabled;
  } catch (e) {}
  term.loadAddon(new CanvasAddon.CanvasAddon());
  const u = new Unicode11Addon.Unicode11Addon();
  term.loadAddon(u);
  term.unicode.activeVersion = '11';
  term.focus();
  try { term._core.coreService.isCursorInitialized = true; } catch (e) {}
  term.refresh(0, term.rows - 1);
  await frame2();
  const r = term.element.querySelector('.xterm-screen').getBoundingClientRect();
  let cell = null;
  try { const d = term._core._renderService.dimensions.device.cell; cell = [d.width, d.height]; } catch (e) {}
  return { w: r.width, h: r.height, cols: term.cols, rows: term.rows, cell: cell, reflow: reflow };
};
window.place = async (x, y, src) => {
  const el = document.getElementById('term');
  el.style.left = x + 'px';
  el.style.top = y + 'px';
  const bg = document.getElementById('bg');
  if (src) {
    await new Promise((res, rej) => { bg.onload = res; bg.onerror = () => rej(new Error('background')); bg.src = src; });
  }
  await frame2();
  return true;
};
const cover = (rects) => {
  const screen = term.element.querySelector('.xterm-screen');
  let box = document.getElementById('masks');
  if (!box) {
    box = document.createElement('div');
    box.id = 'masks';
    box.style.cssText = 'position:absolute;left:0;top:0;width:100%;height:100%;z-index:10;pointer-events:none';
    screen.appendChild(box);
  }
  box.replaceChildren();
  const r = screen.getBoundingClientRect();
  const cw = r.width / term.cols, ch = r.height / term.rows;
  const fill = (term.options.theme && term.options.theme.background) || '#000';
  for (const [c, w0, n, h] of rects) {
    const d = document.createElement('div');
    d.style.cssText = 'position:absolute;background:' + fill + ';left:' + (c * cw) + 'px;top:' + (w0 * ch) + 'px;width:' + (n * cw) + 'px;height:' + (h * ch) + 'px';
    box.appendChild(d);
  }
};
window.play = async (items, rects) => {
  let grid = false;
  for (const it of items) {
    if (Array.isArray(it)) { term.resize(it[0], it[1]); grid = true; continue; }
    const bin = atob(it);
    const a = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) a[i] = bin.charCodeAt(i);
    await new Promise(res => term.write(a, res));
  }
  if (rects) cover(rects);
  if (grid) term.refresh(0, term.rows - 1);
  await frame2();
  const r = term.element.querySelector('.xterm-screen').getBoundingClientRect();
  return { w: r.width, h: r.height, cols: term.cols, rows: term.rows };
};
</script></body></html>
"#;

#[derive(Clone, Debug)]
pub struct Look {
    pub theme: Value,
    pub font: String,
    pub window: bool,
    pub ground: Option<[u8; 3]>,
    pub inset: [u32; 4],
    pub masks: crate::mask::Masks,
}

#[derive(Clone, Copy, Debug)]
pub enum Take {
    Still(f64),
    Clip { frames: usize, fps: Fps },
}

pub struct Output {
    pub frames: Vec<PathBuf>,
    pub captured: usize,
    pub canvas: [u32; 2],
    pub max: Option<([u32; 2], [u32; 2])>,
    pub font_px: f64,
    pub cell: Option<[f64; 2]>,
    pub bg: [u8; 3],
    pub ground: [u8; 3],
    pub chromium: String,
}

pub fn xterm_theme(v: &Value) -> Value {
    let g = |k: &str| v.get(k).cloned().unwrap_or(Value::Null);
    let mut t = json!({
        "background": g("background"),
        "foreground": g("foreground"),
        "cursor": g("cursor"),
        "cursorAccent": v.get("cursorAccent").cloned().unwrap_or_else(|| g("background")),
        "selectionBackground": g("selection"),
    });
    for k in [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "brightBlack",
        "brightRed",
        "brightGreen",
        "brightYellow",
        "brightBlue",
        "brightMagenta",
        "brightCyan",
        "brightWhite",
    ] {
        t[k] = g(k);
    }
    t
}

fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for &c in path.as_os_str().as_encoded_bytes() {
        if c.is_ascii_alphanumeric() || b"/-._~".contains(&c) {
            url.push(c as char);
        } else {
            url.push_str(&format!("%{c:02X}"));
        }
    }
    url
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Bytes(Vec<u8>),
    Grid([u32; 2]),
}

fn pieces(events: &[Event]) -> Vec<(f64, Piece)> {
    events
        .iter()
        .filter_map(|e| match e.kind {
            Kind::Output => Some((e.t, Piece::Bytes(e.data.as_bytes().to_vec()))),
            Kind::Resize => record::grid_of(&e.data).map(|g| (e.t, Piece::Grid(record::shown(g)))),
            _ => None,
        })
        .collect()
}

struct Feeder<'a> {
    out: Vec<(f64, Piece)>,
    next: usize,
    masks: &'a crate::mask::Masks,
    screen: vt100::Parser,
    covered: Vec<[u32; 4]>,
    grid: [u32; 2],
    canvas: [u32; 2],
}

impl<'a> Feeder<'a> {
    fn new(events: &[Event], masks: &'a crate::mask::Masks, grid: [u32; 2]) -> Feeder<'a> {
        Feeder {
            out: pieces(events),
            next: 0,
            masks,
            screen: vt100::Parser::new(grid[1] as u16, grid[0] as u16, 0),
            covered: vec![],
            grid,
            canvas: [0, 0],
        }
    }

    fn due(&mut self, t: f64) -> Result<Option<(Vec<Piece>, String)>, String> {
        let mut found: Vec<Piece> = vec![];
        while self.next < self.out.len() && self.out[self.next].0 <= t + 1e-9 {
            match (&self.out[self.next].1, found.last_mut()) {
                (Piece::Bytes(b), Some(Piece::Bytes(chunk))) => chunk.extend_from_slice(b),
                (p, _) => found.push(p.clone()),
            }
            self.next += 1;
        }
        found.retain(|p| !matches!(p, Piece::Bytes(b) if b.is_empty()));
        if found.is_empty() {
            return Ok(None);
        }
        for p in &found {
            match p {
                Piece::Grid(g) => {
                    self.grid = *g;
                    if !self.masks.is_empty() {
                        record::set_grid(&mut self.screen, *g);
                    }
                }
                Piece::Bytes(b) if !self.masks.is_empty() => self.screen.process(b),
                Piece::Bytes(_) => {}
            }
        }
        let mut rects = "null".to_string();
        if !self.masks.is_empty() {
            let now = self.masks.cells(self.screen.screen());
            if now != self.covered {
                rects = serde_json::to_string(&now).map_err(|e| e.to_string())?;
                self.covered = now;
            }
        }
        Ok(Some((found, rects)))
    }

    fn until(&mut self, b: &mut Browser, t: f64) -> Result<bool, String> {
        let Some((found, rects)) = self.due(t)? else {
            return Ok(false);
        };
        self.play(b, &found, &rects)?;
        Ok(true)
    }

    fn play(&mut self, b: &mut Browser, found: &[Piece], rects: &str) -> Result<(), String> {
        let m = b.eval(&format!("window.play({}, {rects})", batch(found)))?;
        let want = found.iter().rev().find_map(|p| match p {
            Piece::Grid(g) => Some(*g),
            Piece::Bytes(_) => None,
        });
        if let Some(g) = want {
            if m["cols"].as_u64() != Some(g[0] as u64) || m["rows"].as_u64() != Some(g[1] as u64) {
                return Err(format!("xterm.js would not resize to {}x{}", g[0], g[1]));
            }
            self.canvas = [
                m["w"].as_f64().unwrap_or(0.0).round() as u32,
                m["h"].as_f64().unwrap_or(0.0).round() as u32,
            ];
        }
        Ok(())
    }
}

fn reflow_off(m: &Value, resizes: bool) -> Result<(), String> {
    if resizes && m["reflow"] != json!(false) {
        return Err(
            "xterm.js would not turn its reflow off, so a resize would not match the masks".into(),
        );
    }
    Ok(())
}

fn batch(found: &[Piece]) -> String {
    let items: Vec<Value> = found
        .iter()
        .map(|p| match p {
            Piece::Bytes(bytes) => json!(b64(bytes)),
            Piece::Grid(g) => json!([g[0], g[1]]),
        })
        .collect();
    Value::Array(items).to_string()
}

struct Backdrop {
    dev: Layout,
    largest: [u32; 2],
    bg: [u8; 3],
    ground: [u8; 3],
    dir: PathBuf,
    drawn: HashMap<[u32; 2], String>,
    shown: Option<[u32; 2]>,
}

impl Backdrop {
    fn key(&self, grid: [u32; 2]) -> [u32; 2] {
        if self.dev.window { grid } else { [0, 0] }
    }

    fn file(&mut self, grid: [u32; 2], canvas: [u32; 2]) -> Result<String, String> {
        let key = self.key(grid);
        if let Some(name) = self.drawn.get(&key) {
            return Ok(name.clone());
        }
        let (win, _) = self.dev.anchored(self.largest, canvas);
        let pm = compose::background(&self.dev, win, self.bg, self.ground)?;
        let name = if self.dev.window {
            format!("background-{}x{}.png", grid[0], grid[1])
        } else {
            "background.png".to_string()
        };
        pm.save_png(self.dir.join(&name))
            .map_err(|e| e.to_string())?;
        self.drawn.insert(key, name.clone());
        Ok(name)
    }

    fn show(&mut self, b: &mut Browser, grid: [u32; 2], canvas: [u32; 2]) -> Result<bool, String> {
        if self.shown == Some(self.key(grid)) {
            return Ok(false);
        }
        let name = self.file(grid, canvas)?;
        let (_, at) = self.dev.anchored(self.largest, canvas);
        b.eval(&format!("window.place({}, {}, {name:?})", at[0], at[1]))?;
        self.shown = Some(self.key(grid));
        Ok(true)
    }
}

fn corners(png: &[u8], rect: [u32; 4]) -> Result<[u8; 3], String> {
    let img = image::load_from_memory(png)
        .map_err(|e| e.to_string())?
        .to_rgb8();
    let [x, y, w, h] = rect;
    let pick = |px: u32, py: u32| {
        let p = img.get_pixel(
            px.min(img.width().saturating_sub(1)),
            py.min(img.height().saturating_sub(1)),
        );
        [p[0], p[1], p[2]]
    };
    let c = [
        pick(x + 1, y + 1),
        pick(x + w.saturating_sub(2), y + 1),
        pick(x + 1, y + h.saturating_sub(2)),
        pick(x + w.saturating_sub(2), y + h.saturating_sub(2)),
    ];
    Ok(*c
        .iter()
        .max_by_key(|a| c.iter().filter(|b| b == a).count())
        .unwrap_or(&c[0]))
}

fn fonts_of(family: &str) -> Vec<String> {
    vec![
        family.to_string(),
        "Noto Sans CJK JP".into(),
        "Noto Color Emoji".into(),
    ]
}

fn family_css(family: &str) -> String {
    format!("\"{family}\", \"Noto Sans CJK JP\", \"Noto Color Emoji\", monospace")
}

fn check_size(png: &[u8], size: [u32; 2]) -> Result<(), String> {
    let img = image::load_from_memory(png).map_err(|e| e.to_string())?;
    if img.width() != size[0] || img.height() != size[1] {
        return Err(format!(
            "chromium drew {}x{}, not {}x{}",
            img.width(),
            img.height(),
            size[0],
            size[1]
        ));
    }
    Ok(())
}

pub fn render(
    cast: &Cast,
    size: [u32; 2],
    take: Take,
    look: &Look,
    metrics: (f32, f32),
    work: &Path,
) -> Result<Output, String> {
    let page_dir = work.join("page");
    paths::fresh(&page_dir)?;
    for (name, text) in ASSETS {
        std::fs::write(page_dir.join(name), text).map_err(|e| e.to_string())?;
    }
    std::fs::write(page_dir.join("page.html"), PAGE).map_err(|e| e.to_string())?;
    let frames_dir = work.join("frames");
    paths::fresh(&frames_dir)?;
    let [vw, vh] = size;
    let dev = Layout::new(size[0], size[1], look.window).boxed(look.inset)?;
    let avail = dev.avail();
    let start = record::shown([cast.cols, cast.rows]);
    let grid = cast.max_grid();
    let theme = xterm_theme(&look.theme);
    let theme_bg = look
        .theme
        .get("background")
        .and_then(|v| v.as_str())
        .and_then(crate::theme::hex)
        .unwrap_or([0, 0, 0]);
    let page_bg = format!("#{:02x}{:02x}{:02x}", theme_bg[0], theme_bg[1], theme_bg[2]);
    let mut b = Browser::launch(work)?;
    b.open(&file_url(&page_dir.join("page.html")))?;
    b.viewport(vw, vh)?;
    let fit_w = avail[0] as f64 / (grid[0] as f64 * metrics.0 as f64);
    let fit_h = avail[1] as f64 / (grid[1] as f64 * metrics.1 as f64);
    let mut size_px = (fit_w.min(fit_h) * 10.0).floor() / 10.0;
    let boot = |b: &mut Browser, px: f64| -> Result<Value, String> {
        let o = json!({
            "cols": grid[0], "rows": grid[1], "family": family_css(&look.font),
            "fonts": fonts_of(&look.font), "size": px, "theme": theme,
            "vw": vw, "vh": vh, "page": page_bg,
        });
        b.eval(&format!("window.boot({o})"))
    };
    let mut m = boot(&mut b, size_px)?;
    let mut tries = 0;
    loop {
        let w = m["w"].as_f64().unwrap_or(0.0);
        let h = m["h"].as_f64().unwrap_or(0.0);
        if m["cols"].as_u64() != Some(grid[0] as u64) || m["rows"].as_u64() != Some(grid[1] as u64)
        {
            return Err("xterm.js would not take the grid".into());
        }
        let fits = w.round() <= avail[0] as f64 && h.round() <= avail[1] as f64;
        tries += 1;
        if fits {
            let bigger = size_px + 0.1;
            let m2 = boot(&mut b, bigger)?;
            let w2 = m2["w"].as_f64().unwrap_or(0.0);
            let h2 = m2["h"].as_f64().unwrap_or(0.0);
            if tries < 80 && w2.round() <= avail[0] as f64 && h2.round() <= avail[1] as f64 {
                size_px = bigger;
                m = m2;
                continue;
            }
            m = boot(&mut b, size_px)?;
            break;
        }
        if size_px <= 4.0 || tries > 200 {
            return Err(format!(
                "a {}x{} grid does not fit {}x{}",
                grid[0], grid[1], size[0], size[1]
            ));
        }
        size_px = ((size_px - 0.1) * 10.0).round() / 10.0;
        m = boot(&mut b, size_px)?;
    }
    reflow_off(&m, cast.resizes())?;
    let largest = [
        (m["w"].as_f64().unwrap_or(0.0)).round() as u32,
        (m["h"].as_f64().unwrap_or(0.0)).round() as u32,
    ];
    let cell = m["cell"]
        .as_array()
        .map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)]);
    let (_, at) = dev.place(largest);
    b.eval(&format!("window.place({}, {}, null)", at[0], at[1]))?;
    let timeline = Timeline::of(&cast.events);
    let mut feeder = Feeder::new(&cast.events, &look.masks, start);
    feeder.canvas = largest;
    if start != grid {
        feeder.play(&mut b, &[Piece::Grid(start)], "null")?;
    }
    let canvas = feeder.canvas;
    let first = match take {
        Take::Still(at) => at,
        Take::Clip { .. } => timeline.at(0.0),
    };
    feeder.until(&mut b, first)?;
    let probe = b.screenshot()?;
    check_size(&probe, size)?;
    let bg = corners(&probe, [at[0], at[1], feeder.canvas[0], feeder.canvas[1]])?;
    let ground = look.ground.unwrap_or_else(|| compose::default_ground(bg));
    let mut backdrop = Backdrop {
        dev,
        largest,
        bg,
        ground,
        dir: page_dir.clone(),
        drawn: HashMap::new(),
        shown: None,
    };
    backdrop.show(&mut b, feeder.grid, feeder.canvas)?;
    let mut frames = vec![];
    let mut captured = 0;
    match take {
        Take::Still(_) => {
            let png = b.screenshot()?;
            let p = frames_dir.join("still.png");
            std::fs::write(&p, &png).map_err(|e| e.to_string())?;
            captured = 1;
            frames.push(p);
        }
        Take::Clip { frames: n, fps } => {
            let mut last: Option<PathBuf> = None;
            for k in 0..n {
                let t = timeline.at(fps.seconds(k));
                let mut changed = feeder.until(&mut b, t)?;
                changed |= backdrop.show(&mut b, feeder.grid, feeder.canvas)?;
                if changed || last.is_none() {
                    let png = b.screenshot()?;
                    let p = frames_dir.join(format!("frame-{k:06}.png"));
                    std::fs::write(&p, &png).map_err(|e| e.to_string())?;
                    captured += 1;
                    last = Some(p);
                }
                frames.push(last.clone().unwrap_or_default());
                crate::jobs::progress(k + 1);
            }
        }
    }
    Ok(Output {
        frames,
        captured,
        canvas,
        max: cast.resizes().then_some((grid, largest)),
        font_px: size_px,
        cell,
        bg,
        ground,
        chromium: b.version.clone(),
    })
}

pub fn frame_count(shown: f64, fps: Fps, fade: usize) -> usize {
    let body = (fps.frames(shown) - fade as f64).round().max(1.0) as usize;
    body + fade
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clip_holds_its_shown_time_and_its_fade() {
        assert_eq!(frame_count(10.0, Fps::new(120, 1), 0), 1200);
        assert_eq!(frame_count(10.01, Fps::new(120, 1), 0), 1201);
        assert_eq!(frame_count(5.0, Fps::new(120, 1), 12), 600);
        assert_eq!(frame_count(0.0, Fps::new(30, 1), 3), 4);
    }

    #[test]
    fn a_page_url_escapes_what_a_path_may_hold() {
        assert_eq!(
            file_url(Path::new("/work/me/my tree #1/50%/page.html")),
            "file:///work/me/my%20tree%20%231/50%25/page.html"
        );
        assert_eq!(
            file_url(Path::new("/srv/åb?c/page.html")),
            "file:///srv/%C3%A5b%3Fc/page.html"
        );
    }

    fn ev(t: f64, kind: Kind, data: &str) -> Event {
        Event {
            t,
            kind,
            data: data.into(),
        }
    }

    fn feeder<'a>(events: &[Event], masks: &'a crate::mask::Masks, grid: [u32; 2]) -> Feeder<'a> {
        Feeder::new(events, masks, grid)
    }

    fn play(screen: &mut vt100::Parser, found: &[Piece]) {
        for p in found {
            match p {
                Piece::Bytes(b) => screen.process(b),
                Piece::Grid(g) => record::set_grid(screen, *g),
            }
        }
    }

    fn none() -> crate::mask::Masks {
        crate::mask::Masks::new(&[], [8, 2]).unwrap()
    }

    #[test]
    fn frame_n_shows_the_terminal_as_it_was_at_its_shown_time() {
        let events = vec![
            ev(0.0, Kind::Marker, "show"),
            ev(0.1, Kind::Output, "a"),
            ev(0.15, Kind::Input, "x"),
            ev(0.5, Kind::Output, "b"),
            ev(0.6, Kind::Marker, "hide"),
            ev(0.8, Kind::Output, "c"),
            ev(1.0, Kind::Marker, "show"),
            ev(1.2, Kind::Output, "d"),
            ev(2.0, Kind::Marker, "end"),
        ];
        let tl = Timeline::of(&events);
        let fps = Fps::new(10, 1);
        assert_eq!(frame_count(tl.shown(), fps, 0), 16);
        let masks = none();
        let mut f = feeder(&events, &masks, [8, 2]);
        let mut screen = vt100::Parser::new(2, 8, 0);
        let mut seen = vec![];
        for n in 0..16 {
            if let Some((found, rects)) = f.due(tl.at(fps.seconds(n))).unwrap() {
                assert_eq!(rects, "null");
                play(&mut screen, &found);
            }
            seen.push(screen.screen().contents());
        }
        assert_eq!(seen[0], "");
        assert_eq!(seen[1], "a");
        assert_eq!(seen[4], "a");
        assert_eq!(seen[5], "ab");
        assert_eq!(seen[6], "abc");
        assert_eq!(seen[7], "abc");
        assert_eq!(seen[8], "abcd");
        assert_eq!(seen[15], "abcd");
        assert!(f.due(10.0).unwrap().is_none());
    }

    #[test]
    fn a_frame_takes_every_output_due_by_its_time_in_one_chunk() {
        let events = vec![
            ev(0.0, Kind::Output, "1"),
            ev(0.2, Kind::Output, "2"),
            ev(0.3 - 1e-12, Kind::Output, "3"),
            ev(0.4, Kind::Output, "4"),
        ];
        let masks = none();
        let mut f = feeder(&events, &masks, [8, 2]);
        assert_eq!(
            f.due(0.3).unwrap().unwrap().0,
            vec![Piece::Bytes(b"123".to_vec())]
        );
        assert!(f.due(0.3).unwrap().is_none());
        assert_eq!(
            f.due(5.0).unwrap().unwrap().0,
            vec![Piece::Bytes(b"4".to_vec())]
        );
    }

    #[test]
    fn a_mask_sends_its_cells_once_and_again_only_when_they_change() {
        let masks = crate::mask::Masks::new(
            &[crate::mask::Mask {
                pattern: Some("secret".into()),
                rect: None,
            }],
            [20, 2],
        )
        .unwrap();
        let events = vec![
            ev(0.1, Kind::Output, "a secret"),
            ev(0.2, Kind::Output, "!"),
            ev(0.3, Kind::Output, "\x1b[2J\x1b[H"),
        ];
        let mut f = feeder(&events, &masks, [20, 2]);
        let (_, first) = f.due(0.1).unwrap().unwrap();
        let cells: Vec<[u32; 4]> = serde_json::from_str(&first).unwrap();
        assert_eq!(cells, vec![[2, 0, 6, 1]]);
        assert_eq!(f.due(0.2).unwrap().unwrap().1, "null");
        assert_eq!(f.due(0.3).unwrap().unwrap().1, "[]");
    }

    #[test]
    fn an_xterm_theme_takes_the_scheme_and_falls_back_to_the_background() {
        let t = xterm_theme(&json!({
            "background": "#000000", "foreground": "#ffffff", "cursor": "#ff0000",
            "selection": "#333333", "red": "#aa0000", "brightWhite": "#eeeeee"
        }));
        assert_eq!(t["cursorAccent"], "#000000");
        assert_eq!(t["selectionBackground"], "#333333");
        assert_eq!(t["red"], "#aa0000");
        assert_eq!(t["brightWhite"], "#eeeeee");
        assert!(t["green"].is_null());
        assert_eq!(t.as_object().unwrap().len(), 21);
        let own = xterm_theme(&json!({"background": "#000000", "cursorAccent": "#123456"}));
        assert_eq!(own["cursorAccent"], "#123456");
    }

    #[test]
    fn the_page_background_is_the_colour_most_corners_agree_on() {
        let mut img = image::RgbImage::from_pixel(10, 10, image::Rgb([9, 9, 9]));
        img.put_pixel(1, 1, image::Rgb([200, 0, 0]));
        let mut png = vec![];
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert_eq!(corners(&png, [0, 0, 10, 10]).unwrap(), [9, 9, 9]);
        assert_eq!(corners(&png, [0, 0, 400, 400]).unwrap(), [9, 9, 9]);
        assert!(check_size(&png, [10, 10]).is_ok());
        assert_eq!(
            check_size(&png, [12, 10]).unwrap_err(),
            "chromium drew 10x10, not 12x10"
        );
        assert!(corners(b"not a png", [0, 0, 1, 1]).is_err());
        assert!(check_size(b"", [1, 1]).is_err());
    }

    #[test]
    fn the_font_stack_falls_back_to_cjk_emoji_and_monospace() {
        assert_eq!(
            family_css("Mono"),
            "\"Mono\", \"Noto Sans CJK JP\", \"Noto Color Emoji\", monospace"
        );
        assert_eq!(fonts_of("Mono")[0], "Mono");
        assert_eq!(fonts_of("Mono").len(), 3);
        assert!(assets_ok().unwrap().contains(XTERM));
    }

    #[test]
    fn a_frame_between_two_resizes_shows_the_earlier_grid_and_hidden_resizes_still_apply() {
        let events = vec![
            ev(0.0, Kind::Marker, "show"),
            ev(0.1, Kind::Output, "a"),
            ev(0.3, Kind::Resize, "6x2"),
            ev(0.35, Kind::Output, "b"),
            ev(0.6, Kind::Resize, "5x2"),
            ev(0.7, Kind::Marker, "hide"),
            ev(0.8, Kind::Resize, "4x2"),
            ev(0.85, Kind::Output, "c"),
            ev(0.9, Kind::Resize, "3x2"),
            ev(1.0, Kind::Marker, "show"),
            ev(1.5, Kind::Marker, "end"),
        ];
        let tl = Timeline::of(&events);
        let fps = Fps::new(10, 1);
        let masks = none();
        let mut f = feeder(&events, &masks, [8, 2]);
        let mut screen = vt100::Parser::new(2, 8, 0);
        let mut grids = vec![];
        let mut sizes = vec![];
        for n in 0..12 {
            if let Some((found, _)) = f.due(tl.at(fps.seconds(n))).unwrap() {
                play(&mut screen, &found);
            }
            grids.push(f.grid);
            sizes.push(screen.screen().size());
        }
        assert_eq!(grids[0], [8, 2]);
        assert_eq!(grids[2], [8, 2]);
        assert_eq!(grids[3], [6, 2]);
        assert_eq!(grids[5], [6, 2]);
        assert_eq!(grids[6], [5, 2]);
        assert_eq!(grids[7], [3, 2]);
        assert_eq!(grids[11], [3, 2]);
        assert_eq!(sizes[4], (2, 6));
        assert_eq!(sizes[7], (2, 3));
        assert!(screen.screen().contents().starts_with("abc"));
    }

    #[test]
    fn a_chunk_keeps_outputs_and_every_resize_in_their_order() {
        let events = vec![
            ev(0.1, Kind::Output, "a"),
            ev(0.2, Kind::Output, "b"),
            ev(0.3, Kind::Resize, "7x2"),
            ev(0.4, Kind::Resize, "6x2"),
            ev(0.5, Kind::Output, "c"),
            ev(0.6, Kind::Resize, "5x3"),
            ev(0.7, Kind::Marker, "end"),
        ];
        let masks = none();
        let mut f = feeder(&events, &masks, [8, 2]);
        assert_eq!(
            f.due(1.0).unwrap().unwrap().0,
            vec![
                Piece::Bytes(b"ab".to_vec()),
                Piece::Grid([7, 2]),
                Piece::Grid([6, 2]),
                Piece::Bytes(b"c".to_vec()),
                Piece::Grid([5, 3]),
            ]
        );
        assert_eq!(f.grid, [5, 3]);
        assert!(f.due(2.0).unwrap().is_none());
        assert_eq!(
            pieces(&[ev(0.1, Kind::Input, "x"), ev(0.2, Kind::Marker, "end")]),
            vec![]
        );
    }

    #[test]
    fn a_resize_sends_the_rectangles_clipped_to_the_new_grid() {
        let masks = crate::mask::Masks::new(
            &[crate::mask::Mask {
                pattern: None,
                rect: Some([4, 0, 4, 2]),
            }],
            [8, 2],
        )
        .unwrap();
        let events = vec![
            ev(0.1, Kind::Output, "x"),
            ev(0.2, Kind::Resize, "6x2"),
            ev(0.3, Kind::Resize, "4x2"),
            ev(0.4, Kind::Resize, "8x2"),
        ];
        let mut f = feeder(&events, &masks, [8, 2]);
        let rects = |f: &mut Feeder, t: f64| -> Vec<[u32; 4]> {
            serde_json::from_str(&f.due(t).unwrap().unwrap().1).unwrap()
        };
        assert_eq!(rects(&mut f, 0.1), vec![[4, 0, 4, 2]]);
        assert_eq!(rects(&mut f, 0.2), vec![[4, 0, 2, 2]]);
        assert_eq!(rects(&mut f, 0.3), Vec::<[u32; 4]>::new());
        assert_eq!(rects(&mut f, 0.4), vec![[4, 0, 4, 2]]);
        assert_eq!(f.screen.screen().size(), (2, 8));
    }

    #[test]
    fn a_shrink_and_a_regrow_in_one_batch_both_reach_the_page_and_cut_the_text_off() {
        let masks = crate::mask::Masks::new(
            &[crate::mask::Mask {
                pattern: Some("secret".into()),
                rect: None,
            }],
            [40, 2],
        )
        .unwrap();
        let events = vec![
            ev(0.1, Kind::Output, "\x1b[1;25Hsecret"),
            ev(0.2, Kind::Resize, "20x2"),
            ev(0.2, Kind::Resize, "40x2"),
        ];
        let mut f = feeder(&events, &masks, [40, 2]);
        let (found, rects) = f.due(1.0).unwrap().unwrap();
        assert_eq!(
            found,
            vec![
                Piece::Bytes(b"\x1b[1;25Hsecret".to_vec()),
                Piece::Grid([20, 2]),
                Piece::Grid([40, 2]),
            ]
        );
        assert_eq!(f.grid, [40, 2]);
        assert_eq!(f.screen.screen().size(), (2, 40));
        assert!(!f.screen.screen().contents().contains("secret"));
        assert_eq!(rects, "null");
        assert_eq!(
            batch(&found),
            format!("[\"{}\",[20,2],[40,2]]", b64(b"\x1b[1;25Hsecret"))
        );
    }

    #[test]
    fn a_batch_is_one_page_call_with_its_pieces_in_order() {
        let found = vec![
            Piece::Bytes(b"ab".to_vec()),
            Piece::Grid([6, 2]),
            Piece::Bytes(b"c".to_vec()),
        ];
        assert_eq!(
            batch(&found),
            format!("[\"{}\",[6,2],\"{}\"]", b64(b"ab"), b64(b"c"))
        );
        assert_eq!(batch(&[]), "[]");
    }

    #[test]
    fn a_match_mask_follows_the_line_a_row_shrink_keeps_in_view() {
        let masks = crate::mask::Masks::new(
            &[crate::mask::Mask {
                pattern: Some("secret".into()),
                rect: None,
            }],
            [12, 10],
        )
        .unwrap();
        let events = vec![
            ev(
                0.1,
                Kind::Output,
                "a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nsecret\r\n$ ",
            ),
            ev(0.2, Kind::Resize, "12x4"),
        ];
        let mut f = feeder(&events, &masks, [12, 10]);
        let rects = |f: &mut Feeder, t: f64| -> Vec<[u32; 4]> {
            serde_json::from_str(&f.due(t).unwrap().unwrap().1).unwrap()
        };
        assert_eq!(rects(&mut f, 0.1), vec![[0, 7, 6, 1]]);
        assert_eq!(rects(&mut f, 0.2), vec![[0, 2, 6, 1]]);
    }

    fn backdrop(window: bool, name: &str) -> Backdrop {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tmp/unit-replay/{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Backdrop {
            dev: Layout::new(64, 36, window),
            largest: [40, 20],
            bg: [10, 20, 30],
            ground: [200, 200, 200],
            dir,
            drawn: HashMap::new(),
            shown: None,
        }
    }

    #[test]
    fn a_window_background_is_drawn_once_per_grid_and_reused_on_a_revisit() {
        let mut d = backdrop(true, "window");
        let a = d.file([8, 2], [40, 20]).unwrap();
        assert_eq!(a, "background-8x2.png");
        assert!(d.dir.join(&a).is_file());
        std::fs::remove_file(d.dir.join(&a)).unwrap();
        let b = d.file([4, 2], [20, 20]).unwrap();
        assert_eq!(b, "background-4x2.png");
        assert!(d.dir.join(&b).is_file());
        assert_eq!(d.file([8, 2], [40, 20]).unwrap(), a);
        assert!(!d.dir.join(&a).exists());
        assert_eq!(d.drawn.len(), 2);
        let _ = std::fs::remove_dir_all(&d.dir);
    }

    #[test]
    fn a_bare_background_is_drawn_once_for_every_grid() {
        let mut d = backdrop(false, "bare");
        let a = d.file([8, 2], [40, 20]).unwrap();
        assert_eq!(a, "background.png");
        std::fs::remove_file(d.dir.join(&a)).unwrap();
        assert_eq!(d.file([4, 2], [20, 20]).unwrap(), a);
        assert_eq!(d.file([3, 5], [15, 50]).unwrap(), a);
        assert!(!d.dir.join(&a).exists());
        assert_eq!(d.drawn.len(), 1);
        assert_eq!(d.key([4, 2]), d.key([8, 2]));
        let _ = std::fs::remove_dir_all(&d.dir);
    }

    #[test]
    fn a_resizing_cast_needs_the_page_to_report_its_reflow_off() {
        let said = "xterm.js would not turn its reflow off, so a resize would not match the masks";
        for m in [json!({"reflow": true}), json!({"reflow": null}), json!({})] {
            assert_eq!(reflow_off(&m, true).unwrap_err(), said, "{m}");
            reflow_off(&m, false).unwrap();
        }
        reflow_off(&json!({"reflow": false}), true).unwrap();
    }

    #[test]
    fn a_one_column_resize_reaches_the_page_as_the_two_columns_xterm_js_keeps() {
        let masks = none();
        let events = vec![ev(0.1, Kind::Resize, "1x3"), ev(0.2, Kind::Output, "ab")];
        let mut f = feeder(&events, &masks, [8, 2]);
        assert_eq!(
            f.due(1.0).unwrap().unwrap().0,
            vec![Piece::Grid([2, 3]), Piece::Bytes(b"ab".to_vec())]
        );
        assert_eq!(f.grid, [2, 3]);
    }

    #[test]
    fn a_match_mask_stays_on_its_row_through_a_column_shrink_and_covers_what_is_left() {
        let masks = crate::mask::Masks::new(
            &[crate::mask::Mask {
                pattern: Some("sk-\\w+".into()),
                rect: None,
            }],
            [40, 6],
        )
        .unwrap();
        let events = vec![
            ev(
                0.1,
                Kind::Output,
                "token: sk-ABCDEFGHIJKLMNOPQRSTUVWXYZ012\r\n$ ",
            ),
            ev(0.2, Kind::Resize, "20x6"),
            ev(0.3, Kind::Resize, "40x6"),
        ];
        let mut f = feeder(&events, &masks, [40, 6]);
        let rects = |f: &mut Feeder, t: f64| -> Vec<[u32; 4]> {
            serde_json::from_str(&f.due(t).unwrap().unwrap().1).unwrap()
        };
        assert_eq!(rects(&mut f, 0.1), vec![[7, 0, 32, 1]]);
        assert_eq!(rects(&mut f, 0.2), vec![[7, 0, 13, 1]]);
        assert_eq!(f.due(0.3).unwrap().unwrap().1, "null");
        assert_eq!(f.covered, vec![[7, 0, 13, 1]]);
        let shown: Vec<String> = f.screen.screen().rows(0, 40).collect();
        assert_eq!(shown[..2], ["token: sk-ABCDEFGHIJ", "$ "]);
    }
}
