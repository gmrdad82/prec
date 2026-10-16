mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|h| h.join(".local/share"))
        })
}

fn child_pids(pid: u32) -> Vec<u32> {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect()
}

fn descendants(root: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        for child in child_pids(pid) {
            out.push(child);
            queue.push(child);
        }
    }
    out
}

fn read_proc(pid: u32, name: &str) -> Vec<u8> {
    std::fs::read(format!("/proc/{pid}/{name}")).unwrap_or_default()
}

fn fields(raw: &[u8]) -> Vec<String> {
    raw.split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

fn pairs(raw: &[u8]) -> BTreeMap<String, String> {
    raw.split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .filter_map(|a| {
            let text = String::from_utf8_lossy(a);
            let (k, v) = text.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

fn profile_of(dir: &Path) -> Option<PathBuf> {
    let run = dir.join("tmp/prec/run");
    let entries = std::fs::read_dir(&run).ok()?;
    for entry in entries.flatten() {
        let profile = entry.path().join("chromium");
        if profile.is_dir() {
            return Some(profile);
        }
    }
    None
}

enum Look {
    Ready(BTreeMap<String, String>),
    Wiped,
    Pending,
}

fn browser_env(prec: u32, dir: &str) -> Look {
    let mut wiped = false;
    for pid in descendants(prec) {
        let args = fields(&read_proc(pid, "cmdline"));
        let launched = args.iter().any(|a| {
            a.strip_prefix("--user-data-dir=")
                .is_some_and(|p| p.contains(dir))
        }) && args.iter().all(|a| !a.starts_with("--type="));
        if !launched {
            continue;
        }
        let env = pairs(&read_proc(pid, "environ"));
        if env
            .get("HOME")
            .is_some_and(|h| h.ends_with("chromium-home") && h.contains(dir))
            && env.contains_key("LANG")
        {
            return Look::Ready(env);
        }
        if args
            .first()
            .is_some_and(|a| a.contains("chromium") || a.contains("chrome"))
        {
            wiped = true;
        }
    }
    if wiped { Look::Wiped } else { Look::Pending }
}

fn finish(child: &mut Child, dir: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    let needle = dir.display().to_string();
    for proc in common::procs() {
        if proc.args.iter().any(|a| a.contains(&needle)) {
            common::signal(proc.pid, "KILL");
        }
    }
}

fn expected(home: &Path) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if let Ok(v) = std::env::var("USER") {
        env.insert("USER".into(), v);
    }
    if let Ok(v) = std::env::var("LOGNAME") {
        env.insert("LOGNAME".into(), v);
    }
    env.insert("HOME".into(), home.display().to_string());
    env.insert(
        "XDG_CONFIG_HOME".into(),
        home.join(".config").display().to_string(),
    );
    env.insert(
        "XDG_CACHE_HOME".into(),
        home.join(".cache").display().to_string(),
    );
    env.insert(
        "DBUS_SESSION_BUS_ADDRESS".into(),
        "unix:path=/nonexistent".into(),
    );
    if let Some(data) = data_home() {
        env.insert("XDG_DATA_HOME".into(), data.display().to_string());
    }
    env.insert("TZ".into(), "UTC".into());
    env.insert("LANG".into(), "C.UTF-8".into());
    env.insert("LC_ALL".into(), "C.UTF-8".into());
    env.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
    env
}

#[test]
fn chromium_gets_only_the_built_environment() {
    let dir = common::caller("environment", "leak");
    let cast = dir.join("cast/swatch.cast");
    common::write_cast(
        &cast,
        "environment",
        "leak",
        "clip",
        &[(0.0, common::paint(32, 96, 160))],
        2.0,
    );
    let marker = dir.display().to_string();
    let mut got = None;
    for _ in 0..4 {
        let _ = std::fs::remove_dir_all(dir.join("tmp"));
        let mut cmd = common::prec(
            &dir,
            &dir.join("archive"),
            &[
                "render",
                cast.to_str().unwrap(),
                "--size",
                "640x360",
                "--fps",
                "30",
                "--no-encode",
                "--out",
                "tmp/out",
            ],
        );
        cmd.env("LANG", "de_DE.UTF-8")
            .env("TZ", "Asia/Tokyo")
            .env("FONTCONFIG_FILE", "/nonexistent")
            .env("PREC_LEAK_CHECK", "1");
        let mut child = common::spawn(cmd, &dir);
        let prec = child.id();
        let started = Instant::now();
        let mut spins = 0u32;
        while started.elapsed() < Duration::from_secs(40) {
            match browser_env(prec, &marker) {
                Look::Ready(env) => {
                    got = Some(env);
                    break;
                }
                Look::Wiped => break,
                Look::Pending => {}
            }
            spins = spins.wrapping_add(1);
            if spins.is_multiple_of(256) && child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::hint::spin_loop();
        }
        finish(&mut child, &dir);
        if got.is_some() {
            break;
        }
    }
    let got = got.unwrap_or_else(|| {
        panic!(
            "chromium's environment was not readable; prec said:\n{}",
            common::said(&dir)
        )
    });
    let profile = profile_of(&dir).unwrap_or_else(|| {
        panic!(
            "no chromium profile under tmp/prec/run; prec said:\n{}",
            common::said(&dir)
        )
    });
    let home = profile.parent().unwrap().join("chromium-home");
    let want = expected(&home);
    for (key, value) in &want {
        assert_eq!(
            got.get(key).map(String::as_str),
            Some(value.as_str()),
            "{key}"
        );
    }
    for key in [
        "FONTCONFIG_FILE",
        "PREC_LEAK_CHECK",
        "DISPLAY",
        "WAYLAND_DISPLAY",
    ] {
        assert!(!got.contains_key(key), "{key} reached chromium: {got:?}");
    }
    assert_eq!(got.get("LANG").map(String::as_str), Some("C.UTF-8"));
    assert_eq!(got.get("TZ").map(String::as_str), Some("UTC"));
    let extras: Vec<_> = got
        .keys()
        .filter(|k| !want.contains_key(*k))
        .cloned()
        .collect();
    assert!(
        extras.iter().all(|k| k.starts_with("CHROME_")),
        "chromium kept {extras:?}; full environment {got:?}"
    );
}

fn shown_output(cast: &str) -> String {
    let mut shown = None;
    let mut out = String::new();
    for line in cast.lines().skip(1) {
        let e: serde_json::Value = serde_json::from_str(line).unwrap();
        let t = e[0].as_f64().unwrap();
        if shown.is_none() && e[1] == "m" && e[2] == "show" {
            shown = Some(t);
        }
        if shown.is_some_and(|s| t >= s) && e[1] == "o" {
            out.push_str(e[2].as_str().unwrap());
        }
    }
    out
}

#[test]
fn a_neutral_home_hides_the_callers_paths_and_a_passed_value_is_never_written() {
    use std::hash::{BuildHasher, Hasher};
    let dir = common::caller("environment", "neutral-home");
    let home = "/home/prec-neutral-home";
    let host = "prec-neutral-host";
    let secret = format!(
        "s{:016x}{}",
        std::hash::RandomState::new().build_hasher().finish(),
        std::process::id()
    );
    let app = common::demo_app(
        &dir,
        &format!(
            "home = \"{home}\"\nhostname = \"{host}\"\ncwd = \"{{home}}/work\"\npass_env = [\"PREC_TEST_SECRET\"]\n"
        ),
    );
    common::add_shot(
        &app,
        "neutral",
        "Type@5ms \"pwd; echo $HOME; hostname; id -un\"\nEnter\nSleep 1s\nType@5ms 'test -n \"$PREC_TEST_SECRET\" && echo passed'\nEnter\nSleep 1s\n",
        "",
    );
    let archive = dir.join("archive");
    let shot = |verb: &str, extra: &[&str]| {
        let mut args = vec![verb, "demo", "neutral", "--app-dir", app.to_str().unwrap()];
        args.extend_from_slice(extra);
        let mut c = common::prec(&dir, &archive, &args);
        c.env("PREC_TEST_SECRET", &secret);
        c.output().unwrap()
    };
    let recorded = shot("record", &["--out", "tmp/out"]);
    common::expect_ok(&recorded, "prec record");
    let warned = String::from_utf8_lossy(&recorded.stderr).to_string();
    let cast_path = dir.join("tmp/out/demo/clip/demo-clip-neutral.cast");
    let cast = std::fs::read_to_string(&cast_path).unwrap();
    let shown = shown_output(&cast);
    let lines: Vec<&str> = shown
        .split("\r\n")
        .map(|l| l.rsplit('\r').next().unwrap_or(l))
        .collect();
    for want in [
        format!("{home}/work"),
        home.to_string(),
        host.to_string(),
        "passed".into(),
    ] {
        assert!(
            lines.contains(&want.as_str()),
            "{want:?} not shown in {shown:?}"
        );
    }
    let caller_home = std::env::var("HOME").unwrap();
    assert!(
        !shown.contains(&caller_home),
        "the caller's home is shown: {shown:?}"
    );
    assert!(!cast.contains(&secret), "the passed value is in the cast");
    assert!(!warned.contains(&secret), "the passed value was printed");
    let header: serde_json::Value = serde_json::from_str(cast.lines().next().unwrap()).unwrap();
    let meta = &header["x_terminal_renderer"];
    assert_eq!(meta["pass_env"], serde_json::json!(["PREC_TEST_SECRET"]));
    assert_eq!(meta["home"], home);
    assert_eq!(meta["hostname"], host);
    let user = std::env::var("USER").unwrap_or_default();
    let shows_user = user.len() >= 3 && lines.contains(&user.as_str());
    let draft = [
        "--size",
        "320x180",
        "--fps",
        "30",
        "--no-encode",
        "--out",
        "tmp/clip",
    ];
    let manifest = if shows_user {
        assert!(
            warned.contains("the picture shows the user name"),
            "record printed no warning: {warned}"
        );
        let refused = shot("clip", &draft);
        let said = String::from_utf8_lossy(&refused.stderr).to_string();
        assert_eq!(refused.status.code(), Some(1), "{said}");
        assert!(said.contains("the picture shows the user name"), "{said}");
        assert!(!said.contains(&secret), "the passed value was printed");
        assert!(
            !dir.join("tmp/clip").exists(),
            "a refused clip wrote {said}"
        );
        let mut allowed = draft.to_vec();
        allowed.push("--allow-private");
        let o = shot("clip", &allowed);
        common::expect_ok(&o, "prec clip --allow-private");
        common::last_path(&o)
    } else {
        let o = shot("clip", &draft);
        common::expect_ok(&o, "prec clip");
        common::last_path(&o)
    };
    let manifest = std::fs::read_to_string(dir.join(manifest)).unwrap();
    assert!(
        !manifest.contains(&secret),
        "the passed value is in the manifest"
    );
    let m: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(m["pass_env"], serde_json::json!(["PREC_TEST_SECRET"]));
    assert_eq!(m["private_allowed"], shows_user);
}
