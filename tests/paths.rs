mod common;

use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const GROUP: &str = "paths";
const PASSED: &str = "prec-paths-passed-7f3a91";
const SENTINEL: &str = "prec-paths-sentinel-c04e";
const OWN_ENV: [&str; 9] = [
    "HOME",
    "PATH",
    "TERM",
    "PS1",
    "SHELL",
    "TZ",
    "LANG",
    "LC_ALL",
    "COLORTERM",
];

fn prec(dir: &Path, cwd: &Path, args: &[&str]) -> Command {
    let mut c = common::prec(cwd, &dir.join("archive"), args);
    c.env("XDG_CACHE_HOME", dir.join("cache"))
        .env("PREC_CHROMIUM", dir.join("no-chromium"))
        .env("PREC_PATHS_PASSED", PASSED)
        .env("PREC_PATHS_SENTINEL", SENTINEL)
        .env_remove("PREC_PATHS_UNSET");
    c
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn refused(o: &Output) -> String {
    assert_eq!(o.status.code(), Some(1), "{}", err(o));
    assert!(out(o).is_empty(), "a refusal printed {}", out(o));
    let said = err(o);
    assert!(said.starts_with("prec: "), "{said}");
    said.trim_end().to_string()
}

fn set_hooks(app: &Path, hooks: &str, env: &str) {
    let text = std::fs::read_to_string(common::tree().join("examples/demo/render/terminal.toml"))
        .unwrap()
        .replace("[env]\n", &format!("[env]\n{env}"));
    std::fs::write(app.join("render/terminal.toml"), format!("{hooks}{text}")).unwrap();
}

fn shots(dir: &Path, app: &Path) -> Output {
    prec(dir, dir, &["shots", "demo", "--app-dir", app.to_str().unwrap()])
        .output()
        .unwrap()
}

fn record(dir: &Path, cwd: &Path, app: &Path, shot: &str, outdir: &str) -> (u32, Output) {
    let child = prec(
        dir,
        cwd,
        &[
            "record",
            "demo",
            shot,
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            outdir,
        ],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let pid = child.id();
    (pid, child.wait_with_output().unwrap())
}

fn printed(cast: &str) -> String {
    cast.lines()
        .skip(1)
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|e| e[1] == "o")
        .map(|e| e[2].as_str().unwrap_or_default().to_string())
        .collect()
}

fn lines(printed: &str) -> Vec<String> {
    printed
        .split('\n')
        .map(|l| l.trim_end_matches('\r').rsplit('\r').next().unwrap_or("").to_string())
        .collect()
}

fn header(cast: &str) -> Value {
    serde_json::from_str::<Value>(cast.lines().next().unwrap()).unwrap()["x_terminal_renderer"]
        .clone()
}

fn files(root: &Path, skip: &[&str]) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            if skip.iter().any(|s| rel.starts_with(s)) {
                continue;
            }
            if p.is_dir() {
                todo.push(p);
            } else {
                found.insert(rel);
            }
        }
    }
    found
}

