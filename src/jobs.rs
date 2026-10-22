mod env;
mod fit;
mod host;
mod job;
mod rate;
mod record;
mod util;

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use env::Settings;
use job::Job;
use util::Clock;

static SLOT: Slot = Slot::new();

struct Slot {
    job: OnceLock<Job>,
    context: Mutex<Option<String>>,
}

impl Slot {
    const fn new() -> Slot {
        Slot {
            job: OnceLock::new(),
            context: Mutex::new(None),
        }
    }

    fn start(&self, open: impl FnOnce() -> Job) {
        self.job.get_or_init(open);
    }

    fn stages(&self, names: &[&str]) {
        if let Some(job) = self.job.get() {
            job.stages(names);
        }
    }

    fn context(&self, label: Option<String>) {
        if let Ok(mut c) = self.context.lock() {
            *c = label;
        }
    }

    fn with_context(&self, extra: Option<&str>) -> Option<String> {
        let ctx = self.context.lock().ok().and_then(|c| c.clone());
        match (ctx, extra) {
            (Some(c), Some(e)) => Some(format!("{c} {e}")),
            (Some(c), None) => Some(c),
            (None, Some(e)) => Some(e.to_string()),
            (None, None) => None,
        }
    }

    fn stage(&self, name: &str, total: Option<usize>, unit: &str) {
        let Some(job) = self.job.get() else { return };
        job.stage(name, total.map(|t| t as u64), unit);
        if let Some(n) = self.with_context(None) {
            job.stage_note(n);
        }
    }

    fn note(&self, extra: &str) {
        let Some(job) = self.job.get() else { return };
        if let Some(n) = self.with_context(Some(extra)) {
            job.stage_note(n);
        }
    }

    fn progress(&self, done: usize) {
        if let Some(job) = self.job.get() {
            job.progress(done as u64);
        }
    }

    fn end(&self, result: &Result<(), String>) {
        let Some(job) = self.job.get() else { return };
        match result {
            Ok(()) => job.finish(0),
            Err(e) => job.fail(1, e.clone()),
        }
    }

    fn cancelled(&self, code: i32, by: &str) {
        if let Some(job) = self.job.get() {
            job.fail(code, format!("cancelled by {by}"));
        }
    }
}

fn open(
    app: Option<&str>,
    label: &str,
    detail: String,
    device: String,
    out: &Path,
    settings: Settings,
    clock: Clock,
) -> Job {
    job::start_with(crate::render::TOOL, app, label, settings, clock)
        .detail(detail)
        .device(device)
        .out(out)
}

pub fn start(app: Option<&str>, label: &str, detail: String, device: String, out: &Path) {
    SLOT.start(|| {
        open(
            app,
            label,
            detail,
            device,
            out,
            Settings::from_env(),
            Clock::system(),
        )
    });
}

pub fn stages(names: &[&str]) {
    SLOT.stages(names);
}

pub fn context(label: Option<String>) {
    SLOT.context(label);
}

pub fn stage(name: &str, total: Option<usize>, unit: &str) {
    SLOT.stage(name, total, unit);
}

pub fn note(extra: &str) {
    SLOT.note(extra);
}

pub fn progress(done: usize) {
    SLOT.progress(done);
}

pub fn end(result: &Result<(), String>) {
    SLOT.end(result);
}

pub fn cancelled(code: i32, by: &str) {
    SLOT.cancelled(code, by);
}

#[cfg(test)]
mod tests;
