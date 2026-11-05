use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;

const SOCKET_PATH_MAX: usize = 107;
const LONGEST: &str = "pipewire-0-manager";
const DISABLE: &str = "PREC_AUDIO";
const TOOLS: [&str; 4] = ["pipewire", "wireplumber", "pw-record", "pw-dump"];
const PULSE: &str = "pipewire-pulse";
const READY: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(5);
const SCRUBBED: [&str; 11] = [
    "DBUS_SESSION_BUS_ADDRESS",
    "PIPEWIRE_REMOTE",
    "PIPEWIRE_RUNTIME_DIR",
    "PIPEWIRE_CONFIG_DIR",
    "PIPEWIRE_CONFIG_NAME",
    "PIPEWIRE_CONFIG_PREFIX",
    "PULSE_SERVER",
    "PULSE_RUNTIME_PATH",
    "PULSE_CONFIG_PATH",
    "WIREPLUMBER_CONFIG_DIR",
    "XDG_SESSION_ID",
];
const SINK: &str = "context.objects = [\n  { factory = adapter\n    args = { factory.name = support.null-audio-sink node.name = null-sink media.class = Audio/Sink audio.position = [ FL FR ] object.linger = true }\n  }\n]\n";
const OFF: &str = "wireplumber.profiles = {\n  main = {\n    monitor.alsa = disabled\n    monitor.alsa-midi = disabled\n    monitor.bluez = disabled\n    monitor.bluez-midi = disabled\n    monitor.libcamera = disabled\n    monitor.v4l2 = disabled\n    support.logind = disabled\n    support.reserve-device = disabled\n  }\n}\n";
const STREAM_CLASS: &str = "Stream/Output/Audio";

#[derive(Debug, Default, PartialEq)]
pub struct Capture {
    pub start: f64,
    pub samples: Vec<i16>,
}

#[derive(Debug, PartialEq)]
pub struct Mix {
    pub start: f64,
    pub samples: Vec<i16>,
}

impl Mix {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / (RATE as f64 * CHANNELS as f64)
    }
}

fn frames(samples: &[i16]) -> usize {
    samples.len() / CHANNELS as usize
}

fn leading_silence(samples: &[i16]) -> usize {
    let width = CHANNELS as usize;
    samples
        .chunks_exact(width)
        .take_while(|f| f.iter().all(|s| *s == 0))
        .count()
}

pub fn mix(captures: Vec<Capture>, end: f64) -> Option<Mix> {
    let width = CHANNELS as usize;
    let heard: Vec<Capture> = captures
        .into_iter()
        .filter_map(|c| {
            let lead = leading_silence(&c.samples);
            let kept = frames(&c.samples) - lead;
            (kept > 0).then(|| Capture {
                start: c.start + lead as f64 / RATE as f64,
                samples: c.samples[lead * width..frames(&c.samples) * width].to_vec(),
            })
        })
        .collect();
    let base = heard.iter().map(|c| c.start).fold(f64::INFINITY, f64::min);
    if !base.is_finite() {
        return None;
    }
    let limit = ((end - base) * RATE as f64).floor();
    if limit < 1.0 {
        return None;
    }
    let placed: Vec<(usize, &Capture)> = heard
        .iter()
        .map(|c| (((c.start - base) * RATE as f64).round() as usize, c))
        .collect();
    let total = placed
        .iter()
        .map(|(at, c)| at + frames(&c.samples))
        .max()
        .unwrap_or(0)
        .min(limit as usize);
    let mut sum = vec![0i32; total * width];
    for (at, c) in placed {
        for (i, s) in c.samples.iter().enumerate() {
            if let Some(cell) = sum.get_mut(at * width + i) {
                *cell += *s as i32;
            }
        }
    }
    let samples: Vec<i16> = sum
        .into_iter()
        .map(|v| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16)
        .collect();
    (leading_silence(&samples) < frames(&samples)).then_some(Mix {
        start: base,
        samples,
    })
}

pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let block = CHANNELS * 2;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * block as u32).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn write_wav(path: &Path, mix: &Mix) -> Result<(), String> {
    let bytes = wav_bytes(&mix.samples);
    crate::encode::atomic_file(path, |w| {
        std::io::Write::write_all(w, &bytes).map_err(|e| e.to_string())
    })
}

