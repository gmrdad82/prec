mod common;

use common::{add_shot, demo_app, scratch, tree};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GROUP: &str = "cli";

const FOOTER: &str = "For more information, try '--help'.\n";

fn setup(name: &str) -> PathBuf {
    let dir = scratch(GROUP, name);
    std::fs::write(dir.join("config.toml"), "").unwrap();
    dir
}

fn prec(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_prec"));
    c.args(args)
        .current_dir(tree())
        .env("PREC_CONFIG", dir.join("config.toml"))
        .env("PITO_ARCHIVE", dir.join("archive"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("PITO_JOBS", "off")
        .env_remove("PREC_CACHE")
        .env_remove("PREC_CHROMIUM")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE");
    c
}

fn run(c: &mut Command) -> Output {
    c.output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn one_line_refusal(o: &Output) -> String {
    let said = err(o);
    assert_eq!(o.status.code(), Some(1), "{said}");
    assert!(o.stdout.is_empty(), "{}", out(o));
    assert!(said.starts_with("prec: "), "{said:?}");
    assert!(said.ends_with('\n'), "{said:?}");
    assert_eq!(said.lines().count(), 1, "a refusal is one line: {said:?}");
    said.trim_end().to_string()
}

fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn short(head: &str, usage: Option<&str>) -> String {
    match usage {
        Some(u) => format!("{head}\n\n{u}\n\n{FOOTER}"),
        None => format!("{head}\n\n{FOOTER}"),
    }
}

fn unparsed() -> Vec<(Vec<&'static str>, String)> {
    vec![
        (
            vec![],
            short(
                "error: 'prec' requires a subcommand but one was not provided\n  [subcommands: shots, clip, still, record, render, archive, themes, doctor, cache, help]",
                Some("Usage: prec <COMMAND>"),
            ),
        ),
        (
            vec!["tui"],
            short(
                "error: unrecognized subcommand 'tui'",
                Some("Usage: prec <COMMAND>"),
            ),
        ),
        (
            vec!["--x"],
            short(
                "error: unexpected argument '--x' found",
                Some("Usage: prec <COMMAND>"),
            ),
        ),
        (
            vec!["cache", "prune", "--older-than"],
            short(
                "error: a value is required for '--older-than <OLDER_THAN>' but none was supplied",
                None,
            ),
        ),
        (
            vec!["archive"],
            short(
                "error: the following required arguments were not provided:\n  <--retry|--reindex>",
                Some("Usage: prec archive <--retry|--reindex>"),
            ),
        ),
    ]
}

#[test]
fn unparsed_input_gets_clap_short_error_on_stderr_with_exit_2_plain_when_piped() {
    let dir = setup("unparsed");
    for (args, expected) in unparsed() {
        for no_color in [false, true] {
            let mut c = prec(&dir, &args);
            if no_color {
                c.env("NO_COLOR", "1");
            }
            let o = run(&mut c);
            assert_eq!(o.status.code(), Some(2), "{args:?}");
            assert!(o.stdout.is_empty(), "{args:?}: {}", out(&o));
            assert!(!o.stderr.contains(&0x1b), "{args:?}: {:?}", err(&o));
            assert_eq!(err(&o), expected, "{args:?}");
            assert!(!err(&o).contains("Commands:"), "{args:?}");
        }
    }
}

#[test]
fn bad_flags_on_a_subcommand_are_refused_before_anything_runs() {
    let dir = setup("bad-flags");
    for (args, head) in [
        (
            &["clip", "alpha", "intro", "--fps", "25"][..],
            "error: invalid value '25' for '--fps <FPS>': ",
        ),
        (
            &["still", "alpha", "intro", "--web", "wide"],
            "error: invalid value 'wide' for '--web <WEB>': ",
        ),
        (
            &["archive", "--retry", "--reindex"],
            "error: the argument '--retry' cannot be used with '--reindex'",
        ),
        (
            &["cache", "prune", "--dry-run"],
            "error: the following required arguments were not provided:",
        ),
        (&["themes", "--x"], "error: unexpected argument '--x' found"),
    ] {
        let o = run(&mut prec(&dir, args));
        let said = err(&o);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {said}");
        assert!(said.starts_with(head), "{args:?}: {said}");
        assert!(said.ends_with(&format!("\n\n{FOOTER}")), "{args:?}: {said}");
        assert!(o.stdout.is_empty(), "{args:?}");
    }
    assert!(!dir.join("cache").exists());
    assert!(!dir.join("archive").exists());
}

#[cfg(unix)]
fn in_terminal(dir: &Path, args: &[&str], no_color: bool) -> (String, u32) {
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::io::Read;
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 160,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_prec"));
    cmd.args(args);
    cmd.env_clear();
    cmd.env("PATH", std::env::var_os("PATH").unwrap_or_default());
    cmd.env("TERM", "xterm-256color");
    cmd.env("PREC_CONFIG", dir.join("config.toml"));
    cmd.env("XDG_CACHE_HOME", dir.join("cache"));
    cmd.env("PITO_ARCHIVE", dir.join("archive"));
    cmd.env("PITO_JOBS", "off");
    if no_color {
        cmd.env("NO_COLOR", "1");
    }
    cmd.cwd(tree());
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut said = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => said.extend_from_slice(&buf[..n]),
        }
    }
    let status = child.wait().unwrap();
    (
        String::from_utf8_lossy(&said).replace("\r\n", "\n"),
        status.exit_code(),
    )
}

#[cfg(unix)]
#[test]
fn in_a_terminal_the_error_is_coloured_as_clap_colours_it_and_plain_with_no_color() {
    let dir = setup("terminal");
    let (said, code) = in_terminal(&dir, &["tui"], false);
    assert_eq!(code, 2, "{said:?}");
    assert!(
        said.contains("\x1b[1m\x1b[31merror:\x1b[0m unrecognized subcommand '\x1b[33mtui\x1b[0m'"),
        "{said:?}"
    );
    assert!(
        said.contains("\x1b[1m\x1b[4mUsage:\x1b[0m \x1b[1mprec\x1b[0m <COMMAND>"),
        "{said:?}"
    );
    assert!(said.contains("try '\x1b[1m--help\x1b[0m'."), "{said:?}");
    let cases = unparsed();
    let expected = &cases[1].1;
    assert_eq!(&plain(&said), expected);
    let (bare, code) = in_terminal(&dir, &["tui"], true);
    assert_eq!(code, 2, "{bare:?}");
    assert!(!bare.contains('\x1b'), "{bare:?}");
    assert_eq!(&bare, expected);
}

#[test]
fn help_prints_the_whole_help_on_stdout_with_exit_0() {
    let dir = setup("help");
    let long = run(&mut prec(&dir, &["--help"]));
    assert_eq!(long.status.code(), Some(0));
    assert!(long.stderr.is_empty(), "{}", err(&long));
    let help = out(&long);
    assert!(help.contains("\nUsage: prec <COMMAND>\n"), "{help}");
    for c in [
        "shots", "clip", "still", "record", "render", "archive", "themes", "doctor", "cache",
        "help",
    ] {
        assert!(help.contains(&format!("\n  {c} ")), "{c}: {help}");
    }
    assert!(help.contains("-V, --version"), "{help}");
    for args in [&["-h"][..], &["help"]] {
        let o = run(&mut prec(&dir, args));
        assert_eq!(o.status.code(), Some(0), "{args:?}");
        assert!(o.stderr.is_empty(), "{args:?}");
        assert_eq!(out(&o), help, "{args:?}");
    }
    let sub = run(&mut prec(&dir, &["help", "themes"]));
    assert_eq!(sub.status.code(), Some(0));
    assert!(
        out(&sub).contains("Usage: prec themes [OPTIONS]"),
        "{}",
        out(&sub)
    );
    assert!(out(&sub).contains("--all"), "{}", out(&sub));
}

#[test]
fn version_prints_the_version_in_cargo_toml() {
    let dir = setup("version");
    let manifest = std::fs::read_to_string(tree().join("Cargo.toml")).unwrap();
    let table: toml::Table = toml::from_str(&manifest).unwrap();
    let version = table["package"]["version"].as_str().unwrap().to_string();
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    for flag in ["--version", "-V"] {
        let o = run(&mut prec(&dir, &[flag]));
        assert_eq!(o.status.code(), Some(0), "{flag}");
        assert!(o.stderr.is_empty(), "{flag}");
        assert_eq!(out(&o), format!("prec {version}\n"), "{flag}");
    }
}

#[test]
fn a_refusal_after_a_good_parse_is_one_line_with_exit_1() {
    let dir = setup("refusals");
    let refused = |args: &[&str]| one_line_refusal(&run(&mut prec(&dir, args)));
    assert_eq!(
        refused(&["record", "alpha", "intro", "--poster", "first"]),
        "prec: --poster is for clip and render"
    );
    for args in [
        &["still", "alpha", "intro", "--zoom", "2"][..],
        &["clip", "alpha", "intro", "--ortho"],
        &["render", "none.cast", "--turn", "10"],
    ] {
        assert!(
            refused(args).starts_with("prec: prec has no camera: "),
            "{args:?}"
        );
    }
    let missing = dir.join("missing");
    assert_eq!(
        refused(&["shots", "alpha", "--app-dir", missing.to_str().unwrap()]),
        format!("prec: {}: no such folder", missing.display())
    );
    let cast = dir.join("none.cast");
    assert!(
        refused(&["render", cast.to_str().unwrap()])
            .starts_with(&format!("prec: {}: ", cast.display()))
    );
    assert!(!dir.join("archive").exists());
}

#[test]
fn a_config_that_is_named_but_missing_or_wrong_is_refused_before_any_command() {
    let dir = setup("config");
    let absent = dir.join("absent.toml");
    let said = one_line_refusal(&run(prec(&dir, &["themes"]).env("PREC_CONFIG", &absent)));
    assert!(
        said.starts_with(&format!("prec: {}: ", absent.display())),
        "{said}"
    );
    std::fs::write(dir.join("config.toml"), "archiv = \"/srv\"\n").unwrap();
    let said = one_line_refusal(&run(&mut prec(&dir, &["doctor"])));
    assert!(
        said.starts_with(&format!(
            "prec: {}: unknown field `archiv`",
            dir.join("config.toml").display()
        )),
        "{said}"
    );
}

fn shot_lines(stdout: &str) -> Vec<Vec<String>> {
    stdout
        .lines()
        .skip(1)
        .map(|l| l.split_whitespace().take(3).map(String::from).collect())
        .collect()
}

#[test]
fn shots_lists_every_shot_of_an_app_in_name_order() {
    let dir = setup("shots");
    let app = demo_app(&dir, "");
    add_shot(&app, "bare", "Type \"x\"\n", "");
    let o = run(&mut prec(
        &dir,
        &["shots", "demo", "--app-dir", app.to_str().unwrap()],
    ));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let listed = out(&o);
    assert_eq!(
        listed.lines().next().unwrap(),
        "demo — grid 60x16, frame window, font JetBrainsMono Nerd Font"
    );
    assert_eq!(
        shot_lines(&listed),
        [
            ["bare", "clip", "-"],
            ["palette", "still", "SHIPPED"],
            ["tour", "clip", "SHIPPED"],
            ["typing", "clip", "SHIPPED"],
        ]
    );
    assert!(
        listed.lines().any(|l| l.starts_with("palette ")
            && l.ends_with(" Truecolor gradient, box drawing and the CJK 三")),
        "{listed}"
    );
    assert!(!dir.join("archive").exists());
}

fn alpha(dir: &Path, name: &str, text: Option<&str>) -> PathBuf {
    let app = dir.join(name);
    std::fs::create_dir_all(app.join("render")).unwrap();
    if let Some(t) = text {
        std::fs::write(app.join("render/terminal.toml"), t).unwrap();
    }
    app
}

const HEAD: &str = "app = \"alpha\"\nbin = \"alpha\"\ngrid = [100, 30]\n";

#[test]
fn shots_reads_the_apps_own_font_frame_and_grid() {
    let dir = setup("shots-own");
    let app = alpha(
        &dir,
        "app",
        Some(&format!(
            "{HEAD}frame = \"bare\"\nfont = \"Alpha Mono\"\n[shots.intro]\nkind = \"still\"\ntape = \"intro.tape\"\nline = \"The first look\"\nstatus = \"DRAFT\"\n"
        )),
    );
    let o = run(&mut prec(
        &dir,
        &["shots", "alpha", "--app-dir", app.to_str().unwrap()],
    ));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let listed = out(&o);
    let lines: Vec<&str> = listed.lines().collect();
    assert_eq!(lines[0], "alpha — grid 100x30, frame bare, font Alpha Mono");
    assert_eq!(lines.len(), 2, "{listed}");
    assert_eq!(
        lines[1].split_whitespace().collect::<Vec<_>>(),
        ["intro", "still", "DRAFT", "The", "first", "look"]
    );
    let empty = alpha(&dir, "empty", Some(HEAD));
    let o = run(&mut prec(
        &dir,
        &["shots", "alpha", "--app-dir", empty.to_str().unwrap()],
    ));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o).lines().count(), 1, "{}", out(&o));
}

