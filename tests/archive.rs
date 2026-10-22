mod common;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

const MADE: &str = "2026-10-02T09:14:07+02:00";
const LATER: &str = "2026-10-02T09:20:00+02:00";
const LEAF: &str = "prec/alpha/shot/2026-10-02_091407_v1.2.3_site";
const LATER_LEAF: &str = "prec/alpha/shot/2026-10-02_092000_v1.2.4_site";
const OLD: u64 = 1_000_000;
const FRESH: u64 = 4_000_000_000;

fn scratch(name: &str) -> PathBuf {
    common::scratch("archive", name)
}

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut c = common::prec(dir, &dir.join("archive"), args);
    c.env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("NO_COLOR", "1")
        .env_remove("PREC_CACHE");
    c
}

fn prec(dir: &Path, args: &[&str]) -> Output {
    command(dir, args).output().unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn root(dir: &Path) -> PathBuf {
    dir.join("archive")
}

fn masters(dir: &Path) -> PathBuf {
    root(dir).join("Masters")
}

fn catalog(dir: &Path) -> PathBuf {
    masters(dir).join("catalog.jsonl")
}

fn queue(dir: &Path) -> PathBuf {
    dir.join("cache/prec/archive-queue")
}

fn queued(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(queue(dir))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

struct Job {
    entry: PathBuf,
    local: PathBuf,
    files: Vec<(String, Vec<u8>)>,
}

fn listed(files: &[(String, Vec<u8>)]) -> Value {
    json!(
        files
            .iter()
            .map(|(name, bytes)| json!({"name": name, "bytes": bytes.len(), "sha256": sha(bytes)}))
            .collect::<Vec<_>>()
    )
}

fn enqueue(
    dir: &Path,
    key: &str,
    version: &str,
    made: &str,
    leaf: &str,
    files: &[(&str, &[u8])],
) -> Job {
    let files: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(n, b)| (n.to_string(), b.to_vec()))
        .collect();
    let local_dir = dir.join("renders").join(key);
    fs::create_dir_all(&local_dir).unwrap();
    let entry = queue(dir).join(key);
    fs::create_dir_all(&entry).unwrap();
    for (name, bytes) in &files {
        fs::write(local_dir.join(name), bytes).unwrap();
        fs::write(entry.join(name), bytes).unwrap();
    }
    let manifest = json!({
        "product": "alpha",
        "shot": "shot",
        "version": version,
        "caller": "site",
        "made": made,
        "files": files
            .iter()
            .map(|(name, bytes)| json!({"role": "master", "file": name, "bytes": bytes.len(), "sha256": sha(bytes)}))
            .collect::<Vec<_>>(),
        "archive": {"leaf": leaf, "state": "queued"},
    });
    let local = local_dir.join("shot.json");
    fs::write(&local, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    fs::write(
        entry.join("source.json"),
        serde_json::to_vec(&json!({
            "manifest": local.display().to_string(),
            "root": root(dir).display().to_string(),
            "leaf": leaf,
            "files": listed(&files),
            "data": manifest,
        }))
        .unwrap(),
    )
    .unwrap();
    Job {
        entry,
        local,
        files,
    }
}

fn master(dir: &Path, key: &str) -> Job {
    enqueue(
        dir,
        key,
        "v1.2.3",
        MADE,
        LEAF,
        &[("shot.master.png", b"master")],
    )
}

fn record(job: &Job) -> Value {
    read(&job.entry.join("source.json"))
}

fn rewrite(job: &Job, change: impl FnOnce(&mut Value)) {
    let mut r = record(job);
    change(&mut r);
    fs::write(
        job.entry.join("source.json"),
        serde_json::to_vec(&r).unwrap(),
    )
    .unwrap();
}

fn retry(dir: &Path) -> Output {
    let o = prec(dir, &["archive", "--retry"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "");
    o
}

fn hashed_files(leaf: &Path) -> Vec<Value> {
    let mut names: Vec<String> = fs::read_dir(leaf)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "manifest.json")
        .collect();
    names.sort();
    names
        .iter()
        .map(|n| {
            let bytes = fs::read(leaf.join(n)).unwrap();
            json!({"name": n, "bytes": bytes.len(), "sha256": sha(&bytes)})
        })
        .collect()
}

fn sorted_files(line: &Value) -> Vec<Value> {
    let mut files = line["files"].as_array().unwrap().clone();
    files.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    files
}

fn no_partials(dir: &Path) -> bool {
    let mut stack = vec![root(dir)];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().ends_with(".partial") {
                return false;
            }
            if e.path().is_dir() {
                stack.push(e.path());
            }
        }
    }
    true
}

#[test]
fn a_retry_lands_queued_copies_in_the_archive_and_empties_the_queue() {
    let dir = scratch("retry-lands");
    fs::create_dir_all(root(&dir)).unwrap();
    let first = master(&dir, "one");
    let second = enqueue(
        &dir,
        "two",
        "v1.2.4",
        LATER,
        LATER_LEAF,
        &[
            ("shot.master.png", b"later!"),
            ("shot.cast", b"{\"v\":2}\n"),
        ],
    );
    let o = retry(&dir);
    let said = err(&o);
    for leaf in [LEAF, LATER_LEAF] {
        assert!(
            said.contains(&format!(
                "archive: {}\n",
                masters(&dir).join(leaf).display()
            )),
            "{said}"
        );
    }
    assert!(queued(&dir).is_empty());
    let catalog = lines(&catalog(&dir));
    assert_eq!(catalog.len(), 2);
    for (job, leaf) in [(&first, LEAF), (&second, LATER_LEAF)] {
        let at = masters(&dir).join(leaf);
        for (name, bytes) in &job.files {
            assert_eq!(&fs::read(at.join(name)).unwrap(), bytes, "{name}");
        }
        let archived = read(&at.join("manifest.json"));
        assert_eq!(archived["archive"]["state"], "archived");
        assert_eq!(archived["archive"]["leaf"], leaf);
        assert_eq!(read(&job.local), archived);
        let line = catalog.iter().find(|l| l["path"] == leaf).unwrap();
        assert_eq!(*line, archived["archive"]["catalog"]);
        assert_eq!(line["tool"], "prec");
        assert_eq!(sorted_files(line), hashed_files(&at));
    }
    assert!(no_partials(&dir));
}

