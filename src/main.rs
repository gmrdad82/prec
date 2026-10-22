mod app;
mod archive;
mod cache;
mod cdp;
mod compose;
mod config;
mod doctor;
mod encode;
mod font;
mod fps;
mod home;
mod jobs;
mod mask;
mod paths;
mod private;
mod record;
mod render;
mod replay;
mod tape;
mod theme;

use clap::{Args, Parser, Subcommand};
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "prec",
    version,
    arg_required_else_help = false,
    about = "Deterministic renders of an app's TUI and CLI, recorded in a pty and replayed frame by frame into exact video and PNG masters"
)]
struct Cli {
    #[command(subcommand)]
    command: Kind,
}

#[derive(Args, Clone)]
struct Source {
    #[arg(
        long,
        help = "The app's folder in this repository, holding render/terminal.toml; default the repository's top"
    )]
    app_dir: Option<PathBuf>,
    #[arg(
        long,
        help = "A tag or commit of this repository to build, render and archive"
    )]
    rev: Option<String>,
}

#[derive(Args, Clone)]
struct Shot {
    product: String,
    shot: String,
    #[command(flatten)]
    source: Source,
    #[arg(
        long,
        help = "A prebuilt folder holding the app's binary; skips the build"
    )]
    bin: Option<PathBuf>,
    #[arg(long, help = "The largest delivery: 720p, 1080p, 1440p, 2160p or WxH")]
    size: Option<String>,
    #[arg(
        long,
        value_delimiter = ',',
        help = "More still sizes from the same master; clips use pcod"
    )]
    derive: Vec<String>,
    #[arg(
        long,
        value_parser = fps::Fps::parse,
        help = "One of 24, 30, 48, 60, 120, 24000/1001 (23.976 or 23.98), 30000/1001 (29.97), 48000/1001 (47.952) or 60000/1001 (59.94); pcod makes other clip rates"
    )]
    fps: Option<fps::Fps>,
    #[arg(
        long,
        help = "Poster frame for a clip: first, before-closure, or seconds of shown time"
    )]
    poster: Option<String>,
    #[arg(long, default_value = "tmp/renders")]
    out: PathBuf,
    #[arg(long, default_value_t = 7)]
    seed: u64,
    #[arg(
        long = "set",
        help = "key=value: grid=<cols>x<rows>, typing, settle, closure, closure.crossfade, launch, ground, inset"
    )]
    set: Vec<String>,
    #[arg(
        long,
        default_value = "app",
        help = "app, a built-in scheme (see `themes`), or omarchy:<name>"
    )]
    theme: String,
    #[arg(
        long,
        value_delimiter = ',',
        help = "Render once per theme, e.g. \"Catppuccin Latte,Catppuccin Mocha\""
    )]
    themes: Vec<String>,
    #[arg(long, help = "The font family; default JetBrainsMono Nerd Font")]
    font: Option<String>,
    #[arg(long, help = "window or bare")]
    frame: Option<String>,
    #[arg(
        long,
        help = "A materialized fixture set, passed through the app's door"
    )]
    fixtures: Option<PathBuf>,
    #[arg(long, help = "Clip formats belong to pcod")]
    youtube: bool,
    #[arg(long, help = "Keep the raw frames under tmp/prec/run/")]
    keep_frames: bool,
    #[arg(long, help = "Write frames only; implies --keep-frames")]
    no_encode: bool,
    #[arg(
        long,
        value_delimiter = ',',
        help = "Still web copies at these widths, each also at @2x"
    )]
    web: Vec<u32>,
    #[arg(long, value_delimiter = ',', help = "Clip formats belong to pcod")]
    profile: Vec<String>,
    #[arg(long, help = "Skip archive copying")]
    no_archive: bool,
    #[arg(
        long,
        help = "Skip the scan that refuses a picture showing a home path, hostname, user name, email or passed variable"
    )]
    allow_private: bool,
    #[command(flatten)]
    camera: Camera,
}