#[test]
fn only_the_built_environment_reaches_the_session_and_a_callers_variable_never_does() {
    let dir = common::caller(GROUP, "session-env");
    let app = common::demo_app(&dir, "pass_env = [\"PREC_PATHS_PASSED\"]\n");
    common::add_shot(
        &app,
        "env",
        "Type@5ms \"env | cut -d= -f1 | sort | sed 's/^/N:/'; echo done-$((1+1))\"\nEnter\nWait+Screen /done-2/\nType@5ms 'test -z \"${PREC_PATHS_SENTINEL+x}\" && echo sentinel-absent; echo end-$((2+1))'\nEnter\nWait+Screen /end-3/\n",
        "",
    );
    let (_, o) = record(&dir, &dir, &app, "env", "tmp/out");
    common::expect_ok(&o, "prec record");
    let cast_path = dir.join("tmp/out/demo/clip/demo-clip-env.cast");
    assert_eq!(common::last_path(&o), cast_path);
    let cast = std::fs::read_to_string(&cast_path).unwrap();
    let shown = lines(&printed(&cast));
    let names: BTreeSet<&str> = shown.iter().filter_map(|l| l.strip_prefix("N:")).collect();
    let mut want: BTreeSet<&str> = [
        "COLORTERM",
        "DEMO_DATA",
        "HOME",
        "LANG",
        "LC_ALL",
        "PATH",
        "PREC_PATHS_PASSED",
        "PS1",
        "SHELL",
        "TERM",
        "TZ",
    ]
    .into_iter()
    .collect();
    for id in ["USER", "LOGNAME"] {
        if std::env::var_os(id).is_some() {
            want.insert(id);
        }
    }
    let bash: BTreeSet<&str> = ["PWD", "SHLVL", "_", "OLDPWD"].into_iter().collect();
    let extra: Vec<&&str> = names.difference(&want).filter(|n| !bash.contains(*n)).collect();
    assert!(extra.is_empty(), "leaked into the session: {extra:?}");
    let missing: Vec<&&str> = want.difference(&names).collect();
    assert!(missing.is_empty(), "missing in the session: {missing:?}");
    assert!(shown.iter().any(|l| l == "sentinel-absent"), "{shown:?}");
    for leaked in [SENTINEL, PASSED] {
        assert!(!cast.contains(leaked), "{leaked} is in the cast");
        assert!(!err(&o).contains(leaked) && !out(&o).contains(leaked));
    }
    let meta = header(&cast);
    assert_eq!(meta["pass_env"], json!(["PREC_PATHS_PASSED"]));
    let env_keys: BTreeSet<&str> = meta["env"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p[0].as_str().unwrap())
        .collect();
    assert_eq!(
        env_keys,
        ["COLORTERM", "DEMO_DATA", "LANG", "LC_ALL", "TERM", "TZ"]
            .into_iter()
            .collect::<BTreeSet<_>>()
    );
    assert_eq!(meta["home"], Value::Null);
    assert_eq!(meta["hostname"], Value::Null);
}

