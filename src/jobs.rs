use std::path::Path;
use std::sync::{Mutex, OnceLock};

static JOB: OnceLock<pito_jobs::Job> = OnceLock::new();
static CONTEXT: Mutex<Option<String>> = Mutex::new(None);

pub fn start(app: Option<&str>, label: &str, detail: String, device: String, out: &Path) {
    let job = pito_jobs::start(crate::render::TOOL, app, label)
        .detail(detail)
        .device(device)
        .out(out);
    let _ = JOB.set(job);
}

pub fn stages(names: &[&str]) {
    if let Some(job) = JOB.get() {
        job.stages(names);
    }
}

pub fn context(label: Option<String>) {
    if let Ok(mut c) = CONTEXT.lock() {
        *c = label;
    }
}

fn with_context(extra: Option<&str>) -> Option<String> {
    let ctx = CONTEXT.lock().ok().and_then(|c| c.clone());
    match (ctx, extra) {
        (Some(c), Some(e)) => Some(format!("{c} {e}")),
        (Some(c), None) => Some(c),
        (None, Some(e)) => Some(e.to_string()),
        (None, None) => None,
    }
}

pub fn stage(name: &str, total: Option<usize>, unit: &str) {
    let Some(job) = JOB.get() else { return };
    job.stage(name, total.map(|t| t as u64), unit);
    if let Some(n) = with_context(None) {
        job.stage_note(n);
    }
}

pub fn note(extra: &str) {
    let Some(job) = JOB.get() else { return };
    if let Some(n) = with_context(Some(extra)) {
        job.stage_note(n);
    }
}

pub fn progress(done: usize) {
    if let Some(job) = JOB.get() {
        job.progress(done as u64);
    }
}

pub fn end(result: &Result<(), String>) {
    let Some(job) = JOB.get() else { return };
    match result {
        Ok(()) => job.finish(0),
        Err(e) => job.fail(1, e.clone()),
    }
}

pub fn cancelled(code: i32, by: &str) {
    if let Some(job) = JOB.get() {
        job.fail(code, format!("cancelled by {by}"));
    }
}
