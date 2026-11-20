use std::fs;
use std::path::{Path, PathBuf};

pub const FILES: [(&str, &str); 6] = [
    ("SKILL.md", include_str!("../skills/prec/SKILL.md")),
    ("privacy.md", include_str!("../skills/prec/privacy.md")),
    ("recast.md", include_str!("../skills/prec/recast.md")),
    ("recipe.md", include_str!("../skills/prec/recipe.md")),
    ("tapes.md", include_str!("../skills/prec/tapes.md")),
    ("themes.md", include_str!("../skills/prec/themes.md")),
];

const HOMES: [(&str, &str, Option<&str>); 4] = [
    ("Claude skill", ".claude/skills/prec", None),
    ("Agents skill", ".agents/skills/prec", None),
    ("Codex skill", ".codex/skills/prec", Some(".codex")),
    ("Pi skill", ".pi/agent/skills/prec", Some(".pi")),
];

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, PartialEq, Eq)]
pub enum State {
    Matched,
    Stale,
    Missing,
}

pub struct Row {
    label: &'static str,
    state: State,
}

fn home() -> Result<PathBuf, String> {
    ["HOME", "USERPROFILE"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "no home folder: set HOME".to_string())
}

fn wanted(home: &Path) -> Vec<(&'static str, PathBuf)> {
    HOMES
        .iter()
        .filter(|(_, _, tool)| tool.is_none_or(|t| home.join(t).is_dir()))
        .map(|(label, rel, _)| (*label, home.join(rel)))
        .collect()
}

fn matched(dir: &Path) -> bool {
    if dir.is_symlink() {
        return false;
    }
    let mut have: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    have.sort();
    let mut want: Vec<&str> = FILES.iter().map(|(name, _)| *name).collect();
    want.sort();
    have == want
        && FILES
            .iter()
            .all(|(name, text)| fs::read_to_string(dir.join(name)).is_ok_and(|have| have == *text))
}

fn state(dir: &Path) -> State {
    if matched(dir) {
        State::Matched
    } else if dir.is_symlink() || dir.exists() {
        State::Stale
    } else {
        State::Missing
    }
}

