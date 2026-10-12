use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const ENV: &str = "PREC_CONFIG";
pub const DEFAULT_THEME: &str = "Catppuccin Mocha";

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub archive: Option<PathBuf>,
    pub theme: Option<String>,
    pub chromium: Option<PathBuf>,
    pub confine: Option<Confine>,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Confine {
    pub slice: Option<String>,
    pub cpus: Option<String>,
    pub cpus_from: Option<PathBuf>,
    pub cpus_key: Option<String>,
}

static LOADED: OnceLock<(Option<PathBuf>, Config)> = OnceLock::new();

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

fn default_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| Some(home()?.join(".config")))
        .map(|d| d.join("prec/config.toml"))
}

pub fn path() -> Option<PathBuf> {
    match std::env::var_os(ENV).filter(|v| !v.is_empty()) {
        Some(p) => Some(PathBuf::from(p)),
        None => default_path(),
    }
}

pub fn expand(p: &Path) -> PathBuf {
    match p.strip_prefix("~") {
        Ok(rest) => home().map(|h| h.join(rest)).unwrap_or_else(|| p.into()),
        Err(_) => p.into(),
    }
}

pub fn parse(text: &str) -> Result<Config, String> {
    let mut c: Config = toml::from_str(text).map_err(|e| e.message().to_string())?;
    c.archive = c.archive.as_deref().map(expand);
    c.chromium = c.chromium.as_deref().map(expand);
    if let Some(confine) = c.confine.as_mut() {
        confine.cpus_from = confine.cpus_from.as_deref().map(expand);
        if confine.cpus.is_some() && confine.cpus_from.is_some() {
            return Err("[confine] takes cpus or cpus_from, not both".into());
        }
    }
    Ok(c)
}

fn read(path: Option<&Path>, explicit: bool) -> Result<Config, String> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !explicit => Ok(Config::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn load() -> Result<(), String> {
    if LOADED.get().is_some() {
        return Ok(());
    }
    let explicit = std::env::var_os(ENV).is_some_and(|v| !v.is_empty());
    let path = path();
    let config = read(path.as_deref(), explicit)?;
    let _ = LOADED.set((path, config));
    Ok(())
}

pub fn get() -> &'static Config {
    &LOADED.get_or_init(|| (None, Config::default())).1
}

pub fn shown() -> String {
    match LOADED
        .get()
        .and_then(|(p, _)| p.as_ref())
        .or(path().as_ref())
    {
        Some(p) if p.is_file() => p.display().to_string(),
        Some(p) => format!("{} (absent; the defaults apply)", p.display()),
        None => "none (no HOME; the defaults apply)".into(),
    }
}

pub fn theme() -> &'static str {
    get().theme.as_deref().unwrap_or(DEFAULT_THEME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_config_is_the_defaults() {
        assert_eq!(parse("").unwrap(), Config::default());
    }

    #[test]
    fn every_key_parses_and_a_typo_is_refused() {
        let c = parse(
            "archive = \"/srv/archive\"\ntheme = \"nord\"\nchromium = \"/opt/chrome\"\n[confine]\nslice = \"render.slice\"\ncpus = \"4-7\"\n",
        )
        .unwrap();
        assert_eq!(c.archive.as_deref(), Some(Path::new("/srv/archive")));
        assert_eq!(c.theme.as_deref(), Some("nord"));
        let confine = c.confine.unwrap();
        assert_eq!(confine.slice.as_deref(), Some("render.slice"));
        assert_eq!(confine.cpus.as_deref(), Some("4-7"));
        assert!(parse("archiv = \"/srv\"\n").is_err());
        assert!(parse("[confine]\ncpus = \"1\"\ncpus_from = \"/x\"\n").is_err());
    }

    #[test]
    fn a_missing_default_file_is_the_defaults_but_a_named_one_is_an_error() {
        let missing = Path::new("/nonexistent/prec/config.toml");
        assert_eq!(read(Some(missing), false).unwrap(), Config::default());
        assert!(read(Some(missing), true).is_err());
    }
}
