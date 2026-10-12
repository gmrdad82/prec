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
