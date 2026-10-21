use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::env::Settings;
use super::job::start_with;
use super::record::Record;
use super::util::Clock;
use super::{Slot, open};

mod golden;

fn scratch(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tmp")
        .join(format!("unit-jobs-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn settings(dir: &Path) -> Settings {
    Settings {
        dir: Some(dir.to_path_buf()),
        off: false,
        parent: None,
        invoker: None,
        gpu_queue_pid: None,
    }
}

fn manual(at: u64) -> (Clock, Arc<AtomicU64>) {
    Clock::manual(at * 1000)
}

fn at(ms: &AtomicU64, t: u64) {
    ms.store(t * 1000, Ordering::Relaxed);
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".json") && !name.starts_with('.') {
            files.push(entry.path());
        }
    }
    files.sort();
    files
}

fn records(dir: &Path) -> Vec<Record> {
    json_files(dir)
        .iter()
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
        .collect()
}

fn only(dir: &Path) -> Record {
    let found = records(dir);
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap()
}

fn entries(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

fn near(rate: Option<f64>, expected: f64) {
    let Some(got) = rate else {
        panic!("missing rate, expected {expected}");
    };
    assert!(
        (got - expected).abs() < 1e-6,
        "{got} is not near {expected}"
    );
}

#[test]
fn settings_follow_the_environment_map() {
    let off = Settings::from_fn(|key| match key {
        "PITO_JOBS" => Some(" off ".to_string()),
        "PITO_JOBS_DIR" => Some("/custom".to_string()),
        "PITO_JOB_ID" => Some(" desk:9:100 ".to_string()),
        "PITO_INVOKER" => Some("agent".to_string()),
        "GPU_QUEUE_PID" => Some("1440".to_string()),
        "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
        _ => None,
    });
    assert!(off.off);
    assert!(off.dir.is_none());
    assert_eq!(off.parent.as_deref(), Some("desk:9:100"));
    assert_eq!(off.invoker.as_deref(), Some("agent"));
    assert_eq!(off.gpu_queue_pid, Some(1440));

    let custom = Settings::from_fn(|key| match key {
        "PITO_JOBS_DIR" => Some("/custom/jobs".to_string()),
        "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
        "GPU_QUEUE_PID" => Some("nope".to_string()),
        "PITO_JOB_ID" => Some("  ".to_string()),
        _ => None,
    });
    assert!(!custom.off);
    assert_eq!(custom.dir, Some(PathBuf::from("/custom/jobs")));
    assert!(custom.parent.is_none());
    assert_eq!(custom.gpu_queue_pid, None);

    let runtime = Settings::from_fn(|key| match key {
        "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
        _ => None,
    });
    assert_eq!(runtime.dir, Some(PathBuf::from("/run/user/1000/pito-jobs")));

    let missing = Settings::from_fn(|_| None);
    assert!(missing.dir.is_none());
    assert!(!missing.off);
}

#[test]
fn only_the_word_off_turns_records_off() {
    let with = |value: &str| {
        let value = value.to_string();
        Settings::from_fn(move |key| match key {
            "PITO_JOBS" => Some(value.clone()),
            "PITO_JOBS_DIR" => Some("/custom".to_string()),
            _ => None,
        })
    };
    assert!(with("off").off);
    assert!(with("\toff\n").off);
    assert!(!with("OFF").off);
    assert!(!with("0").off);
    assert!(!with("").off);
    assert_eq!(with("on").dir, Some(PathBuf::from("/custom")));
    assert_eq!(with("off").dir, None);
}

#[test]
fn a_blank_jobs_dir_falls_back_to_the_runtime_dir() {
    let blank = Settings::from_fn(|key| match key {
        "PITO_JOBS_DIR" => Some("   ".to_string()),
        "XDG_RUNTIME_DIR" => Some(" /run/user/7 ".to_string()),
        _ => None,
    });
    assert_eq!(blank.dir, Some(PathBuf::from("/run/user/7/pito-jobs")));
    let empty_runtime = Settings::from_fn(|key| match key {
        "XDG_RUNTIME_DIR" => Some(String::new()),
        _ => None,
    });
    assert!(empty_runtime.dir.is_none());
}

#[test]
fn macos_falls_back_to_its_own_folders() {
    let mut temp = |key: &str| match key {
        "TMPDIR" => Some("/var/folders/x/T/".to_string()),
        "HOME" => Some("/u".to_string()),
        _ => None,
    };
    assert_eq!(
        super::env::jobs_dir(&mut temp, true),
        Some(PathBuf::from("/var/folders/x/T/pito-jobs"))
    );
    assert_eq!(super::env::jobs_dir(&mut temp, false), None);
    let mut xdg = |key: &str| match key {
        "TMPDIR" => Some("/var/folders/x/T".to_string()),
        "XDG_RUNTIME_DIR" => Some("/run/user/501".to_string()),
        _ => None,
    };
    assert_eq!(
        super::env::jobs_dir(&mut xdg, true),
        Some(PathBuf::from("/run/user/501/pito-jobs"))
    );
    let mut nothing = |_: &str| None;
    assert_eq!(super::env::jobs_dir(&mut nothing, true), None);
}

#[test]
fn round_trip_matches_the_record() {
    let dir = scratch("round-trip");
    let started = 1_700_000_000_000;
    let (clock, ms) = manual(started);
    let mut cfg = settings(&dir);
    cfg.parent = Some("desk:9:100".to_string());
    cfg.invoker = Some("agent".to_string());
    cfg.gpu_queue_pid = Some(50);
    let out = dir.join("render");
    let job = start_with("prec", Some("demo"), "intro", cfg, clock)
        .detail("demo intro @49fe383 1280x720 30fps")
        .device("CPU")
        .out(&out);

    let running = only(&dir);
    assert_eq!(running.v, 1);
    assert_eq!(running.state, "running");
    assert_eq!(running.pid, std::process::id());
    assert_eq!(
        running.id,
        format!("{}:{}:{}", running.host, running.pid, started)
    );
    assert!(!running.host.is_empty());
    assert!(!running.host.chars().any(|c| c.is_control()));
    assert_eq!(running.started, started);
    assert_eq!(running.tool, "prec");
    assert_eq!(running.app.as_deref(), Some("demo"));
    assert_eq!(running.label, "intro");
    assert_eq!(
        running.detail.as_deref(),
        Some("demo intro @49fe383 1280x720 30fps")
    );
    assert_eq!(running.parent.as_deref(), Some("desk:9:100"));
    assert_eq!(running.invoker.as_deref(), Some("agent"));
    assert_eq!(running.device.as_deref(), Some("CPU"));
    assert_eq!(running.out.as_deref(), Some(out.to_str().unwrap()));
    assert!(running.ended.is_none());
    assert!(running.exit.is_none());
    assert!(running.error.is_none());
    assert!(running.stage.is_none());
    let early = std::fs::read_to_string(&json_files(&dir)[0]).unwrap();
    assert!(!early.contains('\n'));
    assert!(early.contains("\"stage_note\":null"));
    assert!(early.contains("\"ended\":null"));
    assert!(early.contains("\"v\":1"));
    assert!(
        json_files(&dir)
            .iter()
            .all(|path| path.file_name().unwrap().to_string_lossy()
                == format!("{}-{started}.json", running.pid))
    );

    job.stage("build", None, "steps");
    job.stage("replay", Some(600), "frames");
    job.stage_note("dark");
    at(&ms, started + 5_000);
    job.progress(0);
    at(&ms, started + 10_000);
    job.progress(120);
    at(&ms, started + 10_500);
    job.finish(0);
    drop(job);

    let rec = only(&dir);
    assert_eq!(rec.state, "done");
    assert_eq!(rec.stage.as_deref(), Some("replay"));
    assert_eq!(rec.stage_note.as_deref(), Some("dark"));
    let progress = rec.progress.unwrap();
    assert_eq!(progress.done, 120);
    assert_eq!(progress.total, Some(600));
    assert_eq!(progress.unit, "frames");
    near(rec.rate, 24.0);
    let pgpu = rec.pgpu.unwrap();
    assert_eq!(pgpu.pid, 50);
    assert_eq!(pgpu.as_name, "prec");
    assert_eq!(rec.ended, Some(started + 10_500));
    assert_eq!(rec.updated, started + 10_500);
    assert_eq!(rec.exit, Some(0));
    assert!(rec.error.is_none());
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|name| !name.starts_with('.')));
}

