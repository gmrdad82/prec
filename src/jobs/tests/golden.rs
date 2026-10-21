use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::host::{host_name, pid_start};
use super::super::job::{Job, start_with};
use super::super::util::Clock;
use super::super::{Slot, open};
use super::{scratch, settings};

const T0: u64 = 1_791_000_000_000;

const FULL: &[(&str, &str)] = &[
    (
        "start",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":null,"invoker":"agent","parent":"desk:9:100","state":"running","stage":null,"stage_note":null,"progress":null,"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":null,"out":null,"started":1791000000000,"updated":1791000000000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "detail",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":null,"stage_note":null,"progress":null,"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":null,"out":null,"started":1791000000000,"updated":1791000000000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "device",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":null,"stage_note":null,"progress":null,"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":null,"started":1791000000000,"updated":1791000000000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "out",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":null,"stage_note":null,"progress":null,"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000000000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "stages",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":null,"stages":["build","record","replay","master","archive"],"stage_note":null,"progress":null,"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000000000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "stage-build",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"build","stages":["build","record","replay","master","archive"],"stage_note":null,"progress":{"done":0,"total":null,"unit":"steps"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000000250,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "note-build",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"build","stages":["build","record","replay","master","archive"],"stage_note":"demo@abc1234","progress":{"done":0,"total":null,"unit":"steps"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000000250,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "stage-record",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"record","stages":["build","record","replay","master","archive"],"stage_note":null,"progress":{"done":0,"total":null,"unit":"seconds"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000004000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "progress-record-1",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"record","stages":["build","record","replay","master","archive"],"stage_note":null,"progress":{"done":1,"total":null,"unit":"seconds"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000005000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "progress-record-throttled",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"record","stages":["build","record","replay","master","archive"],"stage_note":null,"progress":{"done":1,"total":null,"unit":"seconds"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000005000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "stage-replay",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"replay","stages":["build","record","replay","master","archive"],"stage_note":null,"progress":{"done":0,"total":90,"unit":"frames"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000006000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "note-replay",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"replay","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":0,"total":90,"unit":"frames"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000006000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "progress-replay-0",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"replay","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":0,"total":90,"unit":"frames"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000006000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "progress-replay-30",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"replay","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":30,"total":90,"unit":"frames"},"rate":30.0,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000007000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "progress-replay-70",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"replay","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":70,"total":90,"unit":"frames"},"rate":20.0,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000009500,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "progress-replay-80",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"replay","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":80,"total":90,"unit":"frames"},"rate":17.39130434782609,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000010600,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "note-extra",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"replay","stages":["build","record","replay","master","archive"],"stage_note":"dark archive skipped: no archive","progress":{"done":80,"total":90,"unit":"frames"},"rate":17.39130434782609,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000010600,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "stage-archive",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"archive","stages":["build","record","replay","master","archive"],"stage_note":null,"progress":{"done":0,"total":1,"unit":"outputs"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000010600,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "note-archive",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"archive","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":0,"total":1,"unit":"outputs"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000010600,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "progress-archive",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"running","stage":"archive","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":1,"total":1,"unit":"outputs"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000011000,"ended":null,"exit":null,"error":null}"##,
    ),
    (
        "finish",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"done","stage":"archive","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":1,"total":1,"unit":"outputs"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000011200,"ended":1791000011200,"exit":0,"error":null}"##,
    ),
    (
        "after-finish",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"done","stage":"archive","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":1,"total":1,"unit":"outputs"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000011200,"ended":1791000011200,"exit":0,"error":null}"##,
    ),
    (
        "after-drop",
        r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"demo intro 1280x720 30 fps","invoker":"agent","parent":"desk:9:100","state":"done","stage":"archive","stages":["build","record","replay","master","archive"],"stage_note":"dark","progress":{"done":1,"total":1,"unit":"outputs"},"rate":null,"pgpu":{"pid":4242,"as":"prec"},"device":"cpu: 4 threads","out":"/srv/renders/demo","started":1791000000000,"updated":1791000011200,"ended":1791000011200,"exit":0,"error":null}"##,
    ),
];

const CANCEL_START: &str = r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":null,"label":"take","detail":null,"invoker":null,"parent":null,"state":"running","stage":null,"stage_note":null,"progress":null,"rate":null,"pgpu":null,"device":null,"out":null,"started":1791000000000,"updated":1791000000000,"ended":null,"exit":null,"error":null}"##;

const CANCEL_FAIL: &str = r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":null,"label":"take","detail":null,"invoker":null,"parent":null,"state":"failed","stage":null,"stage_note":null,"progress":null,"rate":null,"pgpu":null,"device":null,"out":null,"started":1791000000000,"updated":1791000001500,"ended":1791000001500,"exit":130,"error":"cancelled by SIGINT"}"##;

