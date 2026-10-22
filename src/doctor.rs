use crate::app::DEFAULT_FONT;
use std::process::Command;

fn found(cmd: &str) -> Option<String> {
    crate::paths::output(Command::new("bash").args(["-c", &format!("command -v {cmd}")]))
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn output(cmd: &str, args: &[&str]) -> String {
    crate::paths::output(Command::new(cmd).args(args))
        .ok()
        .map(|o| {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s
        })
        .unwrap_or_default()
}

const CONFINE: [&str; 3] = ["systemd-run", "prlimit", "taskset"];

const REQUIRED: [&str; 2] = ["fc-match", "git"];

pub fn doctor() -> bool {
    let mut ok = true;
    let mut line = |good: bool, required: bool, what: &str, detail: String| {
        let mark = if good {
            "ok  "
        } else if required {
            "MISS"
        } else {
            "opt "
        };
        if !good && required {
            ok = false;
        }
        println!("{mark} {what:<28} {detail}");
    };
    println!("ok   {:<28} {}", "config", crate::config::shown());
    let confined = crate::config::get().confine.is_some();
    let tools = REQUIRED
        .into_iter()
        .chain(CONFINE.into_iter().filter(|_| confined));
    for cmd in tools {
        let p = found(cmd);
        line(
            p.is_some(),
            true,
            cmd,
            p.unwrap_or_else(|| "not on PATH".into()),
        );
    }
    let chrome = crate::cdp::chromium();
    let cv = output(&chrome, &["--version"]);
    line(
        !cv.trim().is_empty(),
        true,
        "chromium",
        if cv.trim().is_empty() {
            "not found; a render needs a headless Chromium".into()
        } else {
            format!("{chrome}: {}", cv.trim())
        },
    );
    match crate::replay::assets_ok() {
        Ok(v) => line(true, true, "xterm.js", v),
        Err(e) => line(false, true, "xterm.js", e),
    }
    let themes = crate::theme::vendored().len();
    line(
        themes > 0,
        true,
        "themes",
        format!("{themes} vendored from VHS v0.11.0"),
    );
    line(
        true,
        true,
        "encoder",
        format!("pito-codecs {}, H.264 built in", crate::encode::CODECS),
    );
    match crate::archive::root() {
        Some(archive) => line(
            archive.is_dir(),
            false,
            "archive root",
            format!(
                "{} ({})",
                archive.display(),
                if archive.is_dir() {
                    "reachable"
                } else {
                    "unavailable; renders queue"
                }
            ),
        ),
        None => println!(
            "ok   {:<28} none configured; renders are never archived",
            "archive root"
        ),
    }
    match crate::font::metrics(DEFAULT_FONT) {
        Ok(m) => line(
            true,
            true,
            "default font",
            format!("{} ({})", DEFAULT_FONT, m.file.display()),
        ),
        Err(e) => line(false, true, "default font", e),
    }
    let cjk = output("fc-list", &[":lang=ja", "family"]);
    let first = cjk.lines().next().unwrap_or_default().to_string();
    line(
        !first.is_empty(),
        true,
        "CJK fallback",
        if first.is_empty() {
            "no Japanese-capable font".into()
        } else {
            first
        },
    );
    let faketime = [
        "/usr/lib/faketime/libfaketime.so.1",
        "/usr/lib/libfaketime.so.1",
        "/usr/lib/x86_64-linux-gnu/faketime/libfaketime.so.1",
    ]
    .iter()
    .find(|p| std::path::Path::new(p).exists());
    line(
        faketime.is_some(),
        false,
        "libfaketime (optional)",
        faketime
            .map(|p| p.to_string())
            .unwrap_or_else(|| "absent; apps pin their own clock through [env]".into()),
    );
    let root = crate::cache::root();
    println!(
        "ok   {:<28} {} ({})",
        "cache",
        root.display(),
        crate::cache::human(crate::cache::size(&root))
    );
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_off_the_path_is_not_found_and_prints_nothing() {
        assert_eq!(found("prec-no-such-tool"), None);
        assert_eq!(output("prec-no-such-tool", &["--version"]), "");
        assert!(found("sh").is_some_and(|p| p.ends_with("sh")));
    }

    #[test]
    fn output_joins_stdout_and_stderr() {
        assert_eq!(
            output("sh", &["-c", "printf out; printf err >&2"]),
            "outerr"
        );
    }
}