#[test]
fn rate_uses_the_longer_window() {
    let dir = scratch("rate");
    let (clock, ms) = manual(1_000_000);
    let job = start_with("prec", Some("demo"), "still", settings(&dir), clock);
    job.stage("replay", Some(600), "frames");
    job.progress(0);
    assert_eq!(only(&dir).rate, None, "one sample is a single still");

    at(&ms, 1_001_000);
    job.progress(10);
    near(only(&dir).rate, 10.0);

    at(&ms, 1_006_000);
    job.progress(60);
    near(only(&dir).rate, 10.0);

    job.stage("replay", Some(9), "frames");
    at(&ms, 2_000_000);
    job.progress(0);
    at(&ms, 2_008_000);
    job.progress(1);
    near(only(&dir).rate, 0.125);
    at(&ms, 2_016_000);
    job.progress(2);
    near(only(&dir).rate, 0.125);

    job.stage("replay", Some(1_000), "frames");
    for step in 0..=5 {
        at(&ms, 3_000_000 + step * 1_000);
        job.progress(step * 100);
    }
    near(only(&dir).rate, 100.0);
    at(&ms, 3_020_000);
    job.progress(510);
    near(only(&dir).rate, 10.0 / 15.0);

    job.stage("replay", Some(100), "frames");
    at(&ms, 4_000_000);
    job.progress(0);
    at(&ms, 4_000_500);
    job.progress(50);
    at(&ms, 4_010_000);
    job.progress(50);
    near(only(&dir).rate, 0.0);
    job.finish(0);
}

