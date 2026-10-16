use crate::record::{Cast, Kind, Timeline};
use std::path::Path;
use std::process::Command;

const EMAIL: &str = r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}";
const PLAN: &str = r"\b(?:Claude (?:Max|Pro|Team|Enterprise)|ChatGPT (?:Plus|Pro|Team|Business|Enterprise)|Copilot (?:Pro\+?|Business|Enterprise)|Gemini (?:Advanced|Pro|Ultra) plan)\b";
const ORG: &str = r"\S(?:'s|’s) (?:Organi[sz]ation|Workspace)\b|\bOrgani[sz]ation(?: name)?: *\S";
const SHORTEST_PASSED: usize = 4;
const GRID: f64 = 120.0;

pub struct Needles {
    homes: Vec<String>,
    hostname: Option<String>,
    user: Option<String>,
    email: regex::Regex,
    plan: regex::Regex,
    org: regex::Regex,
    passed: Vec<(String, String)>,
}

#[derive(Debug, PartialEq)]
pub struct Finding {
    pub what: String,
    pub at: f64,
}

fn real_home(home: &str) -> Option<String> {
    let home = home.trim_end_matches('/');
    let p = Path::new(home);
    (p.is_absolute() && p.components().count() > 1).then(|| home.to_string())
}

fn hostname() -> Option<String> {
    let text = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| {
            crate::paths::output(Command::new("uname").arg("-n"))
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        })?;
    let name = text.trim().to_string();
    (!name.is_empty() && name != "localhost" && name != "(none)").then_some(name)
}

impl Needles {
    pub fn of_this_machine(passed: &[(String, String)]) -> Needles {
        let mut homes: Vec<String> = Vec::new();
        if let Some(h) = std::env::var("HOME").ok().as_deref().and_then(real_home) {
            if let Some(c) = std::fs::canonicalize(&h)
                .ok()
                .and_then(|c| real_home(&c.to_string_lossy()))
                .filter(|c| *c != h)
            {
                homes.push(c);
            }
            homes.push(h);
        }
        let user = std::env::var("USER")
            .ok()
            .filter(|u| !u.is_empty())
            .or_else(|| std::env::var("LOGNAME").ok());
        Needles::new(homes, hostname(), user, passed)
    }

    fn new(
        homes: Vec<String>,
        hostname: Option<String>,
        user: Option<String>,
        passed: &[(String, String)],
    ) -> Needles {
        Needles {
            homes,
            hostname,
            user: user.filter(|u| u.chars().count() >= 3),
            email: regex::Regex::new(EMAIL).expect("the email pattern"),
            plan: regex::Regex::new(PLAN).expect("the plan pattern"),
            org: regex::Regex::new(ORG).expect("the org pattern"),
            passed: passed
                .iter()
                .filter(|(_, v)| v.chars().count() >= SHORTEST_PASSED)
                .cloned()
                .collect(),
        }
    }

    fn found(&self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        if self.homes.iter().any(|h| text.contains(h.as_str())) {
            out.push("the home path".to_string());
        }
        if self
            .hostname
            .as_ref()
            .is_some_and(|h| word(&text.to_ascii_lowercase(), &h.to_ascii_lowercase()))
        {
            out.push("the hostname".to_string());
        }
        if self.user.as_ref().is_some_and(|u| word(text, u)) {
            out.push("the user name".to_string());
        }
        if self.email.is_match(text) {
            out.push("an email".to_string());
        }
        if self.plan.is_match(text) {
            out.push("a plan label".to_string());
        }
        if self.org.is_match(text) {
            out.push("an organisation label".to_string());
        }
        for (name, value) in &self.passed {
            if text.contains(value.as_str()) {
                out.push(format!("a passed variable {name}"));
            }
        }
        out
    }
}

fn wordy(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

fn word(text: &str, needle: &str) -> bool {
    !needle.is_empty()
        && text.match_indices(needle).any(|(i, _)| {
            !wordy(text[..i].chars().next_back()) && !wordy(text[i + needle.len()..].chars().next())
        })
}

fn shown_at(segments: &[(f64, f64)], t: f64) -> Option<f64> {
    let mut acc = 0.0;
    for (s, e) in segments {
        if t >= *s && t < *e {
            return Some(acc + (t - s));
        }
        acc += e - s;
    }
    None
}

pub fn scan(cast: &Cast, needles: &Needles) -> Vec<Finding> {
    let timeline = Timeline::of(&cast.events);
    let shown = timeline.shown();
    let mut samples: Vec<(f64, f64)> = (0..=(shown * GRID).ceil() as usize)
        .map(|k| {
            let at = k as f64 / GRID;
            (timeline.at(at), at.min(shown))
        })
        .collect();
    let still = timeline.shots.last().copied().unwrap_or(timeline.end);
    samples.push((still, shown_at(&timeline.segments, still).unwrap_or(shown)));
    samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let outputs: Vec<_> = cast
        .events
        .iter()
        .filter(|e| e.kind == Kind::Output)
        .collect();
    let masks = crate::mask::Masks::of_meta(&cast.meta, [cast.cols, cast.rows])
        .unwrap_or_else(|_| crate::mask::Masks::new(&[], [0, 0]).expect("no masks"));
    let mut parser = vt100::Parser::new(cast.rows.max(1) as u16, cast.cols.max(1) as u16, 0);
    let mut findings: Vec<Finding> = Vec::new();
    let mut next = 0;
    let mut fresh = true;
    for (real, at) in samples {
        while next < outputs.len() && outputs[next].t <= real + 1e-9 {
            parser.process(outputs[next].data.as_bytes());
            next += 1;
            fresh = true;
        }
        if !fresh {
            continue;
        }
        fresh = false;
        let text = if masks.is_empty() {
            parser.screen().contents()
        } else {
            masks.text(parser.screen())
        };
        for what in needles.found(&text) {
            if !findings.iter().any(|f| f.what == what) {
                findings.push(Finding { what, at });
            }
        }
    }
    findings.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    findings
}

pub fn lines(cast: &Path, findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|f| {
            format!(
                "{}: the picture shows {} at {:.2}s; hide it in the tape, mask it in terminal.toml or re-record",
                crate::paths::shown(cast),
                f.what,
                f.at
            )
        })
        .collect()
}