#[test]
fn a_retry_with_nothing_queued_says_nothing() {
    let dir = scratch("retry-empty");
    let o = retry(&dir);
    assert_eq!(err(&o), "");
    assert!(!root(&dir).exists());
}

#[test]
fn an_offline_archive_keeps_the_queue_and_says_so() {
    let dir = scratch("retry-offline");
    let job = master(&dir, "one");
    let before = fs::read(job.entry.join("source.json")).unwrap();
    let o = retry(&dir);
    assert!(
        err(&o).contains(&format!(
            "archive: {} is unavailable; queued\n",
            root(&dir).display()
        )),
        "{}",
        err(&o)
    );
    assert!(!root(&dir).exists());
    assert_eq!(fs::read(job.entry.join("source.json")).unwrap(), before);
    assert_eq!(
        fs::read(job.entry.join("shot.master.png")).unwrap(),
        b"master"
    );
    assert_eq!(read(&job.local)["archive"]["state"], "queued");

    fs::create_dir_all(root(&dir)).unwrap();
    retry(&dir);
    assert!(queued(&dir).is_empty());
    assert_eq!(read(&job.local)["archive"]["state"], "archived");
}

#[test]
fn a_read_only_archive_keeps_the_queue_and_says_so() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("retry-read-only");
    let job = master(&dir, "one");
    fs::create_dir_all(root(&dir)).unwrap();
    fs::set_permissions(root(&dir), fs::Permissions::from_mode(0o555)).unwrap();
    let probe = root(&dir).join("probe");
    if fs::write(&probe, b"").is_ok() {
        fs::remove_file(&probe).unwrap();
        fs::set_permissions(root(&dir), fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: this user writes into a read-only folder");
        return;
    }
    let o = retry(&dir);
    fs::set_permissions(root(&dir), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(err(&o).ends_with("; queued\n"), "{}", err(&o));
    assert!(!masters(&dir).exists());
    assert_eq!(queued(&dir), BTreeSet::from(["one".to_string()]));
    assert!(job.entry.join("source.json").is_file());
    assert_eq!(read(&job.local)["archive"]["state"], "queued");

    retry(&dir);
    assert!(queued(&dir).is_empty());
    assert_eq!(
        fs::read(masters(&dir).join(LEAF).join("shot.master.png")).unwrap(),
        b"master"
    );
}

#[test]
fn a_corrupt_queue_entry_is_refused_and_the_others_still_land() {
    let dir = scratch("retry-corrupt");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "good");
    let half = queue(&dir).join("half");
    fs::create_dir_all(&half).unwrap();
    fs::write(half.join("shot.master.png"), b"mas").unwrap();
    let broken = enqueue(
        &dir,
        "broken",
        "v1.2.4",
        LATER,
        LATER_LEAF,
        &[("shot.master.png", b"later!")],
    );
    fs::write(broken.entry.join("source.json"), b"{\"manifest\":").unwrap();
    let no_leaf = enqueue(
        &dir,
        "no-leaf",
        "v1.2.4",
        LATER,
        LATER_LEAF,
        &[("shot.master.png", b"later!")],
    );
    rewrite(&no_leaf, |r| {
        r.as_object_mut().unwrap().remove("leaf");
    });
    let bad_files = enqueue(
        &dir,
        "bad-files",
        "v1.2.4",
        LATER,
        LATER_LEAF,
        &[("shot.master.png", b"later!")],
    );
    rewrite(&bad_files, |r| r["files"] = json!("shot.master.png"));
    let kept: Vec<(PathBuf, Vec<u8>)> = [&broken, &no_leaf, &bad_files]
        .iter()
        .map(|j| {
            let p = j.entry.join("source.json");
            let bytes = fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect();
    let o = retry(&dir);
    let said = err(&o);
    for key in ["half", "broken", "no-leaf", "bad-files"] {
        assert!(
            said.lines().any(|l| l
                .starts_with(&format!("archive: {}", queue(&dir).join(key).display()))
                && l.ends_with("; queued")),
            "{key}: {said}"
        );
    }
    assert!(said.contains("queue has no leaf"), "{said}");
    assert!(said.contains("queue files:"), "{said}");
    assert_eq!(
        queued(&dir),
        BTreeSet::from(["half", "broken", "no-leaf", "bad-files"].map(String::from))
    );
    for (path, bytes) in kept {
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    assert_eq!(fs::read(half.join("shot.master.png")).unwrap(), b"mas");
    assert_eq!(lines(&catalog(&dir)).len(), 1);
    assert!(masters(&dir).join(LEAF).join("manifest.json").is_file());
    assert!(!masters(&dir).join(LATER_LEAF).exists());
}

#[test]
fn a_queued_name_never_lands_outside_the_archive_root() {
    let dir = scratch("retry-escape");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "good");
    let outside = dir.join("outside");
    let elsewhere = dir.join("elsewhere/alpha");
    let cases: [(&str, Value, Option<&str>); 4] = [
        ("dots", json!("../../outside/alpha"), None),
        ("absolute", json!(elsewhere.display().to_string()), None),
        (
            "file-dots",
            json!(LATER_LEAF),
            Some("../../../../../../escaped.png"),
        ),
        ("file-absolute", json!(LATER_LEAF), Some("absolute")),
    ];
    for (key, leaf, file) in &cases {
        let job = enqueue(
            &dir,
            key,
            "v1.2.4",
            LATER,
            LATER_LEAF,
            &[("shot.master.png", b"later!")],
        );
        let file = file.map(|f| {
            if f == "absolute" {
                dir.join("abs.png").display().to_string()
            } else {
                f.to_string()
            }
        });
        rewrite(&job, |r| {
            r["leaf"] = leaf.clone();
            r["data"]["archive"]["leaf"] = leaf.clone();
            if let Some(name) = &file {
                r["files"] = json!([{"name": name, "bytes": 6, "sha256": sha(b"later!")}]);
            }
        });
    }
    let o = retry(&dir);
    let said = err(&o);
    for (key, _, _) in &cases {
        let prefix = format!("archive: refused: {}: ", queue(&dir).join(key).display());
        assert!(said.contains(&prefix), "{key}: {said}");
        let local = read(&dir.join("renders").join(key).join("shot.json"));
        assert_eq!(local["archive"]["state"], "refused", "{key}");
        let error = local["archive"]["error"].as_str().unwrap();
        assert!(
            error.contains("leaves the archive") || error.contains("leaves its leaf"),
            "{key}: {error}"
        );
    }
    assert!(!outside.exists());
    assert!(!dir.join("elsewhere").exists());
    assert!(!dir.join("escaped.png").exists());
    assert!(!dir.join("abs.png").exists());
    assert!(!masters(&dir).join(LATER_LEAF).exists());
    assert!(queued(&dir).is_empty());
    assert_eq!(lines(&catalog(&dir)).len(), 1);

    let again = retry(&dir);
    assert_eq!(err(&again), "");
}

#[test]
fn a_refused_queue_entry_whose_data_is_no_object_is_settled_without_a_crash() {
    let dir = scratch("retry-shapeless");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "good");
    let escape = json!("../../outside/alpha");
    let mut jobs = Vec::new();
    for key in [
        "archive-text",
        "data-text",
        "data-missing",
        "no-data-no-files",
    ] {
        let job = enqueue(
            &dir,
            key,
            "v1.2.4",
            LATER,
            LATER_LEAF,
            &[("shot.master.png", b"later!")],
        );
        let before = fs::read(&job.local).unwrap();
        rewrite(&job, |r| {
            let r = r.as_object_mut().unwrap();
            match key {
                "archive-text" => {
                    r.insert("leaf".into(), escape.clone());
                    r["data"]["archive"] = json!("queued");
                }
                "data-text" => {
                    r.insert("leaf".into(), escape.clone());
                    r.insert("data".into(), json!("queued"));
                }
                "data-missing" => {
                    r.insert("leaf".into(), escape.clone());
                    r.remove("data");
                }
                _ => {
                    r.remove("data");
                    r.remove("files");
                }
            }
        });
        jobs.push((key, job, before));
    }
    let o = retry(&dir);
    let said = err(&o);
    assert!(!said.contains("panicked"), "{said}");
    for (key, job, before) in &jobs {
        let prefix = format!("archive: refused: {}", queue(&dir).join(key).display());
        if *key == "no-data-no-files" {
            assert!(
                said.contains("archive: refused: manifest has no made"),
                "{said}"
            );
        } else {
            assert!(said.contains(&prefix), "{key}: {said}");
        }
        let local = read(&job.local);
        if *key == "archive-text" {
            assert_eq!(local["archive"]["state"], "refused", "{key}");
            assert!(
                local["archive"]["error"]
                    .as_str()
                    .unwrap()
                    .contains("leaves the archive")
            );
        } else {
            assert_eq!(&fs::read(&job.local).unwrap(), before, "{key}");
        }
    }
    assert!(queued(&dir).is_empty());
    assert!(!dir.join("outside").exists());
    assert!(!masters(&dir).join(LATER_LEAF).exists());
    assert_eq!(lines(&catalog(&dir)).len(), 1);
    assert_eq!(err(&retry(&dir)), "");
}

#[test]
fn a_queued_web_copy_with_an_at_in_its_name_lands_in_the_archive() {
    let dir = scratch("retry-web");
    fs::create_dir_all(root(&dir)).unwrap();
    let web = "alpha-still-shot-1280x720-web-640@2x.png";
    let job = enqueue(
        &dir,
        "one",
        "v1.2.3",
        MADE,
        LEAF,
        &[
            ("shot.master.png", b"master"),
            ("alpha-still-shot-1280x720-web-640.png", b"web"),
            (web, b"web@2x"),
        ],
    );
    let o = retry(&dir);
    assert!(!err(&o).contains("refused"), "{}", err(&o));
    assert!(queued(&dir).is_empty());
    let leaf = masters(&dir).join(LEAF);
    assert_eq!(fs::read(leaf.join(web)).unwrap(), b"web@2x");
    let rows = lines(&catalog(&dir));
    assert_eq!(rows.len(), 1);
    assert_eq!(sorted_files(&rows[0]), hashed_files(&leaf));
    assert_eq!(read(&job.local)["archive"]["state"], "archived");
}

#[test]
fn a_queued_copy_lands_under_the_root_it_was_queued_for() {
    let dir = scratch("retry-recorded-root");
    let recorded = dir.join("old-archive");
    fs::create_dir_all(&recorded).unwrap();
    fs::create_dir_all(root(&dir)).unwrap();
    let job = master(&dir, "one");
    rewrite(&job, |r| r["root"] = json!(recorded.display().to_string()));
    retry(&dir);
    assert!(queued(&dir).is_empty());
    assert_eq!(
        fs::read(recorded.join("Masters").join(LEAF).join("shot.master.png")).unwrap(),
        b"master"
    );
    assert_eq!(lines(&recorded.join("Masters/catalog.jsonl")).len(), 1);
    assert!(!masters(&dir).exists());
}

#[test]
fn a_queue_entry_missing_a_listed_file_is_kept_and_lands_once_it_is_whole() {
    let dir = scratch("retry-half-files");
    fs::create_dir_all(root(&dir)).unwrap();
    let job = enqueue(
        &dir,
        "one",
        "v1.2.3",
        MADE,
        LEAF,
        &[
            ("shot.master.png", b"master"),
            ("shot.cast", b"{\"v\":2}\n"),
        ],
    );
    fs::remove_file(job.entry.join("shot.cast")).unwrap();
    let o = retry(&dir);
    let said = err(&o);
    assert!(said.contains("shot.cast"), "{said}");
    assert!(said.ends_with("; queued\n"), "{said}");
    assert!(!masters(&dir).join(LEAF).exists());
    assert!(!catalog(&dir).exists());
    assert_eq!(queued(&dir), BTreeSet::from(["one".to_string()]));
    assert!(job.entry.join("source.json").is_file());
    assert_eq!(read(&job.local)["archive"]["state"], "queued");

    fs::write(job.entry.join("shot.cast"), b"{\"v\"").unwrap();
    let o = retry(&dir);
    assert!(err(&o).ends_with("; queued\n"), "{}", err(&o));
    assert!(!masters(&dir).join(LEAF).exists());
    assert_eq!(queued(&dir), BTreeSet::from(["one".to_string()]));

    fs::write(job.entry.join("shot.cast"), b"{\"v\":2}\n").unwrap();
    retry(&dir);
    assert!(queued(&dir).is_empty());
    assert_eq!(
        hashed_files(&masters(&dir).join(LEAF)),
        sorted_files(&lines(&catalog(&dir))[0])
    );
    assert!(no_partials(&dir));
}

#[test]
fn the_same_render_queued_twice_lands_once_with_one_catalog_line() {
    let dir = scratch("retry-twice");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "one");
    master(&dir, "two");
    retry(&dir);
    assert!(queued(&dir).is_empty());
    let shot = masters(&dir).join("prec/alpha/shot");
    assert_eq!(fs::read_dir(&shot).unwrap().count(), 1);
    let before = fs::read(catalog(&dir)).unwrap();
    assert_eq!(lines(&catalog(&dir)).len(), 1);

    master(&dir, "three");
    retry(&dir);
    assert!(queued(&dir).is_empty());
    assert_eq!(fs::read(catalog(&dir)).unwrap(), before);
    assert_eq!(fs::read_dir(&shot).unwrap().count(), 1);
}