const ERROR_FAIL: &str = r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"still","detail":null,"invoker":null,"parent":null,"state":"failed","stage":null,"stage_note":null,"progress":null,"rate":null,"pgpu":null,"device":null,"out":null,"started":1791000000000,"updated":1791000000042,"ended":1791000000042,"exit":1,"error":"boom \"q\" \\ é😀\n\ttab"}"##;

const BIG_DETAIL: &str = r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"@D@","invoker":null,"parent":null,"state":"running","stage":null,"stage_note":null,"progress":null,"rate":null,"pgpu":null,"device":null,"out":null,"started":1791000000000,"updated":1791000000000,"ended":null,"exit":null,"error":null}"##;

const BIG_FINISH: &str = r##"{"v":1,"id":"@HOST@:@PID@:1791000000000","host":"@HOST@","pid":@PID@,"pid_start":@PSTART@,"tool":"prec","app":"demo","label":"intro","detail":"@D@","invoker":null,"parent":null,"state":"done","stage":null,"stage_note":null,"progress":null,"rate":null,"pgpu":null,"device":null,"out":null,"started":1791000000000,"updated":1791000000007,"ended":1791000000007,"exit":0,"error":null}"##;
fn fill(template: &str) -> String {
    template
        .replace("@HOST@", &host_name())
        .replace("@PID@", &std::process::id().to_string())
        .replace("@PSTART@", &pid_start().to_string())
}

fn full_settings(dir: &Path) -> super::Settings {
    let mut cfg = settings(dir);
    cfg.parent = Some("desk:9:100".to_string());
    cfg.invoker = Some("agent".to_string());
    cfg.gpu_queue_pid = Some(4242);
    cfg
}

fn clock() -> (Clock, std::sync::Arc<AtomicU64>) {
    Clock::manual(T0 * 1000)
}

fn at(clock: &AtomicU64, ms: u64) {
    clock.store((T0 + ms) * 1000, Ordering::Relaxed);
}

fn written(dir: &Path) -> String {
    let names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [format!("{}-{T0}.json", std::process::id())],
        "one record and no temporary file"
    );
    std::fs::read_to_string(dir.join(&names[0])).unwrap()
}

fn golden(tag: &str) -> String {
    let (_, template) = FULL.iter().find(|(name, _)| *name == tag).unwrap();
    fill(template)
}

fn same(dir: &Path, tag: &str) {
    assert_eq!(written(dir), golden(tag), "{tag}");
}

fn sized(template: &str) -> String {
    let rest = fill(template).len() - "@D@".len();
    fill(template).replace("@D@", &"d".repeat(4096 - rest))
}

#[test]
fn the_goldens_cover_every_step_of_a_render() {
    let mut tags: Vec<&str> = FULL.iter().map(|(tag, _)| *tag).collect();
    tags.sort_unstable();
    tags.dedup();
    assert_eq!(tags.len(), FULL.len());
    for (tag, template) in FULL.iter().copied() {
        let filled = fill(template);
        for token in ["@HOST@", "@PID@", "@PSTART@"] {
            assert!(!filled.contains(token), "{tag}");
        }
        let value: serde_json::Value = serde_json::from_str(&filled).unwrap();
        assert_eq!(value["v"], 1, "{tag}");
    }
}

#[test]
fn a_job_writes_the_bytes_pito_jobs_wrote() {
    let dir = scratch("golden-job");
    let (clock, ms) = clock();
    let job = start_with("prec", Some("demo"), "intro", full_settings(&dir), clock);
    same(&dir, "start");
    let job = job.detail("demo intro 1280x720 30 fps");
    same(&dir, "detail");
    let job = job.device("cpu: 4 threads");
    same(&dir, "device");
    let job = job.out("/srv/renders/demo");
    same(&dir, "out");
    job.stages(&["build", "record", "replay", "master", "archive"]);
    same(&dir, "stages");
    at(&ms, 250);
    job.stage("build", None, "steps");
    same(&dir, "stage-build");
    job.stage_note("demo@abc1234");
    same(&dir, "note-build");
    at(&ms, 4_000);
    job.stage("record", None, "seconds");
    same(&dir, "stage-record");
    at(&ms, 5_000);
    job.progress(1);
    same(&dir, "progress-record-1");
    at(&ms, 5_500);
    job.progress(2);
    same(&dir, "progress-record-throttled");
    at(&ms, 6_000);
    job.stage("replay", Some(90), "frames");
    same(&dir, "stage-replay");
    job.stage_note("dark");
    same(&dir, "note-replay");
    job.progress(0);
    same(&dir, "progress-replay-0");
    at(&ms, 7_000);
    job.progress(30);
    same(&dir, "progress-replay-30");
    at(&ms, 9_500);
    job.progress(70);
    same(&dir, "progress-replay-70");
    at(&ms, 10_600);
    job.progress(80);
    same(&dir, "progress-replay-80");
    job.stage_note("dark archive skipped: no archive");
    same(&dir, "note-extra");
    job.stage("archive", Some(1), "outputs");
    same(&dir, "stage-archive");
    job.stage_note("dark");
    same(&dir, "note-archive");
    at(&ms, 11_000);
    job.progress(1);
    same(&dir, "progress-archive");
    at(&ms, 11_200);
    job.finish(0);
    same(&dir, "finish");
    at(&ms, 12_000);
    job.stage("late", Some(3), "frames");
    job.progress(2);
    job.fail(3, "late");
    same(&dir, "after-finish");
    drop(job);
    same(&dir, "after-drop");
}

