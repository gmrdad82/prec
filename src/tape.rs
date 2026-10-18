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
    if v < 0.0 {
        return Err(format!("{s}: a duration is never negative"));
    }
    Ok(Duration::from_secs_f64(v * unit))
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
    match word.split_once('@') {
        Some((h, s)) => (h, Some(s)),
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
            .map(Key::CtrlAlt)
            .ok_or_else(|| format!("{k}: Ctrl+Alt takes one character")),
        (true, false) => one(last)
            .or(if last == "Space" { Some(' ') } else { None })
            .map(Key::Ctrl)
            .ok_or_else(|| format!("{k}: Ctrl takes one character")),
        (false, true) => Ok(Key::Alt(last.to_string())),
        _ => Err(format!(
            "{k}: a chord is Ctrl+, Alt+, Ctrl+Alt+ or Shift+Tab"
        )),
    }
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
        "Sleep" => Ok(Some(Step::Sleep(duration(rest)?))),
        "Hide" => Ok(Some(Step::Hide)),
        "Show" => Ok(Some(Step::Show)),
        "Screenshot" => Ok(Some(Step::Screenshot)),
        "Idle" => Ok(Some(Step::Idle {
            quiet: if rest.is_empty() {
                Duration::from_secs(2)
            } else {
                duration(rest)?
            },
            timeout: speed.unwrap_or(Duration::from_secs(300)),
        })),
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
        "Copy" | "Paste" => Err(format!(
            "{name}: the clipboard has no replay; type the text"
        )),
        n if n == "Wait" || n.starts_with("Wait+") => {
            let screen = n.ends_with("+Screen");
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
}