#[test]
fn another_render_under_a_name_already_there_takes_the_next_name() {
    let dir = scratch("retry-next-name");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "one");
    retry(&dir);
    let other = "2026-10-02T09:14:07+03:00";
    let job = enqueue(
        &dir,
        "two",
        "v1.2.3",
        other,
        LEAF,
        &[("shot.master.png", b"other!")],
    );
    retry(&dir);
    let next = format!("{LEAF}-2");
    assert_eq!(
        fs::read(masters(&dir).join(LEAF).join("shot.master.png")).unwrap(),
        b"master"
    );
    assert_eq!(
        fs::read(masters(&dir).join(&next).join("shot.master.png")).unwrap(),
        b"other!"
    );
    assert_eq!(read(&job.local)["archive"]["leaf"], next);
    let catalog = lines(&catalog(&dir));
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[1]["path"], next);
    assert_eq!(catalog[1]["made"], other);
}

#[test]
fn a_different_render_at_the_same_instant_is_a_conflict_that_leaves_the_archive_alone() {
    let dir = scratch("retry-conflict");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "one");
    retry(&dir);
    let before = fs::read(catalog(&dir)).unwrap();
    let job = enqueue(
        &dir,
        "two",
        "v1.2.3",
        MADE,
        LEAF,
        &[("shot.master.png", b"third!")],
    );
    let o = retry(&dir);
    assert!(err(&o).contains("archive: conflict: "), "{}", err(&o));
    assert!(err(&o).contains("shot.master.png differs"), "{}", err(&o));
    assert!(queued(&dir).is_empty());
    let local = read(&job.local);
    assert_eq!(local["archive"]["state"], "conflict");
    assert!(
        local["archive"]["error"]
            .as_str()
            .unwrap()
            .contains("differs")
    );
    assert_eq!(
        fs::read(masters(&dir).join(LEAF).join("shot.master.png")).unwrap(),
        b"master"
    );
    assert_eq!(fs::read(catalog(&dir)).unwrap(), before);
    assert_eq!(
        fs::read_dir(masters(&dir).join("prec/alpha/shot"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn a_copy_that_fails_its_check_leaves_nothing_under_its_final_name() {
    let dir = scratch("retry-atomic");
    fs::create_dir_all(root(&dir)).unwrap();
    let job = master(&dir, "one");
    fs::write(job.entry.join("shot.master.png"), b"MASTER").unwrap();
    let o = retry(&dir);
    assert!(
        err(&o).contains("copied sha256 differs from the manifest; queued"),
        "{}",
        err(&o)
    );
    let leaf = masters(&dir).join(LEAF);
    assert!(!leaf.exists());
    let partial = masters(&dir).join(format!("{LEAF}.partial"));
    assert!(!partial.join("manifest.json").exists());
    assert!(!catalog(&dir).exists());
    assert_eq!(queued(&dir), BTreeSet::from(["one".to_string()]));
    assert_eq!(read(&job.local)["archive"]["state"], "queued");

    fs::write(job.entry.join("shot.master.png"), b"master").unwrap();
    retry(&dir);
    assert_eq!(fs::read(leaf.join("shot.master.png")).unwrap(), b"master");
    assert!(!partial.exists());
    assert!(no_partials(&dir));
    assert_eq!(lines(&catalog(&dir)).len(), 1);
}

fn pfx_leaf(dir: &Path, stored_as_text: bool) -> String {
    let path = "pfx/beta/hero/2026-10-01_0800_99e8c315bfbe_site";
    let leaf = masters(dir).join(path);
    fs::create_dir_all(&leaf).unwrap();
    let still = b"pfx still";
    fs::write(leaf.join("beta-hero.master.png"), still).unwrap();
    let line = json!({
        "v": 1,
        "tool": "pfx",
        "product": "beta",
        "shot": "hero",
        "version": "99e8c315bfbe",
        "caller": "site",
        "made": "2026-10-01T08:00:00.881639132+02:00",
        "path": path,
        "files": [{"name": "beta-hero.master.png", "bytes": still.len(), "sha256": sha(still)}],
    })
    .to_string();
    let stored = if stored_as_text {
        json!(line)
    } else {
        serde_json::from_str(&line).unwrap()
    };
    fs::write(
        leaf.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "product": "beta",
            "kind": "still",
            "made": "2026-10-01T08:00:00.881639132+02:00",
            "archive": {"leaf": path, "state": "archived", "catalog": stored},
        }))
        .unwrap(),
    )
    .unwrap();
    line
}

