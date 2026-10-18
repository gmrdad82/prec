use crate::tape::{self, Step};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Output,
    Input,
    Marker,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub t: f64,
    pub kind: Kind,
    pub data: String,
}

#[derive(Clone, Debug)]
pub struct Cast {
    pub cols: u32,
    pub rows: u32,
    pub meta: Value,
    pub events: Vec<Event>,
}

pub struct Spec<'a> {
    pub grid: [u32; 2],
    pub env: &'a [(String, String)],
    pub path: &'a str,
    pub cwd: &'a Path,
    pub ns: Option<&'a crate::home::Ns>,
    pub prompt: &'a str,
    pub launch: Option<&'a str>,
    pub settle: Duration,
    pub typing: Duration,
    pub steps: &'a [Step],
    pub still: bool,
}

struct Utf8 {
    pending: Vec<u8>,
}

impl Utf8 {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    out.push_str(s);
                    self.pending.clear();
                    return out;
                }
                Err(e) => {
                    let good = e.valid_up_to();
                    out.push_str(std::str::from_utf8(&self.pending[..good]).unwrap_or_default());
                    match e.error_len() {
                        None => {
                            self.pending.drain(..good);
                            return out;
                        }
                        Some(bad) => {
                            out.push('\u{fffd}');
                            self.pending.drain(..good + bad);
                        }
                    }
                }
            }
        }
    }
}

struct Screen {
    parser: vt100::Parser,
}

impl Screen {
    fn new(grid: [u32; 2]) -> Screen {
        Screen {
            parser: vt100::Parser::new(grid[1] as u16, grid[0] as u16, 0),
        }
    }

    fn feed(&mut self, s: &str) {
        self.parser.process(s.as_bytes());
    }

    fn app_cursor(&self) -> bool {
        self.parser.screen().application_cursor()
    }

    fn shown(&self) -> String {
        displayed(&self.parser.screen().contents())
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| format!("{:>3} | {}", i + 1, l.trim_end()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn picture(&self) -> Vec<u8> {
        self.parser.screen().contents_formatted()
    }

    fn matches(&self, re: &regex::Regex, screen: bool) -> bool {
        let text = displayed(&self.parser.screen().contents());
        if screen {
            return re.is_match(&text);
        }
        text.lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| re.is_match(l))
    }
}

fn displayed(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}'))
        .map(|c| {
            if c != '\n' && c.is_whitespace() {
                ' '
            } else {
                c
            }
        })
        .collect()
}

struct Session {
    start: Instant,
    rx: Receiver<(Duration, Vec<u8>)>,
    decoder: Utf8,
    screen: Screen,
    events: Vec<Event>,
    writer: Box<dyn Write + Send>,
}

impl Session {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn absorb(&mut self, at: Duration, bytes: &[u8]) -> bool {
        let s = self.decoder.push(bytes);
        if s.is_empty() {
            return false;
        }
        self.screen.feed(&s);
        self.events.push(Event {
            t: at.as_secs_f64(),
            kind: Kind::Output,
            data: s,
        });
        true
    }

    fn pump(&mut self) -> bool {
        crate::jobs::progress(self.start.elapsed().as_secs() as usize);
        let mut fresh = false;
        loop {
            match self.rx.try_recv() {
                Ok((at, bytes)) => fresh |= self.absorb(at, &bytes),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return fresh,
            }
        }
    }

    fn await_output(&mut self, limit: Duration) -> bool {
        crate::jobs::progress(self.start.elapsed().as_secs() as usize);
        match self.rx.recv_timeout(limit) {
            Ok((at, bytes)) => {
                let mut fresh = self.absorb(at, &bytes);
                fresh |= self.pump();
                fresh
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => {
                std::thread::sleep(limit);
                false
            }
        }
    }

    fn sleep(&mut self, d: Duration) {
        let until = Instant::now() + d;
        loop {
            self.pump();
            let now = Instant::now();
            if now >= until {
                return;
            }
            std::thread::sleep((until - now).min(Duration::from_millis(2)));
        }
    }

    fn marker(&mut self, label: &str) {
        let t = self.now();
        self.events.push(Event {
            t,
            kind: Kind::Marker,
            data: label.to_string(),
        });
    }

    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.pump();
        let t = self.now();
        self.writer
            .write_all(bytes)
            .and_then(|_| self.writer.flush())
            .map_err(|e| format!("writing to the terminal: {e}"))?;
        self.events.push(Event {
            t,
            kind: Kind::Input,
            data: String::from_utf8_lossy(bytes).to_string(),
        });
        Ok(())
    }