pub fn refuse(cast: &Path, findings: &[Finding]) -> Result<(), String> {
    if findings.is_empty() {
        return Ok(());
    }
    Err(lines(cast, findings).join("\nprec: "))
}

pub fn redact(text: &str, passed: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (name, value) in passed {
        if !value.is_empty() {
            out = out.replace(value.as_str(), &format!("<{name}>"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Event;
    use serde_json::json;

    fn cast(outputs: &[(f64, &str)], marks: &[(f64, &str)]) -> Cast {
        let mut events: Vec<Event> = outputs
            .iter()
            .map(|(t, d)| Event {
                t: *t,
                kind: Kind::Output,
                data: d.to_string(),
            })
            .chain(marks.iter().map(|(t, d)| Event {
                t: *t,
                kind: Kind::Marker,
                data: d.to_string(),
            }))
            .collect();
        events.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        Cast {
            cols: 40,
            rows: 6,
            meta: json!({}),
            events,
        }
    }

    fn needles() -> Needles {
        Needles::new(
            vec!["/home/alpha".into()],
            Some("Beta-Box".into()),
            Some("alpha".into()),
            &[("TOKEN".into(), "s3cret-value".into())],
        )
    }

    fn whats(c: &Cast) -> Vec<(String, f64)> {
        scan(c, &needles())
            .into_iter()
            .map(|f| (f.what, (f.at * 100.0).round() / 100.0))
            .collect()
    }

    #[test]
    fn what_the_screen_shows_is_found_through_escapes_and_what_was_cleared_while_hidden_is_not() {
        let hidden = cast(
            &[
                (0.1, "export PATH=/home/alpha/bin a@b.io s3cret-value\r\n"),
                (0.2, "\x1b[H\x1b[2J"),
                (1.5, "/ho\x1b[1;31mme/al\x1b[0mpha/work\r\n"),
            ],
            &[(0.0, "hide"), (1.0, "show"), (3.0, "end")],
        );
        assert_eq!(
            whats(&hidden),
            [
                ("the home path".to_string(), 0.5),
                ("the user name".to_string(), 0.5)
            ]
        );
        let kept = cast(
            &[(0.1, "mail me: x.y+z@mail.example.org s3cret-value\r\n")],
            &[(0.0, "hide"), (2.0, "show"), (3.0, "end")],
        );
        assert_eq!(
            whats(&kept),
            [
                ("an email".to_string(), 0.0),
                ("a passed variable TOKEN".to_string(), 0.0)
            ]
        );
    }

    #[test]
    fn a_plan_or_organisation_label_is_found_unless_a_mask_hides_it() {
        let n = needles();
        assert_eq!(
            n.found("Opus 5.5 · Claude Max · someone's Organization"),
            ["a plan label", "an organisation label"]
        );
        assert_eq!(n.found("Organization: Acme"), ["an organisation label"]);
        assert!(
            n.found("the project's workspace on a Claude plan")
                .is_empty()
        );
        let mut banner = cast(
            &[(0.1, "Opus 5.5 · Claude Max\r\n")],
            &[(0.0, "show"), (1.0, "end")],
        );
        assert_eq!(whats(&banner), [("a plan label".to_string(), 0.1)]);
        banner.meta = json!({"mask": [{"match": "Claude \\w+"}]});
        assert!(whats(&banner).is_empty());
    }

    #[test]
    fn a_hostname_and_a_user_name_match_only_as_whole_words() {
        let n = needles();
        assert_eq!(n.found("on beta-box now"), ["the hostname"]);
        assert!(n.found("beta-boxes alphabet _alpha").is_empty());
        assert_eq!(n.found("(alpha)"), ["the user name"]);
        assert!(n.found("user@host").is_empty());
        assert_eq!(n.found("me@site.dev"), ["an email"]);
        assert!(
            Needles::new(vec![], None, Some("ab".into()), &[])
                .user
                .is_none()
        );
        assert_eq!(
            redact(
                "echo s3cret-value",
                &[("TOKEN".into(), "s3cret-value".into())]
            ),
            "echo <TOKEN>"
        );
    }
}