#[test]
fn a_retry_appends_after_the_lines_pfx_wrote() {
    let dir = scratch("retry-after-pfx");
    let line = pfx_leaf(&dir, true);
    fs::write(catalog(&dir), format!("{line}\n")).unwrap();
    master(&dir, "one");
    retry(&dir);
    let text = fs::read_to_string(catalog(&dir)).unwrap();
    let mut rows = text.lines();
    assert_eq!(rows.next(), Some(line.as_str()));
    let ours: Value = serde_json::from_str(rows.next().unwrap()).unwrap();
    assert_eq!(ours["path"], LEAF);
    assert!(rows.next().is_none());
}

fn reindex(dir: &Path) -> Output {
    prec(dir, &["archive", "--reindex"])
}

#[test]
fn a_reindex_rebuilds_the_catalog_from_the_folders_byte_stable() {
    let dir = scratch("reindex");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "one");
    enqueue(
        &dir,
        "two",
        "v1.2.4",
        LATER,
        LATER_LEAF,
        &[
            ("shot.master.png", b"later!"),
            ("shot.cast", b"{\"v\":2}\n"),
        ],
    );
    retry(&dir);
    let pfx = pfx_leaf(&dir, true);
    let skipped = masters(&dir).join("prec/alpha/shot/half.partial");
    fs::create_dir_all(&skipped).unwrap();
    fs::write(skipped.join("manifest.json"), b"{").unwrap();
    fs::write(catalog(&dir), b"not a catalog line\n").unwrap();

    let o = reindex(&dir);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "archive: reindexed 3 lines\n");
    let first = fs::read(catalog(&dir)).unwrap();
    let rows = lines(&catalog(&dir));
    let paths: Vec<&str> = rows.iter().map(|l| l["path"].as_str().unwrap()).collect();
    assert_eq!(
        paths,
        [
            "pfx/beta/hero/2026-10-01_0800_99e8c315bfbe_site",
            LEAF,
            LATER_LEAF
        ]
    );
    assert_eq!(rows[0], serde_json::from_str::<Value>(&pfx).unwrap());
    for row in &rows {
        assert_eq!(
            sorted_files(row),
            hashed_files(&masters(&dir).join(row["path"].as_str().unwrap()))
        );
        assert_eq!(
            row["tool"],
            row["path"].as_str().unwrap().split('/').next().unwrap()
        );
    }
    assert!(!masters(&dir).join("catalog.jsonl.partial").exists());

    let o = reindex(&dir);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(fs::read(catalog(&dir)).unwrap(), first);
}

