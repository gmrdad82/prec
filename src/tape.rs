use std::path::Path;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Enter,
    Backspace,
    Delete,
    Insert,
    Tab,
    ShiftTab,
    Escape,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Ctrl(char),
    Alt(String),
    CtrlAlt(char),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Type {
        text: String,
        speed: Option<Duration>,
    },
    Key {
        key: Key,
        count: u32,
        speed: Option<Duration>,
    },
    Sleep(Duration),
    Hide,
    Show,
    Screenshot,
    Wait {
        pattern: String,
        timeout: Duration,
        screen: bool,
    },
    Idle {
        quiet: Duration,
        timeout: Duration,
    },
    Squeeze {
        gap: Option<Duration>,
        ignore: Option<String>,
    },
    Resize {
        grid: [u32; 2],
        over: Option<Duration>,
    },
}

pub fn duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, unit) = if let Some(n) = s.strip_suffix("ms") {
        (n, 0.001)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60.0)
    } else {
        (s, 1.0)
    };
    let v: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("{s}: a duration like 500ms, 1.5s or 2"))?;
    if !v.is_finite() {
        return Err(format!("{s}: a duration like 500ms, 1.5s or 2"));
    }
    if v < 0.0 {
        return Err(format!("{s}: a duration is never negative"));
    }
    Duration::try_from_secs_f64(v * unit).map_err(|_| format!("{s}: a duration too long"))
}

fn unquote(rest: &str) -> Result<String, String> {
    let rest = rest.trim();
    let mut chars = rest.chars();
    let q = chars.next().ok_or("Type needs a quoted string")?;
    if !matches!(q, '"' | '\'' | '`') {
        return Err(format!("Type {rest}: quote the text with \", ' or `"));
    }
    let body: String = chars.collect();
    let end = body
        .rfind(q)
        .ok_or_else(|| format!("Type {rest}: the closing {q} is missing"))?;
    if !body[end + 1..].trim().is_empty() {
        return Err(format!("Type {rest}: text after the closing quote"));
    }
    Ok(body[..end].to_string())
}

fn head(word: &str) -> (&str, Option<&str>) {
    let at = word
        .char_indices()
        .find(|&(i, c)| c == '@' && !word[..i].ends_with('+'))
        .map(|(i, _)| i);
    match at {
        Some(i) => (&word[..i], Some(&word[i + 1..])),
        None => (word, None),
    }
}

fn named(k: &str) -> Option<Key> {
    Some(match k {
        "Enter" => Key::Enter,
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Insert" => Key::Insert,
        "Tab" => Key::Tab,
        "Escape" => Key::Escape,
        "Space" => Key::Space,
        "Up" => Key::Up,
        "Down" => Key::Down,
        "Left" => Key::Left,
        "Right" => Key::Right,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        _ => return None,
    })
}

fn chord(k: &str) -> Result<Key, String> {
    let parts: Vec<&str> = k.split('+').collect();
    let last = *parts.last().unwrap_or(&"");
    let ctrl = parts.contains(&"Ctrl");
    let alt = parts.contains(&"Alt");
    let shift = parts.contains(&"Shift");
    if shift && last == "Tab" && !ctrl && !alt {
        return Ok(Key::ShiftTab);
    }
    let one = |s: &str| {
        let mut c = s.chars();
        match (c.next(), c.next()) {
            (Some(ch), None) => Some(ch),
            _ => None,
        }
    };
    match (ctrl, alt) {
        (true, true) => one(last)
            .ok_or_else(|| format!("{k}: Ctrl+Alt takes one character"))
            .and_then(|c| ctrl_key(k, c))
            .map(Key::CtrlAlt),
        (true, false) => one(last)
            .or(if last == "Space" { Some(' ') } else { None })
            .ok_or_else(|| format!("{k}: Ctrl takes one character"))
            .and_then(|c| ctrl_key(k, c))
            .map(Key::Ctrl),
        (false, true) => Ok(Key::Alt(last.to_string())),
        _ => Err(format!(
            "{k}: a chord is Ctrl+, Alt+, Ctrl+Alt+ or Shift+Tab"
        )),
    }
}

const CTRL_MARKS: &str = " @2[3\\4]5^6_/7?8";

fn ctrl_key(k: &str, c: char) -> Result<char, String> {
    if c.is_ascii_alphabetic() || CTRL_MARKS.contains(c) {
        Ok(c)
    } else {
        Err(format!(
            "{k}: Ctrl takes a letter, Space, one of @ [ \\ ] ^ _ / ? or a digit from 2 to 8"
        ))
    }
}

const MAX_REPEAT: u32 = 9999;

const RESIZE_FORM: &str = "Resize <cols>x<rows> [duration]";

