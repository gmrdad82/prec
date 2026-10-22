use crate::cdp::Browser;
use crate::compose::{self, Layout};
use crate::fps::Fps;
use crate::paths;
use crate::record::{Cast, Event, Kind, Timeline};
use base64::Engine;
use serde_json::{Value, json};
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
  return { w: r.width, h: r.height, cols: term.cols, rows: term.rows, cell: cell };
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
window.feed = (b64, rects) => new Promise(res => {
  const bin = atob(b64);
  const a = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) a[i] = bin.charCodeAt(i);
  term.write(a, () => { if (rects) cover(rects); frame2().then(res); });
});
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

fn outputs(events: &[Event]) -> Vec<(f64, Vec<u8>)> {
    events
        .iter()
        .filter(|e| e.kind == Kind::Output)
        .map(|e| (e.t, e.data.as_bytes().to_vec()))
        .collect()
}

struct Feeder<'a> {
    out: Vec<(f64, Vec<u8>)>,
    next: usize,
    masks: &'a crate::mask::Masks,
    screen: vt100::Parser,
    covered: Vec<[u32; 4]>,
}

impl Feeder<'_> {
    fn due(&mut self, t: f64) -> Result<Option<(Vec<u8>, String)>, String> {
        let mut chunk = vec![];
        while self.next < self.out.len() && self.out[self.next].0 <= t + 1e-9 {
            chunk.extend_from_slice(&self.out[self.next].1);
            self.next += 1;
        }
        if chunk.is_empty() {
            return Ok(None);
        }
        let mut rects = "null".to_string();
        if !self.masks.is_empty() {
            self.screen.process(&chunk);
            let now = self.masks.cells(self.screen.screen());
            if now != self.covered {
                rects = serde_json::to_string(&now).map_err(|e| e.to_string())?;
                self.covered = now;
            }
        }
        Ok(Some((chunk, rects)))
    }

    fn until(&mut self, b: &mut Browser, t: f64) -> Result<bool, String> {
        let Some((chunk, rects)) = self.due(t)? else {
            return Ok(false);
        };
        b.eval(&format!("window.feed({:?}, {rects})", b64(&chunk)))?;
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
    let grid = [cast.cols, cast.rows];
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
    let canvas = [
        (m["w"].as_f64().unwrap_or(0.0)).round() as u32,
        (m["h"].as_f64().unwrap_or(0.0)).round() as u32,
    ];
    let cell = m["cell"]
        .as_array()
        .map(|a| [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)]);
    let (_, at) = dev.place(canvas);
    b.eval(&format!("window.place({}, {}, null)", at[0], at[1]))?;
    let timeline = Timeline::of(&cast.events);
    let mut feeder = Feeder {
        out: outputs(&cast.events),
        next: 0,
        masks: &look.masks,
        screen: vt100::Parser::new(grid[1] as u16, grid[0] as u16, 0),
        covered: vec![],
    };
    let first = match take {
        Take::Still(at) => at,
        Take::Clip { .. } => timeline.at(0.0),
    };
    feeder.until(&mut b, first)?;
    let probe = b.screenshot()?;
    check_size(&probe, size)?;
    let bg = corners(&probe, [at[0], at[1], canvas[0], canvas[1]])?;
    let ground = look.ground.unwrap_or_else(|| compose::default_ground(bg));
    let pm = compose::background(&dev, canvas, bg, ground)?;
    let bg_path = page_dir.join("background.png");
    pm.save_png(&bg_path).map_err(|e| e.to_string())?;
    b.eval(&format!(
        "window.place({}, {}, 'background.png')",
        at[0], at[1]
    ))?;
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
                let changed = feeder.until(&mut b, t)?;
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
        Feeder {
            out: outputs(events),
            next: 0,
            masks,
            screen: vt100::Parser::new(grid[1] as u16, grid[0] as u16, 0),
            covered: vec![],
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
            if let Some((chunk, rects)) = f.due(tl.at(fps.seconds(n))).unwrap() {
                assert_eq!(rects, "null");
                screen.process(&chunk);
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
        assert_eq!(f.due(0.3).unwrap().unwrap().0, b"123");
        assert!(f.due(0.3).unwrap().is_none());
        assert_eq!(f.due(5.0).unwrap().unwrap().0, b"4");
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
}