#[test]
fn a_reindex_trusts_the_catalog_each_leaf_stored_and_reads_no_file() {
    let dir = scratch("reindex-stored");
    fs::create_dir_all(root(&dir)).unwrap();
    enqueue(
        &dir,
        "one",
        "v1.2.3",
        MADE,
        LEAF,
        &[
            ("shot.master.png", b"master"),
            ("shot.cast", b"{\"v\":2}\n"),
        ],
    );
    retry(&dir);
    let stored = lines(&catalog(&dir));
    let leaf = masters(&dir).join(LEAF);
    fs::write(leaf.join("shot.master.png"), b"changed after the copy").unwrap();
    fs::remove_file(leaf.join("shot.cast")).unwrap();
    let o = reindex(&dir);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "archive: reindexed 1 lines\n");
    assert_eq!(lines(&catalog(&dir)), stored);
    assert_eq!(
        sorted_files(&stored[0])
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["shot.cast", "shot.master.png"]
    );
}

#[test]
fn a_reindex_leaves_out_a_leaf_with_no_stored_catalog() {
    let dir = scratch("reindex-no-catalog");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "one");
    retry(&dir);
    assert_eq!(reindex(&dir).status.code(), Some(0));
    let before = fs::read(catalog(&dir)).unwrap();
    for (name, archive) in [
        (
            "skipped",
            json!({"state": "skipped", "reason": "unconfigured"}),
        ),
        ("queued", json!({"state": "queued", "leaf": LATER_LEAF})),
        ("legacy", Value::Null),
    ] {
        let leaf = masters(&dir).join(format!("prec/alpha/shot/{name}"));
        fs::create_dir_all(&leaf).unwrap();
        fs::write(leaf.join("shot.master.png"), b"other").unwrap();
        fs::write(
            leaf.join("manifest.json"),
            json!({"made": LATER, "archive": archive}).to_string(),
        )
        .unwrap();
    }
    let o = reindex(&dir);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "archive: reindexed 1 lines\n");
    assert_eq!(fs::read(catalog(&dir)).unwrap(), before);
}

#[test]
fn a_reindex_refuses_a_broken_manifest_outside_a_partial_and_keeps_the_catalog() {
    let dir = scratch("reindex-broken");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "one");
    retry(&dir);
    let before = fs::read(catalog(&dir)).unwrap();
    let broken = masters(&dir).join("prec/alpha/shot/broken/manifest.json");
    fs::create_dir_all(broken.parent().unwrap()).unwrap();
    fs::write(&broken, b"{\"made\":").unwrap();
    let o = reindex(&dir);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert_eq!(out(&o), "");
    let said = err(&o);
    assert!(said.starts_with("prec: "), "{said}");
    assert!(said.contains(&broken.display().to_string()), "{said}");
    assert_eq!(said.lines().count(), 1, "{said}");
    assert_eq!(fs::read(catalog(&dir)).unwrap(), before);
    assert!(!masters(&dir).join("catalog.jsonl.partial").exists());
}