#[test]
fn samples_keep_one_point_per_instant_and_prune_the_old_ones() {
    let mut samples = super::rate::Samples::default();
    assert_eq!(samples.rate(), None);
    samples.push(1_000, 0);
    assert_eq!(samples.rate(), None);
    samples.push(1_000, 5);
    assert_eq!(samples.rate(), None, "the same instant overwrites");
    samples.push(2_000, 15);
    near(samples.rate(), 10.0);
    samples.push(30_000, 15);
    samples.push(31_000, 25);
    near(samples.rate(), 10.0 / 29.0);
    samples.clear();
    assert_eq!(samples.rate(), None);
    samples.push(5, 9);
    samples.push(5_005, 4);
    near(samples.rate(), -1.0);
}

#[test]
fn a_seconds_stage_shows_no_rate() {
    let dir = scratch("seconds");
    let (clock, ms) = manual(5_000);
    let job = start_with("prec", None, "take", settings(&dir), clock);
    assert!(only(&dir).app.is_none());
    job.stage("record", Some(30), "seconds");
    job.progress(0);
    at(&ms, 6_000);
    job.progress(1);
    at(&ms, 7_000);
    job.progress(2);
    let rec = only(&dir);
    assert_eq!(rec.rate, None);
    assert_eq!(rec.progress.unwrap().done, 2);

    job.stage("replay", Some(10), "frames");
    assert_eq!(only(&dir).rate, None);
    at(&ms, 8_000);
    job.progress(0);
    at(&ms, 9_000);
    job.progress(10);
    near(only(&dir).rate, 10.0);
    job.finish(0);
}

#[test]
fn progress_writes_at_most_once_a_second() {
    let dir = scratch("throttle");
    let (clock, ms) = manual(1_000);
    let job = start_with("prec", Some("demo"), "clip", settings(&dir), clock);
    job.stage("replay", Some(100), "frames");
    at(&ms, 1_100);
    job.progress(7);
    assert_eq!(only(&dir).progress.unwrap().done, 7);

    at(&ms, 1_100 + 999);
    job.progress(8);
    assert_eq!(only(&dir).progress.unwrap().done, 7);
    at(&ms, 1_100 + 1_000);
    job.progress(9);
    assert_eq!(only(&dir).progress.unwrap().done, 9);

    at(&ms, 1_100 + 1_100);
    job.stage("derive", Some(2), "outputs");
    let rec = only(&dir);
    assert_eq!(rec.stage.as_deref(), Some("derive"));
    let progress = rec.progress.unwrap();
    assert_eq!(progress.done, 0);
    assert_eq!(progress.unit, "outputs");
    assert_eq!(rec.updated, 1_100 + 1_100);

    at(&ms, 1_100 + 1_200);
    job.progress(1);
    assert_eq!(only(&dir).progress.unwrap().done, 1);

    at(&ms, 1_100 + 1_300);
    job.progress(2);
    job.finish(0);
    let rec = only(&dir);
    assert_eq!(rec.progress.unwrap().done, 2);
    assert_eq!(rec.state, "done");
    assert_eq!(rec.updated, 1_100 + 1_300);
}

#[test]
fn stage_resets_progress_and_note() {
    let dir = scratch("stage");
    let (clock, ms) = manual(50);
    let job = start_with("prec", Some("demo"), "clip", settings(&dir), clock);
    job.progress(4);
    assert!(only(&dir).progress.is_none());
    job.stage("replay", Some(10), "frames");
    job.stage_note("dark");
    at(&ms, 1_000);
    job.progress(3);
    let rec = only(&dir);
    assert_eq!(rec.stage_note.as_deref(), Some("dark"));
    assert_eq!(rec.progress.unwrap().done, 3);
    job.stage("derive", Some(2), "outputs");
    let rec = only(&dir);
    assert!(rec.stage_note.is_none());
    let progress = rec.progress.unwrap();
    assert_eq!(progress.done, 0);
    assert_eq!(progress.total, Some(2));
    assert_eq!(progress.unit, "outputs");
    assert_eq!(rec.rate, None);
    assert_eq!(rec.state, "running");
    job.finish(2);
    let rec = only(&dir);
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(2));
    assert!(rec.error.is_none());
}

#[test]
fn long_text_is_cut_on_a_char_boundary() {
    let dir = scratch("truncate");
    let label = format!("A{}", "😀".repeat(2_000));
    let note = "é".repeat(8_000);
    let error = format!("{}\"\\{}", "n".repeat(5_000), "😀".repeat(1_500));
    let detail = "keep-me-intact";
    let (clock, _) = manual(9);
    let job = start_with("prec", Some("demo"), &label, settings(&dir), clock).detail(detail);
    job.stage("replay", Some(1), "frames");
    job.stage_note(&note);
    job.fail(2, &error);
    let files = json_files(&dir);
    let len = std::fs::metadata(&files[0]).unwrap().len();
    assert!(len <= 4096, "{len}");
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(!text.contains('\n'));
    let rec = only(&dir);
    assert_eq!(rec.detail.as_deref(), Some(detail));
    assert!(label.starts_with(&rec.label));
    assert!(rec.label.chars().count() < label.chars().count());
    assert!(!rec.label.contains('\u{FFFD}'));
    let got_note = rec.stage_note.unwrap();
    assert!(note.starts_with(&got_note));
    assert!(got_note.chars().count() < note.chars().count());
    let got_error = rec.error.unwrap();
    assert!(error.starts_with(&got_error));
    assert!(got_error.chars().count() < error.chars().count());
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(2));
}