pub const SIDES: std::ops::RangeInclusive<u32> = 2..=1000;

fn resize(t: &str, rest: &str) -> Result<Step, String> {
    let mut words = rest.split_whitespace();
    let size = words.next().unwrap_or_default();
    let over = words.next();
    if words.next().is_some() {
        return Err(format!("{t}: takes {RESIZE_FORM}"));
    }
    let side = |s: &str| {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("{t}: takes {RESIZE_FORM}"));
        }
        s.parse::<u32>()
            .ok()
            .filter(|n| SIDES.contains(n))
            .ok_or_else(|| {
                format!(
                    "{t}: each side is {} to {}, as in {RESIZE_FORM}",
                    SIDES.start(),
                    SIDES.end()
                )
            })
    };
    let (c, r) = size
        .split_once('x')
        .ok_or_else(|| format!("{t}: takes {RESIZE_FORM}"))?;
    let grid = [side(c)?, side(r)?];
    let over = over
        .map(|d| duration(d).map_err(|e| format!("{t}: {e}, as in {RESIZE_FORM}")))
        .transpose()?
        .filter(|d| !d.is_zero());
    Ok(Step::Resize { grid, over })
}

pub fn schedule(from: [u32; 2], to: [u32; 2], over: Duration) -> Vec<(Duration, [u32; 2])> {
    let delta = [0, 1].map(|i| to[i] as i64 - from[i] as i64);
    let n = delta[0].unsigned_abs().max(delta[1].unsigned_abs());
    (1..=n)
        .map(|i| {
            if i == n {
                return (over, to);
            }
            let f = i as f64 / n as f64;
            let u = (1.0 - 2.0 * f).acos() / std::f64::consts::PI;
            let grid =
                [0, 1].map(|k| (from[k] as i64 + (delta[k] as f64 * f).round() as i64) as u32);
            (over.mul_f64(u), grid)
        })
        .collect()
}

fn bare(t: &str, name: &str, rest: &str, speed: Option<Duration>) -> Result<(), String> {
    if speed.is_some() {
        return Err(format!("{t}: {name} takes no @speed"));
    }
    if !rest.is_empty() {
        return Err(format!("{t}: {name} takes nothing after it"));
    }
    Ok(())
}

pub fn parse_line(line: &str) -> Result<Option<Step>, String> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let (word, rest) = match t.split_once(char::is_whitespace) {
        Some((w, r)) => (w, r.trim()),
        None => (t, ""),
    };
    let (name, speed) = head(word);
    let speed = speed.map(duration).transpose()?;
    match name {
        "Output" | "Set" | "Require" | "Env" | "Source" => Err(format!(
            "{name}: a tape holds steps only; the settings come from terminal.toml"
        )),
        "Type" => Ok(Some(Step::Type {
            text: unquote(rest)?,
            speed,
        })),
        "Sleep" if speed.is_some() => Err(format!("{t}: Sleep takes no @speed")),
        "Sleep" => Ok(Some(Step::Sleep(duration(rest)?))),
        "Hide" => bare(t, name, rest, speed).map(|_| Some(Step::Hide)),
        "Show" => bare(t, name, rest, speed).map(|_| Some(Step::Show)),
        "Screenshot" => bare(t, name, rest, speed).map(|_| Some(Step::Screenshot)),
        "Idle" => Ok(Some(Step::Idle {
            quiet: if rest.is_empty() {
                Duration::from_secs(2)
            } else {
                duration(rest)?
            },
            timeout: speed.unwrap_or(Duration::from_secs(300)),
        })),
        "Squeeze" if speed.is_some() => Err(format!("{t}: Squeeze takes no @speed")),
        "Squeeze" if rest == "off" => Ok(Some(Step::Squeeze {
            gap: None,
            ignore: None,
        })),
        "Squeeze" => {
            let (gap, ignore) = match rest.split_once('/') {
                Some((g, r)) => (g.trim(), Some(slashed(t, &format!("/{r}"))?)),
                None => (rest, None),
            };
            Ok(Some(Step::Squeeze {
                gap: Some(if gap.is_empty() {
                    Duration::from_millis(500)
                } else {
                    duration(gap)?
                }),
                ignore,
            }))
        }
        "Resize" if speed.is_some() => Err(format!("{t}: Resize takes no @speed")),
        "Resize" => resize(t, rest).map(Some),
        "Copy" | "Paste" => Err(format!(
            "{name}: the clipboard has no replay; type the text"
        )),
        n if n == "Wait" || n.starts_with("Wait+") => {
            let screen = match n {
                "Wait" | "Wait+Line" => false,
                "Wait+Screen" => true,
                _ => return Err(format!("{t}: a Wait is Wait, Wait+Line or Wait+Screen")),
            };
            Ok(Some(Step::Wait {
                pattern: slashed(t, rest)?,
                timeout: speed.unwrap_or(Duration::from_secs(15)),
                screen,
            }))
        }
        n => {
            let key = match named(n) {
                Some(k) => k,
                None if n.contains('+') => chord(n)?,
                None => return Err(format!("{t}: an unknown step")),
            };
            let count = if rest.is_empty() {
                1
            } else {
                rest.parse::<u32>()
                    .map_err(|_| format!("{t}: the repeat count is a number"))?
            };
            if !(1..=MAX_REPEAT).contains(&count) {
                return Err(format!("{t}: the repeat count is 1 to {MAX_REPEAT}"));
            }
            Ok(Some(Step::Key { key, count, speed }))
        }
    }
}

