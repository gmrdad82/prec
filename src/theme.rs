use crate::paths;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

const VHS_THEMES: &str = include_str!("../assets/vhs-themes.json");

pub const PAIRS: &[(&str, &str, &str)] = &[
    ("catppuccin", "Catppuccin Latte", "Catppuccin Mocha"),
    ("tokyo-night", "TokyoNightLight", "TokyoNight"),
    ("gruvbox", "Gruvbox Light", "GruvboxDark"),
    (
        "solarized",
        "Builtin Solarized Light",
        "Builtin Solarized Dark",
    ),
    ("github", "Github", "GitHub Dark"),
    ("one-half", "OneHalfLight", "OneHalfDark"),
    ("nord", "nord-light", "nord"),
];

pub const SINGLES: &[&str] = &["Rose Pine", "Dracula", "Kanagawa"];

#[derive(Clone, Debug)]
pub struct Choice {
    pub label: Option<String>,
    pub name: String,
    pub json: Value,
}

pub fn hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn omarchy_roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut roots = vec![PathBuf::from("/usr/share/omarchy/themes")];
    if let Some(p) = std::env::var_os("OMARCHY_PATH").filter(|p| !p.is_empty()) {
        roots.push(PathBuf::from(p).join("themes"));
    }
    if let Some(h) = home {
        roots.push(h.join(".local/share/omarchy/themes"));
        roots.push(h.join(".config/omarchy/themes"));
    }
    let mut seen = Vec::new();
    roots.retain(|r| {
        let fresh = !seen.contains(r);
        seen.push(r.clone());
        fresh
    });
    roots
}

pub fn omarchy() -> BTreeMap<String, Value> {
    read_omarchy(&omarchy_roots())
}

pub fn vendored() -> Vec<Value> {
    serde_json::from_str::<Vec<Value>>(VHS_THEMES).unwrap_or_default()
}

pub fn builtin(name: &str) -> Option<Value> {
    vendored().into_iter().find(|t| t["name"] == name)
}

pub fn builtins() -> Vec<String> {
    vendored()
        .iter()
        .filter_map(|t| t["name"].as_str().map(|s| s.to_string()))
        .collect()
}

fn vhs_json(v: &Value) -> Value {
    let mut m = Map::new();
    for k in [
        "name",
        "background",
        "foreground",
        "selection",
        "cursor",
        "cursorAccent",
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
        if let Some(x) = v.get(k) {
            m.insert(k.to_string(), x.clone());
        }
    }
    Value::Object(m)
}

