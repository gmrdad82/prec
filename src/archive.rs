use crate::{cache, jobs, paths};
use chrono::{DateTime, FixedOffset, Local, SecondsFormat};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

const MASTERS: &str = "Masters";
const TOOL: &str = "prec";
const TOOLS: [&str; 2] = [TOOL, "pfx"];
const LEGACY: &str = "pito-replay";

pub fn root() -> Option<PathBuf> {
    let configured = std::env::var_os("PITO_ARCHIVE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::config::get().archive.clone())?;
    Some(paths::absolute(&configured).unwrap_or(configured))
}

fn masters(root: &Path) -> PathBuf {
    root.join(MASTERS)
}

pub fn made() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn caller(raw: Option<&str>) -> String {
    let mut out = String::new();
    for c in raw.unwrap_or("").to_ascii_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let result = out.trim_end_matches('-');
    if result.is_empty() {
        "manual".into()
    } else {
        result.into()
    }
}

pub fn version(rev: &str, commit: Option<&str>, named_tag: Option<&str>) -> String {
    if rev == "local" && commit.is_none() {
        return "local".into();
    }
    let dirty = rev.ends_with("+dirty");
    let named = rev.trim_end_matches("+dirty");
    let base = if let Some(name) = named_tag {
        name.to_string()
    } else {
        commit.unwrap_or(named).chars().take(12).collect()
    };
    format!("{base}{}", if dirty { "-dirty" } else { "" })
}

pub fn eligible(app: &Value) -> Result<(), &'static str> {
    let rev = app["rev"].as_str().unwrap_or("local");
    let commit = app["commit"].as_str();
    let pinned = match app["pinned"].as_bool() {
        Some(pinned) => pinned,
        None => {
            app["tag"].as_str().is_some()
                || !(rev == "local"
                    || rev.ends_with("+dirty")
                    || commit.is_some_and(|c| c.chars().take(12).eq(rev.chars())))
        }
    };
    if pinned && rev != "local" && !rev.ends_with("+dirty") {
        Ok(())
    } else {
        Err("unpinned")
    }
}

pub struct Run<'a> {
    pub no_archive: bool,
    pub no_encode: bool,
    pub product: &'a str,
    pub has_master: bool,
}

fn ride() -> bool {
    std::env::var_os("RIDE_APP").is_some_and(|v| !v.is_empty())
}

pub fn skip_reason(run: &Run, app: &Value) -> Option<&'static str> {
    skip_with(run, app, paths::direct(), root().as_deref(), ride())
}

fn test_run(direct: bool, root: Option<&Path>) -> bool {
    direct || root.is_some_and(|r| r.components().any(|c| c.as_os_str() == "tmp"))
}

fn skip_with(
    run: &Run,
    app: &Value,
    direct: bool,
    root: Option<&Path>,
    ride: bool,
) -> Option<&'static str> {
    if run.no_archive {
        Some("no-archive")
    } else if root.is_none() {
        Some("unconfigured")
    } else if test_run(direct, root) {
        Some("test")
    } else if ride {
        Some("ride")
    } else if run.product == "demo" {
        Some("sample")
    } else if run.no_encode || !run.has_master {
        Some("frames-only")
    } else {
        eligible(app).err()
    }
}

fn folder(
    product: &str,
    shot: &str,
    version: &str,
    made: &str,
    caller: &str,
) -> Result<PathBuf, String> {
    for (what, part) in [
        ("product", product),
        ("shot", shot),
        ("version", version),
        ("caller", caller),
    ] {
        if !paths::safe_name(part) {
            return Err(format!("the {what} {part:?} is not a safe folder name"));
        }
    }
    let date = DateTime::parse_from_rfc3339(made).map_err(|e| format!("invalid made: {e}"))?;
    Ok(Path::new(TOOL).join(product).join(shot).join(format!(
        "{}_{version}_{caller}",
        date.format("%Y-%m-%d_%H%M%S")
    )))
}

fn leaf_shot(manifest: &Value) -> Option<String> {
    let shot = manifest["shot"].as_str()?;
    Some(match manifest["theme"]["label"].as_str() {
        Some(label) => format!("{shot}-{label}"),
        None => shot.to_string(),
    })
}

fn base_leaf(manifest: &Value) -> Result<PathBuf, String> {
    folder(
        manifest["product"]
            .as_str()
            .ok_or("manifest has no product")?,
        &leaf_shot(manifest).ok_or("manifest has no shot")?,
        manifest["version"]
            .as_str()
            .ok_or("manifest has no version")?,
        manifest["made"].as_str().ok_or("manifest has no made")?,
        manifest["caller"]
            .as_str()
            .ok_or("manifest has no caller")?,
    )
}

fn write_manifest(path: &Path, manifest: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())? + "\n";
    crate::encode::atomic_file(path, |w| {
        w.write_all(text.as_bytes()).map_err(|e| e.to_string())
    })
}

fn write_record(path: &Path, record: &Value) -> Result<(), String> {
    crate::encode::atomic_file(path, |w| {
        serde_json::to_writer(w, record).map_err(|e| e.to_string())
    })
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    name: String,
    bytes: u64,
    sha256: String,
}

#[derive(PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    v: u8,
    tool: String,
    product: String,
    shot: String,
    version: String,
    caller: String,
    made: String,
    path: String,
    files: Vec<CatalogFile>,
}

impl Catalog {
    fn read(stored: &Value) -> Result<Catalog, String> {
        match stored {
            Value::String(line) => serde_json::from_str(line),
            object => Catalog::deserialize(object),
        }
        .map_err(|e| format!("archive.catalog: {e}"))
    }

    fn line(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|e| e.to_string())
    }
}

const QUEUED_BEFORE_0_20: [&str; 3] = ["master", "poster", "cast"];

fn selected(manifest: &Value, roles: Option<&[&str]>) -> Vec<CatalogFile> {
    manifest["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            if let Some(roles) = roles
                && !roles.contains(&f["role"].as_str()?)
            {
                return None;
            }
            Some(CatalogFile {
                name: f["file"].as_str()?.to_string(),
                bytes: f["bytes"].as_u64()?,
                sha256: f["sha256"].as_str()?.to_string(),
            })
        })
        .collect()
}

fn catalog(leaf: &Path, manifest: &Value, files: &[CatalogFile]) -> Result<Catalog, String> {
    let text = |key: &str| {
        manifest[key]
            .as_str()
            .map(str::to_string)
            .ok_or(format!("manifest has no {key}"))
    };
    Ok(Catalog {
        v: 1,
        tool: TOOL.into(),
        product: text("product")?,
        shot: text("shot")?,
        version: text("version")?,
        caller: text("caller")?,
        made: text("made")?,
        path: leaf.to_string_lossy().into_owned(),
        files: files.to_vec(),
    })
}

fn lock_path() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/user").join(runtime_uid()));
    runtime.join("pito-archive/catalog.lock")
}