#[test]
fn shots_refuses_a_missing_or_bad_terminal_toml_on_one_line() {
    let dir = setup("shots-bad");
    let cases: [(&str, Option<String>, &str, &str); 8] = [
        ("missing", None, "alpha", "render/terminal.toml: "),
        (
            "syntax",
            Some("app = \n".into()),
            "alpha",
            ": line 1, column ",
        ),
        (
            "unknown",
            Some(format!("{HEAD}colour = \"red\"\n")),
            "alpha",
            ": line 4, column 1: unknown field `colour`",
        ),
        (
            "untaped",
            Some(format!("{HEAD}[shots.intro]\nkind = \"clip\"\n")),
            "alpha",
            "missing field `tape`",
        ),
        (
            "twice",
            Some(format!(
                "{HEAD}[shots.intro]\nkind = \"clip\"\ntape = \"a\"\n[shots.intro]\nkind = \"still\"\ntape = \"b\"\n"
            )),
            "alpha",
            ": line 7, column 8: duplicate key",
        ),
        (
            "kind",
            Some(format!(
                "{HEAD}[shots.intro]\nkind = \"gif\"\ntape = \"a\"\n"
            )),
            "alpha",
            ": shot intro kind is clip or still",
        ),
        (
            "camera",
            Some(format!("{HEAD}camera = {{ zoom = 2 }}\n")),
            "alpha",
            ": prec has no camera: ",
        ),
        (
            "other",
            Some(HEAD.to_string()),
            "beta",
            " describes alpha, not beta",
        ),
    ];
    for (case, text, product, said) in cases {
        let app = alpha(&dir, case, text.as_deref());
        let toml = app.join("render/terminal.toml");
        let o = run(&mut prec(
            &dir,
            &["shots", product, "--app-dir", app.to_str().unwrap()],
        ));
        let refusal = one_line_refusal(&o);
        assert!(
            refusal.starts_with(&format!("prec: {}", toml.display())),
            "{case}: {refusal}"
        );
        assert!(refusal.contains(said), "{case}: {refusal}");
    }
}