#[derive(Debug, PartialEq)]
pub enum Change {
    Appeared { id: u64, serial: u64, pid: u32 },
    Gone(u64),
}

fn pid_of(props: &Value) -> Option<u32> {
    props["application.process.id"]
        .as_u64()
        .or_else(|| props["pipewire.sec.pid"].as_u64())
        .map(|p| p as u32)
}

#[derive(Default)]
struct Node {
    serial: Option<u64>,
    client: Option<u64>,
    pid: Option<u32>,
    stream: bool,
    announced: bool,
}

#[derive(Default)]
pub struct Graph {
    clients: HashMap<u64, u32>,
    nodes: HashMap<u64, Node>,
}

impl Graph {
    pub fn apply(&mut self, dump: &Value) -> Vec<Change> {
        let mut changes = Vec::new();
        for o in dump.as_array().into_iter().flatten() {
            let Some(id) = o["id"].as_u64() else {
                continue;
            };
            if o.get("info").is_some_and(Value::is_null) {
                self.clients.remove(&id);
                if self.nodes.remove(&id).is_some_and(|n| n.announced) {
                    changes.push(Change::Gone(id));
                }
                continue;
            }
            let kind = o["type"].as_str().unwrap_or_default();
            let props = &o["info"]["props"];
            if kind.ends_with(":Client") {
                if let Some(pid) = pid_of(props) {
                    self.clients.insert(id, pid);
                }
            } else if kind.ends_with(":Node") {
                let node = self.nodes.entry(id).or_default();
                if let Some(class) = props["media.class"].as_str() {
                    node.stream = class == STREAM_CLASS;
                }
                node.serial = props["object.serial"].as_u64().or(node.serial);
                node.client = props["client.id"].as_u64().or(node.client);
                node.pid = pid_of(props).or(node.pid);
            }
        }
        let clients = &self.clients;
        let mut appeared: Vec<Change> = self
            .nodes
            .iter_mut()
            .filter(|(_, n)| n.stream && !n.announced)
            .filter_map(|(id, n)| {
                let pid = n
                    .pid
                    .or_else(|| n.client.and_then(|c| clients.get(&c).copied()))?;
                let serial = n.serial?;
                n.announced = true;
                Some(Change::Appeared {
                    id: *id,
                    serial,
                    pid,
                })
            })
            .collect();
        appeared.sort_by_key(|c| match c {
            Change::Appeared { id, .. } | Change::Gone(id) => *id,
        });
        changes.extend(appeared);
        changes
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub session: u32,
}

pub fn proc_of(stat: &str) -> Option<Proc> {
    let (head, tail) = stat.rsplit_once(") ")?;
    let pid = head.split_once(" (")?.0.trim().parse().ok()?;
    let f: Vec<&str> = tail.split_whitespace().collect();
    Some(Proc {
        pid,
        ppid: f.get(1)?.parse().ok()?,
        session: f.get(3)?.parse().ok()?,
    })
}

pub fn in_tree(pid: u32, procs: &[Proc], root: u32) -> bool {
    let by: HashMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut at = pid;
    for _ in 0..procs.len() + 1 {
        if at == root {
            return true;
        }
        match by.get(&at) {
            Some(p) if p.session == root => return true,
            Some(p) if p.ppid > 1 => at = p.ppid,
            _ => return false,
        }
    }
    false
}

fn procs() -> Vec<Proc> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return vec![];
    };
    dir.flatten()
        .filter(|e| e.file_name().to_string_lossy().parse::<u32>().is_ok())
        .filter_map(|e| std::fs::read_to_string(e.path().join("stat")).ok())
        .filter_map(|t| proc_of(&t))
        .collect()
}

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| crate::doctor::found_in(&p, tool).is_some())
}

pub fn missing_tool() -> Option<&'static str> {
    TOOLS.into_iter().find(|t| !on_path(t))
}

pub fn tools_found() -> bool {
    missing_tool().is_none()
}

pub fn pulse_found() -> bool {
    on_path(PULSE)
}

pub fn disabled() -> bool {
    std::env::var(DISABLE).is_ok_and(|v| v == "off")
}