#[test]
fn a_render_through_prec_s_calls_writes_the_same_bytes() {
    let dir = scratch("golden-slot");
    let slot = Slot::new();
    let (clock, ms) = clock();
    let cfg = full_settings(&dir);
    slot.start(|| {
        open(
            Some("demo"),
            "intro",
            "demo intro 1280x720 30 fps".to_string(),
            "cpu: 4 threads".to_string(),
            Path::new("/srv/renders/demo"),
            cfg,
            clock,
        )
    });
    same(&dir, "out");
    slot.stages(&["build", "record", "replay", "master", "archive"]);
    same(&dir, "stages");
    at(&ms, 250);
    slot.stage("build", None, "steps");
    same(&dir, "stage-build");
    slot.note("demo@abc1234");
    same(&dir, "note-build");
    at(&ms, 4_000);
    slot.stage("record", None, "seconds");
    same(&dir, "stage-record");
    at(&ms, 5_000);
    slot.progress(1);
    same(&dir, "progress-record-1");
    at(&ms, 5_500);
    slot.progress(2);
    same(&dir, "progress-record-throttled");
    at(&ms, 6_000);
    slot.context(Some("dark".to_string()));
    slot.stage("replay", Some(90), "frames");
    same(&dir, "note-replay");
    slot.progress(0);
    same(&dir, "progress-replay-0");
    at(&ms, 7_000);
    slot.progress(30);
    same(&dir, "progress-replay-30");
    at(&ms, 9_500);
    slot.progress(70);
    same(&dir, "progress-replay-70");
    at(&ms, 10_600);
    slot.progress(80);
    same(&dir, "progress-replay-80");
    slot.note("archive skipped: no archive");
    same(&dir, "note-extra");
    slot.stage("archive", Some(1), "outputs");
    same(&dir, "note-archive");
    at(&ms, 11_000);
    slot.progress(1);
    same(&dir, "progress-archive");
    at(&ms, 11_200);
    slot.end(&Ok(()));
    same(&dir, "finish");
    at(&ms, 12_000);
    slot.cancelled(130, "SIGINT");
    slot.end(&Err("late".to_string()));
    same(&dir, "after-finish");
}

#[test]
fn a_cancelled_job_writes_the_bytes_pito_jobs_wrote() {
    let dir = scratch("golden-cancel");
    let (clock, ms) = clock();
    let job = start_with("prec", None, "take", settings(&dir), clock);
    assert_eq!(written(&dir), fill(CANCEL_START));
    at(&ms, 1_500);
    job.fail(130, "cancelled by SIGINT");
    assert_eq!(written(&dir), fill(CANCEL_FAIL));
    drop(job);
    assert_eq!(written(&dir), fill(CANCEL_FAIL));
}

#[test]
fn an_error_is_escaped_as_pito_jobs_escaped_it() {
    let dir = scratch("golden-error");
    let (clock, ms) = clock();
    let job = start_with("prec", Some("demo"), "still", settings(&dir), clock);
    at(&ms, 42);
    job.fail(0, "boom \"q\" \\ é😀\n\ttab");
    assert_eq!(written(&dir), fill(ERROR_FAIL));
    drop(job);
}

#[test]
fn an_oversized_record_is_cut_to_the_bytes_pito_jobs_cut_it_to() {
    let dir = scratch("golden-big");
    let (clock, ms) = clock();
    let job: Job =
        start_with("prec", Some("demo"), "intro", settings(&dir), clock).detail("d".repeat(5_000));
    assert_eq!(written(&dir), sized(BIG_DETAIL));
    job.stages(&[&"s".repeat(2_000), &"t".repeat(2_000)]);
    assert_eq!(written(&dir), sized(BIG_DETAIL));
    at(&ms, 7);
    job.finish(0);
    assert_eq!(written(&dir), sized(BIG_FINISH));
}