#[test]
fn a_reindex_refuses_an_unreadable_folder_and_keeps_the_catalog() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("reindex-unreadable");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "one");
    retry(&dir);
    let before = fs::read(catalog(&dir)).unwrap();
    let locked = masters(&dir).join("prec/beta");
    fs::create_dir_all(locked.join("shot")).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: this user reads a folder with no permissions");
        return;
    }
    let o = reindex(&dir);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    let said = err(&o);
    assert!(said.starts_with("prec: "), "{said}");
    assert!(said.contains(&locked.display().to_string()), "{said}");
    assert_eq!(fs::read(catalog(&dir)).unwrap(), before);
    assert!(!masters(&dir).join("catalog.jsonl.partial").exists());
}

#[test]
fn a_reindex_reads_a_catalog_pfx_stored_as_an_object() {
    let dir = scratch("reindex-object");
    let line = pfx_leaf(&dir, false);
    let o = reindex(&dir);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "archive: reindexed 1 lines\n");
    assert_eq!(
        lines(&catalog(&dir)),
        [serde_json::from_str::<Value>(&line).unwrap()]
    );
}

#[test]
fn a_reindex_refuses_a_line_with_an_unknown_key_and_keeps_the_catalog() {
    let dir = scratch("reindex-unknown");
    fs::create_dir_all(root(&dir)).unwrap();
    master(&dir, "one");
    retry(&dir);
    assert_eq!(reindex(&dir).status.code(), Some(0));
    let before = fs::read(catalog(&dir)).unwrap();
    let stranger = masters(&dir).join("pfx/beta/hero/2026-10-03_2020_v1_site/manifest.json");
    fs::create_dir_all(stranger.parent().unwrap()).unwrap();
    let mut line: Value = serde_json::from_str(&pfx_leaf(&dir, true)).unwrap();
    line["extra"] = json!(1);
    for stored in [line.clone(), json!(line.to_string())] {
        fs::write(
            &stranger,
            json!({"made": MADE, "archive": {"state": "archived", "catalog": stored}}).to_string(),
        )
        .unwrap();
        let o = reindex(&dir);
        assert_eq!(o.status.code(), Some(1), "{}", err(&o));
        assert_eq!(out(&o), "");
        let said = err(&o);
        assert!(said.starts_with("prec: "), "{said}");
        assert!(said.contains(&stranger.display().to_string()), "{said}");
        assert!(said.contains("unknown field `extra`"), "{said}");
        assert_eq!(said.lines().count(), 1, "{said}");
        assert_eq!(fs::read(catalog(&dir)).unwrap(), before);
        assert!(!masters(&dir).join("catalog.jsonl.partial").exists());
    }
}

#[test]
fn a_reindex_without_a_tool_folder_or_a_root_is_refused() {
    let dir = scratch("reindex-nothing");
    fs::create_dir_all(root(&dir)).unwrap();
    let o = reindex(&dir);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        err(&o),
        format!(
            "prec: {} has no prec/ or pfx/ folder\n",
            masters(&dir).display()
        )
    );
    assert!(!catalog(&dir).exists());

    let o = command(&dir, &["archive", "--reindex"])
        .env_remove("PITO_ARCHIVE")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).starts_with("prec: archive: no root; "),
        "{}",
        err(&o)
    );
}