#[derive(Clone)]
pub struct Private {
    runtime: PathBuf,
    home: PathBuf,
}

impl Private {
    pub fn runtime(&self) -> &Path {
        &self.runtime
    }

    pub fn apply(&self, c: &mut Command) {
        for name in SCRUBBED {
            c.env_remove(name);
        }
        c.env("XDG_RUNTIME_DIR", &self.runtime)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".state"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_DATA_HOME", self.home.join(".data"));
    }
}

pub struct Sound {
    base: PathBuf,
    private: Private,
    kids: Vec<Child>,
}

static LAID: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

fn laid() -> std::sync::MutexGuard<'static, Vec<PathBuf>> {
    LAID.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn sweep() {
    for base in laid().drain(..) {
        let _ = std::fs::remove_dir_all(base);
    }
}

static PLACED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn fits(dir: &Path) -> bool {
    dir.join(LONGEST).as_os_str().len() <= SOCKET_PATH_MAX
}

fn lay(base: &Path) -> Result<Private, String> {
    use std::os::unix::fs::PermissionsExt;
    let runtime = base.join("x");
    let home = base.join("h");
    let pw = home.join(".config/pipewire/pipewire.conf.d");
    let wp = home.join(".config/wireplumber/wireplumber.conf.d");
    for d in [&runtime, &pw, &wp] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("{}: {e}", runtime.display()))?;
    for (path, text) in [
        (pw.join("10-null.conf"), SINK),
        (wp.join("10-off.conf"), OFF),
    ] {
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(Private { runtime, home })
}

impl Sound {
    pub fn start(parent: &Path) -> Option<Sound> {
        if !cfg!(target_os = "linux") || disabled() {
            return None;
        }
        if let Some(tool) = missing_tool() {
            eprintln!("prec: audio: {tool} is not on PATH; recording without sound");
            return None;
        }
        let near = parent.join("snd");
        let base = if fits(&near.join("x")) {
            near
        } else {
            let n = PLACED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let name = format!("p{}-{n}", std::process::id());
            crate::cache::root().join("sockets").join(name)
        };
        let mut sound = Sound {
            private: Private {
                runtime: base.join("x"),
                home: base.join("h"),
            },
            base,
            kids: vec![],
        };
        if !fits(&sound.private.runtime) {
            eprintln!("prec: audio: a socket path is too long; recording without sound");
            return None;
        }
        match sound.bring_up() {
            Ok(()) => Some(sound),
            Err(e) => {
                eprintln!("prec: audio: {e}; recording without sound");
                None
            }
        }
    }

    pub fn private(&self) -> &Private {
        &self.private
    }