fn vendored() -> Vec<Value> {
    serde_json::from_str(&std::fs::read_to_string(tree().join("assets/vhs-themes.json")).unwrap())
        .unwrap()
}

fn background(themes: &[Value], name: &str) -> f64 {
    let t = themes
        .iter()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("no vendored theme {name}"));
    let hex = t["background"].as_str().unwrap().trim_start_matches('#');
    (0..3)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap() as f64)
        .sum()
}

fn quoted(s: &str) -> Vec<String> {
    s.split('"').skip(1).step_by(2).map(String::from).collect()
}

#[test]
fn themes_lists_the_app_line_the_curated_pairs_and_the_builtin_count() {
    let dir = setup("themes");
    let themes = vendored();
    let o = run(prec(&dir, &["themes"]).env("HOME", &dir));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(o.stderr.is_empty());
    let listed = out(&o);
    let lines: Vec<&str> = listed.lines().collect();
    assert!(lines[0].starts_with("app "), "{listed}");
    assert!(
        lines[0]
            .ends_with("the palette in the app's render/terminal.toml, else \"Catppuccin Mocha\""),
        "{listed}"
    );
    let start = lines
        .iter()
        .position(|l| *l == "curated pairs (light, dark):")
        .unwrap();
    let pairs: Vec<&str> = lines[start + 1..]
        .iter()
        .take_while(|l| !l.trim_start().starts_with("also: "))
        .copied()
        .collect();
    assert!(pairs.len() >= 5, "{listed}");
    for line in &pairs {
        let names = quoted(line);
        assert_eq!(names.len(), 2, "{line}");
        assert!(
            background(&themes, &names[0]) > background(&themes, &names[1]),
            "the light one comes first: {line}"
        );
    }
    let also = lines[start + 1 + pairs.len()];
    let singles = quoted(also);
    assert!(!singles.is_empty(), "{also}");
    for s in singles {
        background(&themes, &s);
    }
    assert_eq!(
        *lines.last().unwrap(),
        format!(
            "{} built-in schemes (vendored from VHS); --all lists them",
            themes.len()
        )
    );
    assert!(!listed.contains("built-in schemes (vendored from VHS):"));
}