fn write(dir: &Path) -> Result<(), String> {
    if dir.is_symlink() {
        fs::remove_file(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (name, text) in FILES {
        fs::write(dir.join(name), text)
            .map_err(|e| format!("{}: {e}", dir.join(name).display()))?;
    }
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if FILES.iter().any(|(kept, _)| *kept == name) {
            continue;
        }
        let path = entry.path();
        let gone = if path.is_dir() && !path.is_symlink() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        gone.map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

pub fn rows_in(home: &Path, install: bool) -> Result<Vec<Row>, String> {
    let dirs = wanted(home);
    if install {
        for (_, dir) in &dirs {
            write(dir)?;
        }
    }
    Ok(dirs
        .iter()
        .map(|(label, dir)| Row {
            label,
            state: state(dir),
        })
        .collect())
}

pub fn lines(rows: &[Row]) -> String {
    let mut out = String::new();
    for row in rows {
        let label = row.label;
        match row.state {
            State::Matched => {
                out.push_str(&format!("{label}: installed, matches prec {VERSION}\n"))
            }
            State::Stale => out.push_str(&format!(
                "{label}: installed, does not match prec {VERSION}; prec skill install refreshes it\n"
            )),
            State::Missing => out.push_str(&format!("{label}: not installed\n")),
        }
    }
    out
}

fn ours(dir: &Path) -> bool {
    dir.is_symlink()
        || fs::read_to_string(dir.join("SKILL.md"))
            .is_ok_and(|t| t.starts_with("---\nname: prec\n"))
}

pub fn remove_in(home: &Path) -> Result<String, String> {
    let mut out = String::new();
    for (label, rel, _) in HOMES {
        let dir = home.join(rel);
        if !dir.is_symlink() && !dir.exists() {
            continue;
        }
        if !ours(&dir) {
            out.push_str(&format!(
                "left {} alone: it is not the prec skill\n",
                dir.display()
            ));
            continue;
        }
        let gone = if dir.is_symlink() {
            fs::remove_file(&dir)
        } else {
            fs::remove_dir_all(&dir)
        };
        gone.map_err(|e| format!("{}: {e}", dir.display()))?;
        out.push_str(&format!("{label}: removed\n"));
    }
    Ok(out)
}

pub fn remove() -> Result<String, String> {
    remove_in(&home()?)
}

pub fn run(install: bool) -> Result<String, String> {
    Ok(lines(&rows_in(&home()?, install)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn home(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("prec-agent-skill-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_shipped_files_are_the_whole_skill_folder() {
        let mut on_disk: Vec<String> =
            fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/skills/prec"))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
        on_disk.sort();
        let mut shipped: Vec<String> = FILES.iter().map(|(n, _)| n.to_string()).collect();
        shipped.sort();
        assert_eq!(on_disk, shipped);
        assert!(FILES[0].1.starts_with("---\nname: prec\n"));
    }

    #[test]
    fn status_says_not_installed_then_install_writes_real_files_that_match() {
        let dir = home("install");
        let rows = rows_in(&dir, false).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(lines(&rows).contains("Claude skill: not installed\n"));
        assert!(!dir.join(".claude").exists());
        let rows = rows_in(&dir, true).unwrap();
        assert!(rows.iter().all(|r| r.state == State::Matched));
        for rel in [".claude/skills/prec", ".agents/skills/prec"] {
            let skill = dir.join(rel);
            assert!(!skill.is_symlink());
            for (name, text) in FILES {
                assert_eq!(fs::read_to_string(skill.join(name)).unwrap(), text);
            }
        }
        assert!(lines(&rows).contains(&format!(
            "Agents skill: installed, matches prec {VERSION}\n"
        )));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn codex_and_pi_get_the_skill_only_when_their_folders_exist() {
        let dir = home("tools");
        fs::create_dir_all(dir.join(".codex")).unwrap();
        let rows = rows_in(&dir, true).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(dir.join(".codex/skills/prec/SKILL.md").is_file());
        assert!(!dir.join(".pi").exists());
        fs::create_dir_all(dir.join(".pi")).unwrap();
        let rows = rows_in(&dir, true).unwrap();
        assert_eq!(rows.len(), 4);
        assert!(dir.join(".pi/agent/skills/prec/SKILL.md").is_file());
        assert!(lines(&rows).contains("Pi skill: installed"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_changed_or_extra_file_reads_stale_and_install_refreshes_it() {
        let dir = home("stale");
        rows_in(&dir, true).unwrap();
        let skill = dir.join(".claude/skills/prec");
        fs::write(skill.join("SKILL.md"), "old").unwrap();
        fs::write(skill.join("extra.md"), "x").unwrap();
        let rows = rows_in(&dir, false).unwrap();
        assert_eq!(rows[0].state, State::Stale);
        assert_eq!(rows[1].state, State::Matched);
        assert!(lines(&rows).contains("does not match prec"));
        let rows = rows_in(&dir, true).unwrap();
        assert!(rows.iter().all(|r| r.state == State::Matched));
        assert!(!skill.join("extra.md").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn an_old_link_reads_stale_and_install_replaces_it_with_files() {
        let dir = home("link");
        let old = dir.join(".local/opt/prec/skills/prec");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("SKILL.md"), "old copy").unwrap();
        fs::create_dir_all(dir.join(".claude/skills")).unwrap();
        symlink(&old, dir.join(".claude/skills/prec")).unwrap();
        let rows = rows_in(&dir, false).unwrap();
        assert_eq!(rows[0].state, State::Stale);
        rows_in(&dir, true).unwrap();
        let skill = dir.join(".claude/skills/prec");
        assert!(!skill.is_symlink() && skill.is_dir());
        assert_eq!(
            fs::read_to_string(skill.join("SKILL.md")).unwrap(),
            FILES[0].1
        );
        assert_eq!(
            fs::read_to_string(old.join("SKILL.md")).unwrap(),
            "old copy"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn uninstall_removes_prec_folders_and_links_and_leaves_a_stranger() {
        let dir = home("remove");
        rows_in(&dir, true).unwrap();
        let other = dir.join(".agents/skills/prec");
        fs::write(other.join("SKILL.md"), "---\nname: other\n---\n").unwrap();
        let said = remove_in(&dir).unwrap();
        assert!(said.contains("Claude skill: removed\n"));
        assert!(said.contains("left "));
        assert!(!dir.join(".claude/skills/prec").exists());
        assert!(other.exists());
        let old = dir.join("old");
        fs::create_dir_all(&old).unwrap();
        fs::remove_dir_all(&other).unwrap();
        symlink(&old, &other).unwrap();
        remove_in(&dir).unwrap();
        assert!(!other.is_symlink());
        assert!(old.exists());
        assert_eq!(remove_in(&dir).unwrap(), "");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_dangling_link_is_replaced_too() {
        let dir = home("dangling");
        fs::create_dir_all(dir.join(".agents/skills")).unwrap();
        symlink(dir.join("gone"), dir.join(".agents/skills/prec")).unwrap();
        assert_eq!(rows_in(&dir, false).unwrap()[1].state, State::Stale);
        let rows = rows_in(&dir, true).unwrap();
        assert!(rows.iter().all(|r| r.state == State::Matched));
        let _ = fs::remove_dir_all(dir);
    }
}
