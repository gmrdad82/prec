mod common;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const KEYS: &[&str] = &[
    "v",
    "id",
    "host",
    "pid",
    "pid_start",
    "tool",
    "app",
    "label",
    "detail",
    "invoker",
    "parent",
    "state",
    "stage",
    "stages",
    "stage_note",
    "progress",
    "rate",
    "pgpu",
    "device",
    "out",
    "started",
    "updated",
    "ended",
    "exit",
    "error",
];

fn case(name: &str) -> (PathBuf, PathBuf) {
    let dir = common::caller("jobs", name);
    let app = common::demo_app(&dir, "");
    common::add_shot(
        &app,
        "short",
        "Sleep 200ms\nType \"demo tick\"\nEnter\nSleep 1s\n",
        "",
    );
    (dir, app)
}

fn record(dir: &Path, app: &Path) -> Command {
    let mut c = common::prec(
        dir,
        &dir.join("archive"),
        &[
            "record",
            "demo",
            "short",
            "--app-dir",
            app.to_str().unwrap(),
            "--out",
            "tmp/out",
        ],
    );
    c.env_remove("PITO_JOBS")
        .env_remove("PITO_JOBS_DIR")
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("TMPDIR")
        .env_remove("PITO_JOB_ID")
        .env_remove("PITO_INVOKER")
        .env_remove("GPU_QUEUE_PID");
    c
}

fn names(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

fn the_record(jobs: &Path, o: &Output) -> (String, String, Value) {
    let found = names(jobs);
    assert_eq!(
        found.len(),
        1,
        "one record and no temporary file: {found:?}"
    );
    let name = found[0].clone();
    assert!(!name.starts_with('.'), "{name}");
    let raw = std::fs::read_to_string(jobs.join(&name)).unwrap();
    assert!(raw.len() <= 4096, "{} bytes", raw.len());
    let value: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{e}: {raw}; prec said:\n{}", stderr(o)));
    (name, raw, value)
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn top_level_keys(raw: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut depth = 0;
    let mut want_key = false;
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' | '[' => {
                depth += 1;
                want_key = c == '{' && depth == 1;
            }
            '}' | ']' => depth -= 1,
            ',' if depth == 1 => want_key = true,
            '"' => {
                let mut text = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        c => text.push(c),
                    }
                }
                if want_key {
                    keys.push(text);
                    want_key = false;
                }
            }
            _ => {}
        }
    }
    keys
}

fn keys_in_order(raw: &str) {
    assert!(raw.starts_with("{\"v\":1,\"id\":\""), "{raw}");
    assert!(raw.ends_with('}'), "{raw}");
    assert!(!raw.contains('\n'), "{raw}");
    assert_eq!(top_level_keys(raw), KEYS, "{raw}");
}

#[test]
fn the_key_scan_sees_extra_duplicate_and_nested_keys() {
    assert_eq!(
        top_level_keys(r#"{"a":{"b":1,"c":[{"d":2}]},"e":"x,\"f\":","a":[1,2]}"#),
        ["a", "e", "a"]
    );
    assert_eq!(top_level_keys("{}"), Vec::<String>::new());
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        found.push(path.clone());
        if entry.file_type().unwrap().is_dir() {
            walk(&path, found);
        }
    }
}

fn looks_like_a_record(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_string_lossy();
    let name = name.trim_start_matches('.');
    let Some(stem) = name
        .strip_suffix(".json")
        .or_else(|| name.strip_suffix(".json.tmp"))
    else {
        return false;
    };
    stem.split_once('-').is_some_and(|(pid, ms)| {
        !pid.is_empty()
            && !ms.is_empty()
            && pid.bytes().all(|b| b.is_ascii_digit())
            && ms.bytes().all(|b| b.is_ascii_digit())
    })
}

fn finished_well(name: &str, raw: &str, rec: &Value, dir: &Path) {
    keys_in_order(raw);
    let pid = rec["pid"].as_u64().unwrap();
    let started = rec["started"].as_u64().unwrap();
    assert_eq!(name, format!("{pid}-{started}.json"));
    assert_eq!(
        rec["id"].as_str().unwrap(),
        format!("{}:{pid}:{started}", rec["host"].as_str().unwrap())
    );
    assert!(!rec["host"].as_str().unwrap().is_empty(), "{rec}");
    assert_eq!(rec["v"], 1, "{rec}");
    assert_eq!(rec["tool"], "prec", "{rec}");
    assert_eq!(rec["app"], "demo", "{rec}");
    assert_eq!(rec["label"], "short", "{rec}");
    assert_eq!(rec["device"], "CPU", "{rec}");
    assert!(
        rec["detail"].as_str().unwrap().starts_with("record short "),
        "{rec}"
    );
    let out = PathBuf::from(rec["out"].as_str().unwrap());
    assert!(out.is_absolute(), "{rec}");
    assert!(out.ends_with("tmp/out/demo/clip"), "{rec}");
    assert!(out.starts_with(dir), "{rec}");
    assert_eq!(
        rec["stages"],
        serde_json::json!(["build", "record"]),
        "{rec}"
    );
    assert_eq!(rec["stage"], "record", "{rec}");
    assert_eq!(rec["progress"]["unit"], "seconds", "{rec}");
    assert_eq!(rec["state"], "done", "{rec}");
    assert_eq!(rec["exit"], 0, "{rec}");
    assert!(rec["error"].is_null(), "{rec}");
    let updated = rec["updated"].as_u64().unwrap();
    assert!(updated >= started, "{rec}");
    assert_eq!(rec["ended"].as_u64(), Some(updated), "{rec}");
}