    fn type_text(&mut self, text: &str, speed: Duration) -> Result<(), String> {
        if speed.is_zero() {
            return self.send(text.as_bytes());
        }
        for ch in text.chars() {
            let mut b = [0u8; 4];
            self.send(ch.encode_utf8(&mut b).as_bytes())?;
            self.sleep(speed);
        }
        Ok(())
    }
}

pub fn record(spec: &Spec) -> Result<Vec<Event>, String> {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: spec.grid[1] as u16,
            cols: spec.grid[0] as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;
    let shell = ["bash", "--noprofile", "--norc", "-i"];
    let argv: Vec<String> = match spec.ns {
        Some(ns) => ns.argv(&shell),
        None => shell.iter().map(|s| s.to_string()).collect(),
    };
    let mut cmd = CommandBuilder::from_argv(argv.iter().map(Into::into).collect());
    cmd.env_clear();
    let mut pairs = vec![
        ("SHELL".to_string(), "/bin/bash".to_string()),
        ("PS1".to_string(), spec.prompt.to_string()),
    ];
    pairs.extend(spec.env.iter().cloned());
    for (k, v) in crate::paths::environment(&pairs, spec.path) {
        cmd.env(k, v);
    }
    cmd.cwd(spec.cwd);
    let mut child =
        crate::paths::spawn_session(|| pair.slave.spawn_command(cmd), |c| c.process_id())
            .map_err(|e| format!("starting bash in the pty: {e}"))?;
    drop(pair.slave);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("pty reader: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("pty writer: {e}"))?;
    let start = Instant::now();
    let (tx, rx) = channel();
    let reader_thread = std::thread::spawn(move || {
        let mut buf = vec![0u8; 65536];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((start.elapsed(), buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut s = Session {
        start,
        rx,
        decoder: Utf8 { pending: vec![] },
        screen: Screen::new(spec.grid),
        events: vec![],
        writer,
    };
    let result = drive(&mut s, spec);
    s.pump();
    let end = s.now();
    let _ = child.kill();
    s.writer = Box::new(std::io::sink());
    drop(pair.master);
    let waited = Instant::now();
    while waited.elapsed() < Duration::from_secs(3) {
        if let Ok(Some(_)) = child.try_wait() {
            if let Some(pid) = child.process_id() {
                crate::paths::reaped(pid);
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = reader_thread.join();
    s.pump();
    result?;
    let mut events: Vec<Event> = s.events.into_iter().filter(|e| e.t <= end).collect();
    events.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    squeeze(&mut events, spec.grid, end);
    events.push(Event {
        t: end,
        kind: Kind::Marker,
        data: "end".into(),
    });
    Ok(events)
}

fn drive(s: &mut Session, spec: &Spec) -> Result<(), String> {
    s.marker("hide");
    s.sleep(Duration::from_millis(150));
    let setup = format!(
        "export PATH={} PS1={} && cd {} && clear\r",
        shell_quote(spec.path),
        shell_quote(spec.prompt),
        shell_quote(
            &spec
                .ns
                .map_or(spec.cwd, |n| n.cwd.as_path())
                .to_string_lossy()
        )
    );
    s.send(setup.as_bytes())?;
    s.sleep(Duration::from_millis(300));
    if let Some(l) = spec.launch {
        s.send(format!("{l}\r").as_bytes())?;
    }
    s.sleep(spec.settle);
    if spec.steps.first() != Some(&Step::Hide) {
        s.marker("show");
    }
    let mut squeezing = false;
    for step in spec.steps {
        match step {
            Step::Type { text, speed } => s.type_text(text, speed.unwrap_or(spec.typing))?,
            Step::Key { key, count, speed } => {
                let delay = speed.unwrap_or(spec.typing);
                for _ in 0..*count {
                    s.pump();
                    let bytes = tape::bytes(key, s.screen.app_cursor());
                    s.send(&bytes)?;
                    s.sleep(delay);
                }
            }
            Step::Sleep(d) if squeezing => {
                s.marker("hold");
                s.sleep(*d);
                s.marker("release");
            }
            Step::Sleep(d) => s.sleep(*d),
            Step::Squeeze { gap, ignore } => {
                s.pump();
                squeezing = gap.is_some();
                s.marker(&match (gap, ignore) {
                    (Some(g), Some(r)) => format!("squeeze {} /{r}/", g.as_secs_f64()),
                    (Some(g), None) => format!("squeeze {}", g.as_secs_f64()),
                    (None, _) => "squeeze off".into(),
                });
            }
            Step::Hide => s.marker("hide"),
            Step::Show => s.marker("show"),
            Step::Screenshot => s.marker("screenshot"),
            Step::Wait {
                pattern,
                timeout,
                screen,
            } => {
                let re = regex::Regex::new(pattern).map_err(|e| e.to_string())?;
                let until = Instant::now() + *timeout;
                s.pump();
                let mut due = true;
                loop {
                    if due && s.screen.matches(&re, *screen) {
                        break;
                    }
                    let now = Instant::now();
                    if now >= until {
                        return Err(format!(
                            "Wait /{pattern}/ timed out after {:.1}s; the screen showed:\n{}",
                            timeout.as_secs_f64(),
                            s.screen.shown()
                        ));
                    }
                    due = s.await_output((until - now).min(Duration::from_millis(5)));
                }
            }
            Step::Idle { quiet, timeout } => {
                let until = Instant::now() + *timeout;
                s.pump();
                let mut picture = s.screen.picture();
                let mut since = Instant::now();
                loop {
                    let now = Instant::now();
                    if now.duration_since(since) >= *quiet {
                        break;
                    }
                    if now >= until {
                        return Err(format!(
                            "Idle {:.1}s timed out after {:.1}s; the screen kept changing:\n{}",
                            quiet.as_secs_f64(),
                            timeout.as_secs_f64(),
                            s.screen.shown()
                        ));
                    }
                    let wait = (*quiet - now.duration_since(since)).min(until - now);
                    if s.await_output(wait.min(Duration::from_millis(20))) {
                        let next = s.screen.picture();
                        if next != picture {
                            picture = next;
                            since = Instant::now();
                        }
                    }
                }
            }
        }
    }
    if spec.still {
        s.sleep(Duration::from_millis(150));
        s.marker("screenshot");
        s.sleep(Duration::from_millis(200));
    }
    Ok(())
}

const BURST: f64 = 0.005;

fn idle_key(screen: &vt100::Screen, ignore: Option<&regex::Regex>) -> String {
    displayed(&screen.contents())
        .lines()
        .filter(|l| !ignore.is_some_and(|r| r.is_match(l)))
        .map(|l| l.chars().filter(|c| c.is_alphabetic()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn squeeze(events: &mut Vec<Event>, grid: [u32; 2], end: f64) {
    if !events
        .iter()
        .any(|e| e.kind == Kind::Marker && e.data.starts_with("squeeze "))
    {
        return;
    }
    let mut parser = vt100::Parser::new(grid[1].max(1) as u16, grid[0].max(1) as u16, 0);
    let mut ignore: Option<regex::Regex> = None;
    let mut key = idle_key(parser.screen(), None);
    let mut gap: Option<f64> = None;
    let mut hold = false;
    let mut since = 0.0;
    let mut cuts: Vec<(f64, f64)> = vec![];
    let mut mark = |t: f64, gap: Option<f64>, hold: bool, since: &mut f64| {
        if let Some(g) = gap
            && !hold
            && t - *since > g
        {
            cuts.push((*since + g / 2.0, t - g / 2.0));
        }
        *since = t;
    };
    let mut i = 0;
    while i < events.len() {
        let e = &events[i];
        match e.kind {
            Kind::Output => {
                let first = e.t;
                let mut last = e.t;
                while i < events.len()
                    && events[i].kind == Kind::Output
                    && events[i].t - last <= BURST
                {
                    parser.process(events[i].data.as_bytes());
                    last = events[i].t;
                    i += 1;
                }
                let next = idle_key(parser.screen(), ignore.as_ref());
                if next != key {
                    key = next;
                    mark(first, gap, hold, &mut since);
                }
                continue;
            }
            Kind::Input => mark(e.t, gap, hold, &mut since),
            Kind::Marker => match e.data.as_str() {
                "hold" => {
                    mark(e.t, gap, hold, &mut since);
                    hold = true;
                }
                "release" => {
                    hold = false;
                    since = e.t;
                }
                "squeeze off" => {
                    mark(e.t, gap, hold, &mut since);
                    gap = None;
                }
                d => {
                    if let Some(g) = d.strip_prefix("squeeze ") {
                        mark(e.t, gap, hold, &mut since);
                        let (g, r) = g.split_once(' ').unwrap_or((g, ""));
                        gap = tape::duration(g).ok().map(|d| d.as_secs_f64());
                        ignore = r
                            .strip_prefix('/')
                            .and_then(|r| r.strip_suffix('/'))
                            .and_then(|r| regex::Regex::new(r).ok());
                        key = idle_key(parser.screen(), ignore.as_ref());
                    }
                }
            },
        }
        i += 1;
    }
    mark(end, gap, hold, &mut since);
    for (a, b) in cuts {
        events.push(Event {
            t: a,
            kind: Kind::Marker,
            data: "cut".into(),
        });
        events.push(Event {
            t: b,
            kind: Kind::Marker,
            data: "resume".into(),
        });
    }
    events.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

pub fn write(path: &Path, cast: &Cast) -> Result<(), String> {
    let mut out = String::new();
    let header = json!({
        "version": 2,
        "width": cast.cols,
        "height": cast.rows,
        "env": {"TERM": "xterm-256color", "SHELL": "/bin/bash"},
        "x_terminal_renderer": cast.meta,
    });
    out.push_str(&header.to_string());
    out.push('\n');
    for e in &cast.events {
        let code = match e.kind {
            Kind::Output => "o",
            Kind::Input => "i",
            Kind::Marker => "m",
        };
        out.push_str(&json!([round6(e.t), code, e.data]).to_string());
        out.push('\n');
    }
    crate::encode::atomic_file(path, |w| {
        w.write_all(out.as_bytes()).map_err(|e| e.to_string())
    })
}

pub fn read(path: &Path) -> Result<Cast, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lines = text.lines();
    let header: Value = serde_json::from_str(lines.next().ok_or("an empty cast")?)
        .map_err(|e| format!("{}: header: {e}", path.display()))?;
    if header["version"] != 2 {
        return Err(format!("{}: an asciicast v2 file", path.display()));
    }
    let mut events = vec![];
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line)
            .map_err(|e| format!("{}: line {}: {e}", path.display(), i + 2))?;
        let t = v[0].as_f64().ok_or("a cast event without a time")?;
        let kind = match v[1].as_str() {
            Some("o") => Kind::Output,
            Some("i") => Kind::Input,
            Some("m") => Kind::Marker,
            _ => continue,
        };
        events.push(Event {
            t,
            kind,
            data: v[2].as_str().unwrap_or_default().to_string(),
        });
    }
    Ok(Cast {
        cols: header["width"].as_u64().unwrap_or(80) as u32,
        rows: header["height"].as_u64().unwrap_or(24) as u32,
        meta: header["x_terminal_renderer"].clone(),
        events,
    })
}

const MIN_SHOWN: f64 = 0.001;

pub struct Timeline {
    pub segments: Vec<(f64, f64)>,
    pub shots: Vec<f64>,
    pub end: f64,
}

impl Timeline {
    pub fn of(events: &[Event]) -> Timeline {
        let mut segments = vec![];
        let mut shots = vec![];
        let mut open: Option<f64> = None;
        let mut cuts = vec![];
        let mut cut: Option<f64> = None;
        let mut end = 0.0;
        for e in events.iter().filter(|e| e.kind == Kind::Marker) {
            match e.data.as_str() {
                "show" => {
                    if open.is_none() {
                        open = Some(e.t);
                    }
                }
                "hide" => {
                    if let Some(s) = open.take()
                        && e.t > s
                    {
                        segments.push((s, e.t));
                    }
                }
                "screenshot" => shots.push(e.t),
                "cut" => cut = Some(e.t),
                "resume" => {
                    if let Some(a) = cut.take() {
                        cuts.push((a, e.t));
                    }
                }
                "end" => end = e.t,
                _ => {}
            }
        }
        if let Some(s) = open
            && end > s
        {
            segments.push((s, end));
        }
        for (a, b) in cuts {
            segments = segments
                .into_iter()
                .flat_map(|(s, e)| {
                    if b <= s || a >= e {
                        vec![(s, e)]
                    } else {
                        [(s, a), (b, e)]
                            .into_iter()
                            .filter(|(s, e)| e > s)
                            .collect()
                    }
                })
                .collect();
        }
        segments.retain(|(s, e)| e - s >= MIN_SHOWN);
        Timeline {
            segments,
            shots,
            end,
        }
    }

    pub fn shown(&self) -> f64 {
        self.segments.iter().map(|(s, e)| e - s).sum()
    }

    pub fn at(&self, shown: f64) -> f64 {
        let mut acc = 0.0;
        for (s, e) in &self.segments {
            let len = e - s;
            if shown < acc + len {
                return s + (shown - acc);
            }
            acc += len;
        }
        self.segments.last().map(|(_, e)| *e).unwrap_or(self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(t: f64, d: &str) -> Event {
        Event {
            t,
            kind: Kind::Marker,
            data: d.into(),
        }
    }

    #[test]
    fn a_cast_that_cannot_be_written_leaves_the_previous_cast() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp/unit-record");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cast = Cast {
            cols: 80,
            rows: 24,
            meta: json!({"product": "demo"}),
            events: vec![m(0.5, "show")],
        };
        let path = dir.join("a.cast");
        std::fs::write(&path, b"previous").unwrap();
        let blocked = dir.join("blocked.cast");
        std::fs::create_dir_all(&blocked).unwrap();
        write(&blocked, &cast).unwrap_err();
        write(&dir.join("gone/a.cast"), &cast).unwrap_err();
        assert!(blocked.is_dir());
        assert!(!dir.join("blocked.cast.partial").exists());
        assert!(!dir.join("gone").exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        write(&path, &cast).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.events.len(), 1);
        assert!(!dir.join("a.cast.partial").exists());
    }

    #[test]
    fn hidden_stretches_are_cut_from_the_shown_timeline() {
        let tl = Timeline::of(&[
            m(0.0, "hide"),
            m(1.0, "show"),
            m(3.0, "hide"),
            m(4.0, "show"),
            m(4.5, "screenshot"),
            m(5.0, "end"),
        ]);
        assert_eq!(tl.segments, vec![(1.0, 3.0), (4.0, 5.0)]);
        assert!((tl.shown() - 3.0).abs() < 1e-9);
        assert!((tl.at(0.5) - 1.5).abs() < 1e-9);
        assert!((tl.at(2.5) - 4.5).abs() < 1e-9);
        assert_eq!(tl.shots, vec![4.5]);
    }

    #[test]
    fn a_tape_that_opens_hidden_shows_nothing_before_its_first_show() {
        let tl = Timeline::of(&[
            m(0.0, "hide"),
            m(1.0, "show"),
            m(1.000002, "hide"),
            m(3.0, "show"),
            m(5.0, "end"),
        ]);
        assert_eq!(tl.segments, vec![(3.0, 5.0)]);
        assert!((tl.at(0.0) - 3.0).abs() < 1e-9);
    }

    fn o(t: f64, d: &str) -> Event {
        Event {
            t,
            kind: Kind::Output,
            data: d.into(),
        }
    }

    #[test]
    fn a_squeeze_cuts_a_spinner_but_keeps_text_input_and_sleeps() {
        let mut ev = vec![m(0.0, "show"), m(0.0, "squeeze 0.5 /^. Thinking/")];
        ev.push(Event {
            t: 1.0,
            kind: Kind::Input,
            data: "\r".into(),
        });
        ev.push(o(1.0, "\u{2733} Thinking 1s"));
        for k in 2..10 {
            let word = if k % 2 == 0 { "tokens" } else { "thought" };
            ev.push(o(k as f64, &format!("\r\u{2736} Thinking {k}s {word}")));
        }
        ev.push(o(10.0, "\r\nDone"));
        ev.push(o(10.2, "\r\nmore"));
        ev.push(m(11.0, "hold"));
        ev.push(m(15.0, "release"));
        ev.push(m(15.0, "squeeze off"));
        ev.push(o(16.0, "\r\n\u{2736} 1"));
        ev.push(o(30.0, "\r\n\u{2736} 2"));
        squeeze(&mut ev, [40, 10], 31.0);
        let tl = Timeline::of(&[ev, vec![m(31.0, "end")]].concat());
        let ms: Vec<(i64, i64)> = tl
            .segments
            .iter()
            .map(|(a, b)| ((a * 1e3).round() as i64, (b * 1e3).round() as i64))
            .collect();
        assert_eq!(
            ms,
            vec![(0, 250), (750, 1250), (9750, 10450), (10750, 31000)]
        );
    }

    #[test]
    fn a_wait_sees_a_no_break_space_as_a_space() {
        let mut s = Screen::new([60, 5]);
        s.feed("  \u{23bf} \u{a0}mustang.toml: car\u{200b}s");
        assert!(s.matches(&regex::Regex::new(r"⎿ +mustang\.toml: cars").unwrap(), true));
    }

    #[test]
    fn a_split_utf8_sequence_waits_for_its_tail() {
        let mut d = Utf8 { pending: vec![] };
        let bytes = "三".as_bytes();
        assert_eq!(d.push(&bytes[..1]), "");
        assert_eq!(d.push(&bytes[1..]), "三");
        assert_eq!(d.push(&[0xff, b'a']), "\u{fffd}a");
    }

    #[test]
    fn the_screen_follows_application_cursor_mode() {
        let mut s = Screen::new([40, 5]);
        s.feed("\x1b[?1049h\x1b[?1h ready");
        assert!(s.app_cursor());
        assert!(s.matches(&regex::Regex::new("ready").unwrap(), false));
        s.feed("\x1b[?1l");
        assert!(!s.app_cursor());
    }

    #[test]
    fn a_wait_reads_the_cells_a_tui_redraws_in_place() {
        let mut s = Screen::new([60, 10]);
        let wait = regex::Regex::new(r"Text only|Names \+ text").unwrap();
        s.feed(
            "\x1b[?1049h\x1b[H\x1b[2J\x1b[10;1HNext only \u{b7} Doctor \u{2192} m enables meaning",
        );
        assert!(!s.matches(&wait, true));
        s.feed("\x1b[10;1HT");
        assert!(s.matches(&wait, true));
        assert!(s.matches(&wait, false));
        s.feed("\x1b[10;1H\x1b[2K\x1b[3;1Hgone");
        assert!(!s.matches(&wait, true));
        assert!(s.matches(&regex::Regex::new("^gone$").unwrap(), false));
    }
}