#[derive(Args, Clone, Default)]
struct Camera {
    #[arg(long, hide = true)]
    turn: Option<String>,
    #[arg(long, hide = true)]
    tilt: Option<String>,
    #[arg(long, hide = true)]
    roll: Option<String>,
    #[arg(long, hide = true)]
    zoom: Option<String>,
    #[arg(long = "move", hide = true)]
    move_: Option<String>,
    #[arg(long, hide = true)]
    target: Option<String>,
    #[arg(long, hide = true)]
    lens: Option<String>,
    #[arg(long, hide = true)]
    ortho: bool,
    #[arg(long, hide = true)]
    margin: Option<String>,
    #[arg(long, hide = true)]
    key: Vec<String>,
}

impl Camera {
    fn refuse(&self) -> Result<(), String> {
        let set = self.turn.is_some()
            || self.tilt.is_some()
            || self.roll.is_some()
            || self.zoom.is_some()
            || self.move_.is_some()
            || self.target.is_some()
            || self.lens.is_some()
            || self.ortho
            || self.margin.is_some()
            || !self.key.is_empty();
        if set {
            return Err(app::NO_CAMERA.into());
        }
        Ok(())
    }
}

#[derive(Args, Clone)]
struct Replay {
    cast: PathBuf,
    #[arg(long, help = "The largest delivery: 720p, 1080p, 1440p, 2160p or WxH")]
    size: Option<String>,
    #[arg(
        long,
        value_delimiter = ',',
        help = "More still sizes from the same master; clips use pcod"
    )]
    derive: Vec<String>,
    #[arg(
        long,
        value_parser = fps::Fps::parse,
        help = "One of 24, 30, 48, 60, 120, 24000/1001 (23.976 or 23.98), 30000/1001 (29.97), 48000/1001 (47.952) or 60000/1001 (59.94); pcod makes other clip rates"
    )]
    fps: Option<fps::Fps>,
    #[arg(
        long,
        help = "Poster frame for a clip: first, before-closure, or seconds of shown time"
    )]
    poster: Option<String>,
    #[arg(long, default_value = "tmp/renders")]
    out: PathBuf,
    #[arg(long, default_value_t = 7)]
    seed: u64,
    #[arg(
        long = "set",
        help = "key=value: closure, closure.crossfade, ground, inset"
    )]
    set: Vec<String>,
    #[arg(
        long,
        default_value = "app",
        help = "app, a built-in scheme (see `themes`), or omarchy:<name>"
    )]
    theme: String,
    #[arg(long, value_delimiter = ',', help = "Render once per theme")]
    themes: Vec<String>,
    #[arg(long, help = "The font family; default the cast's")]
    font: Option<String>,
    #[arg(long, help = "window or bare")]
    frame: Option<String>,
    #[arg(long, help = "Clip formats belong to pcod")]
    youtube: bool,
    #[arg(long, help = "Keep the raw frames under tmp/prec/run/")]
    keep_frames: bool,
    #[arg(long, help = "Write frames only; implies --keep-frames")]
    no_encode: bool,
    #[arg(
        long,
        value_delimiter = ',',
        help = "Still web copies at these widths, each also at @2x"
    )]
    web: Vec<u32>,
    #[arg(long, value_delimiter = ',', help = "Clip formats belong to pcod")]
    profile: Vec<String>,
    #[arg(long, help = "Skip archive copying")]
    no_archive: bool,
    #[arg(
        long,
        help = "Skip the scan that refuses a picture showing a home path, hostname, user name or email"
    )]
    allow_private: bool,
    #[command(flatten)]
    camera: Camera,
}

