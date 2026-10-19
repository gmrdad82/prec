use base64::Engine;
use serde_json::{Value, json};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(60);

const SHIM: &str = r#"exec "$0" "$@" 3<&0 4>&1 0</dev/null 1>&2"#;

const FLAGS: [&str; 14] = [
    "--headless=new",
    "--disable-gpu",
    "--no-first-run",
    "--no-default-browser-check",
    "--hide-scrollbars",
    "--mute-audio",
    "--disable-extensions",
    "--disable-background-timer-throttling",
    "--disable-renderer-backgrounding",
    "--disable-backgrounding-occluded-windows",
    "--disable-features=Translate,MediaRouter,OptimizationHints",
    "--force-color-profile=srgb",
    "--allow-file-access-from-files",
    "--remote-debugging-pipe",
];

pub struct Browser {
    child: Child,
    to: Option<Sender<Vec<u8>>>,
    from: Receiver<Vec<u8>>,
    log: PathBuf,
    next: u64,
    session: Option<String>,
    lost: Option<String>,
    pub version: String,
}

pub fn chromium() -> String {
    if let Some(p) = std::env::var_os("PREC_CHROMIUM")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::config::get().chromium.clone())
    {
        return p.display().to_string();
    }
    for c in [
        "chromium",
        "chromium-browser",
        "google-chrome-stable",
        "google-chrome",
    ] {
        let found =
            crate::paths::output(Command::new("bash").args(["-c", &format!("command -v {c}")]))
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty());
        if let Some(p) = found {
            return p;
        }
    }
    "chromium".into()
}

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

fn browser_env(home: &Path) -> Vec<(String, String)> {
    let mut pairs = vec![
        ("HOME".into(), home.display().to_string()),
        (
            "XDG_CONFIG_HOME".into(),
            home.join(".config").display().to_string(),
        ),
        (
            "XDG_CACHE_HOME".into(),
            home.join(".cache").display().to_string(),
        ),
        (
            "DBUS_SESSION_BUS_ADDRESS".into(),
            "unix:path=/nonexistent".into(),
        ),
    ];
    if let Some(data) = data_home() {
        pairs.push(("XDG_DATA_HOME".into(), data.display().to_string()));
    }
    pairs.push(("TZ".into(), "UTC".into()));
    pairs.push(("LANG".into(), "C.UTF-8".into()));
    pairs.push(("LC_ALL".into(), "C.UTF-8".into()));
    crate::paths::environment(&pairs, &std::env::var("PATH").unwrap_or_default())
}

fn writer(mut pipe: ChildStdin) -> Sender<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for m in rx {
            if pipe.write_all(&m).and_then(|_| pipe.flush()).is_err() {
                break;
            }
        }
    });
    tx
}