#[test]
fn cap_keeps_short_text_and_cuts_long_text_on_a_boundary() {
    assert_eq!(super::fit::cap("short".to_string()), "short");
    let exact = "a".repeat(4096);
    assert_eq!(super::fit::cap(exact.clone()), exact);
    let over = format!("{}é", "a".repeat(4095));
    assert_eq!(super::fit::cap(over), "a".repeat(4095));
}

#[test]
fn a_record_far_over_the_limit_is_shrunk_to_fit() {
    let (clock, _) = manual(1);
    let dir = scratch("unfit");
    let job = start_with("prec", Some("demo"), "intro", settings(&dir), clock);
    drop(job);
    let mut rec = only(&dir);
    rec.label = "l".repeat(4_000);
    rec.detail = Some("d".repeat(4_000));
    rec.stage_note = Some("n".repeat(4_000));
    rec.error = Some("e".repeat(4_000));
    rec.device = Some("v".repeat(4_000));
    rec.out = Some("o".repeat(4_000));
    rec.app = Some("a".repeat(4_000));
    rec.tool = "t".repeat(4_000);
    rec.host = "h".repeat(4_000);
    rec.id = "i".repeat(4_000);
    rec.stages = vec!["s".repeat(4_000); 4];
    let bytes = super::fit::encode(&rec);
    assert!(!bytes.is_empty());
    assert!(bytes.len() <= super::fit::LIMIT, "{}", bytes.len());
    let back: Record = serde_json::from_slice(&bytes).unwrap();
    assert!(back.stages.is_empty());
    assert!(back.label.is_empty());
    assert_eq!(back.detail.as_deref(), Some(""));
    assert_eq!(back.v, 1);
    assert_eq!(back.pid, rec.pid);
    assert_eq!(back.state, rec.state);
}

#[test]
fn with_the_preferred_fields_empty_the_largest_other_field_shrinks_first() {
    let (clock, _) = manual(1);
    let dir = scratch("unfit-extra");
    let job = start_with("prec", None, "", settings(&dir), clock);
    drop(job);
    let mut rec = only(&dir);
    rec.error = None;
    rec.pgpu = Some(super::record::Pgpu {
        pid: 7,
        as_name: "a".repeat(1_500),
    });
    rec.stage = Some("s".repeat(1_500));
    rec.progress = Some(super::record::Progress {
        done: 1,
        total: None,
        unit: "u".repeat(1_500),
    });
    rec.host = "h".repeat(1_500);
    rec.id = "i".repeat(1_500);
    let bytes = super::fit::encode(&rec);
    assert_eq!(bytes.len(), super::fit::LIMIT);
    let back: Record = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(back.pgpu.unwrap().as_name, "");
    assert_eq!(back.stage.as_deref(), Some(""));
    let unit = back.progress.unwrap().unit;
    assert!(!unit.is_empty() && unit.len() < 1_500, "{}", unit.len());
    assert!(unit.bytes().all(|b| b == b'u'));
    assert_eq!(back.host, rec.host);
    assert_eq!(back.id, rec.id);
    assert_eq!(back.tool, "prec");
}

#[test]
fn off_writes_nothing() {
    let dir = scratch("off");
    let mut cfg = settings(&dir);
    cfg.off = true;
    cfg.invoker = Some("agent".to_string());
    let (clock, _) = manual(3);
    let job = start_with("prec", Some("demo"), "clip", cfg, clock);
    job.stages(&["record", "replay"]);
    job.stage("replay", Some(3), "frames");
    job.stage_note("dark");
    job.progress(1);
    job.finish(0);
    drop(job);
    assert_eq!(entries(&dir), 0);
}

#[test]
fn parent_without_invoker_is_recorded_alone() {
    let dir = scratch("parent");
    let mut cfg = settings(&dir);
    cfg.parent = Some("desk:1:2".to_string());
    let (clock, _) = manual(4);
    let job = start_with("prec", None, "logo", cfg, clock);
    let rec = only(&dir);
    assert_eq!(rec.parent.as_deref(), Some("desk:1:2"));
    assert!(rec.invoker.is_none());
    assert!(rec.pgpu.is_none());
    job.finish(0);
}