#[test]
fn themes_all_lists_every_vendored_scheme_in_order_and_names_the_configured_default() {
    let dir = setup("themes-all");
    std::fs::write(dir.join("config.toml"), "theme = \"nord\"\n").unwrap();
    let o = run(prec(&dir, &["themes", "--all"]).env("HOME", &dir));
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let listed = out(&o);
    assert!(
        listed.lines().next().unwrap().ends_with("else \"nord\""),
        "{listed}"
    );
    let (_, all) = listed
        .split_once("\nbuilt-in schemes (vendored from VHS):\n")
        .unwrap();
    let names: Vec<&str> = all.lines().map(|l| l.strip_prefix("  ").unwrap()).collect();
    let vendored: Vec<String> = vendored()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vendored);
    assert!(!listed.contains("--all lists them"));
}

fn report(stdout: &str) -> Vec<(String, String, String)> {
    stdout
        .lines()
        .map(|l| {
            assert!(l.len() > 34, "{l:?}");
            (
                l[..4].trim().to_string(),
                l[5..33].trim().to_string(),
                l[34..].to_string(),
            )
        })
        .collect()
}

fn mark(lines: &[(String, String, String)], what: &str) -> (String, String) {
    let (m, _, d) = lines
        .iter()
        .find(|(_, w, _)| w == what)
        .unwrap_or_else(|| panic!("no {what} line: {lines:?}"));
    (m.clone(), d.clone())
}

