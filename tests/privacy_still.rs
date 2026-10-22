mod common;

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const VAR: &str = "PREC_STILL_PASSED";

fn files_under(root: &Path, skip: &[&Path]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if skip.contains(&path.as_path()) {
                continue;
            }
            let Ok(kind) = e.file_type() else {
                continue;
            };
            if kind.is_dir() {
                queue.push(path);
            } else if kind.is_file() {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn png_texts(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).unwrap();
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let info = reader.info();
    let mut texts = Vec::new();
    for t in &info.uncompressed_latin1_text {
        texts.push(format!("{}={}", t.keyword, t.text));
    }
    for t in &info.compressed_latin1_text {
        texts.push(format!("{}={}", t.keyword, t.get_text().unwrap()));
    }
    for t in &info.utf8_text {
        texts.push(format!("{}={}", t.keyword, t.get_text().unwrap()));
    }
    texts
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn a_passed_value_reaches_the_still_app_and_no_file_the_render_wrote() {
    let dir = common::caller("privacy_still", "passed-still");
    let secret = "s3cr3t-still-7f1c9e".to_string();
    let app = common::demo_app(&dir, &format!("pass_env = [\"{VAR}\"]\n"));
    std::fs::write(
        app.join("render/terminal/quiet.tape"),
        format!(
            "Type@5ms \"demo show\"\nEnter\nType@5ms 'test -n \"${VAR}\" && echo passed-$((1+1))'\nEnter\nWait+Screen /passed-2/\n"
        ),
    )
    .unwrap();
    let toml = app.join("render/terminal.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    text.push_str("\n[shots.quiet]\nkind = \"still\"\ntape = \"render/terminal/quiet.tape\"\n");
    std::fs::write(&toml, text).unwrap();
    let archive = dir.join("archive");
    let jobs = dir.join("jobs");
    let o = common::prec(
        &dir,
        &archive,
        &[
            "still",
            "demo",
            "quiet",
            "--app-dir",
            app.to_str().unwrap(),
            "--size",
            "320x180",
            "--out",
            "tmp/out",
        ],
    )
    .env("XDG_CACHE_HOME", dir.join("cache"))
    .env_remove("PREC_CACHE")
    .env(VAR, &secret)
    .env_remove("PITO_JOBS")
    .env("PITO_JOBS_DIR", &jobs)
    .output()
    .unwrap();
    common::expect_ok(&o, "prec still");
    let said = String::from_utf8_lossy(&o.stderr).to_string();
    let printed = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(
        !said.contains(&secret),
        "the passed value was printed: {said}"
    );
    assert!(!printed.contains(&secret), "the passed value was printed");

    let manifest_path = dir.join(common::last_path(&o));
    let out = dir.join("tmp/out/demo/still");
    assert_eq!(manifest_path.parent(), Some(out.as_path()));
    let manifest: Value = serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["pass_env"], json!([VAR]));
    let cast_name = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["role"] == "cast")
        .and_then(|f| f["file"].as_str())
        .unwrap()
        .to_string();
    let cast = std::fs::read_to_string(out.join(&cast_name)).unwrap();
    let header: Value = serde_json::from_str(cast.lines().next().unwrap()).unwrap();
    assert_eq!(header["x_terminal_renderer"]["pass_env"], json!([VAR]));
    assert!(
        cast.contains("passed-2"),
        "the app never saw the passed value: {cast}"
    );

    let written = files_under(&dir, &[app.as_path(), dir.join(".git").as_path()]);
    let pngs: Vec<&PathBuf> = written
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    assert!(!pngs.is_empty(), "the still wrote no PNG");
    assert!(written.contains(&manifest_path));
    assert!(written.contains(&out.join(&cast_name)));
    let records: Vec<&PathBuf> = written
        .iter()
        .filter(|p| p.starts_with(&jobs) && p.extension().is_some_and(|e| e == "json"))
        .collect();
    assert!(!records.is_empty(), "the render left no job record");
    for path in &written {
        let bytes = std::fs::read(path).unwrap();
        assert!(
            !contains(&bytes, &secret),
            "the passed value is in {}",
            path.display()
        );
    }
    for png in pngs {
        for t in png_texts(png) {
            assert!(
                !t.contains(&secret),
                "the passed value is in {}'s text {t}",
                png.display()
            );
        }
    }
}