#[derive(Subcommand)]
enum Kind {
    #[command(about = "List an app's terminal shots")]
    Shots {
        product: String,
        #[command(flatten)]
        source: Source,
    },
    #[command(about = "Render a clip: H.264 master, poster and manifest")]
    Clip(Shot),
    #[command(about = "Render a still: a 16-bit master and 8-bit PNG deliveries")]
    Still(Shot),
    #[command(
        about = "Record a shot's session in a pty into an asciicast v2 cast, without rendering"
    )]
    Record(Shot),
    #[command(
        about = "Render a recorded cast at any size, fps and theme, without re-running the app"
    )]
    Render(Replay),
    #[command(
        about = "Retry queued archive copies or rebuild the archive catalog",
        group = clap::ArgGroup::new("mode").required(true)
    )]
    Archive {
        #[arg(long, group = "mode")]
        retry: bool,
        #[arg(long, group = "mode")]
        reindex: bool,
    },
    #[command(about = "List the themes")]
    Themes {
        #[arg(long, help = "List every built-in scheme too")]
        all: bool,
    },
    #[command(about = "Check the machine for everything a render needs")]
    Doctor,
    #[command(
        about = "Show the machine cache (app sources, shared target dirs, cached builds), or prune it"
    )]
    Cache {
        #[arg(long, global = true, help = "Print one JSON object on stdout")]
        json: bool,
        #[command(subcommand)]
        action: Option<CacheAction>,
    },
}

#[derive(Subcommand)]
enum CacheAction {
    #[command(
        about = "Remove entries unused for a while (30 days unless --older-than); entries in use stay"
    )]
    Prune {
        #[arg(long, help = "Age like 30d, 2w, 12h or 90m")]
        older_than: Option<String>,
        #[arg(long, help = "Remove every entry that is not in use")]
        all: bool,
        #[arg(
            long,
            requires = "json",
            help = "With --json, report what would be removed and remove nothing"
        )]
        dry_run: bool,
    },
}

fn opts(s: &Shot, theme: &str, made: &str) -> render::Opts {
    render::Opts {
        made: made.to_string(),
        product: s.product.clone(),
        shot: s.shot.clone(),
        bin: s.bin.clone(),
        size: s.size.clone(),
        derive: s.derive.clone(),
        fps: s.fps,
        poster: s.poster.clone(),
        out: s.out.clone(),
        seed: s.seed,
        set: s.set.clone(),
        theme: theme.to_string(),
        font: s.font.clone(),
        frame: s.frame.clone(),
        fixtures: s.fixtures.clone(),
        youtube: s.youtube,
        keep_frames: s.keep_frames,
        no_encode: s.no_encode,
        web: s.web.clone(),
        profile: s.profile.clone(),
        no_archive: s.no_archive,
        allow_private: s.allow_private,
    }
}

fn replay_opts(r: &Replay, theme: &str, made: &str) -> render::Opts {
    render::Opts {
        made: made.to_string(),
        size: r.size.clone(),
        derive: r.derive.clone(),
        fps: r.fps,
        poster: r.poster.clone(),
        out: r.out.clone(),
        seed: r.seed,
        set: r.set.clone(),
        theme: theme.to_string(),
        font: r.font.clone(),
        frame: r.frame.clone(),
        youtube: r.youtube,
        keep_frames: r.keep_frames,
        no_encode: r.no_encode,
        web: r.web.clone(),
        profile: r.profile.clone(),
        no_archive: r.no_archive,
        allow_private: r.allow_private,
        ..Default::default()
    }
}

fn begin(verb: &str, kind: &str, s: &Shot) -> Result<(), String> {
    let out = paths::inside_tmp(&s.out)?.join(&s.product).join(kind);
    let mut detail = vec![
        verb.to_string(),
        s.shot.clone(),
        s.source
            .rev
            .as_ref()
            .map(|r| format!("@{r}"))
            .unwrap_or_else(|| "local".into()),
    ];
    if let Some(size) = &s.size {
        detail.push(size.clone());
    }
    if let Some(fps) = s.fps {
        detail.push(format!("{}fps", fps.label()));
    }
    if let Some(poster) = &s.poster {
        detail.push(format!("poster {poster}"));
    }
    if !s.themes.is_empty() {
        detail.push(s.themes.join(","));
    } else if s.theme != "app" {
        detail.push(s.theme.clone());
    }
    jobs::start(
        Some(&s.product),
        &s.shot,
        detail.join(" "),
        "CPU".to_string(),
        &out,
    );
    Ok(())
}

