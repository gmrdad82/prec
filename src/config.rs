use serde::Deserialize;
use std::ffi::OsString;
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
    absolute(std::env::var_os("HOME"))
}

fn absolute(var: Option<OsString>) -> Option<PathBuf> {
    var.map(PathBuf::from).filter(|p| p.is_absolute())
}

fn default_path() -> Option<PathBuf> {
    default_path_in(std::env::var_os("XDG_CONFIG_HOME"), home())
}

fn default_path_in(xdg: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    absolute(xdg)
        .or_else(|| Some(home?.join(".config")))
        .map(|d| d.join("prec/config.toml"))
}

pub fn path() -> Option<PathBuf> {
    match std::env::var_os(ENV).filter(|v| !v.is_empty()) {
        Some(p) => Some(PathBuf::from(p)),
        None => default_path(),
    }
}

fn expand_in(p: &Path, home: Option<PathBuf>) -> PathBuf {
    match p.strip_prefix("~") {
        Ok(rest) => home.map(|h| h.join(rest)).unwrap_or_else(|| p.into()),
        Err(_) => p.into(),
    }
}

pub fn parse(text: &str) -> Result<Config, String> {
    parse_in(text, home())
}

fn parse_in(text: &str, home: Option<PathBuf>) -> Result<Config, String> {
    let expand = |p: &Path| expand_in(p, home.clone());
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

    fn test_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/test-config")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_default_file_is_under_an_absolute_xdg_config_home_else_home() {
        let home = Some(PathBuf::from("/h"));
        assert_eq!(
            default_path_in(Some("/x".into()), home.clone()),
            Some(PathBuf::from("/x/prec/config.toml"))
        );
        for xdg in [None, Some("rel/config".into()), Some("".into())] {
            assert_eq!(
                default_path_in(xdg, home.clone()),
                Some(PathBuf::from("/h/.config/prec/config.toml"))
            );
        }
        assert_eq!(default_path_in(None, None), None);
        assert_eq!(default_path_in(Some("rel".into()), None), None);
        assert_eq!(absolute(Some("relative/home".into())), None);
        assert_eq!(absolute(Some("/abs".into())), Some(PathBuf::from("/abs")));
    }

    #[test]
    fn a_leading_tilde_is_the_home_and_nothing_else_is_expanded() {
        let home = || Some(PathBuf::from("/h"));
        assert_eq!(
            expand_in(Path::new("~/arch"), home()),
            PathBuf::from("/h/arch")
        );
        assert_eq!(expand_in(Path::new("~"), home()), PathBuf::from("/h"));
        for kept in ["~other/x", "/abs/~/x", "rel/~", "a~/b"] {
            assert_eq!(expand_in(Path::new(kept), home()), PathBuf::from(kept));
        }
        assert_eq!(expand_in(Path::new("~/arch"), None), PathBuf::from("~/arch"));
    }

    #[test]
    fn a_confine_table_parses_alone_or_with_its_cpus_file() {
        let c = parse("[confine]\n").unwrap();
        assert_eq!(c.confine, Some(Confine::default()));
        assert_eq!(c.archive, None);
        let c = parse("[confine]\ncpus_from = \"/etc/machine.env\"\ncpus_key = \"RENDER_CPUS\"\n")
            .unwrap()
            .confine
            .unwrap();
        assert_eq!(c.cpus_from.as_deref(), Some(Path::new("/etc/machine.env")));
        assert_eq!(c.cpus_key.as_deref(), Some("RENDER_CPUS"));
        assert_eq!(c.cpus, None);
        let both = parse("[confine]\ncpus = \"1\"\ncpus_from = \"/x\"\n").unwrap_err();
        assert_eq!(both, "[confine] takes cpus or cpus_from, not both");
    }

    #[test]
    fn a_tilde_in_a_path_key_is_expanded_and_left_alone_elsewhere() {
        let text = "archive = \"~/arch\"\nchromium = \"~/bin/chrome\"\ntheme = \"~nord\"\n[confine]\ncpus_from = \"~/m.env\"\n";
        let c = parse_in(text, Some(PathBuf::from("/home/alpha"))).unwrap();
        assert_eq!(c.archive.as_deref(), Some(Path::new("/home/alpha/arch")));
        assert_eq!(
            c.chromium.as_deref(),
            Some(Path::new("/home/alpha/bin/chrome"))
        );
        assert_eq!(
            c.confine.unwrap().cpus_from.as_deref(),
            Some(Path::new("/home/alpha/m.env"))
        );
        assert_eq!(c.theme.as_deref(), Some("~nord"));
        let c = parse_in(text, None).unwrap();
        assert_eq!(c.archive.as_deref(), Some(Path::new("~/arch")));
        assert_eq!(c.chromium.as_deref(), Some(Path::new("~/bin/chrome")));
        assert_eq!(
            c.confine.unwrap().cpus_from.as_deref(),
            Some(Path::new("~/m.env"))
        );
        let c = parse_in("archive = \"/srv/~/a\"\n", Some(PathBuf::from("/home/alpha"))).unwrap();
        assert_eq!(c.archive.as_deref(), Some(Path::new("/srv/~/a")));
    }

    #[test]
    fn an_unknown_key_or_a_wrong_type_is_refused_by_name() {
        for (text, names) in [
            ("archiv = \"/srv\"\n", "archiv"),
            ("[confine]\ncpu = \"1\"\n", "cpu"),
            ("[confin]\n", "confin"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(&format!("unknown field `{names}`")), "{text}: {e}");
        }
        assert!(parse("archive = 3\n").is_err());
        assert!(parse("confine = \"yes\"\n").is_err());
        assert!(parse("archive = \"/a\"\narchive = \"/b\"\n").is_err());
        assert!(parse("not toml").is_err());
    }

    #[test]
    fn a_config_file_is_read_and_its_errors_name_it() {
        let dir = test_dir("read");
        assert_eq!(read(None, false).unwrap(), Config::default());
        let good = dir.join("good.toml");
        std::fs::write(&good, "archive = \"/srv/a\"\n").unwrap();
        for explicit in [false, true] {
            assert_eq!(
                read(Some(&good), explicit).unwrap().archive.as_deref(),
                Some(Path::new("/srv/a"))
            );
        }
        let empty = dir.join("empty.toml");
        std::fs::write(&empty, "").unwrap();
        assert_eq!(read(Some(&empty), false).unwrap(), Config::default());
        let bad = dir.join("bad.toml");
        std::fs::write(&bad, "archiv = \"/srv\"\n").unwrap();
        let e = read(Some(&bad), false).unwrap_err();
        assert!(e.starts_with(&format!("{}: ", bad.display())), "{e}");
        assert!(e.contains("archiv"), "{e}");
        let e = read(Some(&dir), false).unwrap_err();
        assert!(e.starts_with(&format!("{}: ", dir.display())), "{e}");
        let absent = dir.join("absent.toml");
        let e = read(Some(&absent), true).unwrap_err();
        assert!(e.starts_with(&format!("{}: ", absent.display())), "{e}");
    }
}
