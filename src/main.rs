mod app;
mod archive;
mod cache;
mod cdp;
mod compose;
mod config;
mod doctor;
mod encode;
mod font;
mod jobs;
mod paths;
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
        help = "More still sizes from the same master; clips use pconv"
    )]
    derive: Vec<String>,
    #[arg(long, help = "One of 30, 60 or 120; pconv makes other clip rates")]
    fps: Option<u32>,
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
        help = "key=value: grid=<cols>x<rows>, typing, settle, closure, closure.crossfade, launch, ground"
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
    #[arg(long, help = "Clip formats belong to pconv")]
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
    #[arg(long, value_delimiter = ',', help = "Clip formats belong to pconv")]
    profile: Vec<String>,
    #[arg(long, help = "Skip archive copying")]
    no_archive: bool,
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
        help = "More still sizes from the same master; clips use pconv"
    )]
    derive: Vec<String>,
    #[arg(long, help = "One of 30, 60 or 120; pconv makes other clip rates")]
    fps: Option<u32>,
    #[arg(
        long,
        help = "Poster frame for a clip: first, before-closure, or seconds of shown time"
    )]
    poster: Option<String>,
    #[arg(long, default_value = "tmp/renders")]
    out: PathBuf,
    #[arg(long, default_value_t = 7)]
    seed: u64,
    #[arg(long = "set", help = "key=value: closure, closure.crossfade, ground")]
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
    #[arg(long, help = "Clip formats belong to pconv")]
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
    #[arg(long, value_delimiter = ',', help = "Clip formats belong to pconv")]
    profile: Vec<String>,
    #[arg(long, help = "Skip archive copying")]
    no_archive: bool,
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
    #[command(about = "Render a clip: x264 master, poster and manifest")]
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
        detail.push(format!("{fps}fps"));
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
    begin(kind, kind, s)?;
    let first = opts(s, &s.theme, made);
    render::validate(kind, &first)?;
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
    use super::Cli;
    use clap::Parser;
    use clap::error::ErrorKind;

    #[test]
    fn unparsed_input_gets_the_short_answer() {
        for (args, kind) in [
            (&["prec"][..], ErrorKind::MissingSubcommand),
            (&["prec", "tui"][..], ErrorKind::InvalidSubcommand),
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