fn reader(pipe: ChildStdout) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut r = BufReader::new(pipe);
        loop {
            let mut m = vec![];
            match r.read_until(0, &mut m) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if m.last() == Some(&0) {
                        m.pop();
                    }
                    if tx.send(m).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

impl Browser {
    pub fn launch(work: &Path) -> Result<Browser, String> {
        let profile = work.join("chromium");
        let home = work.join("chromium-home");
        for d in [profile.clone(), home.join(".config"), home.join(".cache")] {
            std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let log = work.join("chromium.log");
        let err = File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
        let bin = chromium();
        let mut cmd = crate::paths::confined("bash", "12G");
        cmd.args(["-c", SHIM, "env", "-i"]);
        for (k, v) in browser_env(&home) {
            cmd.arg(format!("{k}={v}"));
        }
        cmd.arg(&bin)
            .args(FLAGS)
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank");
        let mut child = crate::paths::spawn(
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::from(err)),
        )
        .map_err(|e| format!("{bin}: {e}"))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{bin}: no DevTools pipe"));
        };
        let mut b = Browser {
            child,
            to: Some(writer(stdin)),
            from: reader(stdout),
            log,
            next: 0,
            session: None,
            lost: None,
            version: String::new(),
        };
        match b.browser("Browser.getVersion", json!({})) {
            Ok(v) => {
                b.version = v["product"].as_str().unwrap_or_default().to_string();
                Ok(b)
            }
            Err(e) => Err(format!("{bin} did not start: {e}")),
        }
    }

    fn said(&self) -> String {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = lines[lines.len().saturating_sub(20)..].join("\n");
        if tail.is_empty() {
            String::new()
        } else {
            format!("\nchromium said:\n{tail}")
        }
    }

    fn gone(&mut self, method: &str) -> String {
        let started = Instant::now();
        let status = loop {
            match self.child.try_wait() {
                Ok(Some(s)) => break s.to_string(),
                Ok(None) if started.elapsed() < Duration::from_secs(2) => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                _ => break "its pipe closed".into(),
            }
        };
        let e = format!("chromium exited during {method} ({status})");
        self.lost = Some(e.clone());
        format!("{e}{}", self.said())
    }

    fn lost_by(&self, v: &Value) -> Option<String> {
        let ours = |s: Option<&str>| s.is_some() && s == self.session.as_deref();
        let p = &v["params"];
        match v["method"].as_str()? {
            "Inspector.targetCrashed" if ours(v["sessionId"].as_str()) => {
                Some("chromium's page crashed".into())
            }
            "Inspector.detached" if ours(v["sessionId"].as_str()) => Some(format!(
                "chromium detached the page ({})",
                p["reason"].as_str().unwrap_or("no reason given")
            )),
            "Target.detachedFromTarget" if ours(p["sessionId"].as_str()) => {
                Some("chromium detached the page".into())
            }
            _ => None,
        }
    }

    fn message(&mut self, method: &str, params: Value, session: Option<String>) -> (u64, Vec<u8>) {
        self.next += 1;
        let mut msg = json!({"id": self.next, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        let mut bytes = msg.to_string().into_bytes();
        bytes.push(0);
        (self.next, bytes)
    }

    fn send(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
    ) -> Result<Value, String> {
        if let Some(l) = &self.lost {
            return Err(format!("{l}, so {method} cannot run"));
        }
        let (id, bytes) = self.message(method, params, session);
        if !self.to.as_ref().is_some_and(|t| t.send(bytes).is_ok()) {
            return Err(self.gone(method));
        }
        let until = Instant::now() + DEADLINE;
        loop {
            let m = match self
                .from
                .recv_timeout(until.saturating_duration_since(Instant::now()))
            {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => {
                    let e = format!(
                        "chromium did not answer {method} within {} s",
                        DEADLINE.as_secs()
                    );
                    self.lost = Some(e.clone());
                    return Err(e);
                }
                Err(RecvTimeoutError::Disconnected) => return Err(self.gone(method)),
            };
            let mut v: Value =
                serde_json::from_slice(&m).map_err(|e| format!("DevTools {method}: {e}"))?;
            if v["id"].as_u64() == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(format!("{method}: {err}"));
                }
                return Ok(v["result"].take());
            }
            if let Some(e) = self.lost_by(&v) {
                self.lost = Some(e.clone());
                return Err(format!("{e} during {method}"));
            }
        }
    }

    pub fn browser(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.send(method, params, None)
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let s = self.session.clone();
        self.send(method, params, s)
    }

    pub fn open(&mut self, url: &str) -> Result<(), String> {
        let t = self.browser("Target.createTarget", json!({"url": "about:blank"}))?;
        let target = t["targetId"].as_str().ok_or("no target")?.to_string();
        let a = self.browser(
            "Target.attachToTarget",
            json!({"targetId": target, "flatten": true}),
        )?;
        self.session = a["sessionId"].as_str().map(|s| s.to_string());
        self.call("Inspector.enable", json!({}))?;
        self.call("Page.enable", json!({}))?;
        self.call("Runtime.enable", json!({}))?;
        self.call("Page.navigate", json!({"url": url}))?;
        let started = Instant::now();
        loop {
            let r = self
                .eval("document.readyState === 'complete' && typeof window.boot === 'function'")?;
            if r.as_bool() == Some(true) {
                return Ok(());
            }
            if started.elapsed() > Duration::from_secs(30) {
                return Err(format!("{url} never finished loading"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn viewport(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": width, "height": height, "deviceScaleFactor": 1, "mobile": false}),
        )?;
        self.call(
            "Emulation.setFocusEmulationEnabled",
            json!({"enabled": true}),
        )?;
        self.call(
            "Emulation.setDefaultBackgroundColorOverride",
            json!({"color": {"r": 0, "g": 0, "b": 0, "a": 1}}),
        )?;
        Ok(())
    }

    pub fn eval(&mut self, expr: &str) -> Result<Value, String> {
        let r = self.call(
            "Runtime.evaluate",
            json!({"expression": expr, "awaitPromise": true, "returnByValue": true}),
        )?;
        if let Some(ex) = r.get("exceptionDetails") {
            return Err(format!(
                "page: {}",
                ex["exception"]["description"]
                    .as_str()
                    .or(ex["text"].as_str())
                    .unwrap_or("an exception")
            ));
        }
        Ok(r["result"]["value"].clone())
    }

    pub fn screenshot(&mut self) -> Result<Vec<u8>, String> {
        let mut r = self.call(
            "Page.captureScreenshot",
            json!({"format": "png", "optimizeForSpeed": true, "captureBeyondViewport": false, "fromSurface": true}),
        )?;
        let Value::String(data) = r["data"].take() else {
            return Err("an empty screenshot".into());
        };
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| e.to_string())
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let pid = self.child.id();
        if self.lost.is_some() {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
            let _ = self.child.kill();
            let _ = self.child.wait();
            crate::paths::reaped(pid);
            return;
        }
        if let Some(to) = self.to.take() {
            let (_, bytes) = self.message("Browser.close", json!({}), None);
            let _ = to.send(bytes);
        }
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            if let Ok(Some(_)) = self.child.try_wait() {
                crate::paths::reaped(pid);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        crate::paths::reaped(pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(script: &str) -> Browser {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/cdp-unit");
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = crate::paths::spawn(
            Command::new("bash")
                .args(["-c", script])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null()),
        )
        .unwrap();
        let (stdin, stdout) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
        Browser {
            child,
            to: Some(writer(stdin)),
            from: reader(stdout),
            log: dir.join("chromium.log"),
            next: 0,
            session: Some("S".into()),
            lost: None,
            version: String::new(),
        }
    }

    #[test]
    fn a_crashed_page_fails_the_command_by_name_and_the_browser_is_killed_at_once() {
        let mut b = peer(
            r#"IFS= read -r -d '' _; printf '{"method":"Inspector.targetCrashed","sessionId":"S","params":{}}\0'; exec sleep 30"#,
        );
        let e = b
            .send("Runtime.evaluate", json!({}), Some("S".into()))
            .unwrap_err();
        assert!(
            e.contains("crashed") && e.contains("Runtime.evaluate"),
            "{e}"
        );
        assert!(
            b.send("Page.navigate", json!({}), Some("S".into()))
                .is_err()
        );
        let pid = b.child.id();
        let started = Instant::now();
        drop(b);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_err());
    }

    #[test]
    fn another_sessions_crash_is_not_ours() {
        let mut b = peer(
            r#"IFS= read -r -d '' _; printf '{"method":"Inspector.targetCrashed","sessionId":"T","params":{}}\0{"id":1,"result":{"ok":true}}\0'; exec sleep 30"#,
        );
        let v = b
            .send("Runtime.evaluate", json!({}), Some("S".into()))
            .unwrap();
        assert_eq!(v["ok"], true);
        b.lost = Some("done".into());
    }
}