fn shoot(kind: &str, s: &Shot, made: &str) -> Result<(), String> {
    s.camera.refuse()?;
    let first = opts(s, &s.theme, made);
    render::validate(kind, &first)?;
    begin(kind, kind, s)?;
    let build = s.bin.is_none();
    let guess = serde_json::json!({
        "rev": s.source.rev.as_deref().unwrap_or("local"),
        "pinned": s.source.rev.is_some() && build,
    });
    jobs::stages(&render::stages(
        kind,
        build,
        true,
        true,
        render::archives(kind, &s.product, &first, &guess),
    ));
    let themes = if s.themes.is_empty() {
        vec![s.theme.clone()]
    } else {
        s.themes.clone()
    };
    let (cast, _kept) = {
        let _held = render::hold(
            &s.product,
            &s.shot,
            &format!("{kind} {} {}", s.product, s.shot),
        )?;
        render::record_cast(
            resolve(&s.product, &s.source)?,
            kind,
            &opts(s, &themes[0], made),
            false,
        )?
    };
    let app = record::read(&cast)?.meta["app"].clone();
    jobs::stages(&render::stages(
        kind,
        build,
        true,
        true,
        render::archives(kind, &s.product, &first, &app),
    ));
    for t in themes {
        let m = render::render_cast(&cast, &opts(s, &t, made))?;
        println!("{}", paths::printed(&m));
    }
    Ok(())
}

fn resolve(product: &str, source: &Source) -> Result<app::Resolved, String> {
    app::resolve(product, source.app_dir.as_deref(), source.rev.as_deref())
}