fn slashed(t: &str, rest: &str) -> Result<String, String> {
    let pattern = rest
        .trim()
        .strip_prefix('/')
        .and_then(|p| p.strip_suffix('/'))
        .ok_or_else(|| format!("{t}: takes /regex/"))?
        .to_string();
    regex::Regex::new(&pattern).map_err(|e| format!("{t}: {e}"))?;
    Ok(pattern)
}

pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut out = vec![];
    for (i, line) in text.lines().enumerate() {
        if let Some(s) = parse_line(line).map_err(|e| format!("line {}: {e}", i + 1))? {
            out.push(s);
        }
    }
    Ok(out)
}

pub fn load(path: &Path) -> Result<Vec<Step>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn bytes(key: &Key, app_cursor: bool) -> Vec<u8> {
    let arrow = |c: u8| {
        if app_cursor {
            vec![0x1b, b'O', c]
        } else {
            vec![0x1b, b'[', c]
        }
    };
    match key {
        Key::Enter => b"\r".to_vec(),
        Key::Backspace => vec![0x7f],
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::Insert => b"\x1b[2~".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::ShiftTab => b"\x1b[Z".to_vec(),
        Key::Escape => vec![0x1b],
        Key::Space => b" ".to_vec(),
        Key::Up => arrow(b'A'),
        Key::Down => arrow(b'B'),
        Key::Right => arrow(b'C'),
        Key::Left => arrow(b'D'),
        Key::Home => arrow(b'H'),
        Key::End => arrow(b'F'),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::Ctrl(c) => vec![ctrl_byte(*c)],
        Key::CtrlAlt(c) => vec![0x1b, ctrl_byte(*c)],
        Key::Alt(k) => {
            let mut v = vec![0x1b];
            match named(k) {
                Some(n) => v.extend(bytes(&n, app_cursor)),
                None => v.extend(k.as_bytes()),
            }
            v
        }
    }
}