pub fn resolve(arg: &str, app_theme: Option<&Value>, product: &str) -> Result<Choice, String> {
    if arg == "app" {
        let Some(t) = app_theme else {
            let fallback = crate::config::theme();
            let mut choice = resolve(fallback, None, product)
                .map_err(|e| format!("{product}'s terminal.toml has no theme, and {e}"))?;
            choice.label = None;
            choice.name = "app".into();
            return Ok(choice);
        };
        let mut j = vhs_json(t);
        if j.get("name").is_none() {
            j["name"] = json!(product);
        }
        return Ok(Choice {
            label: None,
            name: "app".into(),
            json: j,
        });
    }
    if let Some(name) = arg.strip_prefix("omarchy:") {
        let all = omarchy();
        let t = all.get(name).ok_or_else(|| {
            if all.is_empty() {
                format!("no Omarchy theme {name}: no Omarchy themes are installed here")
            } else {
                format!(
                    "no Omarchy theme {name}; installed: {}",
                    all.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            }
        })?;
        let j = vhs_json(t);
        return Ok(Choice {
            label: Some(paths::slug(&format!("omarchy-{name}"))),
            name: arg.to_string(),
            json: j,
        });
    }
    let t = builtin(arg).ok_or_else(|| format!("no theme {arg:?}; see `prec themes`"))?;
    Ok(Choice {
        label: Some(paths::slug(arg)),
        name: arg.to_string(),
        json: vhs_json(&t),
    })
}

fn luminance(c: [u8; 3]) -> f32 {
    let lin = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

fn from_colors_toml(name: &str, text: &str) -> Option<Value> {
    let t: toml::Table = toml::from_str(text).ok()?;
    let g = |k: &str| t.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let mut out = Map::new();
    out.insert("name".into(), json!(name));
    let direct = g("color0").is_some();
    let keys = [
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
    ];
    let template = [
        "background",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "foreground",
        "muted",
        "bright_red",
        "bright_green",
        "bright_yellow",
        "bright_blue",
        "bright_magenta",
        "bright_cyan",
        "bright_foreground",
    ];
    for (i, k) in keys.iter().enumerate() {
        let v = if direct {
            g(&format!("color{i}"))
        } else {
            g(template[i])
        }?;
        out.insert(k.to_string(), json!(v));
    }
    let bg = g("background")?;
    let fg = g("foreground")?;
    out.insert("background".into(), json!(bg));
    out.insert("foreground".into(), json!(fg));
    let sel = g("selection_background")
        .or_else(|| g("selection"))
        .unwrap_or_else(|| fg.clone());
    out.insert("selection".into(), json!(sel));
    let cur = g("cursor")
        .or_else(|| g("bright_foreground"))
        .unwrap_or_else(|| fg.clone());
    out.insert("cursor".into(), json!(cur));
    out.insert("cursorAccent".into(), json!(bg));
    let mode = g("mode").unwrap_or_else(|| {
        if hex(&bg).map(luminance).unwrap_or(0.0) > 0.4 {
            "light".into()
        } else {
            "dark".into()
        }
    });
    out.insert("mode".into(), json!(mode));
    Some(Value::Object(out))
}

fn from_kitty(name: &str, text: &str) -> Option<Value> {
    let mut kv = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        if let (Some(k), Some(v)) = (it.next(), it.next())
            && v.starts_with('#')
        {
            kv.insert(k.to_string(), v.to_string());
        }
    }
    let mut toml_text = String::new();
    for i in 0..16 {
        toml_text.push_str(&format!("color{i} = {:?}\n", kv.get(&format!("color{i}"))?));
    }
    toml_text.push_str(&format!("background = {:?}\n", kv.get("background")?));
    toml_text.push_str(&format!("foreground = {:?}\n", kv.get("foreground")?));
    if let Some(s) = kv.get("selection_background") {
        toml_text.push_str(&format!("selection_background = {s:?}\n"));
    }
    if let Some(s) = kv.get("cursor") {
        toml_text.push_str(&format!("cursor = {s:?}\n"));
    }
    from_colors_toml(name, &toml_text)
}

fn read_omarchy(roots: &[PathBuf]) -> BTreeMap<String, Value> {
    let mut all: BTreeMap<String, Value> = BTreeMap::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        dirs.sort();
        for d in dirs {
            let name = d
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let theme = std::fs::read_to_string(d.join("colors.toml"))
                .ok()
                .and_then(|t| from_colors_toml(&name, &t))
                .or_else(|| {
                    std::fs::read_to_string(d.join("kitty.conf"))
                        .ok()
                        .and_then(|t| from_kitty(&name, &t))
                });
            if let Some(t) = theme {
                all.insert(name, t);
            }
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_omarchy_palette_maps_the_way_its_kitty_template_does() {
        let t = from_colors_toml(
            "x",
            "mode = \"light\"\nbackground = \"#ffffff\"\nforeground = \"#000000\"\nmuted = \"#888888\"\nred = \"#ff0000\"\ngreen = \"#00ff00\"\nyellow = \"#ffff00\"\nblue = \"#0000ff\"\nmagenta = \"#ff00ff\"\ncyan = \"#00ffff\"\nbright_red = \"#ff1111\"\nbright_green = \"#11ff11\"\nbright_yellow = \"#ffff11\"\nbright_blue = \"#1111ff\"\nbright_magenta = \"#ff11ff\"\nbright_cyan = \"#11ffff\"\nbright_foreground = \"#111111\"\nselection = \"#cccccc\"\n",
        )
        .unwrap();
        assert_eq!(t["black"], "#ffffff");
        assert_eq!(t["white"], "#000000");
        assert_eq!(t["brightBlack"], "#888888");
        assert_eq!(t["brightWhite"], "#111111");
        assert_eq!(t["mode"], "light");
        assert_eq!(hex("#1e1e2e"), Some([0x1e, 0x1e, 0x2e]));
    }
}