#[test]
fn gpu_queue_pid_names_the_tool() {
    let dir = scratch("pgpu");
    let mut cfg = settings(&dir);
    cfg.gpu_queue_pid = Some(1440);
    let (clock, _) = manual(6);
    let job = start_with("prec", Some("demo"), "clip", cfg, clock);
    let pgpu = only(&dir).pgpu.unwrap();
    assert_eq!(pgpu.pid, 1440);
    assert_eq!(pgpu.as_name, "prec");
    let text = std::fs::read_to_string(&json_files(&dir)[0]).unwrap();
    assert!(
        text.contains(r#""pgpu":{"pid":1440,"as":"prec"}"#),
        "{text}"
    );
    job.finish(0);
}

#[test]
fn out_paths_are_absolute() {
    let dir = scratch("out");
    let (clock, _) = manual(8);
    let relative = start_with("prec", None, "clip", settings(&dir), clock).out("rel/intro");
    let expected = std::env::current_dir().unwrap().join("rel/intro");
    assert_eq!(only(&dir).out.as_deref(), Some(expected.to_str().unwrap()));
    relative.finish(0);

    let dir = scratch("out-abs");
    let abs = dir.join("absolute");
    let (clock, _) = manual(9);
    let job = start_with("prec", None, "clip", settings(&dir), clock).out(&abs);
    assert_eq!(only(&dir).out.as_deref(), Some(abs.to_str().unwrap()));
    job.finish(0);
}

#[test]
fn drop_records_failed_unless_already_finished() {
    let dir = scratch("drop");
    let (clock, ms) = manual(11);
    {
        let job = start_with("prec", Some("demo"), "clip", settings(&dir), clock);
        job.stage("replay", Some(4), "frames");
        at(&ms, 40);
    }
    let rec = only(&dir);
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(1));
    assert_eq!(rec.error.as_deref(), Some("dropped"));
    assert_eq!(rec.ended, Some(40));

    let (clock, _) = manual(12);
    let job = start_with("prec", Some("demo"), "clip", settings(&dir), clock);
    job.finish(0);
    drop(job);
    let found = records(&dir);
    assert_eq!(found.len(), 2);
    let done = found.iter().find(|rec| rec.started == 12).unwrap();
    assert_eq!(done.state, "done");
    assert_eq!(done.exit, Some(0));
    assert!(done.error.is_none());
}

#[test]
fn fail_wins_and_a_zero_code_becomes_one() {
    let dir = scratch("fail");
    let (clock, _) = manual(13);
    let job = start_with("prec", None, "clip", settings(&dir), clock);
    job.fail(0, "chromium died");
    job.finish(0);
    let rec = only(&dir);
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(1));
    assert_eq!(rec.error.as_deref(), Some("chromium died"));
    drop(job);
    let rec = only(&dir);
    assert_eq!(rec.error.as_deref(), Some("chromium died"));
}

#[test]
fn a_missing_directory_or_a_blocked_path_does_not_panic() {
    let dir = scratch("io");
    let (clock, _) = manual(14);
    let job = start_with(
        "prec",
        None,
        "clip",
        Settings {
            dir: None,
            off: false,
            parent: None,
            invoker: None,
            gpu_queue_pid: None,
        },
        clock,
    );
    job.stage("replay", Some(1), "frames");
    job.progress(1);
    job.finish(0);

    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let (clock, _) = manual(15);
    let job = start_with(
        "prec",
        None,
        "clip",
        Settings {
            dir: Some(blocker.join("jobs")),
            off: false,
            parent: None,
            invoker: None,
            gpu_queue_pid: None,
        },
        clock,
    );
    job.stage("replay", Some(1), "frames");
    job.progress(1);
    job.fail(4, "disk full");
    drop(job);
    assert!(blocker.is_file());
    assert_eq!(std::fs::read(&blocker).unwrap(), b"x");
    assert_eq!(entries(&dir), 1);
}

#[test]
fn a_folder_that_does_not_exist_yet_is_made() {
    let dir = scratch("made");
    let nested = dir.join("run").join("pito-jobs");
    let (clock, _) = manual(16);
    let job = start_with("prec", None, "clip", settings(&nested), clock);
    job.finish(0);
    assert_eq!(only(&nested).state, "done");
}

#[test]
fn a_write_that_fails_leaves_no_temp_file_and_the_next_one_lands() {
    let dir = scratch("retry");
    let (clock, ms) = manual(17);
    let job = start_with("prec", None, "clip", settings(&dir), clock);
    let file = json_files(&dir)[0].clone();
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::create_dir(&tmp).unwrap();
    at(&ms, 18);
    job.stage("replay", Some(2), "frames");
    let stuck = only(&dir);
    assert!(stuck.stage.is_none());
    assert_eq!(stuck.updated, 17);
    std::fs::remove_dir(&tmp).unwrap();
    at(&ms, 19);
    job.stage_note("dark");
    let rec = only(&dir);
    assert_eq!(rec.stage.as_deref(), Some("replay"));
    assert_eq!(rec.stage_note.as_deref(), Some("dark"));
    assert_eq!(rec.updated, 19);
    job.finish(0);
    assert!(!tmp.exists());
    assert_eq!(entries(&dir), 1);
}