fn run(command: Kind) -> Result<(), String> {
    let made = archive::made();
    match command {
        Kind::Shots { product, source } => {
            let r = resolve(&product, &source)?;
            let t = &r.terminal;
            println!(
                "{} — grid {}x{}, frame {}, font {}",
                t.app,
                t.grid[0],
                t.grid[1],
                t.frame,
                t.font.as_deref().unwrap_or(app::DEFAULT_FONT)
            );
            for (name, s) in &t.shots {
                println!(
                    "{:<24} {:<5} {:<12} {}",
                    name,
                    s.kind,
                    s.status.as_deref().unwrap_or("-"),
                    s.line.as_deref().unwrap_or("")
                );
            }
            Ok(())
        }
        Kind::Clip(s) => shoot("clip", &s, &made),
        Kind::Still(s) => shoot("still", &s, &made),
        Kind::Record(s) => {
            if s.poster.is_some() {
                return Err("--poster is for clip and render".into());
            }
            s.camera.refuse()?;
            let resolved = resolve(&s.product, &s.source)?;
            let kind = resolved
                .terminal
                .shots
                .get(&s.shot)
                .map(|x| x.kind.clone())
                .ok_or_else(|| format!("{} has no shot {}", s.product, s.shot))?;
            render::validate(&kind, &opts(&s, &s.theme, &made))?;
            begin("record", &kind, &s)?;
            jobs::stages(&render::stages(&kind, s.bin.is_none(), true, false, false));
            let _held = render::hold(
                &s.product,
                &s.shot,
                &format!("record {} {}", s.product, s.shot),
            )?;
            let (p, _) = render::record_cast(resolved, &kind, &opts(&s, &s.theme, &made), true)?;
            println!("{}", paths::printed(&p));
            Ok(())
        }
        Kind::Render(r) => {
            r.camera.refuse()?;
            let meta = record::read(&r.cast)?.meta;
            let product = meta["product"].as_str().unwrap_or_default();
            let kind = meta["kind"].as_str().unwrap_or("clip");
            render::validate(kind, &replay_opts(&r, &r.theme, &made))?;
            let out = paths::inside_tmp(&r.out)?.join(product).join(kind);
            jobs::start(
                Some(product).filter(|p| !p.is_empty()),
                meta["shot"].as_str().unwrap_or("shot"),
                format!(
                    "render {}",
                    r.cast.file_name().unwrap_or_default().to_string_lossy()
                ),
                "CPU".to_string(),
                &out,
            );
            jobs::stages(&render::stages(
                kind,
                false,
                false,
                true,
                render::archives(
                    kind,
                    product,
                    &replay_opts(&r, &r.theme, &made),
                    &meta["app"],
                ),
            ));
            let themes = if r.themes.is_empty() {
                vec![r.theme.clone()]
            } else {
                r.themes.clone()
            };
            for t in themes {
                let m = render::render_cast(&r.cast, &replay_opts(&r, &t, &made))?;
                println!("{}", paths::printed(&m));
            }
            Ok(())
        }
        Kind::Archive { retry, reindex } => {
            if retry {
                archive::retry();
            } else if reindex {
                println!("archive: reindexed {} lines", archive::reindex()?);
            }
            Ok(())
        }
        Kind::Themes { all } => {
            println!(
                "app                          the palette in the app's render/terminal.toml, else {:?}",
                config::theme()
            );
            println!("\ncurated pairs (light, dark):");
            for (n, l, d) in theme::PAIRS {
                println!("  {n:<14} {l:?}, {d:?}");
            }
            println!(
                "  also: {}",
                theme::SINGLES
                    .iter()
                    .map(|s| format!("{s:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let omarchy = theme::omarchy();
            if !omarchy.is_empty() {
                println!("\nomarchy (installed here):");
                for (n, t) in omarchy {
                    println!("  omarchy:{n:<24} {}", t["mode"].as_str().unwrap_or("?"));
                }
            }
            let b = theme::builtins();
            if all {
                println!("\nbuilt-in schemes (vendored from VHS):");
                for n in &b {
                    println!("  {n}");
                }
            } else {
                println!(
                    "\n{} built-in schemes (vendored from VHS); --all lists them",
                    b.len()
                );
            }
            Ok(())
        }
        Kind::Cache { json, action } => match action {
            None if json => cache::show_json(),
            None => cache::show(),
            Some(CacheAction::Prune {
                older_than,
                all,
                dry_run,
            }) => cache::prune(older_than.as_deref(), all, json, dry_run),
        },
        Kind::Doctor => {
            if doctor::doctor() {
                Ok(())
            } else {
                Err("the machine is missing something above".into())
            }
        }
    }
}

fn cancel(signal: i32) -> ! {
    let name = match signal {
        SIGTERM => "SIGTERM",
        SIGINT => "SIGINT",
        _ => "SIGHUP",
    };
    let _ = writeln!(std::io::stderr(), "prec: {name}: stopping");
    render::tear_down(paths::stop_children);
    jobs::cancelled(128 + signal, name);
    let _ = writeln!(std::io::stderr(), "prec: cancelled by {name}");
    std::process::exit(128 + signal);
}

fn watch_signals() -> Result<(), String> {
    let cancels = [SIGTERM, SIGINT, SIGHUP];
    for s in cancels {
        signal_hook::flag::register_usize(s, paths::cancel_flag(), s as usize)
            .map_err(|e| format!("signal {s}: {e}"))?;
    }
    let mut signals = Signals::new(cancels).map_err(|e| format!("signals: {e}"))?;
    std::thread::spawn(move || {
        if let Some(s) = signals.forever().next() {
            cancel(s);
        }
    });
    Ok(())
}

fn main() {
    let command = Cli::parse().command;
    if let Err(e) = config::load() {
        eprintln!("prec: {e}");
        std::process::exit(1);
    }
    let renders = matches!(&command, Kind::Clip(_) | Kind::Still(_) | Kind::Render(_));
    let result = watch_signals().and_then(|()| run(command));
    paths::settle();
    jobs::end(&result);
    if renders {
        archive::retry_at_start();
    }
    if let Err(e) = result {
        eprintln!("prec: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use clap::error::ErrorKind;

    fn parsed(args: &[&str]) -> Kind {
        Cli::try_parse_from(args)
            .unwrap_or_else(|e| panic!("{args:?}: {e}"))
            .command
    }

    fn shot(args: &[&str]) -> Shot {
        match parsed(args) {
            Kind::Clip(s) | Kind::Still(s) | Kind::Record(s) => s,
            _ => panic!("{args:?} is no shot"),
        }
    }

    fn refusal(args: &[&str]) -> clap::Error {
        Cli::try_parse_from(args).err().unwrap()
    }

    #[test]
    fn a_clip_takes_its_flags_and_keeps_its_defaults() {
        let s = shot(&["prec", "clip", "alpha", "intro"]);
        assert_eq!((s.product.as_str(), s.shot.as_str()), ("alpha", "intro"));
        assert_eq!(s.out, PathBuf::from("tmp/renders"));
        assert_eq!(s.seed, 7);
        assert_eq!(s.theme, "app");
        assert!(s.themes.is_empty() && s.derive.is_empty() && s.web.is_empty());
        assert!(s.fps.is_none() && s.size.is_none() && s.bin.is_none());
        assert!(s.source.app_dir.is_none() && s.source.rev.is_none());
        assert!(!s.no_archive && !s.allow_private && !s.keep_frames && !s.no_encode);
        assert!(s.camera.refuse().is_ok());
        let s = shot(&[
            "prec",
            "still",
            "alpha",
            "intro",
            "--fps",
            "30000/1001",
            "--themes",
            "Catppuccin Latte,Catppuccin Mocha",
            "--derive",
            "720p,1080p",
            "--web",
            "640,1280",
            "--set",
            "grid=80x24",
            "--set",
            "typing=10ms",
            "--bin",
            "tmp/bin",
            "--app-dir",
            "examples/demo",
            "--rev",
            "v1.0.0",
            "--seed",
            "9",
            "--no-archive",
        ]);
        assert_eq!(s.fps, Some(fps::Fps::new(30000, 1001)));
        assert_eq!(s.themes, ["Catppuccin Latte", "Catppuccin Mocha"]);
        assert_eq!(s.derive, ["720p", "1080p"]);
        assert_eq!(s.web, [640, 1280]);
        assert_eq!(s.set, ["grid=80x24", "typing=10ms"]);
        assert_eq!(s.bin, Some(PathBuf::from("tmp/bin")));
        assert_eq!(s.source.app_dir, Some(PathBuf::from("examples/demo")));
        assert_eq!(s.source.rev.as_deref(), Some("v1.0.0"));
        assert_eq!(s.seed, 9);
        assert!(s.no_archive);
    }

    #[test]
    fn every_hidden_camera_flag_parses_and_is_refused() {
        for flag in [
            &["--turn", "10"][..],
            &["--tilt", "5"],
            &["--roll", "1"],
            &["--zoom", "2"],
            &["--move", "1,0"],
            &["--target", "0,0"],
            &["--lens", "35"],
            &["--ortho"],
            &["--margin", "4"],
            &["--key", "0:zoom=1"],
        ] {
            let mut args = vec!["prec", "clip", "alpha", "intro"];
            args.extend_from_slice(flag);
            assert_eq!(
                shot(&args).camera.refuse().unwrap_err(),
                app::NO_CAMERA,
                "{flag:?}"
            );
            let mut cast = vec!["prec", "render", "a.cast"];
            cast.extend_from_slice(flag);
            let Kind::Render(r) = parsed(&cast) else {
                panic!("{cast:?}")
            };
            assert_eq!(r.camera.refuse().unwrap_err(), app::NO_CAMERA, "{flag:?}");
        }
        let mut cli = Cli::command();
        cli.build();
        for name in ["clip", "still", "record", "render"] {
            let help = cli
                .find_subcommand_mut(name)
                .unwrap()
                .render_help()
                .to_string();
            assert!(help.contains("--theme"), "{help}");
            assert!(
                !help.contains("--turn") && !help.contains("--key"),
                "{help}"
            );
        }
    }

    #[test]
    fn a_shots_options_reach_the_render_and_a_replay_has_no_product() {
        let s = shot(&[
            "prec",
            "clip",
            "alpha",
            "intro",
            "--size",
            "1080p",
            "--poster",
            "first",
            "--frame",
            "bare",
            "--font",
            "Alpha Mono",
            "--fixtures",
            "tmp/set",
            "--profile",
            "web",
            "--youtube",
            "--keep-frames",
            "--no-encode",
            "--allow-private",
        ]);
        let o = opts(&s, "nord", "2026-01-02T03:04:05+00:00");
        assert_eq!(o.made, "2026-01-02T03:04:05+00:00");
        assert_eq!((o.product.as_str(), o.shot.as_str()), ("alpha", "intro"));
        assert_eq!(o.theme, "nord");
        assert_eq!(o.size.as_deref(), Some("1080p"));
        assert_eq!(o.poster.as_deref(), Some("first"));
        assert_eq!(o.frame.as_deref(), Some("bare"));
        assert_eq!(o.font.as_deref(), Some("Alpha Mono"));
        assert_eq!(o.fixtures, Some(PathBuf::from("tmp/set")));
        assert_eq!(o.profile, ["web"]);
        assert!(o.youtube && o.keep_frames && o.no_encode && o.allow_private);
        assert_eq!(o.seed, 7);
        let Kind::Render(r) = parsed(&["prec", "render", "a.cast", "--seed", "3", "--no-archive"])
        else {
            panic!("render")
        };
        assert_eq!(r.cast, PathBuf::from("a.cast"));
        let o = replay_opts(&r, "Dracula", "made");
        assert_eq!(o.product, "");
        assert_eq!(o.shot, "");
        assert_eq!(o.bin, None);
        assert_eq!(o.fixtures, None);
        assert_eq!(o.theme, "Dracula");
        assert_eq!(o.seed, 3);
        assert!(o.no_archive);
    }

    #[test]
    fn the_other_commands_parse_their_flags() {
        let Kind::Shots { product, source } =
            parsed(&["prec", "shots", "alpha", "--app-dir", "app"])
        else {
            panic!("shots")
        };
        assert_eq!(product, "alpha");
        assert_eq!(source.app_dir, Some(PathBuf::from("app")));
        assert!(matches!(
            parsed(&["prec", "archive", "--retry"]),
            Kind::Archive {
                retry: true,
                reindex: false
            }
        ));
        assert!(matches!(
            parsed(&["prec", "archive", "--reindex"]),
            Kind::Archive {
                retry: false,
                reindex: true
            }
        ));
        assert!(matches!(
            parsed(&["prec", "themes"]),
            Kind::Themes { all: false }
        ));
        assert!(matches!(
            parsed(&["prec", "themes", "--all"]),
            Kind::Themes { all: true }
        ));
        assert!(matches!(parsed(&["prec", "doctor"]), Kind::Doctor));
        assert!(matches!(
            parsed(&["prec", "cache"]),
            Kind::Cache {
                json: false,
                action: None
            }
        ));
        assert_eq!(
            refusal(&["prec", "cache", "--json", "prune", "--dry-run"]).kind(),
            ErrorKind::MissingRequiredArgument
        );
        for args in [
            &["prec", "cache", "prune", "--json", "--dry-run"][..],
            &["prec", "cache", "prune", "--dry-run", "--json"],
        ] {
            let Kind::Cache {
                json: true,
                action:
                    Some(CacheAction::Prune {
                        older_than: None,
                        all: false,
                        dry_run: true,
                    }),
            } = parsed(args)
            else {
                panic!("{args:?}")
            };
        }
        let Kind::Cache {
            action: Some(CacheAction::Prune {
                older_than, all, ..
            }),
            ..
        } = parsed(&["prec", "cache", "prune", "--older-than", "2w", "--all"])
        else {
            panic!("prune")
        };
        assert_eq!(older_than.as_deref(), Some("2w"));
        assert!(all);
    }

    #[test]
    fn a_refused_value_or_a_missing_one_is_clap_error_with_exit_2() {
        let e = refusal(&["prec", "clip", "alpha", "intro", "--fps", "25"]);
        assert_eq!((e.kind(), e.exit_code()), (ErrorKind::ValueValidation, 2));
        assert!(
            e.render()
                .to_string()
                .starts_with("error: invalid value '25' for '--fps <FPS>': 24, 30, 48, 60, 120"),
            "{e}"
        );
        let e = refusal(&["prec", "still", "alpha", "intro", "--web", "wide"]);
        assert_eq!((e.kind(), e.exit_code()), (ErrorKind::ValueValidation, 2));
        let e = refusal(&["prec", "clip", "alpha"]);
        assert_eq!(
            (e.kind(), e.exit_code()),
            (ErrorKind::MissingRequiredArgument, 2)
        );
        let e = refusal(&["prec", "cache", "prune", "--dry-run"]);
        assert_eq!(
            (e.kind(), e.exit_code()),
            (ErrorKind::MissingRequiredArgument, 2)
        );
        let e = refusal(&["prec", "cache", "prune", "--older-than"]);
        assert_eq!(e.exit_code(), 2);
        let said = e.render().to_string();
        assert_eq!(
            said,
            "error: a value is required for '--older-than <OLDER_THAN>' but none was supplied\n\nFor more information, try '--help'.\n"
        );
    }

    #[test]
    fn help_and_version_exit_0_with_the_crates_version() {
        for flag in ["--help", "-h"] {
            let e = refusal(&["prec", flag]);
            assert_eq!((e.kind(), e.exit_code()), (ErrorKind::DisplayHelp, 0));
            let said = e.render().to_string();
            assert!(said.contains("Usage: prec <COMMAND>"), "{said}");
            for c in Cli::command().get_subcommands() {
                assert!(said.contains(&format!("\n  {}", c.get_name())), "{said}");
            }
        }
        for flag in ["--version", "-V"] {
            let e = refusal(&["prec", flag]);
            assert_eq!((e.kind(), e.exit_code()), (ErrorKind::DisplayVersion, 0));
            assert_eq!(
                e.render().to_string(),
                format!("prec {}\n", env!("CARGO_PKG_VERSION"))
            );
        }
    }

    #[test]
    fn unparsed_input_gets_the_short_answer() {
        for (args, kind) in [
            (&["prec"][..], ErrorKind::MissingSubcommand),
            (&["prec", "tui"][..], ErrorKind::InvalidSubcommand),
            (&["prec", "--x"][..], ErrorKind::UnknownArgument),
            (&["prec", "archive"][..], ErrorKind::MissingRequiredArgument),
            (
                &["prec", "archive", "--retry", "--reindex"][..],
                ErrorKind::ArgumentConflict,
            ),
        ] {
            let e = Cli::try_parse_from(args).err().unwrap();
            assert_eq!((e.kind(), e.exit_code()), (kind, 2), "{args:?}");
            let said = e.render().to_string();
            assert!(said.starts_with("error: "), "{said}");
            assert!(
                said.ends_with("\n\nFor more information, try '--help'.\n"),
                "{said}"
            );
            assert!(!said.contains("Commands:"), "{said}");
        }
    }
}
