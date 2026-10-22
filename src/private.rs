use crate::record::{Cast, Kind, Timeline};
use std::path::Path;
use std::process::Command;

const EMAIL: &str = r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}";
const PLAN: &str = r"\b(?:Claude (?:Max|Pro|Team|Enterprise)|ChatGPT (?:Plus|Pro|Team|Business|Enterprise)|Copilot (?:Pro\+?|Business|Enterprise)|Gemini (?:Advanced|Pro|Ultra) plan)\b";
const USAGE: &str = r"(?i)\byou(?:'|’)ve (?:used \d+% of|hit|reached) your [\w-]+(?: [\w-]+)? limit\b|\b(?:usage|session|weekly|daily|monthly|\d+-hour) limit\b|\blimit (?:will )?resets?\b|\bresets (?:at |in |on )?(?:\d|(?:mon|tue|wed|thu|fri|sat|sun|jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\b)";
const ORG: &str = r"\S(?:'s|’s) (?:Organi[sz]ation|Workspace)\b|\bOrgani[sz]ation(?: name)?: *\S";
const SHORTEST_PASSED: usize = 4;
const GRID: f64 = 120.0;

pub struct Needles {
    homes: Vec<String>,
    hostname: Option<String>,
    user: Option<String>,
    email: regex::Regex,
    plan: regex::Regex,
    usage: regex::Regex,
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
    host_name(&text)
}

fn host_name(text: &str) -> Option<String> {
    let name = text.trim().to_string();
    (!name.is_empty() && name != "localhost" && name != "(none)").then_some(name)
}

impl Needles {
    pub fn of_this_machine(passed: &[(String, String)]) -> Needles {
        Needles::of(
            std::env::var("HOME").ok(),
            std::env::var("USER").ok(),
            std::env::var("LOGNAME").ok(),
            hostname(),
            passed,
        )
    }