#[test]
fn atomic_write_replaces_the_file_and_leaves_no_temp() {
    let dir = scratch("atomic");
    super::util::atomic_write(&dir, "a.json", b"one").unwrap();
    super::util::atomic_write(&dir, "a.json", b"two").unwrap();
    assert_eq!(std::fs::read(dir.join("a.json")).unwrap(), b"two");
    assert_eq!(entries(&dir), 1);
    let blocked = dir.join("a.json").join("sub");
    assert!(super::util::atomic_write(&blocked, "b.json", b"x").is_err());
    assert_eq!(std::fs::read(dir.join("a.json")).unwrap(), b"two");
}

#[test]
fn a_stale_temp_file_is_replaced_and_never_kept() {
    let dir = scratch("stale-tmp");
    std::fs::write(dir.join(".a.json.tmp"), b"stale stale stale").unwrap();
    super::util::atomic_write(&dir, "a.json", b"new").unwrap();
    assert_eq!(std::fs::read(dir.join("a.json")).unwrap(), b"new");
    assert_eq!(entries(&dir), 1);
}

#[cfg(unix)]
#[test]
fn a_symlink_in_the_temp_file_s_place_is_never_followed() {
    let dir = scratch("symlink-tmp");
    let jobs = dir.join("jobs");
    std::fs::create_dir(&jobs).unwrap();
    let victim = dir.join("victim");
    std::fs::write(&victim, b"keep me").unwrap();
    let (clock, ms) = manual(20);
    let job = start_with("prec", None, "clip", settings(&jobs), clock);
    let file = json_files(&jobs)[0].clone();
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    let tmp = jobs.join(format!(".{name}.tmp"));
    std::os::unix::fs::symlink(&victim, &tmp).unwrap();
    at(&ms, 21);
    job.stage("replay", Some(2), "frames");
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
    assert!(std::fs::symlink_metadata(&tmp).is_err());
    let rec = only(&jobs);
    assert_eq!(rec.stage.as_deref(), Some("replay"));
    assert_eq!(rec.updated, 21);
    job.finish(0);
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
    assert_eq!(only(&jobs).state, "done");
    assert_eq!(entries(&jobs), 1);
}