fn doctor(dir: &Path, path: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_prec"));
    c.arg("doctor")
        .current_dir(tree())
        .env_clear()
        .env("PATH", path)
        .env("HOME", dir)
        .env("PREC_CONFIG", dir.join("config.toml"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("PITO_JOBS", "off");
    c
}

#[test]
fn doctor_marks_every_missing_tool_and_exits_1() {
    let dir = setup("doctor-missing");
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let o = run(&mut doctor(&dir, &empty));
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert_eq!(err(&o), "prec: the machine is missing something above\n");
    let lines = report(&out(&o));
    assert_eq!(
        mark(&lines, "config"),
        ("ok".into(), dir.join("config.toml").display().to_string())
    );
    for (what, said) in [
        ("fc-match", "not on PATH"),
        ("git", "not on PATH"),
        ("chromium", "not found; a render needs a headless Chromium"),
        ("CJK fallback", "no Japanese-capable font"),
    ] {
        assert_eq!(mark(&lines, what), ("MISS".into(), said.into()), "{what}");
    }
    let (font, said) = mark(&lines, "default font");
    assert_eq!(font, "MISS");
    assert!(said.starts_with("fc-match: "), "{said}");
    for what in ["xterm.js", "themes", "encoder"] {
        assert_eq!(mark(&lines, what).0, "ok", "{what}");
    }
    assert_eq!(
        mark(&lines, "archive root"),
        (
            "ok".into(),
            "none configured; renders are never archived".into()
        )
    );
    assert_eq!(
        mark(&lines, "cache"),
        (
            "ok".into(),
            format!("{} (0 B)", dir.join("cache/prec").display())
        )
    );
    assert!(lines.iter().all(|(_, w, _)| w != "systemd-run"));
}

#[cfg(unix)]
fn be16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

#[cfg(unix)]
fn font_with_m() -> Vec<u8> {
    let mut head = Vec::new();
    for v in [1u16, 0, 1, 0, 0, 0, 0x5F0F, 0x3CF5, 0, 1000] {
        be16(&mut head, v);
    }
    head.extend_from_slice(&[0; 16]);
    for v in [0u16, 0xFF38, 600, 800, 0, 8, 2, 0, 0] {
        be16(&mut head, v);
    }
    let mut hhea = Vec::new();
    for v in [1u16, 0, 800, 0xFF38, 0] {
        be16(&mut hhea, v);
    }
    hhea.extend_from_slice(&[0; 24]);
    be16(&mut hhea, 2);
    let mut maxp = Vec::new();
    for v in [0u16, 0x5000, 2] {
        be16(&mut maxp, v);
    }
    let mut hmtx = Vec::new();
    for v in [500u16, 0, 600, 0] {
        be16(&mut hmtx, v);
    }
    let mut cmap = Vec::new();
    for v in [0u16, 1, 0, 3, 0, 12] {
        be16(&mut cmap, v);
    }
    let code = u16::from(b'M');
    for v in [
        4u16,
        32,
        0,
        4,
        4,
        1,
        0,
        code,
        0xFFFF,
        0,
        code,
        0xFFFF,
        1u16.wrapping_sub(code),
        1,
        0,
        0,
    ] {
        be16(&mut cmap, v);
    }
    let tables: [(&[u8; 4], Vec<u8>); 5] = [
        (b"cmap", cmap),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"maxp", maxp),
    ];
    let mut out = Vec::new();
    for v in [1u16, 0, 5, 64, 2, 16] {
        be16(&mut out, v);
    }
    let start = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&((start + body.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while !body.len().is_multiple_of(4) {
            body.push(0);
        }
    }
    out.extend_from_slice(&body);
    out
}

#[cfg(unix)]
fn on_path(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("no {name} on PATH"))
}

#[cfg(unix)]
fn stub(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn stocked(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(on_path("bash"), bin.join("bash")).unwrap();
    let font = dir.join("alpha.ttf");
    std::fs::write(&font, font_with_m()).unwrap();
    stub(
        &bin,
        "fc-match",
        &format!(
            "printf '%s\\n%s' '{}' 'JetBrainsMono Nerd Font'",
            font.display()
        ),
    );
    stub(&bin, "fc-list", "echo 'Alpha CJK'");
    stub(&bin, "git", "exit 0");
    stub(&bin, "chromium", "echo 'Chromium 1.0 stub'");
    bin
}

#[cfg(unix)]
#[test]
fn doctor_passes_when_everything_is_there_and_an_offline_archive_only_queues() {
    let dir = setup("doctor-stocked");
    let bin = stocked(&dir);
    std::fs::create_dir_all(dir.join("archive")).unwrap();
    let o = run(doctor(&dir, &bin).env("PITO_ARCHIVE", dir.join("archive")));
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), err(&o));
    assert!(o.stderr.is_empty(), "{}", err(&o));
    let lines = report(&out(&o));
    assert!(lines.iter().all(|(m, _, _)| m != "MISS"), "{}", out(&o));
    for (what, said) in [
        ("fc-match", bin.join("fc-match").display().to_string()),
        ("git", bin.join("git").display().to_string()),
        (
            "chromium",
            format!("{}: Chromium 1.0 stub", bin.join("chromium").display()),
        ),
        (
            "default font",
            format!(
                "JetBrainsMono Nerd Font ({})",
                dir.join("alpha.ttf").display()
            ),
        ),
        ("CJK fallback", "Alpha CJK".into()),
        (
            "archive root",
            format!("{} (reachable)", dir.join("archive").display()),
        ),
    ] {
        assert_eq!(mark(&lines, what), ("ok".into(), said), "{what}");
    }
    let offline = dir.join("offline");
    let o = run(doctor(&dir, &bin).env("PITO_ARCHIVE", &offline));
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert_eq!(
        mark(&report(&out(&o)), "archive root"),
        (
            "opt".into(),
            format!("{} (unavailable; renders queue)", offline.display())
        )
    );
    assert!(!offline.exists());
}

#[cfg(unix)]
#[test]
fn doctor_checks_the_confinement_tools_only_when_the_config_confines() {
    let dir = setup("doctor-confine");
    let bin = stocked(&dir);
    std::fs::write(dir.join("config.toml"), "[confine]\ncpus = \"0\"\n").unwrap();
    let o = run(&mut doctor(&dir, &bin));
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    let lines = report(&out(&o));
    let missing: Vec<&str> = lines
        .iter()
        .filter(|(m, _, _)| m == "MISS")
        .map(|(_, w, _)| w.as_str())
        .collect();
    assert_eq!(missing, ["systemd-run", "prlimit", "taskset"]);
    for tool in ["systemd-run", "prlimit", "taskset"] {
        stub(&bin, tool, "exit 0");
    }
    let o = run(&mut doctor(&dir, &bin));
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    let lines = report(&out(&o));
    for tool in ["systemd-run", "prlimit", "taskset"] {
        assert_eq!(
            mark(&lines, tool),
            ("ok".into(), bin.join(tool).display().to_string())
        );
    }
}