    fn of(
        home: Option<String>,
        user: Option<String>,
        logname: Option<String>,
        hostname: Option<String>,
        passed: &[(String, String)],
    ) -> Needles {
        let mut homes: Vec<String> = Vec::new();
        if let Some(h) = home.as_deref().and_then(real_home) {
            if let Some(c) = std::fs::canonicalize(&h)
                .ok()
                .and_then(|c| real_home(&c.to_string_lossy()))
                .filter(|c| *c != h)
            {
                homes.push(c);
            }
            homes.push(h);
        }
        let user = user.filter(|u| !u.is_empty()).or(logname);
        Needles::new(homes, hostname, user, passed)
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
            usage: regex::Regex::new(USAGE).expect("the usage pattern"),
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
        if self.usage.is_match(text) {
            out.push("a usage limit".to_string());
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
    let mut spans: Vec<(usize, usize, &str)> = Vec::new();
    for (name, value) in passed.iter().filter(|(_, v)| !v.is_empty()) {
        let mut from = 0;
        while let Some(at) = text.get(from..).and_then(|rest| rest.find(value.as_str())) {
            let start = from + at;
            spans.push((start, start + value.len(), name.as_str()));
            from = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    let covered = |i: usize, (s, e, _): (usize, usize, &str)| {
        spans
            .iter()
            .enumerate()
            .any(|(j, &(js, je, _))| j != i && js <= s && e <= je && (je - js > e - s || j < i))
    };
    let mut kept: Vec<(usize, usize, &str)> = spans
        .iter()
        .enumerate()
        .filter(|&(i, &span)| !covered(i, span))
        .map(|(_, &span)| span)
        .collect();
    kept.sort_by_key(|&(s, e, _)| (s, std::cmp::Reverse(e)));
    let mut out = String::new();
    let mut cursor = 0;
    let mut last: Option<&str> = None;
    for (s, e, name) in kept {
        if s >= cursor {
            out.push_str(&text[cursor..s]);
            out.push_str(&format!("<{name}>"));
            last = Some(name);
        } else if last != Some(name) {
            out.push_str(&format!("<{name}>"));
            last = Some(name);
        }
        cursor = cursor.max(e);
    }
    out.push_str(&text[cursor..]);
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
            vec!["/srv/alpha".into()],
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
                (
                    0.1,
                    "export PATH=/srv/alpha/bin a@example.com s3cret-value\r\n",
                ),
                (0.2, "\x1b[H\x1b[2J"),
                (1.5, "/sr\x1b[1;31mv/al\x1b[0mpha/work\r\n"),
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
    fn a_plan_organisation_or_usage_label_is_found_unless_a_mask_hides_it() {
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
        for line in [
            "You've used 77% of your weekly limit",
            "You’ve hit your session limit · resets 3pm",
            "Approaching usage limit",
            "5-hour limit reached ∙ resets Oct 21",
            "Your limit will reset at 10pm",
        ] {
            assert_eq!(n.found(line), ["a usage limit"], "{line}");
        }
        for line in ["the cache resets the index", "a rate limit of 4 requests"] {
            assert!(n.found(line).is_empty(), "{line}");
        }
        let mut banner = cast(
            &[(0.1, "Opus 5.5 · Claude Max\r\n")],
            &[(0.0, "show"), (1.0, "end")],
        );
        assert_eq!(whats(&banner), [("a plan label".to_string(), 0.1)]);
        banner.meta = json!({"mask": [{"match": "Claude \\w+"}]});
        assert!(whats(&banner).is_empty());
        let mut usage = cast(
            &[(
                0.1,
                "\x1b[33mYou've used 77% of your weekly limit\x1b[0m\r\n",
            )],
            &[(0.0, "show"), (1.0, "end")],
        );
        assert_eq!(whats(&usage), [("a usage limit".to_string(), 0.1)]);
        usage.meta = json!({"mask": [{"match": "You've used .*limit"}]});
        assert!(whats(&usage).is_empty());
    }

    #[test]
    fn a_hostname_and_a_user_name_match_only_as_whole_words() {
        let n = needles();
        assert_eq!(n.found("on beta-box now"), ["the hostname"]);
        assert!(n.found("beta-boxes alphabet _alpha").is_empty());
        assert_eq!(n.found("(alpha)"), ["the user name"]);
        assert!(n.found("user@host").is_empty());
        assert_eq!(n.found("me@example.com"), ["an email"]);
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

    #[test]
    fn each_kind_of_private_text_is_named_and_neutral_text_is_not() {
        let n = needles();
        for (text, what) in [
            (
                "cd /srv/alpha/work",
                &["the home path", "the user name"][..],
            ),
            ("/srv/alphabet/x", &["the home path"]),
            ("user@beta-box:~$", &["the hostname"]),
            ("BETA-BOX ready", &["the hostname"]),
            ("logged in as alpha", &["the user name"]),
            ("write to ops@example.com", &["an email"]),
            ("Plan: ChatGPT Plus", &["a plan label"]),
            ("Copilot Pro+ enabled", &["a plan label"]),
            ("weekly limit reached", &["a usage limit"]),
            ("Organisation name: Acme", &["an organisation label"]),
            ("Bearer s3cret-value", &["a passed variable TOKEN"]),
        ] {
            assert_eq!(n.found(text), what, "{text}");
        }
        for neutral in [
            "",
            "demo show: 3 frames rendered in 0.42s",
            "the alphabet and the beta-boxes",
            "s3cret-valu",
            "a pro plan for the team",
            "limited edition",
        ] {
            assert!(
                n.found(neutral).is_empty(),
                "{neutral:?}: {:?}",
                n.found(neutral)
            );
        }
    }

    #[test]
    fn a_line_showing_several_kinds_names_each_once_in_a_fixed_order() {
        assert_eq!(
            needles().found("alpha@beta-box /srv/alpha a@example.com s3cret-value usage limit"),
            [
                "the home path",
                "the hostname",
                "the user name",
                "an email",
                "a usage limit",
                "a passed variable TOKEN"
            ]
        );
    }

    #[test]
    fn a_short_passed_value_or_user_name_is_not_looked_for() {
        let n = Needles::new(
            vec![],
            None,
            Some("abc".into()),
            &[
                ("SHORT".into(), "abc".into()),
                ("FOUR".into(), "wxyz".into()),
                ("EMPTY".into(), String::new()),
            ],
        );
        assert_eq!(
            n.found("abc wxyz"),
            ["the user name", "a passed variable FOUR"]
        );
        assert!(n.found("").is_empty());
        assert!(
            Needles::new(vec![], None, Some(String::new()), &[])
                .found("x")
                .is_empty()
        );
    }

    #[test]
    fn a_word_match_needs_a_non_word_character_or_an_edge_on_each_side() {
        for (text, hit) in [
            ("alpha", true),
            ("alpha!", true),
            ("-alpha-", true),
            ("alpha1", false),
            ("_alpha", false),
            ("ßalpha", false),
            ("alphaé", false),
            ("x alphax alpha", true),
        ] {
            assert_eq!(word(text, "alpha"), hit, "{text:?}");
        }
        assert!(!word("anything", ""));
    }

    #[test]
    fn a_home_needle_is_an_absolute_path_below_the_root() {
        assert_eq!(real_home("/srv/alpha/").as_deref(), Some("/srv/alpha"));
        assert_eq!(real_home("/alpha").as_deref(), Some("/alpha"));
        for none in ["/", "", "home/alpha", "~"] {
            assert_eq!(real_home(none), None, "{none:?}");
        }
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/test-private")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    #[test]
    fn this_machines_home_is_looked_for_as_given_and_where_its_link_leads() {
        let dir = test_dir("linked-home");
        std::fs::create_dir_all(dir.join("real")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
        let link = dir.join("link").display().to_string();
        let real = dir.join("real").display().to_string();
        let n = Needles::of(Some(format!("{link}/")), None, None, None, &[]);
        assert_eq!(n.homes, [real.as_str(), link.as_str()]);
        assert_eq!(n.found(&format!("cd {real}/x")), ["the home path"]);
        assert_eq!(n.found(&format!("cd {link}/x")), ["the home path"]);
        let n = Needles::of(Some(real.clone()), None, None, None, &[]);
        assert_eq!(n.homes, [real.as_str()]);
        let absent = dir.join("absent").display().to_string();
        assert_eq!(
            Needles::of(Some(absent.clone()), None, None, None, &[]).homes,
            [absent]
        );
        for none in ["/", "", "relative/home", "~"] {
            let n = Needles::of(Some(none.into()), None, None, None, &[]);
            assert!(n.homes.is_empty(), "{none:?}");
        }
        assert!(Needles::of(None, None, None, None, &[]).homes.is_empty());
    }

    #[test]
    fn this_machines_user_is_user_else_logname() {
        let user = |u: Option<&str>, l: Option<&str>| {
            Needles::of(
                None,
                u.map(str::to_string),
                l.map(str::to_string),
                None,
                &[],
            )
            .user
        };
        assert_eq!(user(Some("alpha"), Some("beta")).as_deref(), Some("alpha"));
        assert_eq!(user(Some(""), Some("beta")).as_deref(), Some("beta"));
        assert_eq!(user(None, Some("beta")).as_deref(), Some("beta"));
        assert_eq!(user(Some(""), None), None);
        assert_eq!(user(None, None), None);
        assert_eq!(user(Some("ab"), Some("beta")), None);
        let n = Needles::of(
            None,
            Some("".into()),
            Some("gamma".into()),
            Some("delta-box".into()),
            &[("K".into(), "passed-1".into())],
        );
        assert_eq!(
            n.found("gamma on delta-box with passed-1"),
            ["the hostname", "the user name", "a passed variable K"]
        );
        assert!(n.found("gammas on delta-boxes").is_empty());
    }

    #[test]
    fn a_generic_hostname_is_not_looked_for() {
        assert_eq!(host_name("beta-box\n").as_deref(), Some("beta-box"));
        for generic in ["localhost\n", "(none)", "", "  \n"] {
            assert_eq!(host_name(generic), None, "{generic:?}");
        }
    }

    #[test]
    fn redaction_replaces_exactly_each_passed_value_longest_first() {
        let passed = [
            ("KEY".to_string(), "abcd1234".to_string()),
            (
                "URL".to_string(),
                "https://site/?k=abcd1234&s=zz9".to_string(),
            ),
            ("EMPTY".to_string(), String::new()),
        ];
        assert_eq!(
            redact(
                "get https://site/?k=abcd1234&s=zz9 with abcd1234, abcd1234",
                &passed
            ),
            "get <URL> with <KEY>, <KEY>"
        );
        assert_eq!(redact("pre-abcd1234-post", &passed), "pre-<KEY>-post");
        assert_eq!(redact("abcd123 bcd1234", &passed), "abcd123 bcd1234");
        assert_eq!(redact("nothing here", &passed), "nothing here");
        assert_eq!(redact("abcd1234", &[]), "abcd1234");
    }

    #[test]
    fn overlapping_passed_values_are_both_hidden_and_a_mask_is_never_matched_again() {
        let pair = |a: &str, b: &str| {
            [
                ("A".to_string(), a.to_string()),
                ("B".to_string(), b.to_string()),
            ]
        };
        assert_eq!(redact("abcdXYZW", &pair("abcdXY", "XYZW")), "<A><B>");
        assert_eq!(redact("<abcdXYZW>", &pair("XYZW", "abcdXY")), "<<B><A>>");
        let named = [
            ("NAME_HOLDS_TOKEN".to_string(), "value-one".to_string()),
            ("T".to_string(), "TOKEN".to_string()),
        ];
        assert_eq!(
            redact("value-one and TOKEN", &named),
            "<NAME_HOLDS_TOKEN> and <T>"
        );
        let short = [("N".to_string(), "aa".to_string())];
        assert_eq!(redact("aaaa", &short), "<N>");
        assert_eq!(redact("aa-aa", &short), "<N>-<N>");
        let twice = pair("same", "same");
        assert_eq!(redact("x same y", &twice), "x <A> y");
        let wide = [("W".to_string(), "é1é".to_string())];
        assert_eq!(redact("aé1é1éb", &wide), "a<W>b");
        for (text, passed) in [
            ("abcdXYZW", pair("abcdXY", "XYZW")),
            ("value-one and TOKEN", named.clone()),
        ] {
            let hidden = redact(text, &passed);
            let bare = passed
                .iter()
                .fold(hidden.clone(), |h, (n, _)| h.replace(&format!("<{n}>"), ""));
            for (_, v) in &passed {
                assert!(!bare.contains(v.as_str()), "{v} in {hidden}");
            }
        }
    }

    #[test]
    fn a_finding_is_told_with_the_cast_kind_and_time_one_line_each() {
        let cast = Path::new("/nowhere/demo-clip-s.cast");
        let found = [
            Finding {
                what: "the hostname".into(),
                at: 1.254,
            },
            Finding {
                what: "an email".into(),
                at: 0.0,
            },
        ];
        let lines = lines(cast, &found);
        assert_eq!(
            lines,
            [
                "/nowhere/demo-clip-s.cast: the picture shows the hostname at 1.25s; hide it in the tape, mask it in terminal.toml or re-record",
                "/nowhere/demo-clip-s.cast: the picture shows an email at 0.00s; hide it in the tape, mask it in terminal.toml or re-record",
            ]
        );
        assert_eq!(refuse(cast, &[]), Ok(()));
        assert_eq!(
            refuse(cast, &found).unwrap_err(),
            format!("{}\nprec: {}", lines[0], lines[1])
        );
    }

    #[test]
    fn a_moment_is_placed_on_the_shown_timeline() {
        let segments = [(0.0, 1.0), (2.0, 3.5)];
        assert_eq!(shown_at(&segments, 0.0), Some(0.0));
        assert_eq!(shown_at(&segments, 0.5), Some(0.5));
        assert_eq!(shown_at(&segments, 2.5), Some(1.5));
        assert_eq!(shown_at(&segments, 1.5), None);
        assert_eq!(shown_at(&segments, 3.5), None);
        assert_eq!(shown_at(&[], 1.0), None);
    }

    #[test]
    fn a_mask_hides_exactly_what_it_matches_and_the_rest_is_still_found() {
        let mut c = cast(
            &[(0.1, "Claude Max · ops@example.com\r\n")],
            &[(0.0, "show"), (1.0, "end")],
        );
        assert_eq!(
            whats(&c),
            [
                ("an email".to_string(), 0.1),
                ("a plan label".to_string(), 0.1)
            ]
        );
        c.meta = json!({"mask": [{"match": "Claude \\w+"}]});
        assert_eq!(whats(&c), [("an email".to_string(), 0.1)]);
        c.meta = json!({"mask": [{"rect": [0, 0, 40, 1]}]});
        assert!(whats(&c).is_empty());
        c.meta = json!({"mask": [{"rect": [0, 1, 40, 1]}]});
        assert_eq!(whats(&c).len(), 2);
    }

    #[test]
    fn a_cast_with_nothing_on_screen_has_no_findings() {
        assert!(whats(&cast(&[], &[(0.0, "show"), (1.0, "end")])).is_empty());
        assert!(
            whats(&cast(
                &[(0.1, "neutral text\r\n")],
                &[(0.0, "show"), (1.0, "end")]
            ))
            .is_empty()
        );
    }
}