fn ctrl_byte(c: char) -> u8 {
    match c {
        ' ' | '@' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '/' | '7' => 0x1f,
        '?' | '8' => 0x7f,
        c => (c.to_ascii_uppercase() as u8) & 0x1f,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_read_like_vhs() {
        let s = parse("Type \"demo tui\"\nEnter\nSleep 2s\nDown@200ms 3\nCtrl+C\nType@0ms `a \"b\"`\n# note\nHide\nShow\nScreenshot\nWait+Screen@5s /ready/\nShift+Tab\nAlt+Enter\nIdle@10m 3s\nIdle\nSqueeze\nSqueeze 2s /^. \\w+… /\nSqueeze off\n").unwrap();
        assert_eq!(s.len(), 17);
        assert_eq!(
            s[14],
            Step::Squeeze {
                gap: Some(Duration::from_millis(500)),
                ignore: None
            }
        );
        assert_eq!(
            s[15],
            Step::Squeeze {
                gap: Some(Duration::from_secs(2)),
                ignore: Some(r"^. \w+… ".into())
            }
        );
        assert_eq!(
            s[16],
            Step::Squeeze {
                gap: None,
                ignore: None
            }
        );
        assert!(parse("Squeeze 1s /(/\n").is_err());
        assert_eq!(
            s[12],
            Step::Idle {
                quiet: Duration::from_secs(3),
                timeout: Duration::from_secs(600)
            }
        );
        assert_eq!(
            s[13],
            Step::Idle {
                quiet: Duration::from_secs(2),
                timeout: Duration::from_secs(300)
            }
        );
        assert_eq!(
            s[0],
            Step::Type {
                text: "demo tui".into(),
                speed: None
            }
        );
        assert_eq!(s[2], Step::Sleep(Duration::from_secs(2)));
        assert_eq!(
            s[3],
            Step::Key {
                key: Key::Down,
                count: 3,
                speed: Some(Duration::from_millis(200))
            }
        );
        assert_eq!(bytes(&Key::Ctrl('c'), false), vec![3]);
        assert_eq!(
            s[5],
            Step::Type {
                text: "a \"b\"".into(),
                speed: Some(Duration::ZERO)
            }
        );
        assert_eq!(bytes(&Key::Up, true), b"\x1bOA".to_vec());
        assert_eq!(bytes(&Key::Up, false), b"\x1b[A".to_vec());
        assert!(parse("Set Width 10\n").is_err());
        assert!(parse("Frobnicate\n").is_err());
        assert_eq!(duration("1.5s").unwrap(), Duration::from_millis(1500));
    }

    fn one(line: &str) -> Step {
        parse_line(line).unwrap().unwrap()
    }

    fn refused(text: &str) -> String {
        parse(text).unwrap_err()
    }

    #[test]
    fn every_named_key_parses_with_its_count_and_speed() {
        let keys = [
            ("Enter", Key::Enter),
            ("Backspace", Key::Backspace),
            ("Delete", Key::Delete),
            ("Insert", Key::Insert),
            ("Tab", Key::Tab),
            ("Escape", Key::Escape),
            ("Space", Key::Space),
            ("Up", Key::Up),
            ("Down", Key::Down),
            ("Left", Key::Left),
            ("Right", Key::Right),
            ("Home", Key::Home),
            ("End", Key::End),
            ("PageUp", Key::PageUp),
            ("PageDown", Key::PageDown),
        ];
        for (word, key) in keys {
            assert_eq!(
                one(word),
                Step::Key {
                    key: key.clone(),
                    count: 1,
                    speed: None
                },
                "{word}"
            );
            assert_eq!(
                one(&format!("  {word}@10ms 4\r")),
                Step::Key {
                    key,
                    count: 4,
                    speed: Some(Duration::from_millis(10))
                },
                "{word}"
            );
        }
    }

    #[test]
    fn chords_name_ctrl_alt_and_shift_tab() {
        let key = |line: &str| match one(line) {
            Step::Key { key, .. } => key,
            s => panic!("{line}: {s:?}"),
        };
        assert_eq!(key("Ctrl+c"), Key::Ctrl('c'));
        assert_eq!(key("Ctrl+Space"), Key::Ctrl(' '));
        assert_eq!(key("Ctrl+Alt+x"), Key::CtrlAlt('x'));
        assert_eq!(key("Alt+Up"), Key::Alt("Up".into()));
        assert_eq!(key("Alt+x"), Key::Alt("x".into()));
        assert_eq!(key("Shift+Tab"), Key::ShiftTab);
        assert!(refused("Ctrl+ab\n").contains("Ctrl takes one character"));
        assert!(refused("Ctrl+Alt+ab\n").contains("Ctrl+Alt takes one character"));
        assert!(refused("Shift+A\n").contains("a chord is Ctrl+, Alt+, Ctrl+Alt+ or Shift+Tab"));
        assert!(refused("Shift+Tab x\n").contains("the repeat count is a number"));
    }

    #[test]
    fn type_takes_any_of_three_quotes_and_refuses_the_rest() {
        assert_eq!(
            one("Type 'it''s'"),
            Step::Type {
                text: "it''s".into(),
                speed: None
            }
        );
        assert_eq!(
            one("Type@1s \"\""),
            Step::Type {
                text: String::new(),
                speed: Some(Duration::from_secs(1))
            }
        );
        assert!(refused("Type\n").contains("Type needs a quoted string"));
        assert!(refused("Type plain\n").contains("quote the text"));
        assert!(refused("Type \"open\n").contains("the closing \" is missing"));
        assert!(refused("Type \"a\" b\n").contains("text after the closing quote"));
    }

    #[test]
    fn wait_takes_a_regex_a_timeout_and_the_whole_screen() {
        assert_eq!(
            one("Wait /ready$/"),
            Step::Wait {
                pattern: "ready$".into(),
                timeout: Duration::from_secs(15),
                screen: false
            }
        );
        assert_eq!(
            one("Wait+Screen@2s /a\u{a0}b/"),
            Step::Wait {
                pattern: "a\u{a0}b".into(),
                timeout: Duration::from_secs(2),
                screen: true
            }
        );
        assert_eq!(
            one("Wait@500ms /x/"),
            Step::Wait {
                pattern: "x".into(),
                timeout: Duration::from_millis(500),
                screen: false
            }
        );
        assert!(refused("Wait\n").contains("Wait: takes /regex/"));
        assert!(refused("Wait ready\n").contains("takes /regex/"));
        assert!(refused("Wait /ready\n").contains("takes /regex/"));
        assert!(refused("Wait /(/\n").starts_with("line 1: Wait /(/: "));
        assert!(refused("Wait@soon /x/\n").contains("soon: a duration like 500ms, 1.5s or 2"));
        assert!(refused("Wait@ /x/\n").contains("a duration like"));
    }

    #[test]
    fn sleep_idle_and_squeeze_read_their_durations() {
        assert_eq!(one("Sleep 2"), Step::Sleep(Duration::from_secs(2)));
        assert_eq!(one("Sleep 1m"), Step::Sleep(Duration::from_secs(60)));
        assert_eq!(one("Sleep 250ms"), Step::Sleep(Duration::from_millis(250)));
        assert!(refused("Sleep\n").contains("a duration like"));
        assert!(refused("Sleep -1s\n").contains("a duration is never negative"));
        assert!(refused("Sleep fast\n").contains("fast: a duration like"));
        assert!(refused("Idle@x 1s\n").contains("a duration like"));
        assert!(refused("Idle forever\n").contains("a duration like"));
        assert_eq!(
            one("Squeeze /^spin/"),
            Step::Squeeze {
                gap: Some(Duration::from_millis(500)),
                ignore: Some("^spin".into())
            }
        );
        assert_eq!(
            one("Squeeze 1s"),
            Step::Squeeze {
                gap: Some(Duration::from_secs(1)),
                ignore: None
            }
        );
        assert!(refused("Squeeze soon\n").contains("a duration like"));
        assert!(refused("Squeeze 1s /open\n").contains("takes /regex/"));
    }

    #[test]
    fn a_refusal_names_its_line_and_why() {
        assert_eq!(
            refused("Enter\n\n# a note\nFrobnicate\n"),
            "line 4: Frobnicate: an unknown step"
        );
        assert_eq!(
            refused("Enter\nCopy\n"),
            "line 2: Copy: the clipboard has no replay; type the text"
        );
        assert!(refused("Paste\n").starts_with("line 1: Paste: the clipboard"));
        for word in ["Output", "Set", "Require", "Env", "Source"] {
            assert_eq!(
                refused(&format!("Hide\n{word} x\n")),
                format!(
                    "line 2: {word}: a tape holds steps only; the settings come from terminal.toml"
                )
            );
        }
        assert!(refused("Down -1\n").starts_with("line 1: Down -1: the repeat count"));
        assert!(refused("Enter@\n").starts_with("line 1: "));
    }

    #[test]
    fn an_empty_tape_has_no_steps_and_a_tape_may_end_hidden() {
        assert_eq!(parse("").unwrap(), vec![]);
        assert_eq!(parse("\n   \n# only notes\n\t\n").unwrap(), vec![]);
        let s = parse("Show\nType \"a\"\nHide\n").unwrap();
        assert_eq!(s.first(), Some(&Step::Show));
        assert_eq!(s.last(), Some(&Step::Hide));
        assert_eq!(parse("Screenshot").unwrap(), vec![Step::Screenshot]);
    }

    #[test]
    fn every_key_sends_the_bytes_a_terminal_sends() {
        let cases: [(Key, &[u8], &[u8]); 16] = [
            (Key::Enter, b"\r", b"\r"),
            (Key::Backspace, b"\x7f", b"\x7f"),
            (Key::Delete, b"\x1b[3~", b"\x1b[3~"),
            (Key::Insert, b"\x1b[2~", b"\x1b[2~"),
            (Key::Tab, b"\t", b"\t"),
            (Key::ShiftTab, b"\x1b[Z", b"\x1b[Z"),
            (Key::Escape, b"\x1b", b"\x1b"),
            (Key::Space, b" ", b" "),
            (Key::Up, b"\x1b[A", b"\x1bOA"),
            (Key::Down, b"\x1b[B", b"\x1bOB"),
            (Key::Right, b"\x1b[C", b"\x1bOC"),
            (Key::Left, b"\x1b[D", b"\x1bOD"),
            (Key::Home, b"\x1b[H", b"\x1bOH"),
            (Key::End, b"\x1b[F", b"\x1bOF"),
            (Key::PageUp, b"\x1b[5~", b"\x1b[5~"),
            (Key::PageDown, b"\x1b[6~", b"\x1b[6~"),
        ];
        for (key, normal, app) in cases {
            assert_eq!(bytes(&key, false), normal, "{key:?}");
            assert_eq!(bytes(&key, true), app, "{key:?}");
        }
        assert_eq!(bytes(&Key::Alt("x".into()), false), b"\x1bx");
        assert_eq!(bytes(&Key::Alt("Enter".into()), false), b"\x1b\r");
        assert_eq!(bytes(&Key::Alt("Up".into()), true), b"\x1b\x1bOA");
        assert_eq!(bytes(&Key::CtrlAlt('c'), false), vec![0x1b, 3]);
    }

    #[test]
    fn ctrl_maps_letters_and_the_punctuation_terminals_know() {
        for (c, b) in [
            ('a', 1),
            ('C', 3),
            ('z', 26),
            (' ', 0),
            ('@', 0),
            ('2', 0),
            ('[', 0x1b),
            ('3', 0x1b),
            ('\\', 0x1c),
            ('4', 0x1c),
            (']', 0x1d),
            ('5', 0x1d),
            ('^', 0x1e),
            ('6', 0x1e),
            ('_', 0x1f),
            ('/', 0x1f),
            ('7', 0x1f),
            ('?', 0x7f),
            ('8', 0x7f),
        ] {
            assert_eq!(bytes(&Key::Ctrl(c), false), vec![b], "Ctrl+{c}");
        }
    }

    #[test]
    fn a_duration_that_is_not_finite_or_too_long_is_refused_by_its_line() {
        for v in ["inf", "infinity", "nan", "NaN", "-inf"] {
            assert_eq!(
                refused(&format!("Enter\nSleep {v}\n")),
                format!("line 2: {v}: a duration like 500ms, 1.5s or 2"),
                "{v}"
            );
        }
        assert_eq!(refused("Sleep 1e30\n"), "line 1: 1e30: a duration too long");
        assert_eq!(
            refused("Sleep 1e30m\n"),
            "line 1: 1e30m: a duration too long"
        );
        assert!(refused("Wait@infs /x/\n").starts_with("line 1: infs: a duration like"));
        assert!(refused("Idle@nan 1s\n").starts_with("line 1: nan: a duration like"));
        assert!(refused("Idle 1e300\n").starts_with("line 1: 1e300: a duration too long"));
        assert!(refused("Squeeze nan\n").starts_with("line 1: nan: a duration like"));
        assert!(refused("Type@1e300s \"a\"\n").starts_with("line 1: 1e300s: a duration too long"));
        assert!(refused("Down@1e300ms\n").starts_with("line 1: 1e300ms: a duration too long"));
        assert_eq!(duration("1e3ms").unwrap(), Duration::from_secs(1));
        assert_eq!(duration("0").unwrap(), Duration::ZERO);
    }

    #[test]
    fn a_wait_takes_only_the_line_or_the_screen_modifier() {
        let wait = |line: &str| match one(line) {
            Step::Wait { screen, .. } => screen,
            s => panic!("{line}: {s:?}"),
        };
        assert!(!wait("Wait /x/"));
        assert!(!wait("Wait+Line /x/"));
        assert!(!wait("Wait+Line@2s /x/"));
        assert!(wait("Wait+Screen /x/"));
        for bad in [
            "Wait+Scren /x/",
            "Wait+Bogus@1s /x/",
            "Wait+ /x/",
            "Wait+Screen+Line /x/",
        ] {
            let e = refused(&format!("Hide\n{bad}\n"));
            assert!(
                e.starts_with("line 2: ")
                    && e.ends_with(": a Wait is Wait, Wait+Line or Wait+Screen"),
                "{bad}: {e}"
            );
        }
    }

    #[test]
    fn a_step_with_no_arguments_refuses_what_follows_it_and_a_speed() {
        for line in ["Hide now", "Show x", "Screenshot out.png"] {
            let name = line.split(' ').next().unwrap();
            assert_eq!(
                refused(&format!("Enter\n{line}\n")),
                format!("line 2: {line}: {name} takes nothing after it")
            );
        }
        for (line, name) in [
            ("Hide@1s", "Hide"),
            ("Show@1s", "Show"),
            ("Screenshot@1s", "Screenshot"),
            ("Sleep@1s 2", "Sleep"),
            ("Squeeze@1s", "Squeeze"),
            ("Squeeze@1s off", "Squeeze"),
        ] {
            assert_eq!(
                refused(&format!("{line}\n")),
                format!("line 1: {line}: {name} takes no @speed")
            );
        }
        assert_eq!(one("  Hide  "), Step::Hide);
        assert_eq!(
            one("Squeeze off"),
            Step::Squeeze {
                gap: None,
                ignore: None
            }
        );
    }

    #[test]
    fn ctrl_refuses_a_character_no_terminal_sends() {
        for k in [
            "Ctrl+\u{e9}",
            "Ctrl+\u{4e09}",
            "Ctrl+1",
            "Ctrl+9",
            "Ctrl+0",
            "Ctrl+!",
            "Ctrl+Alt+1",
            "Ctrl+Alt+\u{e9}",
        ] {
            let e = refused(&format!("Enter\n{k}\n"));
            assert_eq!(
                e,
                format!(
                    "line 2: {k}: Ctrl takes a letter, Space, one of @ [ \\ ] ^ _ / ? or a digit from 2 to 8"
                ),
                "{k}"
            );
        }
        for c in "@2[3\\4]5^6_/7?8azAZ".chars() {
            assert_eq!(
                one(&format!("Ctrl+{c}")),
                Step::Key {
                    key: Key::Ctrl(c),
                    count: 1,
                    speed: None
                },
                "Ctrl+{c}"
            );
            assert_eq!(
                one(&format!("Ctrl+Alt+{c}")),
                Step::Key {
                    key: Key::CtrlAlt(c),
                    count: 1,
                    speed: None
                },
                "Ctrl+Alt+{c}"
            );
        }
        assert_eq!(
            one("Ctrl+@@100ms 2"),
            Step::Key {
                key: Key::Ctrl('@'),
                count: 2,
                speed: Some(Duration::from_millis(100))
            }
        );
        assert_eq!(bytes(&Key::Ctrl('@'), false), vec![0]);
        assert_eq!(bytes(&Key::CtrlAlt('@'), false), vec![0x1b, 0]);
        assert_eq!(
            refused("Enter\nCtrl+@@\n"),
            "line 2: : a duration like 500ms, 1.5s or 2"
        );
    }

    #[test]
    fn a_repeat_count_is_one_to_its_maximum() {
        assert_eq!(
            one("Down 9999"),
            Step::Key {
                key: Key::Down,
                count: 9999,
                speed: None
            }
        );
        for count in ["0", "10000", "4000000000"] {
            assert_eq!(
                refused(&format!("Enter\nDown {count}\n")),
                format!("line 2: Down {count}: the repeat count is 1 to 9999"),
                "{count}"
            );
        }
        assert!(refused("Enter 0\n").ends_with("the repeat count is 1 to 9999"));
        assert!(refused("Ctrl+C 0\n").ends_with("the repeat count is 1 to 9999"));
        assert!(refused("Down 99999999999\n").ends_with("the repeat count is a number"));
    }

    #[test]
    fn a_tape_file_is_read_and_its_refusals_name_the_file() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp/unit-tape-load");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.tape");
        std::fs::write(&good, "Type \"demo\"\nEnter\n").unwrap();
        assert_eq!(load(&good).unwrap().len(), 2);
        let bad = dir.join("bad.tape");
        std::fs::write(&bad, "Enter\nSleep never\n").unwrap();
        let e = load(&bad).unwrap_err();
        assert!(
            e.starts_with(&format!("{}: line 2: ", bad.display())),
            "{e}"
        );
        let gone = dir.join("gone.tape");
        assert!(
            load(&gone)
                .unwrap_err()
                .starts_with(&gone.display().to_string())
        );
    }

    #[test]
    fn a_resize_takes_a_size_and_an_optional_travel_time() {
        assert_eq!(
            one("Resize 60x16"),
            Step::Resize {
                grid: [60, 16],
                over: None
            }
        );
        assert_eq!(
            one("  Resize 38x16 3s  "),
            Step::Resize {
                grid: [38, 16],
                over: Some(Duration::from_secs(3))
            }
        );
        assert_eq!(
            one("Resize 2x1000 250ms"),
            Step::Resize {
                grid: [2, 1000],
                over: Some(Duration::from_millis(250))
            }
        );
        assert_eq!(
            one("Resize 1000x2 0s"),
            Step::Resize {
                grid: [1000, 2],
                over: None
            }
        );
        let s = parse("Sleep 1.2s\nResize 38x16 3s\nSleep 1.5s\nResize 96x16 3s\nSleep 1.2s\n")
            .unwrap();
        assert_eq!(s.len(), 5);
        assert_eq!(
            s[3],
            Step::Resize {
                grid: [96, 16],
                over: Some(Duration::from_secs(3))
            }
        );
    }

    #[test]
    fn a_resize_refuses_a_bad_size_a_side_out_of_range_a_bad_time_and_a_speed() {
        let form = "Resize <cols>x<rows> [duration]";
        for bad in [
            "Resize",
            "Resize 60",
            "Resize 60x",
            "Resize x16",
            "Resize 60*16",
            "Resize 60X16",
            "Resize 60x16x2",
            "Resize -60x16",
            "Resize +60x16",
            "Resize 6.5x16",
            "Resize sixtyx16",
            "Resize 60x16 1s now",
        ] {
            assert_eq!(
                refused(&format!("Enter\n{bad}\n")),
                format!("line 2: {bad}: takes {form}"),
                "{bad}"
            );
        }
        for bad in [
            "Resize 0x16",
            "Resize 60x0",
            "Resize 1x16",
            "Resize 60x1",
            "Resize 1001x16",
            "Resize 60x1001",
            "Resize 99999999999x16",
        ] {
            assert_eq!(
                refused(&format!("{bad}\n")),
                format!("line 1: {bad}: each side is 2 to 1000, as in {form}"),
                "{bad}"
            );
        }
        assert_eq!(
            refused("Resize 60x16 soon\n"),
            format!(
                "line 1: Resize 60x16 soon: soon: a duration like 500ms, 1.5s or 2, as in {form}"
            )
        );
        assert_eq!(
            refused("Resize 60x16 -1s\n"),
            format!("line 1: Resize 60x16 -1s: -1s: a duration is never negative, as in {form}")
        );
        assert_eq!(
            refused("Resize@1s 60x16\n"),
            "line 1: Resize@1s 60x16: Resize takes no @speed"
        );
    }

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1e3
    }

    #[test]
    fn a_resize_travels_through_every_whole_step_on_an_eased_curve() {
        let over = Duration::from_secs(2);
        let s = schedule([96, 16], [38, 16], over);
        assert_eq!(s.len(), 58);
        let cols: Vec<u32> = s.iter().map(|(_, g)| g[0]).collect();
        assert_eq!(cols, (38..96).rev().collect::<Vec<u32>>());
        assert!(s.iter().all(|(_, g)| g[1] == 16));
        assert_eq!(s.last().unwrap(), &(over, [38, 16]));
        assert!(s.windows(2).all(|w| w[0].0 < w[1].0), "{s:?}");
        let gaps: Vec<f64> = std::iter::once(ms(s[0].0))
            .chain(s.windows(2).map(|w| ms(w[1].0) - ms(w[0].0)))
            .collect();
        let mid = gaps[gaps.len() / 2];
        assert!(gaps[0] > 3.0 * mid, "{gaps:?}");
        assert!(gaps[gaps.len() - 1] > 3.0 * mid, "{gaps:?}");
        assert!((ms(s[28].0) - 1000.0).abs() < 30.0, "{:?}", s[28]);
        let half: Vec<f64> = gaps[..29].to_vec();
        assert!(half.windows(2).all(|w| w[0] >= w[1] - 1e-9), "{half:?}");
        let u1 = (1.0 - 2.0 / 58.0f64).acos() / std::f64::consts::PI;
        assert!((s[0].0.as_secs_f64() - 2.0 * u1).abs() < 1e-9);
    }

    #[test]
    fn a_resize_moves_both_sides_on_one_curve_and_grows_as_it_shrinks() {
        let grow = schedule([20, 6], [40, 10], Duration::from_secs(1));
        assert_eq!(grow.len(), 20);
        assert_eq!(
            grow.iter().map(|(_, g)| g[0]).collect::<Vec<u32>>(),
            (21..=40).collect::<Vec<u32>>()
        );
        let rows: Vec<u32> = grow.iter().map(|(_, g)| g[1]).collect();
        assert!(rows.windows(2).all(|w| w[0] <= w[1]), "{rows:?}");
        assert_eq!((rows[0], rows[19]), (6, 10));
        for r in 7..=10 {
            assert!(rows.contains(&r), "{rows:?}");
        }
        let shrink = schedule([40, 10], [20, 6], Duration::from_secs(1));
        assert_eq!(
            shrink.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
            grow.iter().map(|(t, _)| *t).collect::<Vec<_>>()
        );
        assert_eq!(shrink[0].1, [39, 10]);
        assert_eq!(shrink[19], (Duration::from_secs(1), [20, 6]));
        let tall = schedule([10, 4], [12, 20], Duration::from_secs(1));
        assert_eq!(tall.len(), 16);
        assert_eq!(
            tall.iter().map(|(_, g)| g[1]).collect::<Vec<u32>>(),
            (5..=20).collect::<Vec<u32>>()
        );
        assert_eq!(tall.last().unwrap().1, [12, 20]);
    }

    #[test]
    fn a_resize_to_the_same_grid_has_no_steps_and_one_step_lands_at_the_end() {
        assert!(schedule([60, 16], [60, 16], Duration::from_secs(3)).is_empty());
        assert_eq!(
            schedule([60, 16], [61, 16], Duration::from_secs(3)),
            vec![(Duration::from_secs(3), [61, 16])]
        );
        assert_eq!(
            schedule([60, 16], [60, 15], Duration::from_millis(5)),
            vec![(Duration::from_millis(5), [60, 15])]
        );
        let two = schedule([2, 2], [4, 2], Duration::from_secs(1));
        assert_eq!(
            two,
            vec![
                (Duration::from_millis(500), [3, 2]),
                (Duration::from_secs(1), [4, 2])
            ]
        );
    }
}