#[test]
fn a_passed_value_the_app_prints_is_named_never_shown_and_a_clip_of_it_is_refused() {
    let dir = common::caller(GROUP, "passed-printed");
    let app = common::demo_app(&dir, "pass_env = [\"PREC_PATHS_PASSED\"]\n");
    common::add_shot(
        &app,
        "echo",
        "Type@5ms 'echo \"$PREC_PATHS_PASSED\"; echo done-$((1+1))'\nEnter\nWait+Screen /done-2/\n",
        "",
    );
    let (_, o) = record(&dir, &dir, &app, "echo", "tmp/out");
    common::expect_ok(&o, "prec record");
    let warned = err(&o);
    let cast_path = dir.join("tmp/out/demo/clip/demo-clip-echo.cast");
    assert!(
        warned.contains(
            "prec: tmp/out/demo/clip/demo-clip-echo.cast: the picture shows a passed variable PREC_PATHS_PASSED at "
        ),
        "{warned}"
    );
    assert!(!warned.contains(PASSED) && !out(&o).contains(PASSED));
    let cast = std::fs::read_to_string(&cast_path).unwrap();
    assert!(lines(&printed(&cast)).iter().any(|l| l == PASSED));
    let child = prec(
        &dir,
        &dir,
        &[
            "clip",
            "demo",
            "echo",
            "--app-dir",
            app.to_str().unwrap(),
            "--size",
            "320x180",
            "--fps",
            "30",
            "--no-encode",
            "--out",
            "tmp/clip",
        ],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let pid = child.id();
    let o = child.wait_with_output().unwrap();
    let said = err(&o);
    assert_eq!(o.status.code(), Some(1), "{said}");
    assert!(out(&o).is_empty());
    assert!(!said.contains(PASSED), "{said}");
    let run_cast = format!("tmp/prec/run/demo-clip-echo-cast-{pid}/demo-clip-echo.cast");
    assert!(
        said.lines().any(|l| l.starts_with(&format!(
            "prec: {run_cast}: the picture shows a passed variable PREC_PATHS_PASSED at "
        ))),
        "{said}"
    );
    assert!(dir.join(run_cast).is_file(), "{said}");
    assert!(!dir.join("tmp/clip").exists(), "a refused clip wrote {said}");
}

#[test]
fn a_passed_value_in_a_failing_setup_is_masked_in_the_error() {
    let dir = common::caller(GROUP, "passed-error");
    let app = common::demo_app(
        &dir,
        &format!(
            "pass_env = [\"PREC_PATHS_PASSED\"]\nsetup = 'test -n \"$PREC_PATHS_PASSED\" && exit 3; echo {PASSED}'\n"
        ),
    );
    let (_, o) = record(&dir, &dir, &app, "palette", "tmp/out");
    let said = refused(&o);
    assert_eq!(
        said,
        "prec: test -n \"$PREC_PATHS_PASSED\" && exit 3; echo <PREC_PATHS_PASSED> failed"
    );
    assert!(!dir.join("tmp/out/demo/still/demo-still-palette.cast").exists());
}

#[test]
fn an_unset_passed_variable_is_refused_before_anything_runs() {
    let dir = common::caller(GROUP, "passed-unset");
    let app = common::demo_app(
        &dir,
        "pass_env = [\"PREC_PATHS_UNSET\"]\nsetup = 'touch {tmp}/setup-ran'\n",
    );
    let (_, o) = record(&dir, &dir, &app, "palette", "tmp/out");
    assert_eq!(refused(&o), "prec: pass_env PREC_PATHS_UNSET is not set");
    assert!(!dir.join("tmp/setup-ran").exists());
    assert!(!dir.join("tmp/out/demo/still/demo-still-palette.cast").exists());
}

#[test]
fn a_passed_name_prec_sets_or_the_tape_sets_or_a_bad_one_is_refused_at_load() {
    let dir = common::caller(GROUP, "passed-names");
    let app = common::demo_app(&dir, "");
    let toml = app.join("render/terminal.toml");
    let at = |what: &str| format!("prec: {}: {what}", toml.display());
    for own in OWN_ENV {
        set_hooks(&app, &format!("pass_env = [\"{own}\"]\n"), "");
        assert_eq!(
            refused(&shots(&dir, &app)),
            at(&format!("pass_env {own}: prec sets {own} itself"))
        );
    }
    for (pass, env, why) in [
        (
            "[\"DEMO_DATA\"]",
            "",
            "pass_env DEMO_DATA is also in [env]".to_string(),
        ),
        (
            "[\"TOKEN\"]",
            "TOKEN = \"x\"\n",
            "pass_env TOKEN is also in [env]".to_string(),
        ),
        (
            "[\"A_B\", \"A_B\"]",
            "",
            "pass_env A_B is named twice".to_string(),
        ),
        (
            "[\"1BAD\"]",
            "",
            "pass_env \"1BAD\": letters, digits and '_', not starting with a digit".to_string(),
        ),
        (
            "[\"A-B\"]",
            "",
            "pass_env \"A-B\": letters, digits and '_', not starting with a digit".to_string(),
        ),
        (
            "[\"\"]",
            "",
            "pass_env \"\": letters, digits and '_', not starting with a digit".to_string(),
        ),
    ] {
        set_hooks(&app, &format!("pass_env = {pass}\n"), env);
        assert_eq!(refused(&shots(&dir, &app)), at(&why), "{pass} {env}");
    }
    set_hooks(&app, "pass_env = [\"_TOKEN_1\", \"B\"]\n", "");
    common::expect_ok(&shots(&dir, &app), "two good names");
}

#[test]
fn a_home_must_be_absolute_with_two_plain_parts_and_a_hostname_needs_it() {
    let dir = common::caller(GROUP, "home-shape");
    let app = common::demo_app(&dir, "");
    let toml = app.join("render/terminal.toml");
    let at = |what: &str| format!("prec: {}: {what}", toml.display());
    for home in [
        "home/alpha",
        "/alpha",
        "/home/al pha",
        "/home/../alpha",
        "/home/./alpha",
        "/home/alpha/",
        "/home//alpha",
        "/home/~",
        "",
    ] {
        set_hooks(&app, &format!("home = \"{home}\"\n"), "");
        assert_eq!(
            refused(&shots(&dir, &app)),
            at(&format!(
                "home {home:?}: an absolute path of two or more parts, each of letters, digits, '-', '_' and '.'"
            )),
            "{home}"
        );
    }
    set_hooks(&app, "hostname = \"beta-box\"\n", "");
    assert_eq!(refused(&shots(&dir, &app)), at("hostname needs home"));
    let long = "a".repeat(64);
    for host in ["a_b", "a b", "", long.as_str(), "..", "a/b"] {
        set_hooks(
            &app,
            &format!("home = \"/home/prec-alpha\"\nhostname = \"{host}\"\n"),
            "",
        );
        assert_eq!(
            refused(&shots(&dir, &app)),
            at(&format!(
                "hostname {host:?}: letters, digits, '-' and '.', 63 at most"
            )),
            "{host}"
        );
    }
    set_hooks(&app, "home = \"/home/prec-alpha\"\n", "HOME = \"/x\"\n");
    assert_eq!(
        refused(&shots(&dir, &app)),
        at("home and [env] HOME both set the session's home; keep one")
    );
    for hooks in [
        "home = \"/home/prec-alpha\"\n".to_string(),
        "home = \"/srv/prec/alpha.v2\"\nhostname = \"beta-box.lan\"\n".to_string(),
        format!("home = \"/home/prec-alpha\"\nhostname = \"{}\"\n", "a".repeat(63)),
    ] {
        set_hooks(&app, &hooks, "");
        let o = shots(&dir, &app);
        common::expect_ok(&o, &hooks);
        assert!(out(&o).starts_with("demo — grid 60x16"), "{}", out(&o));
    }
}

#[test]
#[cfg(target_os = "linux")]
fn a_home_without_a_hostname_shows_the_default_one() {
    let dir = common::caller(GROUP, "default-host");
    let home = "/home/prec-paths-default-host";
    let app = common::demo_app(&dir, &format!("home = \"{home}\"\n"));
    common::add_shot(
        &app,
        "host",
        "Type@5ms 'hostname; echo \"$HOME\"; echo done-$((1+1))'\nEnter\nWait+Screen /done-2/\n",
        "",
    );
    let (_, o) = record(&dir, &dir, &app, "host", "tmp/out");
    common::expect_ok(&o, "prec record");
    let cast = std::fs::read_to_string(dir.join("tmp/out/demo/clip/demo-clip-host.cast")).unwrap();
    let shown = lines(&printed(&cast));
    let at = shown
        .iter()
        .position(|l| l == "prec")
        .unwrap_or_else(|| panic!("no default hostname in {shown:?}"));
    assert_eq!(shown[at + 1], home);
    let meta = header(&cast);
    assert_eq!(meta["hostname"], "prec");
    assert_eq!(meta["home"], home);
}

#[test]
#[cfg(target_os = "linux")]
fn a_home_that_exists_on_this_machine_is_refused() {
    let dir = common::caller(GROUP, "home-exists");
    let app = common::demo_app(&dir, "home = \"/usr/lib\"\n");
    let (_, o) = record(&dir, &dir, &app, "palette", "tmp/out");
    assert_eq!(
        refused(&o),
        "prec: home /usr/lib: that path exists on this machine; name one that doesn't"
    );
}

#[test]
fn outside_a_git_tree_prec_refuses_and_writes_nothing() {
    let dir = std::fs::canonicalize(common::scratch(GROUP, "no-git")).unwrap();
    let inner = dir.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    let o = prec(&dir, &inner, &["shots", "demo"])
        .env("GIT_CEILING_DIRECTORIES", dir.as_os_str())
        .output()
        .unwrap();
    assert_eq!(
        refused(&o),
        "prec: prec runs inside a git tree; its outputs land in that tree's tmp/"
    );
    assert_eq!(std::fs::read_dir(inner).unwrap().count(), 0);
}

#[test]
fn the_out_folder_resolves_from_the_working_folder_and_must_stay_in_the_callers_tmp() {
    let dir = common::caller(GROUP, "out-folder");
    let app = common::demo_app(&dir, "");
    let tmp = dir.join("tmp");
    let abs = tmp.join("abs").display().to_string();
    for (cwd, flag, lands) in [
        (dir.clone(), "tmp/../tmp/x", tmp.join("x")),
        (app.clone(), "../tmp/sub", tmp.join("sub")),
        (dir.clone(), abs.as_str(), tmp.join("abs")),
    ] {
        let (_, o) = record(&dir, &cwd, &app, "palette", flag);
        common::expect_ok(&o, flag);
        let cast = lands.join("demo/still/demo-still-palette.cast");
        assert_eq!(common::last_path(&o), cast, "{flag}");
        assert!(cast.is_file(), "{flag}");
    }
    for (cwd, flag, abs) in [
        (dir.clone(), "out", dir.join("out")),
        (dir.clone(), "tmp/../out", dir.join("out")),
        (dir.clone(), ".", dir.clone()),
        (app.clone(), "tmp/x", app.join("tmp/x")),
        (dir.clone(), "tmpx", dir.join("tmpx")),
    ] {
        let (_, o) = record(&dir, &cwd, &app, "palette", flag);
        assert_eq!(
            refused(&o),
            format!(
                "prec: {} is outside the caller's tmp/ ({}); prec writes nowhere else",
                abs.display(),
                tmp.display()
            ),
            "{flag}"
        );
        assert!(!dir.join("out").exists() && !dir.join("tmpx").exists() && !app.join("tmp").exists());
    }
}

#[test]
fn a_record_writes_only_in_the_callers_tmp_and_the_cache_and_names_its_folders() {
    let dir = common::caller(GROUP, "allowed-places");
    let app = common::demo_app(&dir, "");
    common::add_shot(&app, "again", "Type \"demo show\"\nEnter\nSleep 600ms\n", "");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let skip = ["tmp", ".git", "cache"];
    let before = files(&dir, &skip);
    let mut pids = vec![];
    for shot in ["palette", "again"] {
        let child = prec(
            &dir,
            &dir,
            &[
                "record",
                "demo",
                shot,
                "--app-dir",
                app.to_str().unwrap(),
                "--out",
                "tmp/out",
            ],
        )
        .env("HOME", &home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
        pids.push(child.id());
        common::expect_ok(&child.wait_with_output().unwrap(), shot);
    }
    assert_eq!(files(&dir, &skip), before);
    assert_eq!(std::fs::read_dir(&home).unwrap().count(), 0);
    assert!(!dir.join("archive").exists());
    let run = dir.join("tmp/prec/run");
    for (shot, kind, pid) in [("palette", "still", pids[0]), ("again", "clip", pids[1])] {
        let work = run.join(format!("demo-{kind}-{shot}-record"));
        assert!(work.join("scratch/home").is_dir(), "{}", work.display());
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("tmp/prec/locks/demo-{shot}.lock"))).unwrap(),
            format!("prec {pid} (record demo {shot})\n")
        );
        assert!(
            dir.join(format!("tmp/out/demo/{kind}/demo-{kind}-{shot}.cast"))
                .is_file()
        );
    }
}

