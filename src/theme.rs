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

    const COLOURS: [&str; 18] = [
        "background",
        "foreground",
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

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/theme-unit")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn palette(extra: &str) -> String {
        let mut text = String::from("background = \"#101010\"\nforeground = \"#f0f0f0\"\n");
        for i in 0..16 {
            text.push_str(&format!("color{i} = \"#{:02x}{:02x}{:02x}\"\n", i, i, i));
        }
        text.push_str(extra);
        text
    }

    #[test]
    fn every_vendored_theme_has_a_unique_name_and_colours_that_parse() {
        let all = vendored();
        assert!(all.len() > 300, "{}", all.len());
        let names = builtins();
        assert_eq!(names.len(), all.len());
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
        for t in &all {
            let name = t["name"].as_str().unwrap();
            for k in COLOURS {
                let v = t[k].as_str().unwrap_or_else(|| panic!("{name}: no {k}"));
                assert!(hex(v).is_some(), "{name}: {k} {v}");
            }
            for k in ["selection", "cursor"] {
                if let Some(v) = t.get(k) {
                    assert!(hex(v.as_str().unwrap()).is_some(), "{name}: {k} {v}");
                }
            }
            assert_ne!(
                hex(t["background"].as_str().unwrap()),
                hex(t["foreground"].as_str().unwrap()),
                "{name}"
            );
        }
    }

    #[test]
    fn the_curated_pairs_and_singles_are_builtins_and_each_light_is_lighter() {
        for (pair, light, dark) in PAIRS {
            let l = builtin(light).unwrap_or_else(|| panic!("{pair}: no {light}"));
            let d = builtin(dark).unwrap_or_else(|| panic!("{pair}: no {dark}"));
            let lum = |t: &Value| luminance(hex(t["background"].as_str().unwrap()).unwrap());
            assert!(lum(&l) > lum(&d), "{pair}");
        }
        for s in SINGLES {
            assert!(builtin(s).is_some(), "{s}");
        }
        assert!(builtin(crate::config::DEFAULT_THEME).is_some());
        assert!(builtin("No Such Theme").is_none());
    }

    #[test]
    fn hex_takes_six_digits_with_or_without_the_hash() {
        assert_eq!(hex(" #FFFFFF "), Some([255, 255, 255]));
        assert_eq!(hex("00ff7f"), Some([0, 255, 127]));
        assert_eq!(hex("#abc"), None);
        assert_eq!(hex("#abcdef0"), None);
        assert_eq!(hex("#zzzzzz"), None);
        assert_eq!(hex(""), None);
    }

    #[test]
    fn a_named_theme_resolves_to_its_vhs_keys_with_a_slug_label() {
        let c = resolve("Catppuccin Mocha", None, "alpha").unwrap();
        assert_eq!(c.label.as_deref(), Some("catppuccin-mocha"));
        assert_eq!(c.name, "Catppuccin Mocha");
        assert_eq!(c.json["name"], "Catppuccin Mocha");
        assert!(c.json.get("isDark").is_none());
        for k in COLOURS {
            assert!(c.json.get(k).is_some(), "{k}");
        }
        assert_eq!(
            resolve("No Such", None, "alpha").unwrap_err(),
            "no theme \"No Such\"; see `prec themes`"
        );
        let e = resolve("omarchy:zz-no-such-theme", None, "alpha").unwrap_err();
        assert!(e.starts_with("no Omarchy theme zz-no-such-theme"), "{e}");
    }

    #[test]
    fn app_takes_the_apps_palette_named_after_the_product_or_falls_back() {
        let own = json!({"background": "#000000", "foreground": "#ffffff", "mode": "dark"});
        let c = resolve("app", Some(&own), "alpha").unwrap();
        assert_eq!(c.label, None);
        assert_eq!(c.name, "app");
        assert_eq!(
            c.json,
            json!({"name": "alpha", "background": "#000000", "foreground": "#ffffff"})
        );
        let named = json!({"name": "Alpha Night", "background": "#000000"});
        assert_eq!(
            resolve("app", Some(&named), "alpha").unwrap().json["name"],
            "Alpha Night"
        );
        let fallback = resolve("app", None, "alpha").unwrap();
        assert_eq!(fallback.label, None);
        assert_eq!(fallback.name, "app");
        assert_eq!(fallback.json["name"], crate::config::theme());
    }

    #[test]
    fn a_colors_toml_with_ansi_slots_reads_directly_and_guesses_its_mode() {
        let dark = from_colors_toml("alpha", &palette("")).unwrap();
        assert_eq!(dark["name"], "alpha");
        assert_eq!(dark["black"], "#000000");
        assert_eq!(dark["brightWhite"], "#0f0f0f");
        assert_eq!(dark["selection"], "#f0f0f0");
        assert_eq!(dark["cursor"], "#f0f0f0");
        assert_eq!(dark["cursorAccent"], "#101010");
        assert_eq!(dark["mode"], "dark");
        let light = from_colors_toml(
            "beta",
            &palette("selection_background = \"#333333\"\ncursor = \"#444444\"\n")
                .replace("#101010", "#fafafa"),
        )
        .unwrap();
        assert_eq!(light["mode"], "light");
        assert_eq!(light["selection"], "#333333");
        assert_eq!(light["cursor"], "#444444");
        assert!(from_colors_toml("x", &palette("").replace("color5 =", "colour5 =")).is_none());
        assert!(from_colors_toml("x", &palette("").replace("background =", "bg =")).is_none());
        assert!(from_colors_toml("x", "not toml [").is_none());
    }

    #[test]
    fn a_kitty_conf_reads_its_colours_and_skips_comments() {
        let mut conf = String::from(
            "# a comment\n\nfont_family Alpha Mono\nbackground #fafafa\nforeground #101010\nselection_background #222222\ncursor #333333\n",
        );
        for i in 0..16 {
            conf.push_str(&format!("color{i} #{:02x}0000\n", i));
        }
        let t = from_kitty("gamma", &conf).unwrap();
        assert_eq!(t["name"], "gamma");
        assert_eq!(t["red"], "#010000");
        assert_eq!(t["brightWhite"], "#0f0000");
        assert_eq!(t["selection"], "#222222");
        assert_eq!(t["cursor"], "#333333");
        assert_eq!(t["mode"], "light");
        assert!(from_kitty("gamma", &conf.replace("color9 ", "# color9 ")).is_none());
        assert!(from_kitty("gamma", &conf.replace("background #fafafa\n", "")).is_none());
    }

    #[test]
    fn omarchy_themes_are_read_from_every_root_and_a_later_root_wins() {
        let dir = scratch("omarchy");
        let (first, second) = (dir.join("first"), dir.join("second"));
        for d in ["alpha", "beta", "gamma"] {
            std::fs::create_dir_all(first.join(d)).unwrap();
        }
        std::fs::create_dir_all(second.join("alpha")).unwrap();
        std::fs::write(first.join("alpha/colors.toml"), palette("")).unwrap();
        std::fs::write(
            second.join("alpha/colors.toml"),
            palette("mode = \"light\"\n"),
        )
        .unwrap();
        let mut kitty = String::from("background #000000\nforeground #ffffff\n");
        for i in 0..16 {
            kitty.push_str(&format!("color{i} #0000{:02x}\n", i));
        }
        std::fs::write(first.join("beta/kitty.conf"), kitty).unwrap();
        std::fs::write(first.join("gamma/colors.toml"), "broken").unwrap();
        let all = read_omarchy(&[first, dir.join("missing"), second]);
        assert_eq!(all.keys().collect::<Vec<_>>(), ["alpha", "beta"]);
        assert_eq!(all["alpha"]["mode"], "light");
        assert_eq!(all["beta"]["blue"], "#000004");
        assert!(read_omarchy(&[dir.join("missing")]).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