fn runtime_uid() -> String {
    use std::os::unix::fs::MetadataExt;
    fs::metadata("/proc/self")
        .map(|m| m.uid().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn append(root: &Path, catalog: &Catalog, fresh: bool) -> Result<(), String> {
    let path = root.join("catalog.jsonl");
    if !fresh {
        match fs::read_to_string(&path) {
            Ok(text) => {
                if text
                    .lines()
                    .any(|l| serde_json::from_str::<Catalog>(l).is_ok_and(|line| line == *catalog))
                {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let row = format!("{}\n", catalog.line()?);
    let written = file
        .write(row.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if written != row.len() {
        return Err(format!("{}: short catalog write", path.display()));
    }
    Ok(())
}

fn instant(made: &str) -> Result<DateTime<FixedOffset>, String> {
    DateTime::parse_from_rfc3339(made).map_err(|e| format!("made {made:?}: {e}"))
}

#[cfg(test)]
thread_local! {
    static HASHED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static SETTLED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn digest(mut from: impl Read, mut to: impl Write) -> io::Result<(u64, String)> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.update(&buffer[..n]);
        to.write_all(&buffer[..n])?;
        total += n as u64;
    }
    #[cfg(test)]
    HASHED.with(|h| h.set(h.get() + total));
    Ok((total, format!("{:x}", hasher.finalize())))
}

fn hash(path: &Path) -> Result<(u64, String), String> {
    let named = |e: io::Error| format!("{}: {e}", path.display());
    digest(File::open(path).map_err(named)?, io::sink()).map_err(named)
}

fn copy_hashed(src: &Path, dst: &Path) -> Result<(u64, String), String> {
    let from = File::open(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let named = |e: io::Error| format!("{}: {e}", dst.display());
    let mut to = File::create(dst).map_err(named)?;
    let copied = digest(from, &mut to).map_err(named)?;
    to.sync_all().map_err(named)?;
    Ok(copied)
}

#[derive(Debug)]
enum Outcome {
    Archived(PathBuf),
    Conflict(String),
    Refused(String),
    Retry(String),
}

fn report(outcome: &Outcome) {
    match outcome {
        Outcome::Archived(leaf) => eprintln!("archive: {}", leaf.display()),
        Outcome::Conflict(e) => eprintln!("archive: conflict: {e}"),
        Outcome::Refused(e) => eprintln!("archive: refused: {e}"),
        Outcome::Retry(e) => eprintln!("archive: {e}; queued"),
    }
}

fn holds(leaf: &Path, made: &str, files: &[CatalogFile]) -> Result<bool, Outcome> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", leaf.display());
    let text = match fs::read(leaf.join("manifest.json")) {
        Ok(text) => text,
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(false);
        }
        Err(e) => return Err(Outcome::Retry(at(&e))),
    };
    let old: Value = serde_json::from_slice(&text).map_err(|e| Outcome::Conflict(at(&e)))?;
    let theirs = old["made"]
        .as_str()
        .ok_or_else(|| "its manifest has no made".to_string())
        .and_then(instant)
        .map_err(|e| Outcome::Conflict(at(&e)))?;
    if theirs != instant(made).map_err(|e| Outcome::Conflict(at(&e)))? {
        return Ok(false);
    }
    let differs = |f: &CatalogFile| {
        Outcome::Conflict(at(&format!(
            "{} differs from this render's manifest",
            f.name
        )))
    };
    for f in files {
        match fs::metadata(leaf.join(&f.name)) {
            Ok(meta) if !meta.is_file() => return Ok(false),
            Ok(meta) if meta.len() != f.bytes => return Err(differs(f)),
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(Outcome::Retry(at(&e))),
        }
    }
    for f in files {
        let (bytes, sha256) = hash(&leaf.join(&f.name)).map_err(Outcome::Retry)?;
        if bytes != f.bytes || sha256 != f.sha256 {
            return Err(differs(f));
        }
    }
    Ok(true)
}

fn place(
    root: &Path,
    base: &Path,
    made: &str,
    files: &[CatalogFile],
) -> Result<(PathBuf, bool), Outcome> {
    let name = base
        .file_name()
        .ok_or_else(|| Outcome::Refused("leaf has no name".into()))?
        .to_string_lossy()
        .into_owned();
    let mut n = 1;
    loop {
        let leaf = if n == 1 {
            base.to_path_buf()
        } else {
            base.with_file_name(format!("{name}-{n}"))
        };
        let path = root.join(&leaf);
        if !path
            .try_exists()
            .map_err(|e| Outcome::Retry(format!("{}: {e}", path.display())))?
        {
            return Ok((leaf, true));
        }
        if holds(&path, made, files)? {
            return Ok((leaf, false));
        }
        n += 1;
    }
}

fn build(
    source: &Path,
    leaf: &Path,
    files: &[CatalogFile],
    archived: &Value,
) -> Result<(), String> {
    let partial = crate::encode::temp_of(leaf);
    remove(&partial, fs::remove_dir_all(&partial))?;
    let parent = leaf.parent().ok_or("leaf has no parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    fs::create_dir(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
    for f in files {
        let dest = partial.join(&f.name);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let src = source.join(&f.name);
        let (bytes, sha256) = copy_hashed(&src, &dest)?;
        if bytes != f.bytes || sha256 != f.sha256 {
            return Err(format!(
                "{}: copied sha256 differs from the manifest",
                src.display()
            ));
        }
    }
    write_manifest(&partial.join("manifest.json"), archived)?;
    fs::rename(&partial, leaf).map_err(|e| format!("{}: {e}", leaf.display()))
}

fn copy_entry(
    source: &Path,
    root: &Path,
    base: &Path,
    lock_file: &Path,
    manifest: &mut Value,
    files: &[CatalogFile],
) -> Outcome {
    match archive_into(source, root, base, lock_file, manifest, files) {
        Ok(leaf) => Outcome::Archived(leaf),
        Err(outcome) => outcome,
    }
}

fn archive_into(
    source: &Path,
    root: &Path,
    base: &Path,
    lock_file: &Path,
    manifest: &mut Value,
    files: &[CatalogFile],
) -> Result<PathBuf, Outcome> {
    let _guard = paths::lock(
        lock_file,
        paths::Lock::Exclusive,
        "the archive catalog, shared with pfx",
    )
    .map_err(Outcome::Retry)?;
    let made = manifest["made"]
        .as_str()
        .ok_or_else(|| Outcome::Refused("manifest has no made".into()))?;
    let masters = masters(root);
    let (relative, fresh) = place(&masters, base, made, files)?;
    let leaf = masters.join(&relative);
    let catalog = catalog(&relative, manifest, files).map_err(Outcome::Refused)?;
    let mut archived = manifest.clone();
    archived["archive"] = json!({
        "leaf": catalog.path,
        "state": "archived",
        "catalog": serde_json::to_value(&catalog).map_err(|e| Outcome::Retry(e.to_string()))?,
    });
    if fresh {
        build(source, &leaf, files, &archived).map_err(Outcome::Retry)?;
    }
    append(&masters, &catalog, fresh).map_err(Outcome::Retry)?;
    *manifest = archived;
    Ok(leaf)
}

const QUEUE: &str = "archive-queue";

fn queue_root() -> PathBuf {
    cache::root().join(QUEUE)
}

pub fn queue_stats(cache_root: &Path) -> (u64, u64) {
    let queue = cache_root.join(QUEUE);
    let entries = fs::read_dir(&queue)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).count() as u64)
        .unwrap_or(0);
    (entries, cache::size(&queue))
}

struct Queued {
    local: PathBuf,
    root: PathBuf,
    base: PathBuf,
    manifest: Value,
    files: Vec<CatalogFile>,
}

fn remove(path: &Path, removed: io::Result<()>) -> Result<(), String> {
    match removed {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
        _ => Ok(()),
    }
}

fn queue(job: &Queued, source: &Path, queue_root: &Path) -> Result<PathBuf, String> {
    let key = paths::hash_bytes(
        format!(
            "{}:{}",
            job.local.display(),
            job.root.join(&job.base).display()
        )
        .as_bytes(),
    );
    let entry = queue_root.join(key);
    fs::create_dir_all(&entry).map_err(|e| format!("{}: {e}", entry.display()))?;
    let record = entry.join("source.json");
    remove(&record, fs::remove_file(&record))?;
    for f in &job.files {
        let src = source.join(&f.name);
        let dst = entry.join(&f.name);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        remove(&dst, fs::remove_file(&dst))?;
        let bytes = fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
        if bytes != f.bytes {
            return Err(format!(
                "{}: {bytes} bytes, the manifest says {}",
                src.display(),
                f.bytes
            ));
        }
    }
    write_record(
        &record,
        &json!({
            "manifest": job.local.display().to_string(),
            "root": job.root.display().to_string(),
            "leaf": job.base.display().to_string(),
            "files": job.files,
            "data": job.manifest,
        }),
    )?;
    Ok(entry)
}

fn settle(
    job: &mut Queued,
    files: &Path,
    entry: Option<&Path>,
    lock_file: &Path,
) -> Result<Outcome, String> {
    #[cfg(test)]
    SETTLED.with(|s| s.borrow_mut().push(files.to_path_buf()));
    let made = job.manifest["made"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let outcome = copy_entry(
        files,
        &job.root,
        &job.base,
        lock_file,
        &mut job.manifest,
        &job.files,
    );
    let state = match &outcome {
        Outcome::Retry(_) => return Ok(outcome),
        Outcome::Archived(_) => None,
        Outcome::Conflict(e) => Some(("conflict", e)),
        Outcome::Refused(e) => Some(("refused", e)),
    };
    if let Some((state, error)) = state {
        job.manifest["archive"]["state"] = json!(state);
        job.manifest["archive"]["error"] = json!(error);
    }
    refresh_local(&job.local, &made, &job.manifest)?;
    if let Some(entry) = entry
        && let Err(e) = remove(entry, fs::remove_dir_all(entry))
    {
        eprintln!("archive: {e}");
    }
    Ok(outcome)
}

pub fn finish(path: &Path, manifest: &mut Value, skip: Option<&str>) -> Result<(), String> {
    let root = root();
    let skip = skip.or(root.is_none().then_some("unconfigured"));
    let root = root.unwrap_or_default();
    finish_at(&root, &lock_path(), &queue_root(), path, manifest, skip)
}

fn finish_at(
    root: &Path,
    lock_file: &Path,
    queue_root: &Path,
    path: &Path,
    manifest: &mut Value,
    skip: Option<&str>,
) -> Result<(), String> {
    let base = base_leaf(manifest);
    if let Some(reason) = skip {
        manifest["archive"] = json!({"state": "skipped", "reason": reason});
        if let Ok(base) = &base {
            manifest["archive"]["leaf"] = json!(base.display().to_string());
        }
        write_manifest(path, manifest)?;
        eprintln!("archive: skipped ({reason})");
        jobs::note(&format!("archive skipped: {reason}"));
        return Ok(());
    }
    let base = match base {
        Ok(base) => base,
        Err(e) => {
            manifest["archive"] = json!({"state": "refused", "error": e});
            write_manifest(path, manifest)?;
            report(&Outcome::Refused(e));
            jobs::note("archive refused");
            return Ok(());
        }
    };
    manifest["archive"] = json!({"leaf": base.display().to_string(), "state": "queued"});
    write_manifest(path, manifest)?;
    jobs::stage("archive", Some(1), "outputs");
    let mut job = Queued {
        local: path.to_path_buf(),
        root: root.to_path_buf(),
        base,
        files: selected(manifest, None),
        manifest: manifest.clone(),
    };
    let source = path.parent().unwrap_or(path);
    let entry = queue(&job, source, queue_root)
        .map_err(|e| eprintln!("archive queue: {e}"))
        .ok();
    let outcome = if root.is_dir() {
        settle(
            &mut job,
            entry.as_deref().unwrap_or(source),
            entry.as_deref(),
            lock_file,
        )?
    } else {
        Outcome::Retry(format!("{} is unavailable", root.display()))
    };
    *manifest = job.manifest;
    report(&outcome);
    match outcome {
        Outcome::Archived(_) => jobs::progress(1),
        Outcome::Conflict(_) => jobs::note("archive conflict"),
        Outcome::Refused(_) => jobs::note("archive refused"),
        Outcome::Retry(_) => jobs::note("archive queued"),
    }
    Ok(())
}

pub fn retry() {
    retry_at(&queue_root(), &lock_path());
}

pub fn retry_at_start() {
    start_at(
        &queue_root(),
        &lock_path(),
        paths::direct(),
        root().as_deref(),
        ride(),
    );
}

fn start_at(queue_root: &Path, lock_file: &Path, direct: bool, root: Option<&Path>, ride: bool) {
    if !test_run(direct, root) && !ride {
        retry_at(queue_root, lock_file);
    }
}

fn retry_at(queue_root: &Path, lock_file: &Path) {
    retry_with(queue_root, lock_file, Path::is_dir);
}

fn retry_with(queue_root: &Path, lock_file: &Path, mut check: impl FnMut(&Path) -> bool) {
    let Ok(entries) = fs::read_dir(queue_root) else {
        return;
    };
    let mut online: HashMap<PathBuf, bool> = HashMap::new();
    for entry in entries.flatten() {
        let outcome = retry_entry(&entry.path(), lock_file, &mut |root: &Path| {
            *online
                .entry(root.to_path_buf())
                .or_insert_with(|| check(root))
        });
        report(&outcome);
    }
}

fn retry_entry(dir: &Path, lock_file: &Path, online: &mut dyn FnMut(&Path) -> bool) -> Outcome {
    let mut job = match read_queued(dir) {
        Ok(job) => job,
        Err(e) => return Outcome::Retry(format!("{}: {e}", dir.display())),
    };
    if let Err(e) = contained(&job) {
        return Outcome::Refused(format!("{}: {e}", dir.display()));
    }
    if !online(&job.root) {
        return Outcome::Retry(format!("{} is unavailable", job.root.display()));
    }
    settle(&mut job, dir, Some(dir), lock_file).unwrap_or_else(Outcome::Retry)
}

fn inside(path: &Path) -> bool {
    let mut parts = path.components().peekable();
    parts.peek().is_some()
        && parts.all(|c| match c {
            std::path::Component::Normal(part) => part.to_str().is_some_and(paths::safe_name),
            _ => false,
        })
}

fn contained(job: &Queued) -> Result<(), String> {
    if !inside(&job.base) {
        return Err(format!(
            "the queued leaf {:?} leaves the archive",
            job.base.display().to_string()
        ));
    }
    match job.files.iter().find(|f| !inside(Path::new(&f.name))) {
        Some(f) => Err(format!("the queued file {:?} leaves its leaf", f.name)),
        None => Ok(()),
    }
}

fn read_queued(dir: &Path) -> Result<Queued, String> {
    let path = dir.join("source.json");
    let mut record: Value =
        serde_json::from_slice(&fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    if record["root"].is_null()
        || record["leaf"]
            .as_str()
            .is_some_and(|leaf| Path::new(leaf).starts_with(LEGACY))
    {
        migrate(&mut record)?;
        write_record(&path, &record)?;
    }
    let text = |key: &str| {
        record[key]
            .as_str()
            .map(PathBuf::from)
            .ok_or(format!("queue has no {key}"))
    };
    let (local, root, base) = (text("manifest")?, text("root")?, text("leaf")?);
    let manifest = record["data"].take();
    let files = match record["files"].take() {
        Value::Null => selected(&manifest, Some(&QUEUED_BEFORE_0_20)),
        files => serde_json::from_value(files).map_err(|e| format!("queue files: {e}"))?,
    };
    Ok(Queued {
        local,
        root,
        base,
        manifest,
        files,
    })
}

fn migrate(record: &mut Value) -> Result<(), String> {
    if record["root"].is_null() {
        let previous = PathBuf::from(
            record["data"]["archive"]["leaf"]
                .as_str()
                .ok_or("queue has no archive leaf")?,
        );
        let root = previous
            .ancestors()
            .find(|a| a.file_name().is_some_and(|n| n == LEGACY))
            .and_then(Path::parent)
            .ok_or("queue has no archive root")?
            .to_path_buf();
        record["root"] = json!(root.display().to_string());
    }
    let data = &mut record["data"];
    let base = base_leaf(data)?;
    data["archive"] = json!({"leaf": base.display().to_string(), "state": "queued"});
    record["leaf"] = json!(base.display().to_string());
    Ok(())
}

fn refresh_local(local: &Path, made: &str, manifest: &Value) -> Result<(), String> {
    let Ok(bytes) = fs::read(local) else {
        return Ok(());
    };
    let Ok(current) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(());
    };
    let same = match (current["made"].as_str().map(instant), instant(made)) {
        (Some(Ok(theirs)), Ok(ours)) => theirs == ours,
        _ => false,
    };
    if same {
        write_manifest(local, manifest)?;
    }
    Ok(())
}

pub fn reindex() -> Result<usize, String> {
    let root = root().ok_or(
        "archive: no root; set archive in prec's config.toml or PITO_ARCHIVE (prec doctor shows where)",
    )?;
    reindex_at(&root, &lock_path())
}

fn reindex_at(root: &Path, lock_file: &Path) -> Result<usize, String> {
    let masters = masters(root);
    let mut stack: Vec<PathBuf> = TOOLS
        .iter()
        .map(|tool| masters.join(tool))
        .filter(|dir| dir.is_dir())
        .collect();
    if stack.is_empty() {
        return Err(format!(
            "{} has no {} folder",
            masters.display(),
            TOOLS.map(|tool| format!("{tool}/")).join(" or ")
        ));
    }
    let _guard = paths::lock(
        lock_file,
        paths::Lock::Exclusive,
        "the archive catalog, shared with pfx",
    )?;
    let mut lines = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
            if path.is_dir() {
                if !path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .ends_with(".partial")
                {
                    stack.push(path);
                }
            } else if path.file_name().is_some_and(|n| n == "manifest.json") {
                lines.extend(
                    indexed(&masters, &path).map_err(|e| format!("{}: {e}", path.display()))?,
                );
            }
        }
    }
    lines.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    crate::encode::atomic_file(&masters.join("catalog.jsonl"), |w| {
        for (_, _, line) in &lines {
            writeln!(w, "{line}").map_err(|e| e.to_string())?;
        }
        Ok(())
    })?;
    Ok(lines.len())
}

fn indexed(
    masters: &Path,
    path: &Path,
) -> Result<Option<(DateTime<FixedOffset>, String, String)>, String> {
    let manifest: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let stored = &manifest["archive"]["catalog"];
    if stored.is_null() {
        return Ok(None);
    }
    let mut catalog = Catalog::read(stored)?;
    let leaf = path
        .parent()
        .and_then(|leaf| leaf.strip_prefix(masters).ok())
        .ok_or("the leaf lies outside the archive's masters")?;
    catalog.tool = leaf
        .components()
        .next()
        .ok_or("the leaf has no tool folder")?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    catalog.path = leaf.to_string_lossy().into_owned();
    let made = instant(&catalog.made)?;
    let line = catalog.line()?;
    Ok(Some((made, catalog.path, line)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MADE: &str = "2026-10-02T09:14:07+02:00";

    impl Outcome {
        fn unwrap(self) -> PathBuf {
            match self {
                Outcome::Archived(leaf) => leaf,
                other => panic!("not archived: {other:?}"),
            }
        }

        fn conflict(&self) -> bool {
            matches!(self, Outcome::Conflict(_))
        }
    }

    fn queued(local: &Path, root: &Path, base: &Path, manifest: &Value) -> Queued {
        Queued {
            local: local.to_path_buf(),
            root: root.to_path_buf(),
            base: base.to_path_buf(),
            manifest: manifest.clone(),
            files: selected(manifest, None),
        }
    }

    fn archive(
        source: &Path,
        root: &Path,
        base: &Path,
        lock_file: &Path,
        manifest: &mut Value,
    ) -> Outcome {
        let files = selected(manifest, None);
        copy_entry(source, root, base, lock_file, manifest, &files)
    }

    fn work(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/archive-unit")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn render(
        w: &Path,
        dir: &str,
        product: &str,
        version: &str,
        made: &str,
        bytes: &[u8],
    ) -> (PathBuf, PathBuf, Value) {
        let source = w.join(dir);
        fs::create_dir_all(&source).unwrap();
        let master = source.join("shot.master.png");
        fs::write(&master, bytes).unwrap();
        let local = source.join("shot.json");
        let base = folder(product, "shot", version, made, "site").unwrap();
        let manifest = json!({
            "product":product, "shot":"shot", "version":version, "caller":"site", "made":made,
            "files":[{"role":"master", "file":"shot.master.png", "bytes":bytes.len(), "sha256":paths::hash_file(&master).unwrap()}],
            "archive":{"leaf":base.display().to_string(),"state":"queued"}
        });
        write_manifest(&local, &manifest).unwrap();
        (local, base, manifest)
    }

    fn fixture(w: &Path, made: &str) -> (PathBuf, PathBuf, PathBuf, Value) {
        let root = w.join("archive");
        fs::create_dir_all(&root).unwrap();
        let (local, base, manifest) = render(w, "local", "alpha", "v1.2.3", made, b"master");
        (local, root, base, manifest)
    }

    fn read(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn entries(queue_root: &Path) -> usize {
        fs::read_dir(queue_root).map(|rd| rd.count()).unwrap_or(0)
    }

    #[test]
    fn leaf_uses_recorded_time_to_the_second_version_and_cleaned_caller() {
        assert_eq!(
            folder("alpha", "shot", "v1.2.3", MADE, "site").unwrap(),
            Path::new("prec/alpha/shot/2026-10-02_091407_v1.2.3_site")
        );
        assert_eq!(caller(Some("--SiTe__Docs!!")), "site-docs");
        assert_eq!(caller(Some("___")), "manual");
        assert_eq!(caller(None), "manual");
        assert_eq!(
            version("main", Some("0123456789abcdef"), None),
            "0123456789ab"
        );
    }

    #[test]
    fn an_unsafe_name_is_refused() {
        for (product, shot, version, caller) in [
            ("alpha", "shot", "release/1.0", "site"),
            ("..", "shot", "v1", "site"),
            ("alpha", "", "v1", "site"),
            ("alpha", "shot", "v1", "a b"),
        ] {
            assert!(folder(product, shot, version, MADE, caller).is_err());
        }
        let w = work("unsafe");
        let root = w.join("archive");
        let (local, _, mut manifest) = render(&w, "local", "alpha", "v1", MADE, b"master");
        manifest["version"] = json!("release/1.0");
        let queue_root = w.join("queue");
        finish_at(
            &root,
            &w.join("runtime/catalog.lock"),
            &queue_root,
            &local,
            &mut manifest,
            None,
        )
        .unwrap();
        let written = read(&local);
        assert_eq!(written["archive"]["state"], "refused");
        assert!(written["archive"]["leaf"].is_null());
        assert!(
            written["archive"]["error"]
                .as_str()
                .unwrap()
                .contains("release/1.0")
        );
        assert!(!root.exists());
        assert!(!queue_root.exists());
    }

    #[test]
    fn leaf_is_published_once_and_conflicts_are_preserved() {
        let w = work("publish");
        let (local, root, base, mut manifest) = fixture(&w, MADE);
        let leaf = masters(&root).join(&base);
        let lock_file = w.join("runtime/catalog.lock");
        let partial = leaf.with_file_name(format!(
            "{}.partial",
            leaf.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir_all(&partial).unwrap();
        fs::write(partial.join("stale"), b"stale").unwrap();
        let source = local.parent().unwrap();
        assert_eq!(
            archive(source, &root, &base, &lock_file, &mut manifest).unwrap(),
            leaf
        );
        assert!(!partial.exists());
        assert_eq!(fs::read(leaf.join("shot.master.png")).unwrap(), b"master");
        assert_eq!(read(&leaf.join("manifest.json")), manifest);
        assert!(manifest["archive"]["catalog"].is_object());
        assert_eq!(manifest["archive"]["leaf"], base.display().to_string());
        let catalog = fs::read_to_string(masters(&root).join("catalog.jsonl")).unwrap();
        assert_eq!(parsed(&catalog), [manifest["archive"]["catalog"].clone()]);
        assert_eq!(
            archive(source, &root, &base, &lock_file, &mut manifest).unwrap(),
            leaf
        );
        assert_eq!(
            fs::read_to_string(masters(&root).join("catalog.jsonl")).unwrap(),
            catalog
        );
        assert_eq!(fs::read_dir(leaf.parent().unwrap()).unwrap().count(), 1);
        fs::write(leaf.join("shot.master.png"), b"other!").unwrap();
        assert!(archive(source, &root, &base, &lock_file, &mut manifest).conflict());
        assert_eq!(fs::read(leaf.join("shot.master.png")).unwrap(), b"other!");
        let queue_root = w.join("queue");
        finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        assert_eq!(manifest["archive"]["state"], "conflict");
        assert_eq!(fs::read(leaf.join("shot.master.png")).unwrap(), b"other!");
        assert_eq!(entries(&queue_root), 0);
    }

    #[test]
    fn renders_in_one_minute_get_their_own_leaves_and_the_same_render_is_left_alone() {
        let w = work("leaf-names");
        let root = w.join("archive");
        fs::create_dir_all(&root).unwrap();
        let lock_file = w.join("runtime/catalog.lock");
        let (a, base_a, mut first) = render(&w, "a", "alpha", "v1.2.3", MADE, b"master");
        let (b, base_b, mut later) = render(
            &w,
            "b",
            "alpha",
            "v1.2.3",
            "2026-10-02T09:14:45+02:00",
            b"later!",
        );
        let (c, base_c, mut same_second) = render(&w, "c", "alpha", "v1.2.3", MADE, b"second");
        let smaller = c.parent().unwrap().join("shot-1280x720.master.png");
        fs::rename(c.parent().unwrap().join("shot.master.png"), &smaller).unwrap();
        same_second["files"][0]["file"] = json!("shot-1280x720.master.png");
        assert_eq!(base_a, base_c);
        let leaf_a = archive(a.parent().unwrap(), &root, &base_a, &lock_file, &mut first).unwrap();
        let leaf_b = archive(b.parent().unwrap(), &root, &base_b, &lock_file, &mut later).unwrap();
        let leaf_c = archive(
            c.parent().unwrap(),
            &root,
            &base_c,
            &lock_file,
            &mut same_second,
        )
        .unwrap();
        let shot = masters(&root).join("prec/alpha/shot");
        assert_eq!(leaf_a, shot.join("2026-10-02_091407_v1.2.3_site"));
        assert_eq!(leaf_b, shot.join("2026-10-02_091445_v1.2.3_site"));
        assert_eq!(leaf_c, shot.join("2026-10-02_091407_v1.2.3_site-2"));
        assert_eq!(fs::read(leaf_a.join("shot.master.png")).unwrap(), b"master");
        assert_eq!(
            fs::read(leaf_c.join("shot-1280x720.master.png")).unwrap(),
            b"second"
        );
        assert_eq!(
            same_second["archive"]["leaf"],
            "prec/alpha/shot/2026-10-02_091407_v1.2.3_site-2"
        );
        assert_eq!(
            same_second["archive"]["catalog"]["path"],
            "prec/alpha/shot/2026-10-02_091407_v1.2.3_site-2"
        );
        let catalog = fs::read_to_string(masters(&root).join("catalog.jsonl")).unwrap();
        assert_eq!(catalog.lines().count(), 3);
        let (_, _, mut again) = render(&w, "a", "alpha", "v1.2.3", MADE, b"master");
        assert_eq!(
            archive(a.parent().unwrap(), &root, &base_a, &lock_file, &mut again).unwrap(),
            leaf_a
        );
        assert_eq!(fs::read_dir(&shot).unwrap().count(), 3);
        assert_eq!(
            fs::read_to_string(masters(&root).join("catalog.jsonl")).unwrap(),
            catalog
        );
        fs::write(leaf_a.join("extra.txt"), b"extra").unwrap();
        assert!(
            holds(
                &leaf_a,
                "2026-10-02T07:14:07+00:00",
                &selected(&first, None)
            )
            .unwrap()
        );
        assert!(
            !holds(
                &leaf_a,
                "2026-10-02T09:14:08+02:00",
                &selected(&first, None)
            )
            .unwrap()
        );
        let (d, base_d, mut clash) = render(&w, "d", "alpha", "v1.2.3", MADE, b"third!");
        assert!(archive(d.parent().unwrap(), &root, &base_d, &lock_file, &mut clash).conflict());
        assert_eq!(fs::read_dir(&shot).unwrap().count(), 3);
    }

    #[test]
    fn a_render_is_queued_before_its_copy_and_dequeued_once_archived() {
        let w = work("write-ahead");
        let (local, root, base, mut manifest) = fixture(&w, MADE);
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        fs::remove_dir_all(&root).unwrap();
        finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        assert_eq!(read(&local)["archive"]["state"], "queued");
        assert_eq!(entries(&queue_root), 1);
        fs::create_dir_all(&root).unwrap();
        retry_at(&queue_root, &lock_file);
        assert!(masters(&root).join(&base).join("manifest.json").is_file());
        assert_eq!(read(&local)["archive"]["state"], "archived");
        assert_eq!(entries(&queue_root), 0);

        let w = work("write-ahead-online");
        let (local, root, base, mut manifest) = fixture(&w, MADE);
        let queue_root = w.join("queue");
        finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        assert_eq!(read(&local)["archive"]["state"], "archived");
        assert!(masters(&root).join(&base).join("manifest.json").is_file());
        assert_eq!(entries(&queue_root), 0);
    }

    #[test]
    fn a_manifest_that_cannot_be_written_fails_the_run_and_keeps_the_previous_one() {
        let w = work("manifest-error");
        let (local, root, _, mut manifest) = fixture(&w, MADE);
        let before = fs::read(&local).unwrap();
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        let missing = w.join("gone/shot.json");
        let err = finish_at(
            &root,
            &lock_file,
            &queue_root,
            &missing,
            &mut manifest,
            None,
        )
        .unwrap_err();
        assert!(err.contains("gone"), "{err}");
        assert!(!w.join("gone").exists());
        assert_eq!(entries(&queue_root), 0);
        let blocked = w.join("blocked.json");
        fs::create_dir_all(&blocked).unwrap();
        finish_at(
            &root,
            &lock_file,
            &queue_root,
            &blocked,
            &mut manifest,
            Some("test"),
        )
        .unwrap_err();
        assert!(blocked.is_dir());
        assert!(!blocked.with_file_name("blocked.json.partial").exists());
        assert_eq!(fs::read(&local).unwrap(), before);
        let mut broken = manifest.clone();
        broken["archive"] = json!({"state": "queued"});
        assert!(write_manifest(&missing, &broken).is_err());
        write_manifest(&local, &broken).unwrap();
        assert_eq!(read(&local), broken);
        assert!(!local.with_file_name("shot.json.partial").exists());
    }

    #[test]
    fn two_offline_renders_at_one_manifest_path_keep_two_entries() {
        let w = work("queue-keys");
        let root = w.join("archive");
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        let (local, base_1, mut v1) = render(&w, "local", "alpha", "v1.2.3", MADE, b"master");
        finish_at(&root, &lock_file, &queue_root, &local, &mut v1, None).unwrap();
        let (same, base_2, mut v2) = render(
            &w,
            "local",
            "alpha",
            "v1.2.4",
            "2026-10-02T09:20:00+02:00",
            b"second",
        );
        assert_eq!(same, local);
        finish_at(&root, &lock_file, &queue_root, &local, &mut v2, None).unwrap();
        assert_eq!(entries(&queue_root), 2);
        fs::create_dir_all(&root).unwrap();
        retry_at(&queue_root, &lock_file);
        assert_eq!(entries(&queue_root), 0);
        assert_eq!(
            fs::read(masters(&root).join(&base_1).join("shot.master.png")).unwrap(),
            b"master"
        );
        assert_eq!(
            fs::read(masters(&root).join(&base_2).join("shot.master.png")).unwrap(),
            b"second"
        );
        let written = read(&local);
        assert_eq!(written["version"], "v1.2.4");
        assert_eq!(written["archive"]["state"], "archived");
        assert_eq!(
            fs::read_to_string(masters(&root).join("catalog.jsonl"))
                .unwrap()
                .lines()
                .count(),
            2
        );
    }

    #[test]
    fn a_retry_leaves_a_newer_local_manifest_alone() {
        let w = work("newer-local");
        let (local, root, base, mut manifest) = fixture(&w, MADE);
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        fs::remove_dir_all(&root).unwrap();
        finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        let mut newer = manifest.clone();
        newer["made"] = json!("2026-10-02T10:00:00+02:00");
        newer["archive"] = json!({"state":"skipped","reason":"unpinned"});
        write_manifest(&local, &newer).unwrap();
        fs::create_dir_all(&root).unwrap();
        retry_at(&queue_root, &lock_file);
        assert!(masters(&root).join(&base).join("manifest.json").is_file());
        assert_eq!(read(&local), newer);
        assert_eq!(entries(&queue_root), 0);

        fs::remove_dir_all(&root).unwrap();
        finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        let mut same = manifest.clone();
        same["made"] = json!("2026-10-02T07:14:07+00:00");
        write_manifest(&local, &same).unwrap();
        fs::create_dir_all(&root).unwrap();
        retry_at(&queue_root, &lock_file);
        assert_eq!(read(&local)["archive"]["state"], "archived");
        assert_eq!(read(&local)["made"], MADE);
    }

    #[test]
    fn a_test_run_never_retries_the_queue_at_startup() {
        let w = work("test-run");
        let (local, root, base, mut manifest) = fixture(&w, MADE);
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        fs::remove_dir_all(&root).unwrap();
        finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        fs::create_dir_all(&root).unwrap();
        let elsewhere = Some(Path::new("/srv/archive"));
        start_at(&queue_root, &lock_file, true, elsewhere, false);
        start_at(
            &queue_root,
            &lock_file,
            false,
            Some(Path::new("/w/tmp/archive")),
            false,
        );
        start_at(&queue_root, &lock_file, false, elsewhere, true);
        assert_eq!(entries(&queue_root), 1);
        assert!(!masters(&root).join(&base).exists());
        retry_at(&queue_root, &lock_file);
        assert_eq!(entries(&queue_root), 0);
        assert!(masters(&root).join(&base).join("manifest.json").is_file());
    }

    #[test]
    fn each_theme_of_one_run_gets_its_own_leaf() {
        let w = work("themes");
        let (local, root, base, mut app) = fixture(&w, MADE);
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        let source = local.parent().unwrap();
        let latte = source.join("shot-catppuccin-latte.master.png");
        fs::write(&latte, b"latte!").unwrap();
        let mut themed = app.clone();
        themed["files"] = json!([{"role":"master", "file":"shot-catppuccin-latte.master.png", "bytes":6, "sha256":paths::hash_file(&latte).unwrap()}]);
        themed["theme"] = json!({"name":"Catppuccin Latte", "label":"catppuccin-latte"});
        let themed_local = source.join("shot-catppuccin-latte.json");
        write_manifest(&themed_local, &themed).unwrap();
        finish_at(&root, &lock_file, &queue_root, &local, &mut app, None).unwrap();
        finish_at(
            &root,
            &lock_file,
            &queue_root,
            &themed_local,
            &mut themed,
            None,
        )
        .unwrap();
        assert_eq!(app["archive"]["state"], "archived");
        assert_eq!(themed["archive"]["state"], "archived");
        assert_eq!(app["archive"]["leaf"], base.display().to_string());
        let themed_leaf = "prec/alpha/shot-catppuccin-latte/2026-10-02_091407_v1.2.3_site";
        assert_eq!(themed["archive"]["leaf"], themed_leaf);
        assert_eq!(
            fs::read(
                masters(&root)
                    .join(themed_leaf)
                    .join("shot-catppuccin-latte.master.png")
            )
            .unwrap(),
            b"latte!"
        );
        let catalog = fs::read_to_string(masters(&root).join("catalog.jsonl")).unwrap();
        let lines: Vec<Value> = catalog
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l["shot"] == "shot"));
        assert_eq!(lines[1]["path"], themed_leaf);
    }

    #[test]
    fn a_failed_copy_leaves_no_manifest_until_all_files_verify() {
        let w = work("manifest-last");
        let (local, root, base, mut manifest) = fixture(&w, MADE);
        fs::write(local.parent().unwrap().join("shot.master.png"), b"changed").unwrap();
        assert!(matches!(
            archive(
                local.parent().unwrap(),
                &root,
                &base,
                &w.join("runtime/catalog.lock"),
                &mut manifest
            ),
            Outcome::Retry(_)
        ));
        let leaf = masters(&root).join(&base);
        let partial = leaf.with_file_name(format!(
            "{}.partial",
            leaf.file_name().unwrap().to_string_lossy()
        ));
        assert!(partial.exists());
        assert!(!partial.join("manifest.json").exists());
        assert!(!leaf.exists());
        assert!(!masters(&root).join("catalog.jsonl").exists());
    }

    #[test]
    fn retry_uses_the_original_made_even_without_local_outputs() {
        let w = work("retry");
        let (local, root, base, manifest) = fixture(&w, MADE);
        let queue_root = w.join("queue");
        queue(
            &queued(&local, &root, &base, &manifest),
            local.parent().unwrap(),
            &queue_root,
        )
        .unwrap();
        fs::remove_dir_all(local.parent().unwrap()).unwrap();
        retry_at(&queue_root, &w.join("runtime/catalog.lock"));
        let archived = read(&masters(&root).join(&base).join("manifest.json"));
        assert_eq!(archived["made"], MADE);
        assert_eq!(archived["archive"]["state"], "archived");
        assert_eq!(
            fs::read_to_string(masters(&root).join("catalog.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(entries(&queue_root), 0);
    }

    #[test]
    fn a_queued_copy_keeps_its_bytes_when_the_local_file_is_rewritten_in_place() {
        let w = work("rewritten");
        let (local, root, base, manifest) = fixture(&w, MADE);
        let queue_root = w.join("queue");
        queue(
            &queued(&local, &root, &base, &manifest),
            local.parent().unwrap(),
            &queue_root,
        )
        .unwrap();
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(local.parent().unwrap().join("shot.master.png"))
            .unwrap()
            .write_all(b"newer!")
            .unwrap();
        retry_at(&queue_root, &w.join("runtime/catalog.lock"));
        assert_eq!(
            fs::read(masters(&root).join(&base).join("shot.master.png")).unwrap(),
            b"master"
        );
        assert_eq!(entries(&queue_root), 0);
    }

    fn still(w: &Path) -> (PathBuf, PathBuf, PathBuf, Value) {
        let (local, root, base, mut manifest) = fixture(w, MADE);
        let source = local.parent().unwrap();
        let mut files = manifest["files"].as_array().unwrap().clone();
        for (name, bytes, extra) in [
            ("shot.png", &b"png8"[..], json!({"size":[640, 360]})),
            ("shot-320x180.png", &b"half"[..], json!({"size":[320, 180]})),
            (
                "shot-web-160.png",
                &b"web1"[..],
                json!({"role":"web", "size":[160, 90], "web":160}),
            ),
            ("shot.cast", &b"cast"[..], json!({"role":"cast"})),
        ] {
            fs::write(source.join(name), bytes).unwrap();
            let mut entry = json!({"file":name, "bytes":bytes.len(), "sha256":paths::hash_file(&source.join(name)).unwrap()});
            for (k, v) in extra.as_object().unwrap() {
                entry[k] = v.clone();
            }
            files.push(entry);
        }
        manifest["files"] = json!(files);
        write_manifest(&local, &manifest).unwrap();
        (local, root, base, manifest)
    }

    fn names(catalog: &Value) -> Vec<&str> {
        catalog["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn a_still_archives_its_png_delivery_derived_sizes_and_web_copies() {
        let w = work("still-deliveries");
        let (local, root, base, mut manifest) = still(&w);
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        assert_eq!(manifest["archive"]["state"], "archived");
        let all = [
            "shot.master.png",
            "shot.png",
            "shot-320x180.png",
            "shot-web-160.png",
            "shot.cast",
        ];
        let leaf = masters(&root).join(&base);
        for name in all {
            assert_eq!(
                fs::read(leaf.join(name)).unwrap(),
                fs::read(local.parent().unwrap().join(name)).unwrap(),
                "{name}"
            );
        }
        assert_eq!(names(&manifest["archive"]["catalog"]), all);
        let line = parsed(&fs::read_to_string(masters(&root).join("catalog.jsonl")).unwrap());
        assert_eq!(line, [manifest["archive"]["catalog"].clone()]);
        assert_eq!(entries(&queue_root), 0);
    }

    #[test]
    fn a_still_queued_by_prec_0_19_archives_what_it_queued() {
        let w = work("queued-0-19");
        let (local, root, base, manifest) = still(&w);
        let queue_root = w.join("queue");
        let mut job = queued(&local, &root, &base, &manifest);
        job.files = selected(&manifest, Some(&QUEUED_BEFORE_0_20));
        let entry = queue(&job, local.parent().unwrap(), &queue_root).unwrap();
        let record = entry.join("source.json");
        let mut written = read(&record);
        written.as_object_mut().unwrap().remove("files");
        fs::write(&record, serde_json::to_vec(&written).unwrap()).unwrap();
        assert!(!entry.join("shot.png").exists());
        retry_at(&queue_root, &w.join("runtime/catalog.lock"));
        let local_manifest = read(&local);
        assert_eq!(local_manifest["archive"]["state"], "archived");
        assert_eq!(
            names(&local_manifest["archive"]["catalog"]),
            ["shot.master.png", "shot.cast"]
        );
        assert!(masters(&root).join(&base).join("shot.cast").is_file());
        assert!(!masters(&root).join(&base).join("shot.png").exists());
        assert_eq!(entries(&queue_root), 0);
    }

    #[test]
    fn retry_reads_queue_entries_older_precs_wrote() {
        let legacy = Path::new("pito-replay/alpha/shot/2026-10-02_091407_v1.2.3_site");
        for with_root in [false, true] {
            let w = work(if with_root {
                "legacy-0-20"
            } else {
                "legacy-0-17"
            });
            let (local, root, base, mut manifest) = fixture(&w, MADE);
            let leaf = if with_root {
                legacy.to_path_buf()
            } else {
                root.join(legacy)
            };
            manifest["archive"] = json!({"leaf": leaf.display().to_string(), "state":"queued"});
            write_manifest(&local, &manifest).unwrap();
            let queue_root = w.join("queue");
            let entry = queue(
                &queued(&local, &root, &base, &manifest),
                local.parent().unwrap(),
                &queue_root,
            )
            .unwrap();
            let mut record = json!({"manifest":local.display().to_string(),"data":manifest});
            if with_root {
                record["root"] = json!(root.display().to_string());
                record["leaf"] = json!(legacy.display().to_string());
            }
            fs::write(
                entry.join("source.json"),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
            retry_at(&queue_root, &w.join("runtime/catalog.lock"));
            let written = read(&local);
            assert_eq!(written["archive"]["state"], "archived");
            assert_eq!(written["archive"]["leaf"], base.display().to_string());
            assert_eq!(written["made"], MADE);
            assert!(masters(&root).join(&base).join("manifest.json").is_file());
            assert!(!root.join("pito-replay").exists());
            assert_eq!(
                fs::read_to_string(masters(&root).join("catalog.jsonl"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
            assert_eq!(entries(&queue_root), 0);
        }
    }

    fn parsed(text: &str) -> Vec<Value> {
        text.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    const PFX_LINE: &str = r#"{"v":1,"tool":"pfx","product":"alpha","shot":"hero","version":"99e8c315bfbe","caller":"site","made":"2026-10-03T20:20:37.881639132+02:00","path":"pfx/alpha/hero/2026-10-03_2020_99e8c315bfbe_site","files":[{"name":"alpha-still-hero-3840x2160.master.png","bytes":21646432,"sha256":"4e535f37e5da8eebb0c359f9fa8b50d783c90157b8b89a07a664272fa5dbf4a0"}]}"#;

    fn pfx_leaf(root: &Path, archive: Value) {
        let leaf = masters(root).join("pfx/alpha/hero/2026-10-03_2020_99e8c315bfbe_site");
        fs::create_dir_all(&leaf).unwrap();
        fs::write(
            leaf.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "product":"alpha", "kind":"still", "shot":"hero", "version":"99e8c315bfbe",
                "made":"2026-10-03T20:20:37.881639132+02:00", "archive":archive,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn reindex_keeps_both_tools_in_instant_then_path_order() {
        let w = work("reindex");
        let root = w.join("archive");
        fs::create_dir_all(&root).unwrap();
        let lock_file = w.join("runtime/catalog.lock");
        let (a, base_a, mut beta) = render(
            &w,
            "a",
            "beta",
            "v1.2.3",
            "2026-10-03T19:00:00+00:00",
            b"beta!!",
        );
        let (b, base_b, mut alpha) = render(
            &w,
            "b",
            "alpha",
            "v1.2.3",
            "2026-10-03T21:00:00+02:00",
            b"alpha!",
        );
        archive(a.parent().unwrap(), &root, &base_a, &lock_file, &mut beta).unwrap();
        archive(b.parent().unwrap(), &root, &base_b, &lock_file, &mut alpha).unwrap();
        let appended = fs::read_to_string(masters(&root).join("catalog.jsonl")).unwrap();
        let prec: Vec<&str> = appended.lines().collect();
        let pfx: Value = serde_json::from_str(PFX_LINE).unwrap();
        pfx_leaf(
            &root,
            json!({"leaf":pfx["path"], "state":"archived", "catalog":pfx}),
        );
        fs::create_dir_all(masters(&root).join("prec/p/s/skip.partial")).unwrap();
        fs::write(
            masters(&root).join("prec/p/s/skip.partial/manifest.json"),
            b"{",
        )
        .unwrap();
        assert_eq!(reindex_at(&root, &lock_file).unwrap(), 3);
        assert_eq!(
            parsed(&fs::read_to_string(masters(&root).join("catalog.jsonl")).unwrap()),
            parsed(&format!("{PFX_LINE}\n{}\n{}\n", prec[1], prec[0]))
        );
        assert!(!masters(&root).join("catalog.jsonl.partial").exists());

        let older =
            masters(&root).join("pfx/alpha/hero/2026-10-01_0900_v0.1.0_manual/manifest.json");
        fs::create_dir_all(older.parent().unwrap()).unwrap();
        for archive in [
            json!({"state":"archived"}),
            json!({"state":"archived","catalog":null}),
        ] {
            fs::write(&older, json!({"made":MADE, "archive":archive}).to_string()).unwrap();
            assert_eq!(reindex_at(&root, &lock_file).unwrap(), 3);
        }

        let before = fs::read(masters(&root).join("catalog.jsonl")).unwrap();
        let broken = masters(&root).join("prec/alpha/broken/x/manifest.json");
        fs::create_dir_all(broken.parent().unwrap()).unwrap();
        let mut unknown = pfx.clone();
        unknown["extra"] = json!(1);
        for text in [
            "{".to_string(),
            json!({"made":MADE, "archive":{"catalog":{"v":1}}}).to_string(),
            json!({"made":MADE, "archive":{"catalog":"{"}}).to_string(),
            json!({"made":MADE, "archive":{"catalog":unknown}}).to_string(),
        ] {
            fs::write(&broken, text).unwrap();
            let error = reindex_at(&root, &lock_file).unwrap_err();
            assert!(error.contains(&broken.display().to_string()));
        }
        assert_eq!(
            fs::read(masters(&root).join("catalog.jsonl")).unwrap(),
            before
        );
        assert!(reindex_at(&w.join("missing"), &lock_file).is_err());
    }

    #[test]
    fn reindex_reads_the_older_shapes_and_names_each_leaf_where_it_lies() {
        let w = work("reindex-old");
        let root = w.join("archive");
        let line = r#"{"v":1,"tool":"prec","product":"alpha","shot":"shot","version":"v1.2.2","caller":"site","made":"2026-10-04T08:00:00+02:00","path":"prec/alpha/shot/2026-10-04_0800_v1.2.2_site","files":[{"name":"shot.master.png","bytes":6,"sha256":"00"}]}"#;
        let old_line = line
            .replace(r#""tool":"prec""#, r#""tool":"pito-replay""#)
            .replace(r#""path":"prec/"#, r#""path":"pito-replay/"#);
        let leaf = masters(&root).join("prec/alpha/shot/2026-10-04_0800_v1.2.2_site");
        fs::create_dir_all(&leaf).unwrap();
        fs::write(
            leaf.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "made":"2026-10-04T08:00:00+02:00",
                "archive":{"leaf":leaf.display().to_string(), "state":"archived", "catalog":old_line},
            }))
            .unwrap(),
        )
        .unwrap();
        let mut pfx: Value = serde_json::from_str(PFX_LINE).unwrap();
        pfx["tool"] = json!("pito-engine");
        pfx["path"] = json!("pito-engine/alpha/hero/2026-10-03_2020_99e8c315bfbe_site");
        pfx_leaf(
            &root,
            json!({"folder":root.join(pfx["path"].as_str().unwrap()), "state":"archived", "catalog":pfx}),
        );
        assert_eq!(
            reindex_at(&root, &w.join("runtime/catalog.lock")).unwrap(),
            2
        );
        assert_eq!(
            parsed(&fs::read_to_string(masters(&root).join("catalog.jsonl")).unwrap()),
            parsed(&format!("{PFX_LINE}\n{line}\n"))
        );
    }

    #[test]
    fn skip_order_includes_missing_master() {
        let app = json!({"rev":"v1", "tag":"v1", "pinned":true});
        let run = Run {
            no_archive: false,
            no_encode: false,
            product: "alpha",
            has_master: false,
        };
        assert_eq!(
            skip_with(&run, &app, false, Some(Path::new("/srv/archive")), false),
            Some("frames-only")
        );
        assert_eq!(
            skip_with(&run, &app, false, Some(Path::new("/w/tmp/archive")), false),
            Some("test")
        );
        assert!(eligible(&app).is_ok());
    }

    #[test]
    fn skipped_render_records_its_leaf_without_touching_archive() {
        let w = work("skip");
        let (local, root, base, mut manifest) = fixture(&w, MADE);
        finish_at(
            &root,
            &w.join("runtime/catalog.lock"),
            &w.join("queue"),
            &local,
            &mut manifest,
            Some("test"),
        )
        .unwrap();
        assert_eq!(manifest["archive"]["state"], "skipped");
        assert_eq!(manifest["archive"]["reason"], "test");
        assert_eq!(manifest["archive"]["leaf"], base.display().to_string());
        assert!(!masters(&root).join(&base).exists());
        assert!(!masters(&root).join("catalog.jsonl").exists());
        assert!(!w.join("queue").exists());
    }

    #[test]
    fn a_transient_read_error_leaves_the_render_queued_and_a_retry_archives_it() {
        let w = work("transient");
        let (local, root, base, mut manifest) = fixture(&w, MADE);
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        let source = local.parent().unwrap();
        let leaf = archive(source, &root, &base, &lock_file, &mut manifest.clone()).unwrap();
        let stored = leaf.join("manifest.json");
        let aside = w.join("manifest.aside");
        fs::rename(&stored, &aside).unwrap();
        fs::create_dir(&stored).unwrap();
        assert!(matches!(
            archive(source, &root, &base, &lock_file, &mut manifest.clone()),
            Outcome::Retry(_)
        ));
        finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        assert_eq!(manifest["archive"]["state"], "queued");
        assert_eq!(read(&local)["archive"]["state"], "queued");
        assert_eq!(entries(&queue_root), 1);
        fs::remove_dir(&stored).unwrap();
        fs::rename(&aside, &stored).unwrap();
        retry_at(&queue_root, &lock_file);
        let written = read(&local);
        assert_eq!(written["archive"]["state"], "archived");
        assert_eq!(written["archive"]["leaf"], base.display().to_string());
        assert_eq!(entries(&queue_root), 0);
        assert_eq!(fs::read_dir(leaf.parent().unwrap()).unwrap().count(), 1);
        assert_eq!(
            fs::read_to_string(masters(&root).join("catalog.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }

    #[test]
    fn a_fresh_render_and_a_retried_one_take_one_path() {
        let w = work("one-path");
        let (local, root, base, mut fresh) = fixture(&w, MADE);
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        SETTLED.take();
        finish_at(&root, &lock_file, &queue_root, &local, &mut fresh, None).unwrap();
        let settled = SETTLED.take();
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].parent(), Some(queue_root.as_path()));
        assert_eq!(fresh["archive"]["state"], "archived");

        let offline = w.join("offline");
        let (later, later_base, mut retried) = render(&w, "b", "alpha", "v1.2.4", MADE, b"later!");
        finish_at(
            &offline,
            &lock_file,
            &queue_root,
            &later,
            &mut retried,
            None,
        )
        .unwrap();
        assert!(SETTLED.take().is_empty());
        fs::rename(&root, &offline).unwrap();
        retry_at(&queue_root, &lock_file);
        let settled = SETTLED.take();
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].parent(), Some(queue_root.as_path()));
        for (local, base) in [(&local, &base), (&later, &later_base)] {
            let written = read(local);
            assert_eq!(written["archive"]["state"], "archived");
            assert_eq!(
                read(&masters(&offline).join(base).join("manifest.json")),
                written
            );
        }
        assert_eq!(entries(&queue_root), 0);
    }

    #[test]
    fn a_master_is_hashed_once_while_it_is_copied() {
        let w = work("one-pass");
        let root = w.join("archive");
        fs::create_dir_all(&root).unwrap();
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        let bytes = vec![7u8; 3 << 20];
        let (local, base, mut manifest) = render(&w, "a", "alpha", "v1.2.3", MADE, &bytes);
        HASHED.set(0);
        finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        assert_eq!(manifest["archive"]["state"], "archived");
        assert_eq!(HASHED.get(), bytes.len() as u64);
        assert_eq!(
            fs::read(masters(&root).join(&base).join("shot.master.png")).unwrap(),
            bytes
        );

        let offline = w.join("offline");
        let (later, _, mut queued) = render(&w, "b", "alpha", "v1.2.4", MADE, &bytes);
        HASHED.set(0);
        finish_at(&offline, &lock_file, &queue_root, &later, &mut queued, None).unwrap();
        assert_eq!(HASHED.get(), 0);
        fs::rename(&root, &offline).unwrap();
        retry_at(&queue_root, &lock_file);
        assert_eq!(HASHED.get(), bytes.len() as u64);
        assert_eq!(read(&later)["archive"]["state"], "archived");
    }

    #[test]
    fn the_startup_retry_checks_each_root_once() {
        let w = work("one-check");
        let root = w.join("offline");
        let lock_file = w.join("runtime/catalog.lock");
        let queue_root = w.join("queue");
        for (dir, made) in [("a", MADE), ("b", "2026-10-02T09:20:00+02:00")] {
            let (local, _, mut manifest) = render(&w, dir, "alpha", "v1.2.3", made, b"master");
            finish_at(&root, &lock_file, &queue_root, &local, &mut manifest, None).unwrap();
        }
        assert_eq!(entries(&queue_root), 2);
        let mut checks = Vec::new();
        retry_with(&queue_root, &lock_file, |r| {
            checks.push(r.to_path_buf());
            false
        });
        assert_eq!(checks, [root]);
        assert_eq!(entries(&queue_root), 2);
    }

    #[test]
    fn an_unwritable_catalog_keeps_the_old_one_and_leaves_no_partial() {
        let w = work("reindex-unwritable");
        let (local, root, base, mut manifest) = fixture(&w, MADE);
        let lock_file = w.join("runtime/catalog.lock");
        archive(
            local.parent().unwrap(),
            &root,
            &base,
            &lock_file,
            &mut manifest,
        )
        .unwrap();
        let catalog = masters(&root).join("catalog.jsonl");
        let old = fs::read(&catalog).unwrap();
        fs::remove_file(&catalog).unwrap();
        fs::create_dir(&catalog).unwrap();
        fs::write(catalog.join("catalog.jsonl"), &old).unwrap();
        assert!(reindex_at(&root, &lock_file).is_err());
        assert_eq!(fs::read(catalog.join("catalog.jsonl")).unwrap(), old);
        assert!(!masters(&root).join("catalog.jsonl.partial").exists());
    }

    #[test]
    fn only_a_relative_path_of_safe_names_stays_inside() {
        assert!(inside(Path::new(
            "prec/alpha/shot/2026-10-02_091407_v1.2.3_site"
        )));
        assert!(inside(Path::new("shot.master.png")));
        for path in [
            "",
            "..",
            "../alpha",
            "prec/../../alpha",
            "prec/./alpha",
            "/srv/archive/alpha",
            "prec/a b",
        ] {
            assert!(!inside(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn a_queued_leaf_or_file_outside_the_archive_is_refused_and_kept() {
        let w = work("escape");
        let (local, root, base, manifest) = fixture(&w, MADE);
        let queue_root = w.join("queue");
        let lock_file = w.join("runtime/catalog.lock");
        let entry = queue(
            &queued(&local, &root, &base, &manifest),
            local.parent().unwrap(),
            &queue_root,
        )
        .unwrap();
        let record = entry.join("source.json");
        let good = read(&record);
        for (key, value) in [
            ("leaf", json!("../../outside/alpha")),
            (
                "leaf",
                json!(w.join("elsewhere/alpha").display().to_string()),
            ),
            (
                "files",
                json!([{"name":"../../../../../../escaped.png","bytes":6,"sha256":"00"}]),
            ),
            (
                "files",
                json!([{"name":w.join("abs.png").display().to_string(),"bytes":6,"sha256":"00"}]),
            ),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            fs::write(&record, serde_json::to_vec(&bad).unwrap()).unwrap();
            let outcome = retry_entry(&entry, &lock_file, &mut |_: &Path| true);
            assert!(matches!(outcome, Outcome::Refused(_)), "{outcome:?}");
            assert!(record.is_file());
        }
        assert!(!w.join("outside").exists());
        assert!(!w.join("elsewhere").exists());
        assert!(!w.join("abs.png").exists());
        assert!(!masters(&root).exists());
        assert_eq!(read(&local)["archive"]["state"], "queued");
    }
}