#[test]
fn a_recording_publishes_its_job_record_in_pito_jobs_dir() {
    let (dir, app) = case("jobs-dir");
    let jobs = dir.join("jobs");
    let mut c = record(&dir, &app);
    c.env("PITO_JOBS_DIR", &jobs)
        .env("XDG_RUNTIME_DIR", dir.join("ignored"));
    let o = common::ok(c);
    let (name, raw, rec) = the_record(&jobs, &o);
    finished_well(&name, &raw, &rec, &dir);
    assert!(rec["parent"].is_null(), "{rec}");
    assert!(rec["invoker"].is_null(), "{rec}");
    assert!(rec["pgpu"].is_null(), "{rec}");
    assert!(!dir.join("ignored").exists());
}

#[test]
fn without_pito_jobs_dir_the_record_lands_in_the_runtime_dir() {
    let (dir, app) = case("runtime-dir");
    let runtime = dir.join("run");
    std::fs::create_dir_all(&runtime).unwrap();
    let mut c = record(&dir, &app);
    c.env("PITO_JOBS_DIR", "  ")
        .env("XDG_RUNTIME_DIR", &runtime);
    let o = common::ok(c);
    assert_eq!(names(&runtime), ["pito-jobs"]);
    let (name, raw, rec) = the_record(&runtime.join("pito-jobs"), &o);
    finished_well(&name, &raw, &rec, &dir);
}

#[test]
fn the_parent_job_the_invoker_and_the_gpu_queue_reach_the_record() {
    let (dir, app) = case("callers");
    let jobs = dir.join("jobs");
    let mut c = record(&dir, &app);
    c.env("PITO_JOBS_DIR", &jobs)
        .env("PITO_JOB_ID", " desk:9:100 ")
        .env("PITO_INVOKER", "agent")
        .env("GPU_QUEUE_PID", "4242");
    let o = common::ok(c);
    let (name, raw, rec) = the_record(&jobs, &o);
    finished_well(&name, &raw, &rec, &dir);
    assert_eq!(rec["parent"], "desk:9:100", "{rec}");
    assert_eq!(rec["invoker"], "agent", "{rec}");
    assert_eq!(
        rec["pgpu"],
        serde_json::json!({"pid": 4242, "as": "prec"}),
        "{rec}"
    );
}

#[test]
fn pito_jobs_off_writes_no_record() {
    let (dir, app) = case("off");
    let jobs = dir.join("jobs");
    let mut c = record(&dir, &app);
    c.env("PITO_JOBS", "off")
        .env("PITO_JOBS_DIR", &jobs)
        .env("XDG_RUNTIME_DIR", dir.join("run"));
    common::ok(c);
    assert!(!jobs.exists());
    assert!(!dir.join("run").exists());
    assert!(dir.join("tmp/out/demo/clip/demo-clip-short.cast").is_file());
}

#[test]
fn with_no_folder_named_there_is_no_record_and_the_recording_succeeds() {
    let (dir, app) = case("none");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut c = record(&dir, &app);
    c.env("HOME", &home);
    let o = common::ok(c);
    assert!(dir.join("tmp/out/demo/clip/demo-clip-short.cast").is_file());
    assert!(!stderr(&o).contains("pito-jobs"), "{}", stderr(&o));
    let mut found = Vec::new();
    walk(&dir, &mut found);
    for path in &found {
        assert_ne!(path.file_name().unwrap(), "pito-jobs", "{}", path.display());
        assert!(!looks_like_a_record(path), "{}", path.display());
    }
}

#[test]
fn a_jobs_dir_that_cannot_be_made_never_fails_the_recording() {
    let (dir, app) = case("blocked");
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let mut c = record(&dir, &app);
    c.env("PITO_JOBS_DIR", blocker.join("jobs"));
    common::ok(c);
    assert_eq!(std::fs::read(&blocker).unwrap(), b"x");
    assert!(dir.join("tmp/out/demo/clip/demo-clip-short.cast").is_file());
}