fn doctor(dir: &Path, env: &[(&str, &Path)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_prec"));
    c.arg("doctor")
        .current_dir(dir)
        .env_remove("PREC_CONFIG")
        .env_remove("PITO_ARCHIVE")
        .env("PITO_JOBS", "off")
        .env("PREC_DIRECT", "1")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("PREC_CHROMIUM", dir.join("no-chromium"));
    for (k, v) in env {
        c.env(k, v);
    }
    c.output().unwrap()
}

fn detail(o: &Output, what: &str) -> Option<String> {
    out(o)
        .lines()
        .find(|l| l.get(5..33).map(str::trim_end) == Some(what))
        .map(|l| l[34..].to_string())
}

fn config(dir: &Path, text: &str) -> PathBuf {
    let path = dir.join("config/prec/config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn without_a_config_file_the_defaults_apply_and_nothing_is_archived() {
    let dir = common::caller(GROUP, "config-absent");
    let o = doctor(&dir, &[]);
    assert_eq!(
        detail(&o, "config").as_deref(),
        Some(
            format!(
                "{} (absent; the defaults apply)",
                dir.join("config/prec/config.toml").display()
            )
            .as_str()
        ),
        "{}",
        out(&o)
    );
    assert_eq!(
        detail(&o, "archive root").as_deref(),
        Some("none configured; renders are never archived")
    );
    for tool in ["systemd-run", "prlimit", "taskset"] {
        assert_eq!(detail(&o, tool), None, "{tool} is checked unconfined");
        assert!(!out(&o).contains(tool), "{tool}: {}", out(&o));
    }
    let path = config(&dir, "");
    let o = doctor(&dir, &[]);
    assert_eq!(
        detail(&o, "config").as_deref(),
        Some(path.display().to_string().as_str())
    );
    assert_eq!(
        detail(&o, "archive root").as_deref(),
        Some("none configured; renders are never archived")
    );
}

#[test]
fn the_configs_archive_is_used_unless_pito_archive_names_another() {
    let dir = common::caller(GROUP, "config-archive");
    let from_config = dir.join("from-config");
    config(&dir, &format!("archive = \"{}\"\n", from_config.display()));
    let o = doctor(&dir, &[]);
    assert_eq!(
        detail(&o, "archive root"),
        Some(format!(
            "{} (unavailable; renders queue)",
            from_config.display()
        )),
        "{}",
        out(&o)
    );
    std::fs::create_dir_all(&from_config).unwrap();
    let o = doctor(&dir, &[]);
    assert_eq!(
        detail(&o, "archive root"),
        Some(format!("{} (reachable)", from_config.display()))
    );
    let from_env = dir.join("from-env");
    std::fs::create_dir_all(&from_env).unwrap();
    let o = doctor(&dir, &[("PITO_ARCHIVE", &from_env)]);
    assert_eq!(
        detail(&o, "archive root"),
        Some(format!("{} (reachable)", from_env.display()))
    );
    let o = doctor(&dir, &[("PITO_ARCHIVE", Path::new(""))]);
    assert_eq!(
        detail(&o, "archive root"),
        Some(format!("{} (reachable)", from_config.display()))
    );
}

#[test]
fn a_tilde_and_a_relative_xdg_config_home_resolve_from_home() {
    let dir = common::caller(GROUP, "config-home");
    let home = dir.join("home");
    let path = home.join(".config/prec/config.toml");
    let relative = Path::new("relative/config");
    let o = doctor(&dir, &[("HOME", &home), ("XDG_CONFIG_HOME", relative)]);
    assert_eq!(
        detail(&o, "config"),
        Some(format!("{} (absent; the defaults apply)", path.display()))
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "archive = \"~/arch\"\n").unwrap();
    let o = doctor(&dir, &[("HOME", &home), ("XDG_CONFIG_HOME", relative)]);
    assert_eq!(detail(&o, "config"), Some(path.display().to_string()));
    assert_eq!(
        detail(&o, "archive root"),
        Some(format!(
            "{} (unavailable; renders queue)",
            home.join("arch").display()
        ))
    );
}

#[test]
fn a_confine_table_makes_doctor_check_the_confining_tools() {
    let dir = common::caller(GROUP, "config-confine");
    config(&dir, "[confine]\nslice = \"render.slice\"\n");
    let o = doctor(&dir, &[]);
    for tool in ["systemd-run", "prlimit", "taskset"] {
        assert!(detail(&o, tool).is_some(), "{tool}: {}", out(&o));
    }
}

#[test]
fn an_unreadable_or_unknown_config_is_refused_before_anything_runs() {
    let dir = common::caller(GROUP, "config-refused");
    let path = config(&dir, "archiv = \"/srv\"\n");
    let o = doctor(&dir, &[]);
    let said = refused(&o);
    assert!(
        said.starts_with(&format!("prec: {}: ", path.display())),
        "{said}"
    );
    assert!(said.contains("unknown field `archiv`"), "{said}");
    config(&dir, "[confine]\ncpus = \"1\"\ncpus_from = \"/x\"\n");
    assert_eq!(
        refused(&doctor(&dir, &[])),
        format!(
            "prec: {}: [confine] takes cpus or cpus_from, not both",
            path.display()
        )
    );
    config(&dir, "");
    let named = dir.join("named.toml");
    let o = doctor(&dir, &[("PREC_CONFIG", &named)]);
    assert!(
        refused(&o).starts_with(&format!("prec: {}: ", named.display())),
        "{}",
        err(&o)
    );
    std::fs::write(&named, "theme = \"nord\"\n").unwrap();
    let o = doctor(&dir, &[("PREC_CONFIG", &named)]);
    assert_eq!(detail(&o, "config"), Some(named.display().to_string()));
}

#[test]
fn two_records_of_one_shot_at_once_take_turns_through_the_shots_lock() {
    let dir = common::caller(GROUP, "same-shot");
    let app = common::demo_app(&dir, "");
    let start = |out: &str| {
        prec(
            &dir,
            &dir,
            &[
                "record",
                "demo",
                "palette",
                "--app-dir",
                app.to_str().unwrap(),
                "--out",
                out,
            ],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
    };
    let a = start("tmp/out-a");
    let b = start("tmp/out-b");
    let pids = [a.id(), b.id()];
    let outs = [a.wait_with_output().unwrap(), b.wait_with_output().unwrap()];
    for (o, out) in outs.iter().zip(["out-a", "out-b"]) {
        common::expect_ok(o, out);
        let cast_path = dir.join(format!("tmp/{out}/demo/still/demo-still-palette.cast"));
        assert_eq!(common::last_path(o), cast_path);
        let cast = std::fs::read_to_string(&cast_path).unwrap();
        let head: Value = serde_json::from_str(cast.lines().next().unwrap()).unwrap();
        assert_eq!(head["version"], 2, "{out}");
        assert!(!printed(&cast).is_empty(), "{out} recorded nothing");
        assert!(
            cast.lines().skip(1).all(|l| serde_json::from_str::<Value>(l).is_ok()),
            "{out}"
        );
    }
    let run = dir.join("tmp/prec/run");
    let records: Vec<String> = std::fs::read_dir(&run)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("demo-still-palette-record"))
        .collect();
    assert_eq!(records, ["demo-still-palette-record"]);
    let holder = std::fs::read_to_string(dir.join("tmp/prec/locks/demo-palette.lock")).unwrap();
    assert!(
        pids.iter()
            .any(|pid| holder == format!("prec {pid} (record demo palette)\n")),
        "{holder}"
    );
    let waited: Vec<bool> = outs
        .iter()
        .map(|o| err(o).contains("waiting for prec "))
        .collect();
    assert!(waited.iter().filter(|w| **w).count() <= 1, "{waited:?}");
}

#[test]
fn a_hook_that_prints_a_passed_value_shows_it_on_precs_own_output() {
    let dir = common::caller(GROUP, "passed-hook");
    let app = common::demo_app(
        &dir,
        "pass_env = [\"PREC_PATHS_PASSED\"]\nsetup = 'echo \"hook-saw-$PREC_PATHS_PASSED\"; exit 3'\n",
    );
    let (_, o) = record(&dir, &dir, &app, "palette", "tmp/out");
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert_eq!(out(&o), format!("hook-saw-{PASSED}\n"));
    assert_eq!(
        err(&o).trim_end().lines().last(),
        Some("prec: echo \"hook-saw-$PREC_PATHS_PASSED\"; exit 3 failed")
    );
    assert!(!err(&o).contains(PASSED), "{}", err(&o));
    assert!(!dir.join("tmp/out/demo/still/demo-still-palette.cast").exists());
}

#[test]
fn a_user_or_logname_in_pass_env_is_taken_though_prec_sets_both() {
    let dir = common::caller(GROUP, "passed-user");
    let app = common::demo_app(&dir, "");
    for own in ["USER", "LOGNAME"] {
        set_hooks(&app, &format!("pass_env = [\"{own}\"]\n"), "");
        let o = shots(&dir, &app);
        common::expect_ok(&o, own);
        assert!(out(&o).starts_with("demo — grid 60x16"), "{}", out(&o));
    }
}

#[test]
fn the_callers_home_path_on_screen_is_named_in_a_warning() {
    let dir = common::caller(GROUP, "scan-home");
    let app = common::demo_app(&dir, "");
    common::add_shot(
        &app,
        "home",
        "Type@5ms 'echo /prec-paths-scan/alpha/notes; echo done-$((1+1))'\nEnter\nWait+Screen /done-2/\n",
        "",
    );
    let child = prec(
        &dir,
        &dir,
        &[
            "record",
            "demo",
            "home",
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            "tmp/out",
        ],
    )
    .env("HOME", "/prec-paths-scan/alpha")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let o = child.wait_with_output().unwrap();
    common::expect_ok(&o, "prec record");
    let said = err(&o);
    assert!(
        said.lines().any(|l| l.starts_with(
            "prec: tmp/out/demo/clip/demo-clip-home.cast: the picture shows the home path at "
        )),
        "{said}"
    );
    assert!(!Path::new("/prec-paths-scan").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn this_machines_hostname_on_screen_is_named_in_a_warning() {
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    let host = host.trim();
    if host.is_empty() || host == "localhost" || host == "(none)" {
        return;
    }
    let dir = common::caller(GROUP, "scan-host");
    let app = common::demo_app(&dir, "");
    common::add_shot(
        &app,
        "host",
        "Type@5ms 'cat /proc/sys/kernel/hostname; echo done-$((1+1))'\nEnter\nWait+Screen /done-2/\n",
        "",
    );
    let (_, o) = record(&dir, &dir, &app, "host", "tmp/out");
    common::expect_ok(&o, "prec record");
    let said = err(&o);
    assert!(
        said.lines().any(|l| l.starts_with(
            "prec: tmp/out/demo/clip/demo-clip-host.cast: the picture shows the hostname at "
        )),
        "{said}"
    );
}

#[test]
fn an_empty_prec_config_means_the_default_file() {
    let dir = common::caller(GROUP, "config-empty-env");
    let from_config = dir.join("from-config");
    std::fs::create_dir_all(&from_config).unwrap();
    let path = config(&dir, &format!("archive = \"{}\"\n", from_config.display()));
    let o = doctor(&dir, &[("PREC_CONFIG", Path::new(""))]);
    assert_eq!(detail(&o, "config"), Some(path.display().to_string()), "{}", out(&o));
    assert_eq!(
        detail(&o, "archive root"),
        Some(format!("{} (reachable)", from_config.display()))
    );
}

#[test]
fn a_config_file_prec_cannot_read_is_refused_by_name() {
    use std::os::unix::fs::PermissionsExt;
    let dir = common::caller(GROUP, "config-unreadable");
    let path = config(&dir, "archive = \"/srv\"\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&path).is_ok() {
        return;
    }
    let o = doctor(&dir, &[]);
    let said = refused(&o);
    assert!(
        said.starts_with(&format!("prec: {}: ", path.display())),
        "{said}"
    );
    assert!(!said.contains("unknown field"), "{said}");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}