    fn daemon(&mut self, tool: &str) -> Result<(), String> {
        let mut c = Command::new(tool);
        c.current_dir(&self.private.home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        self.private.apply(&mut c);
        die_with_prec(&mut c);
        let child = crate::paths::spawn(&mut c).map_err(|e| format!("{tool}: {e}"))?;
        self.kids.push(child);
        Ok(())
    }

    fn alive(&mut self) -> bool {
        self.kids
            .iter_mut()
            .all(|k| matches!(k.try_wait(), Ok(None)))
    }

    fn wait(&mut self, what: &str, mut ready: impl FnMut(&Private) -> bool) -> Result<(), String> {
        let until = Instant::now() + READY;
        loop {
            if ready(&self.private) {
                return Ok(());
            }
            if !self.alive() {
                return Err(format!("a daemon stopped before {what}"));
            }
            if Instant::now() > until {
                return Err(format!("no {what} in time"));
            }
            std::thread::sleep(POLL);
        }
    }

    fn bring_up(&mut self) -> Result<(), String> {
        laid().push(self.base.clone());
        self.private = lay(&self.base)?;
        self.daemon("pipewire")?;
        self.wait("pipewire socket", |p| p.runtime.join("pipewire-0").exists())?;
        self.daemon("wireplumber")?;
        let pulse = pulse_found();
        if pulse {
            self.daemon(PULSE)?;
        }
        self.wait("default sink", |p| {
            has_default_sink(p) && (!pulse || p.runtime.join("pulse/native").exists())
        })
    }
}

#[cfg(target_os = "linux")]
fn die_with_prec(c: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        c.pre_exec(|| {
            nix::sys::prctl::set_pdeathsig(Signal::SIGTERM).map_err(std::io::Error::from)
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn die_with_prec(_: &mut Command) {}

fn names_default_sink(dump: &[u8]) -> bool {
    serde_json::from_slice::<Value>(dump).is_ok_and(|v| {
        v.as_array().into_iter().flatten().any(|o| {
            o["metadata"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|m| m["key"] == "default.audio.sink" && !m["value"].is_null())
        })
    })
}

fn has_default_sink(p: &Private) -> bool {
    let mut c = Command::new("pw-dump");
    c.arg("--no-colors");
    p.apply(&mut c);
    crate::paths::output(&mut c).is_ok_and(|o| names_default_sink(&o.stdout))
}

impl Drop for Sound {
    fn drop(&mut self) {
        for k in self.kids.iter().rev() {
            let _ = kill(Pid::from_raw(k.id() as i32), Signal::SIGTERM);
        }
        let until = Instant::now() + Duration::from_secs(3);
        for k in self.kids.iter_mut().rev() {
            while Instant::now() < until && !matches!(k.try_wait(), Ok(Some(_))) {
                std::thread::sleep(POLL);
            }
            let _ = k.kill();
            let _ = k.wait();
            crate::paths::reaped(k.id());
        }
        laid().retain(|b| b != &self.base);
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

struct Tap {
    child: Child,
    reader: Option<JoinHandle<Capture>>,
}

fn capture(out: impl Read, clock: Instant) -> Capture {
    let mut out = BufReader::with_capacity(1 << 16, out);
    let mut buf = vec![0u8; 1 << 14];
    let mut bytes: Vec<u8> = Vec::new();
    let mut start = f64::INFINITY;
    let width = CHANNELS as usize * 2;
    while let Ok(n) = out.read(&mut buf) {
        if n == 0 {
            break;
        }
        let at = clock.elapsed().as_secs_f64();
        bytes.extend_from_slice(&buf[..n]);
        let got = (bytes.len() / width) as f64;
        start = start.min(at - got / RATE as f64);
    }
    let samples = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .take(bytes.len() / width * CHANNELS as usize)
        .map(|b| i16::from_le_bytes(*b))
        .collect();
    Capture {
        start: if start.is_finite() { start } else { 0.0 },
        samples,
    }
}

impl Tap {
    fn start(serial: u64, clock: Instant, private: &Private) -> Option<Tap> {
        let mut cmd = Command::new("pw-record");
        cmd.args(["--target", &serial.to_string(), "--raw", "--rate"])
            .arg(RATE.to_string())
            .arg("--channels")
            .arg(CHANNELS.to_string())
            .args(["--format", "s16", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        private.apply(&mut cmd);
        let mut child = crate::paths::spawn(&mut cmd).ok()?;
        let Some(out) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            crate::paths::reaped(child.id());
            return None;
        };
        Some(Tap {
            child,
            reader: Some(std::thread::spawn(move || capture(out, clock))),
        })
    }

    fn finish(mut self) -> Capture {
        let _ = kill(Pid::from_raw(self.child.id() as i32), Signal::SIGTERM);
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until && !matches!(self.child.try_wait(), Ok(Some(_))) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.reader
            .take()
            .and_then(|r| r.join().ok())
            .unwrap_or_default()
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        crate::paths::reaped(self.child.id());
    }
}

fn watch(out: impl Read, root: u32, clock: Instant, private: &Private) -> Vec<Capture> {
    let mut graph = Graph::default();
    let mut live: HashMap<u64, Tap> = HashMap::new();
    let mut done = Vec::new();
    let dumps = serde_json::Deserializer::from_reader(BufReader::new(out)).into_iter::<Value>();
    for dump in dumps {
        let Ok(dump) = dump else {
            break;
        };
        for change in graph.apply(&dump) {
            match change {
                Change::Appeared { id, serial, pid } => {
                    if in_tree(pid, &procs(), root)
                        && let Some(tap) = Tap::start(serial, clock, private)
                    {
                        live.insert(id, tap);
                    }
                }
                Change::Gone(id) => {
                    if let Some(tap) = live.remove(&id) {
                        done.push(tap.finish());
                    }
                }
            }
        }
    }
    done.extend(live.into_values().map(Tap::finish));
    done
}

pub struct Audio {
    watcher: Child,
    thread: Option<JoinHandle<Vec<Capture>>>,
}

impl Audio {
    pub fn start(root: u32, clock: Instant, private: &Private) -> Option<Audio> {
        let mut cmd = Command::new("pw-dump");
        cmd.args(["--no-colors", "--monitor"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        private.apply(&mut cmd);
        let mut watcher = crate::paths::spawn(&mut cmd).ok()?;
        let Some(out) = watcher.stdout.take() else {
            let _ = watcher.kill();
            let _ = watcher.wait();
            crate::paths::reaped(watcher.id());
            return None;
        };
        let private = private.clone();
        Some(Audio {
            watcher,
            thread: Some(std::thread::spawn(move || {
                watch(out, root, clock, &private)
            })),
        })
    }

    pub fn finish(mut self, end: f64) -> Option<Mix> {
        self.stop();
        let captures = self
            .thread
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or_default();
        mix(captures, end)
    }

    fn stop(&mut self) {
        let _ = self.watcher.kill();
        let _ = self.watcher.wait();
        crate::paths::reaped(self.watcher.id());
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn client(id: u64, pid: u32) -> Value {
        json!({"id": id, "type": "PipeWire:Interface:Client", "info": {"props": {"pipewire.sec.pid": pid}}})
    }

    fn stream(id: u64, serial: u64, client: Option<u64>, pid: Option<u32>, class: &str) -> Value {
        let mut props = json!({"media.class": class, "object.serial": serial});
        if let Some(c) = client {
            props["client.id"] = json!(c);
        }
        if let Some(p) = pid {
            props["pipewire.sec.pid"] = json!(p);
        }
        json!({"id": id, "type": "PipeWire:Interface:Node", "info": {"state": "running", "props": props}})
    }

    #[test]
    fn a_playback_stream_is_found_through_its_client_or_its_own_pid() {
        let mut g = Graph::default();
        let dump = json!([
            client(10, 4000),
            stream(20, 21, Some(10), None, "Stream/Output/Audio"),
            stream(22, 23, Some(99), Some(5000), "Stream/Output/Audio"),
            stream(24, 25, Some(10), None, "Stream/Input/Audio"),
            stream(26, 27, None, None, "Audio/Sink"),
            {"id": 3, "type": "PipeWire:Interface:Metadata", "props": {}}
        ]);
        assert_eq!(
            g.apply(&dump),
            [
                Change::Appeared {
                    id: 20,
                    serial: 21,
                    pid: 4000
                },
                Change::Appeared {
                    id: 22,
                    serial: 23,
                    pid: 5000
                },
            ]
        );
        assert_eq!(g.apply(&dump), []);
    }

    #[test]
    fn a_pulse_stream_is_dated_by_the_clients_own_process_not_the_servers() {
        let mut g = Graph::default();
        let c = json!({"id": 10, "type": "PipeWire:Interface:Client", "info": {"props": {"pipewire.sec.pid": 900, "application.process.id": 4000}}});
        assert_eq!(
            g.apply(&json!([
                c,
                stream(20, 21, Some(10), None, "Stream/Output/Audio")
            ])),
            [Change::Appeared {
                id: 20,
                serial: 21,
                pid: 4000
            }]
        );
    }

    #[test]
    fn a_private_folder_fits_only_if_pipewires_longest_socket_does() {
        let room = SOCKET_PATH_MAX - "/pipewire-0-manager".len();
        assert!(fits(Path::new(&"a".repeat(room))));
        assert!(!fits(Path::new(&"a".repeat(room + 1))));
    }

    #[test]
    fn a_stream_seen_before_its_serial_is_announced_once_the_serial_comes() {
        let mut g = Graph::default();
        let bare = json!([{"id": 20, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Output/Audio", "pipewire.sec.pid": 4000}}}]);
        assert_eq!(g.apply(&bare), []);
        let full = json!([{"id": 20, "type": "PipeWire:Interface:Node", "info": {"props": {"object.serial": 21}}}]);
        assert_eq!(
            g.apply(&full),
            [Change::Appeared {
                id: 20,
                serial: 21,
                pid: 4000
            }]
        );
    }

    #[test]
    fn a_stream_waits_for_its_client_and_goes_when_removed() {
        let mut g = Graph::default();
        assert_eq!(
            g.apply(&json!([stream(
                20,
                21,
                Some(10),
                None,
                "Stream/Output/Audio"
            )])),
            []
        );
        assert_eq!(
            g.apply(&json!([client(10, 4000)])),
            [Change::Appeared {
                id: 20,
                serial: 21,
                pid: 4000
            }]
        );
        assert_eq!(
            g.apply(&json!([{"id": 20, "info": null}, {"id": 10, "info": null}])),
            [Change::Gone(20)]
        );
        assert_eq!(g.apply(&json!([{"id": 31, "info": null}])), []);
    }

    #[test]
    fn a_stream_that_reopens_under_a_new_id_appears_again_and_an_update_keeps_its_props() {
        let mut g = Graph::default();
        g.apply(&json!([client(10, 4000)]));
        let first = g.apply(&json!([stream(
            20,
            21,
            Some(10),
            None,
            "Stream/Output/Audio"
        )]));
        assert_eq!(first.len(), 1);
        let update = json!([{"id": 20, "type": "PipeWire:Interface:Node", "info": {"props": {"node.name": "x"}}}]);
        assert_eq!(g.apply(&update), []);
        g.apply(&json!([{"id": 20, "info": null}]));
        let again = g.apply(&json!([stream(
            30,
            31,
            Some(10),
            None,
            "Stream/Output/Audio"
        )]));
        assert_eq!(
            again,
            [Change::Appeared {
                id: 30,
                serial: 31,
                pid: 4000
            }]
        );
    }

    #[test]
    fn a_default_sink_is_named_only_by_a_metadata_entry_with_a_value() {
        let named = br#"[{"id": 1, "metadata": [{"key": "default.audio.sink", "value": {"name": "null-sink"}}]}]"#;
        let bare = br#"[{"id": 1, "metadata": [{"key": "default.audio.sink", "value": null}]}, {"id": 2, "info": {"props": {"note": "default.audio.sink"}}}]"#;
        assert!(names_default_sink(named));
        assert!(!names_default_sink(bare));
        assert!(!names_default_sink(b"[{\"id\": 1, \"metadata\": []}]"));
        assert!(!names_default_sink(b"not json"));
    }

    #[test]
    fn a_dump_that_is_not_a_list_changes_nothing() {
        let mut g = Graph::default();
        assert_eq!(g.apply(&json!({"id": 1})), []);
        assert_eq!(g.apply(&json!([{"type": "x"}])), []);
    }

    fn table() -> Vec<Proc> {
        [
            (100, 1, 100),
            (101, 100, 100),
            (102, 101, 100),
            (200, 1, 200),
            (201, 200, 200),
            (300, 1, 100),
        ]
        .into_iter()
        .map(|(pid, ppid, session)| Proc { pid, ppid, session })
        .collect()
    }

    #[test]
    fn a_process_is_in_the_tree_by_parent_or_by_session() {
        let t = table();
        assert!(in_tree(100, &t, 100));
        assert!(in_tree(102, &t, 100));
        assert!(in_tree(300, &t, 100));
        assert!(!in_tree(201, &t, 100));
        assert!(!in_tree(200, &t, 100));
        assert!(!in_tree(999, &t, 100));
    }

    #[test]
    fn a_parent_loop_ends_the_walk() {
        let t = [
            Proc {
                pid: 5,
                ppid: 6,
                session: 5,
            },
            Proc {
                pid: 6,
                ppid: 5,
                session: 6,
            },
        ];
        assert!(!in_tree(5, &t, 9));
    }

    #[test]
    fn a_stat_line_gives_pid_parent_and_session() {
        let line = "4242 (my (odd) app) S 4000 4242 4100 34816 4242 4194560 1 0";
        assert_eq!(
            proc_of(line),
            Some(Proc {
                pid: 4242,
                ppid: 4000,
                session: 4100
            })
        );
        assert_eq!(proc_of("garbage"), None);
        assert_eq!(proc_of("1 (a) S x"), None);
    }

    fn stereo(frames: &[(i16, i16)]) -> Vec<i16> {
        frames.iter().flat_map(|(l, r)| [*l, *r]).collect()
    }

    fn cap(start: f64, frames: &[(i16, i16)]) -> Capture {
        Capture {
            start,
            samples: stereo(frames),
        }
    }

    #[test]
    fn silence_alone_gives_no_mix_and_leading_silence_moves_the_start() {
        assert_eq!(mix(vec![], 10.0), None);
        assert_eq!(mix(vec![cap(1.0, &[(0, 0); 480])], 10.0), None);
        assert_eq!(mix(vec![Capture::default()], 10.0), None);
        let m = mix(
            vec![cap(1.0, &[(0, 0), (0, 0), (5, 0), (0, 0), (7, 7)])],
            10.0,
        )
        .unwrap();
        assert!((m.start - (1.0 + 2.0 / RATE as f64)).abs() < 1e-12);
        assert_eq!(m.samples, stereo(&[(5, 0), (0, 0), (7, 7)]));
    }

    #[test]
    fn captures_mix_on_the_clock_by_summing_and_clamping() {
        let a = cap(1.0, &[(100, 100), (200, 200), (300, 300)]);
        let b = cap(
            1.0 + 2.0 / RATE as f64,
            &[(1000, -1000), (i16::MAX, i16::MIN)],
        );
        let m = mix(vec![a, b], 10.0).unwrap();
        assert_eq!(m.start, 1.0);
        assert_eq!(
            m.samples,
            stereo(&[(100, 100), (200, 200), (1300, -700), (i16::MAX, i16::MIN)])
        );
    }

    #[test]
    fn a_silent_capture_does_not_move_the_mix_and_the_mix_is_cut_at_the_end() {
        let loud = cap(2.0, &[(9, 9); 100]);
        let quiet = cap(0.5, &[(0, 0); 100]);
        let m = mix(vec![quiet, loud], 2.0 + 40.0 / RATE as f64).unwrap();
        assert_eq!(m.start, 2.0);
        assert_eq!(m.samples.len(), 80);
        assert_eq!(mix(vec![cap(2.0, &[(9, 9); 100])], 2.0), None);
        assert_eq!(mix(vec![cap(2.0, &[(9, 9); 100])], 1.0), None);
    }

    #[test]
    fn a_later_capture_leaves_silence_between() {
        let a = cap(0.0, &[(1, 1)]);
        let b = cap(4.0 / RATE as f64, &[(2, 2)]);
        let m = mix(vec![b, a], 1.0).unwrap();
        assert_eq!(m.samples, stereo(&[(1, 1), (0, 0), (0, 0), (0, 0), (2, 2)]));
        assert!((m.seconds() - 5.0 / RATE as f64).abs() < 1e-12);
    }

    #[test]
    fn a_wav_has_a_48k_stereo_16_bit_header_and_its_samples() {
        let w = wav_bytes(&stereo(&[(1, -1), (256, 2)]));
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(w[20..22].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(w[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 48_000);
        assert_eq!(u32::from_le_bytes(w[28..32].try_into().unwrap()), 192_000);
        assert_eq!(u16::from_le_bytes(w[32..34].try_into().unwrap()), 4);
        assert_eq!(u16::from_le_bytes(w[34..36].try_into().unwrap()), 16);
        assert_eq!(&w[36..40], b"data");
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 8);
        assert_eq!(&w[44..], [1, 0, 255, 255, 0, 1, 2, 0]);
    }

    #[test]
    fn a_capture_counts_whole_frames_and_dates_its_first_sample_from_the_earliest_read() {
        let bytes: Vec<u8> = (0..9i16).flat_map(|s| s.to_le_bytes()).collect();
        let c = capture(std::io::Cursor::new(bytes), Instant::now());
        assert_eq!(c.samples, (0..8).collect::<Vec<i16>>());
        assert!(c.start.is_finite());
        let none = capture(std::io::Cursor::new(Vec::new()), Instant::now());
        assert_eq!(none, Capture::default());
    }
}
