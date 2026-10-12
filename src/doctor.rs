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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Need {
    Tool,
    Encoder,
    Filter,
}

const CONFINE: [&str; 3] = ["systemd-run", "prlimit", "taskset"];

const REQUIRED: [(Need, &str); 10] = [
    (Need::Tool, "ffmpeg"),
    (Need::Tool, "ffprobe"),
    (Need::Tool, "fc-match"),
    (Need::Tool, "git"),
    (Need::Encoder, "libx264"),
    (Need::Filter, "format"),
    (Need::Filter, "blend"),
    (Need::Filter, "concat"),
    (Need::Filter, "zscale"),
    (Need::Filter, "setparams"),
];

fn listed(listing: &str, name: &str) -> bool {
    listing
        .lines()
        .any(|l| l.split_whitespace().nth(1) == Some(name))
}

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
        .iter()
        .filter(|(k, _)| *k == Need::Tool)
        .map(|(_, c)| *c)
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
    let encoders = output("ffmpeg", &["-hide_banner", "-encoders"]);
    for (_, e) in REQUIRED.iter().filter(|(k, _)| *k == Need::Encoder) {
        let has = listed(&encoders, e);
        line(
            has,
            true,
            &format!("ffmpeg encoder {e}"),
            if has { "present" } else { "missing" }.into(),
        );
    }
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
    let filters = output("ffmpeg", &["-hide_banner", "-filters"]);
    for (_, f) in REQUIRED.iter().filter(|(k, _)| *k == Need::Filter) {
        let has = listed(&filters, f);
        line(
            has,
            true,
            &format!("ffmpeg filter {f}"),
            if has { "present" } else { "missing" }.into(),
        );
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

    const ENCODERS: &str = "Encoders:\n V..... = Video\n ------\n V....D ffv1                 FFmpeg video codec #1\n V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)\n";
    const FILTERS: &str = "Filters:\n T.. = Timeline support\n ... scale             V->V       Scale the input video size and/or convert the image format.\n ... blend             VV->V      Blend two video frames into each other.\n ... zscale            V->V       Apply resizing.\n";

    fn names(kind: Need) -> Vec<&'static str> {
        REQUIRED
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, n)| *n)
            .collect()
    }

    #[test]
    fn a_listing_matches_names_and_not_descriptions() {
        assert!(listed(ENCODERS, "ffv1"));
        assert!(listed(ENCODERS, "libx264"));
        assert!(!listed(ENCODERS, "codec"));
        assert!(listed(FILTERS, "scale"));
        assert!(listed(FILTERS, "blend"));
        assert!(!listed(FILTERS, "overlay"));
        assert!(!listed(FILTERS, "video"));
    }

    #[test]
    fn the_table_is_what_the_sources_call() {
        let graph = [include_str!("compose.rs"), include_str!("encode.rs")].join("\n");
        for f in names(Need::Filter) {
            assert!(
                graph.contains(&format!("{f}=")) || graph.contains(&format!("{f},")),
                "{f} is required but no filter graph calls it"
            );
        }
        assert!(!graph.contains("overlay"), "overlay is called");
        assert!(!names(Need::Filter).contains(&"overlay"));
        for e in names(Need::Encoder) {
            assert!(
                graph.contains(&format!("\"{e}\"")),
                "{e} is never asked for"
            );
        }
    }
}