#[cfg(unix)]
#[test]
fn a_symlink_in_the_record_s_place_is_replaced_not_followed() {
    let dir = scratch("symlink-dest");
    let jobs = dir.join("jobs");
    std::fs::create_dir(&jobs).unwrap();
    let victim = dir.join("victim");
    std::fs::write(&victim, b"keep me").unwrap();
    std::os::unix::fs::symlink(&victim, jobs.join("a.json")).unwrap();
    super::util::atomic_write(&jobs, "a.json", b"record").unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
    assert!(
        !std::fs::symlink_metadata(jobs.join("a.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(jobs.join("a.json")).unwrap(), b"record");
}

#[test]
fn filled_trims_and_drops_blank_values() {
    use super::util::filled;
    assert_eq!(filled(None), None);
    assert_eq!(filled(Some(String::new())), None);
    assert_eq!(filled(Some(" \t\n".to_string())), None);
    assert_eq!(filled(Some(" a b ".to_string())), Some("a b".to_string()));
}

#[test]
fn stages_are_written_and_round_trip() {
    let dir = scratch("stages");
    let (clock, _) = manual(70);
    let job = start_with("prec", Some("demo"), "clip", settings(&dir), clock);
    job.stage("record", None, "seconds");
    let early = std::fs::read_to_string(&json_files(&dir)[0]).unwrap();
    assert!(!early.contains("stages"), "{early}");
    assert!(only(&dir).stages.is_empty());

    job.stages(&["record", "replay"]);
    assert_eq!(only(&dir).stages, ["record", "replay"]);
    job.stages(&["record", "replay", "master"]);
    let rec = only(&dir);
    assert_eq!(rec.stages, ["record", "replay", "master"]);
    assert_eq!(rec.stage.as_deref(), Some("record"));
    let text = std::fs::read_to_string(&json_files(&dir)[0]).unwrap();
    assert!(
        text.contains("\"stages\":[\"record\",\"replay\",\"master\"]"),
        "{text}"
    );
    let back: Record = serde_json::from_str(&text).unwrap();
    assert_eq!(back, rec);

    let many: Vec<String> = (0..20).map(|n| format!("s{n}")).collect();
    let names: Vec<&str> = many.iter().map(String::as_str).collect();
    job.stages(&names);
    assert_eq!(only(&dir).stages.len(), 16);
    job.finish(0);
}

#[test]
fn stages_are_a_no_op_after_finish() {
    let dir = scratch("stages-finished");
    let (clock, _) = manual(80);
    let job = start_with("prec", Some("demo"), "clip", settings(&dir), clock);
    job.stages(&["record", "replay"]);
    job.finish(0);
    job.stages(&["record", "replay", "master"]);
    assert_eq!(only(&dir).stages, ["record", "replay"]);
}

#[test]
fn oversized_stages_are_dropped_to_keep_the_file_small() {
    let dir = scratch("stages-big");
    let (clock, _) = manual(90);
    let job = start_with("prec", Some("demo"), "clip", settings(&dir), clock);
    let long = "x".repeat(2_000);
    job.stages(&[&long, &long, &long]);
    let len = std::fs::metadata(&json_files(&dir)[0]).unwrap().len();
    assert!(len <= 4096, "{len}");
    assert_eq!(only(&dir).label, "clip");
    job.finish(0);
}

#[test]
fn a_wall_clock_step_back_does_not_loosen_the_progress_throttle_or_the_rate() {
    let dir = scratch("wall-step");
    let (clock, wall, mono) = Clock::split(1_000_000_000);
    let job = start_with("prec", Some("demo"), "clip", settings(&dir), clock);
    job.stage("replay", Some(100), "frames");
    mono.store(1_000_000_000 + 1_000_000, Ordering::Relaxed);
    wall.store(1_000_000_000 + 1_000_000, Ordering::Relaxed);
    job.progress(10);
    assert_eq!(only(&dir).progress.unwrap().done, 10);

    wall.store(500_000_000, Ordering::Relaxed);
    mono.store(1_000_000_000 + 1_100_000, Ordering::Relaxed);
    job.progress(11);
    assert_eq!(only(&dir).progress.unwrap().done, 10);

    mono.store(1_000_000_000 + 2_000_000, Ordering::Relaxed);
    job.progress(20);
    let rec = only(&dir);
    assert_eq!(rec.progress.unwrap().done, 20);
    assert_eq!(rec.updated, 500_000);
    near(rec.rate, 10.0);
    job.finish(0);
}

fn slot_open(slot: &Slot, settings: Settings, clock: Clock, label: &str, out: &Path) {
    slot.start(|| {
        open(
            Some("demo"),
            label,
            "demo intro 1280x720".to_string(),
            "CPU".to_string(),
            out,
            settings,
            clock,
        )
    });
}

#[test]
fn a_render_publishes_its_whole_life() {
    let dir = scratch("slot-life");
    let slot = Slot::new();
    let (clock, ms) = manual(1_000);
    slot_open(
        &slot,
        settings(&dir),
        clock,
        "intro",
        Path::new("/srv/renders/demo"),
    );
    let rec = only(&dir);
    assert_eq!(rec.tool, crate::render::TOOL);
    assert_eq!(rec.app.as_deref(), Some("demo"));
    assert_eq!(rec.label, "intro");
    assert_eq!(rec.detail.as_deref(), Some("demo intro 1280x720"));
    assert_eq!(rec.device.as_deref(), Some("CPU"));
    assert_eq!(rec.out.as_deref(), Some("/srv/renders/demo"));
    assert_eq!(rec.state, "running");

    slot.stages(&["record", "replay", "archive"]);
    slot.stage("record", None, "seconds");
    assert_eq!(only(&dir).stage_note, None);
    slot.context(Some("dark".to_string()));
    slot.stage("replay", Some(90), "frames");
    let rec = only(&dir);
    assert_eq!(rec.stages, ["record", "replay", "archive"]);
    assert_eq!(rec.stage.as_deref(), Some("replay"));
    assert_eq!(rec.stage_note.as_deref(), Some("dark"));
    let progress = rec.progress.unwrap();
    assert_eq!((progress.done, progress.total), (0, Some(90)));

    at(&ms, 2_000);
    slot.progress(45);
    assert_eq!(only(&dir).progress.unwrap().done, 45);
    slot.note("archive queued");
    assert_eq!(
        only(&dir).stage_note.as_deref(),
        Some("dark archive queued")
    );
    slot.context(None);
    slot.note("archive refused");
    assert_eq!(only(&dir).stage_note.as_deref(), Some("archive refused"));

    at(&ms, 3_000);
    slot.end(&Ok(()));
    let rec = only(&dir);
    assert_eq!(rec.state, "done");
    assert_eq!(rec.exit, Some(0));
    assert_eq!(rec.ended, Some(3_000));
    assert!(rec.error.is_none());

    slot.end(&Err("late".to_string()));
    slot.cancelled(130, "SIGINT");
    assert_eq!(only(&dir).state, "done");
}

#[test]
fn a_failed_render_records_its_error_with_exit_one() {
    let dir = scratch("slot-fail");
    let slot = Slot::new();
    let (clock, _) = manual(5);
    slot_open(&slot, settings(&dir), clock, "intro", Path::new("/srv/out"));
    slot.end(&Err("tape: no such file".to_string()));
    let rec = only(&dir);
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(1));
    assert_eq!(rec.error.as_deref(), Some("tape: no such file"));
}

#[test]
fn a_cancelled_render_names_the_signal_and_keeps_its_code() {
    let dir = scratch("slot-cancel");
    let slot = Slot::new();
    let (clock, _) = manual(6);
    slot_open(&slot, settings(&dir), clock, "intro", Path::new("/srv/out"));
    slot.stage("replay", Some(10), "frames");
    slot.cancelled(143, "SIGTERM");
    slot.end(&Ok(()));
    let rec = only(&dir);
    assert_eq!(rec.state, "failed");
    assert_eq!(rec.exit, Some(143));
    assert_eq!(rec.error.as_deref(), Some("cancelled by SIGTERM"));
    assert_eq!(rec.stage.as_deref(), Some("replay"));
}

#[test]
fn a_second_start_is_ignored_and_writes_nothing() {
    let dir = scratch("slot-twice");
    let slot = Slot::new();
    let (clock, _) = manual(7);
    slot_open(&slot, settings(&dir), clock, "first", Path::new("/srv/out"));
    let called = AtomicBool::new(false);
    let (clock, _) = manual(8);
    let second = settings(&dir);
    slot.start(|| {
        called.store(true, Ordering::Relaxed);
        open(
            None,
            "second",
            String::new(),
            String::new(),
            Path::new("/x"),
            second,
            clock,
        )
    });
    assert!(!called.load(Ordering::Relaxed));
    let rec = only(&dir);
    assert_eq!(rec.label, "first");
    assert_eq!(rec.started, 7);
    slot.end(&Ok(()));
    assert_eq!(only(&dir).state, "done");
}

#[test]
fn every_call_before_start_is_a_quiet_no_op() {
    let dir = scratch("slot-early");
    let slot = Slot::new();
    slot.stages(&["record"]);
    slot.context(Some("dark".to_string()));
    slot.stage("record", Some(1), "seconds");
    slot.note("archive queued");
    slot.progress(3);
    slot.cancelled(130, "SIGINT");
    slot.end(&Err("boom".to_string()));
    assert_eq!(entries(&dir), 0);
    let (clock, _) = manual(9);
    slot_open(&slot, settings(&dir), clock, "intro", Path::new("/srv/out"));
    let rec = only(&dir);
    assert_eq!(rec.state, "running");
    assert!(rec.stage.is_none());
    assert!(rec.stages.is_empty());
    assert!(rec.progress.is_none());
    slot.stage("record", None, "seconds");
    assert_eq!(only(&dir).stage_note.as_deref(), Some("dark"));
    slot.end(&Ok(()));
}

#[test]
fn pito_jobs_off_writes_nothing_through_a_whole_render() {
    let dir = scratch("slot-off");
    let target = dir.to_string_lossy().into_owned();
    let cfg = Settings::from_fn(|key| match key {
        "PITO_JOBS" => Some("off".to_string()),
        "PITO_JOBS_DIR" => Some(target.clone()),
        "XDG_RUNTIME_DIR" => Some(target.clone()),
        _ => None,
    });
    let slot = Slot::new();
    let (clock, ms) = manual(10);
    slot_open(&slot, cfg, clock, "intro", Path::new("/srv/out"));
    slot.stages(&["record", "replay"]);
    slot.stage("replay", Some(4), "frames");
    at(&ms, 2_000);
    slot.progress(4);
    slot.note("archive queued");
    slot.end(&Ok(()));
    assert_eq!(entries(&dir), 0);
}

#[test]
fn no_runtime_dir_means_no_record_and_the_render_goes_on() {
    let dir = scratch("slot-none");
    let target = dir.to_string_lossy().into_owned();
    let cfg = Settings::from_fn(|key| match key {
        "PITO_JOBS_DIR" | "XDG_RUNTIME_DIR" => Some("  ".to_string()),
        "TMPDIR" if !super::env::MACOS => Some(target.clone()),
        _ => None,
    });
    assert!(cfg.dir.is_none());
    let slot = Slot::new();
    let (clock, _) = manual(11);
    slot_open(&slot, cfg, clock, "intro", Path::new("/srv/out"));
    slot.stage("replay", Some(4), "frames");
    slot.progress(4);
    slot.end(&Err("boom".to_string()));
    assert_eq!(entries(&dir), 0);
}

#[test]
fn a_blocked_jobs_dir_never_fails_the_render() {
    let dir = scratch("slot-blocked");
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let slot = Slot::new();
    let (clock, _) = manual(12);
    slot_open(
        &slot,
        settings(&blocker),
        clock,
        "intro",
        Path::new("/srv/out"),
    );
    slot.stages(&["replay"]);
    slot.stage("replay", Some(2), "frames");
    slot.progress(2);
    slot.cancelled(130, "SIGINT");
    assert_eq!(std::fs::read(&blocker).unwrap(), b"x");
    assert_eq!(entries(&dir), 1);
}

#[test]
fn the_context_joins_the_extra_note() {
    let slot = Slot::new();
    assert_eq!(slot.with_context(None), None);
    assert_eq!(slot.with_context(Some("x")).as_deref(), Some("x"));
    slot.context(Some("dark".to_string()));
    assert_eq!(slot.with_context(None).as_deref(), Some("dark"));
    assert_eq!(
        slot.with_context(Some("archive queued")).as_deref(),
        Some("dark archive queued")
    );
    slot.context(None);
    assert_eq!(slot.with_context(None), None);
}