#[test]
fn archive_with_neither_or_both_flags_is_clap_error() {
    let dir = scratch("archive-flags");
    let job = master(&dir, "one");
    fs::create_dir_all(root(&dir)).unwrap();
    for args in [&["archive"][..], &["archive", "--retry", "--reindex"][..]] {
        let o = prec(&dir, args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert_eq!(out(&o), "");
        let said = err(&o);
        assert!(said.starts_with("error: "), "{said}");
        assert!(said.contains("\n\nUsage: prec archive "), "{said}");
        assert!(
            said.ends_with("\n\nFor more information, try '--help'.\n"),
            "{said}"
        );
    }
    assert!(job.entry.join("source.json").is_file());
    assert!(!masters(&dir).exists());
}

fn cache_root(dir: &Path) -> PathBuf {
    dir.join("cache/prec")
}

fn entry(dir: &Path, rel: &str, payload: usize, used: Option<u64>) -> u64 {
    let at = cache_root(dir).join(rel);
    fs::create_dir_all(&at).unwrap();
    fs::write(at.join("payload"), vec![7u8; payload]).unwrap();
    match used {
        Some(t) => {
            let text = t.to_string();
            fs::write(at.join(".used"), &text).unwrap();
            (payload + text.len()) as u64
        }
        None => {
            File::open(&at)
                .unwrap()
                .set_modified(UNIX_EPOCH + Duration::from_secs(OLD))
                .unwrap();
            payload as u64
        }
    }
}

fn hold(dir: &Path, section: &str, name: &str) -> File {
    let path = cache_root(dir)
        .join("locks")
        .join(section)
        .join(format!("{name}.lock"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = File::create(&path).unwrap();
    f.lock_shared().unwrap();
    f
}

struct Cache {
    old: u64,
    held: u64,
    target: u64,
    work: u64,
    git: u64,
    queue: u64,
}

fn populate(dir: &Path) -> Cache {
    let queue_entry = queue(dir).join("pending");
    fs::create_dir_all(&queue_entry).unwrap();
    fs::write(queue_entry.join("shot.master.png"), vec![1u8; 150]).unwrap();
    Cache {
        old: entry(dir, "src/app-aaaaaaaaaaaa/old", 100, Some(OLD)),
        held: entry(dir, "src/app-aaaaaaaaaaaa/held", 200, Some(OLD)),
        target: entry(dir, "target/app-aaaaaaaaaaaa", 300, None),
        work: entry(dir, "work/other-bbbbbbbbbbbb", 50, Some(FRESH)),
        git: entry(dir, "git/app-aaaaaaaaaaaa.git", 25, Some(FRESH)),
        queue: 150,
    }
}

fn json_of(o: &Output) -> Value {
    assert_eq!(o.status.code(), Some(0), "{}", err(o));
    let text = out(o);
    assert_eq!(text.lines().count(), 1, "{text}");
    serde_json::from_str(&text).unwrap()
}

fn keys(v: &Value) -> Vec<&str> {
    let mut k: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    k.sort();
    k
}

#[test]
fn the_cache_json_has_the_keys_a_monitor_reads_and_counts_sizes_right() {
    let dir = scratch("cache-json");
    let c = populate(&dir);
    let _lock = hold(&dir, "src", "app-aaaaaaaaaaaa@held");
    let v = json_of(&prec(&dir, &["cache", "--json"]));
    assert_eq!(
        keys(&v),
        [
            "archive_queue",
            "at",
            "bytes",
            "entries",
            "prune",
            "root",
            "root_set_by",
            "tool",
            "tool_version",
            "v"
        ]
    );
    assert_eq!(v["v"], 1);
    assert_eq!(v["tool"], "prec");
    assert_eq!(v["tool_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["root"], cache_root(&dir).display().to_string());
    assert!(v["root_set_by"].is_null());
    assert!(v["at"].is_u64());
    assert_eq!(
        v["bytes"],
        c.old + c.held + c.target + c.work + c.git + c.queue
    );
    assert_eq!(v["archive_queue"], json!({"entries": 1, "bytes": c.queue}));
    assert_eq!(
        v["prune"],
        json!({"after_days": 30, "stale_bytes": c.old + c.target})
    );
    let entries = v["entries"].as_array().unwrap();
    let found: Vec<(&str, &str, u64, bool, u64)> = entries
        .iter()
        .map(|e| {
            assert_eq!(
                keys(e),
                ["bytes", "in_use", "name", "path", "repo", "section", "used"]
            );
            (
                e["section"].as_str().unwrap(),
                e["name"].as_str().unwrap(),
                e["bytes"].as_u64().unwrap(),
                e["in_use"].as_bool().unwrap(),
                e["used"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("git", "app-aaaaaaaaaaaa.git", c.git, false, FRESH * 1000),
            ("src", "app-aaaaaaaaaaaa@held", c.held, true, OLD * 1000),
            ("src", "app-aaaaaaaaaaaa@old", c.old, false, OLD * 1000),
            ("target", "app-aaaaaaaaaaaa", c.target, false, OLD * 1000),
            ("work", "other-bbbbbbbbbbbb", c.work, false, FRESH * 1000),
        ]
    );
    assert_eq!(entries[1]["repo"], "app");
    assert_eq!(entries[4]["repo"], "other");
    assert_eq!(
        entries[2]["path"],
        cache_root(&dir)
            .join("src/app-aaaaaaaaaaaa/old")
            .display()
            .to_string()
    );
    assert!(cache_root(&dir).join("src/app-aaaaaaaaaaaa/old").exists());
}

#[test]
fn the_cache_listing_shows_each_entry_with_its_size() {
    let dir = scratch("cache-list");
    let c = populate(&dir);
    let o = prec(&dir, &["cache"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let text = out(&o);
    let rows: Vec<&str> = text.lines().collect();
    let total = c.old + c.held + c.target + c.work + c.git + c.queue;
    assert_eq!(
        rows[0],
        format!("cache: {} ({total} B)", cache_root(&dir).display())
    );
    let expected = [
        ("git", "app-aaaaaaaaaaaa.git", c.git),
        ("src", "app-aaaaaaaaaaaa@held", c.held),
        ("src", "app-aaaaaaaaaaaa@old", c.old),
        ("target", "app-aaaaaaaaaaaa", c.target),
        ("work", "other-bbbbbbbbbbbb", c.work),
    ];
    assert_eq!(rows.len(), 1 + expected.len(), "{text}");
    for (row, (section, name, bytes)) in rows[1..].iter().zip(expected) {
        let words: Vec<&str> = row.split_whitespace().collect();
        let bytes = bytes.to_string();
        assert_eq!(words[..4], [section, name, bytes.as_str(), "B"], "{row}");
        assert_eq!(words[4], "used", "{row}");
        assert!(row.ends_with(" ago"), "{row}");
    }
}

#[test]
fn an_empty_or_missing_cache_prints_the_empty_shape_and_creates_nothing() {
    let dir = scratch("cache-missing");
    let v = json_of(&prec(&dir, &["cache", "--json"]));
    assert_eq!(v["root"], cache_root(&dir).display().to_string());
    assert_eq!(v["bytes"], 0);
    assert_eq!(v["entries"], json!([]));
    assert_eq!(v["archive_queue"], json!({"entries": 0, "bytes": 0}));
    assert_eq!(v["prune"], json!({"after_days": 30, "stale_bytes": 0}));
    let o = prec(&dir, &["cache"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        format!("cache: {} (0 B)\n  (empty)\n", cache_root(&dir).display())
    );
    let p = json_of(&prec(&dir, &["cache", "prune", "--json"]));
    assert_eq!(
        p,
        json!({"v": 1, "freed": 0, "removed": [], "kept_in_use": 0})
    );
    assert!(!dir.join("cache").exists());

    fs::create_dir_all(cache_root(&dir)).unwrap();
    let v = json_of(&prec(&dir, &["cache", "--json"]));
    assert_eq!(v["entries"], json!([]));
    assert_eq!(v["bytes"], 0);
}

#[test]
fn the_cache_root_set_by_prec_cache_is_named() {
    let dir = scratch("cache-env");
    let moved = dir.join("moved");
    fs::create_dir_all(moved.join("work/app-aaaaaaaaaaaa")).unwrap();
    let o = command(&dir, &["cache", "--json"])
        .env("PREC_CACHE", &moved)
        .output()
        .unwrap();
    let v = json_of(&o);
    assert_eq!(v["root"], moved.display().to_string());
    assert_eq!(v["root_set_by"], "PREC_CACHE");
    assert_eq!(v["entries"][0]["name"], "app-aaaaaaaaaaaa");
    let o = command(&dir, &["cache"])
        .env("PREC_CACHE", &moved)
        .output()
        .unwrap();
    assert_eq!(
        out(&o).lines().nth(1).map(str::trim),
        Some("set by PREC_CACHE")
    );
}

#[test]
fn a_prune_removes_the_stale_and_keeps_the_fresh_and_the_held() {
    let dir = scratch("cache-prune");
    let c = populate(&dir);
    let _lock = hold(&dir, "src", "app-aaaaaaaaaaaa@held");
    let before = fs::read_dir(cache_root(&dir).join("src/app-aaaaaaaaaaaa"))
        .unwrap()
        .count();
    let dry = json_of(&prec(&dir, &["cache", "prune", "--json", "--dry-run"]));
    assert_eq!(
        json_of(&prec(&dir, &["cache", "--json", "prune", "--dry-run"])),
        dry
    );
    assert_eq!(
        fs::read_dir(cache_root(&dir).join("src/app-aaaaaaaaaaaa"))
            .unwrap()
            .count(),
        before
    );
    assert!(cache_root(&dir).join("target/app-aaaaaaaaaaaa").exists());
    let v = json_of(&prec(&dir, &["cache", "prune", "--json"]));
    assert_eq!(v, dry);
    assert_eq!(keys(&v), ["freed", "kept_in_use", "removed", "v"]);
    assert_eq!(
        v,
        json!({
            "v": 1,
            "freed": c.old + c.target,
            "removed": [
                {"section": "src", "name": "app-aaaaaaaaaaaa@old", "bytes": c.old},
                {"section": "target", "name": "app-aaaaaaaaaaaa", "bytes": c.target},
            ],
            "kept_in_use": 1,
        })
    );
    assert!(!cache_root(&dir).join("src/app-aaaaaaaaaaaa/old").exists());
    assert!(!cache_root(&dir).join("target/app-aaaaaaaaaaaa").exists());
    assert!(
        cache_root(&dir)
            .join("src/app-aaaaaaaaaaaa/held/payload")
            .exists()
    );
    assert!(
        cache_root(&dir)
            .join("work/other-bbbbbbbbbbbb/payload")
            .exists()
    );
    assert!(
        cache_root(&dir)
            .join("git/app-aaaaaaaaaaaa.git/payload")
            .exists()
    );
    assert!(queue(&dir).join("pending/shot.master.png").exists());

    let all = json_of(&prec(&dir, &["cache", "prune", "--all", "--json"]));
    assert_eq!(all["kept_in_use"], 1);
    assert_eq!(all["freed"], c.work + c.git);
    assert!(
        cache_root(&dir)
            .join("src/app-aaaaaaaaaaaa/held/payload")
            .exists()
    );
    assert!(queue(&dir).join("pending/shot.master.png").exists());
}

#[test]
fn a_prune_keeps_what_is_younger_than_older_than() {
    let dir = scratch("cache-older-than");
    let c = populate(&dir);
    let v = json_of(&prec(
        &dir,
        &["cache", "prune", "--older-than", "99999w", "--json"],
    ));
    assert_eq!(
        v,
        json!({"v": 1, "freed": 0, "removed": [], "kept_in_use": 0})
    );
    assert!(
        cache_root(&dir)
            .join("src/app-aaaaaaaaaaaa/old/payload")
            .exists()
    );
    assert!(
        cache_root(&dir)
            .join("target/app-aaaaaaaaaaaa/payload")
            .exists()
    );

    let o = prec(&dir, &["cache", "prune", "--older-than", "1d"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let text = out(&o);
    let rows: Vec<&str> = text.lines().collect();
    assert_eq!(
        rows[..3],
        [
            format!("removed src/app-aaaaaaaaaaaa@held ({} B)", c.held),
            format!("removed src/app-aaaaaaaaaaaa@old ({} B)", c.old),
            format!("removed target/app-aaaaaaaaaaaa ({} B)", c.target),
        ]
    );
    assert!(
        rows[3].starts_with(&format!(
            "freed {} B from {}; cache now ",
            c.held + c.old + c.target,
            cache_root(&dir).display()
        )),
        "{text}"
    );
    assert_eq!(rows.len(), 4);
    assert!(
        cache_root(&dir)
            .join("work/other-bbbbbbbbbbbb/payload")
            .exists()
    );
}

#[test]
fn a_bad_older_than_is_refused_and_removes_nothing() {
    let dir = scratch("cache-bad-age");
    populate(&dir);
    for age in ["soon", "5x", "-1d", "1.5d", "", "999999999999999d"] {
        let o = prec(
            &dir,
            &["cache", "prune", &format!("--older-than={age}"), "--all"],
        );
        assert_eq!(o.status.code(), Some(1), "{age}: {}", err(&o));
        assert_eq!(out(&o), "");
        assert_eq!(
            err(&o),
            format!("prec: --older-than {age}: e.g. 30d, 12h or 90m\n")
        );
    }
    assert!(
        cache_root(&dir)
            .join("src/app-aaaaaaaaaaaa/old/payload")
            .exists()
    );
    assert!(
        cache_root(&dir)
            .join("work/other-bbbbbbbbbbbb/payload")
            .exists()
    );
}

#[test]
fn a_dry_run_without_json_is_clap_error() {
    let dir = scratch("cache-dry-run");
    populate(&dir);
    let o = prec(&dir, &["cache", "prune", "--dry-run"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(out(&o), "");
    let said = err(&o);
    assert!(said.starts_with("error: "), "{said}");
    assert!(
        said.ends_with("\n\nFor more information, try '--help'.\n"),
        "{said}"
    );
    assert!(
        cache_root(&dir)
            .join("src/app-aaaaaaaaaaaa/old/payload")
            .exists()
    );
}
