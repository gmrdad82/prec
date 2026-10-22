use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const PRELUDE: &str = r#"set -e
stash=$1 parent=$2 name=$3 home=$4 host=$5 cwd=$6 uid=$7 gid=$8
shift 8
mkdir -p "$stash"
mount --rbind "$parent" "$stash"
cd "$stash"
mount -t tmpfs tmpfs "$parent"
for c in * .[!.]* ..?*; do
  if [ -L "$c" ]; then
    ln -s "$(readlink "$c")" "$parent/$c"
  elif [ -d "$c" ]; then
    mkdir "$parent/$c"
    mount --rbind "./$c" "$parent/$c"
  elif [ -e "$c" ]; then
    : > "$parent/$c"
    mount --bind "./$c" "$parent/$c"
  fi
done
mkdir -p "$parent/$name"
mount --bind "$home" "$parent/$name"
hostname "$host"
cd "$cwd"
exec unshare --user --map-user="$uid" --map-group="$gid" "$@"
"#;

pub struct Ns {
    pub home: PathBuf,
    pub hostname: String,
    pub cwd: PathBuf,
    parent: PathBuf,
    name: String,
    scratch_home: PathBuf,
    stash: PathBuf,
    uid: u32,
    gid: u32,
}

impl Ns {
    pub fn new(
        home: &str,
        hostname: &str,
        scratch_home: &Path,
        stash: &Path,
    ) -> Result<Ns, String> {
        if !cfg!(target_os = "linux") {
            return Err("home needs Linux user namespaces".into());
        }
        let home = PathBuf::from(home);
        if home.symlink_metadata().is_ok() {
            return Err(format!(
                "home {}: that path exists on this machine; name one that doesn't",
                home.display()
            ));
        }
        let name = home
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| format!("home {}: no last part", home.display()))?;
        let parent = home
            .parent()
            .and_then(|p| std::fs::canonicalize(p).ok())
            .filter(|p| p.is_dir())
            .ok_or_else(|| {
                format!(
                    "home {}: its parent is not a folder on this machine",
                    home.display()
                )
            })?;
        let meta = std::fs::metadata(scratch_home)
            .map_err(|e| format!("{}: {e}", scratch_home.display()))?;
        Ok(Ns {
            cwd: home.clone(),
            home,
            hostname: hostname.to_string(),
            parent,
            name,
            scratch_home: scratch_home.to_path_buf(),
            stash: stash.to_path_buf(),
            uid: meta.uid(),
            gid: meta.gid(),
        })
    }

    pub fn inside(&self, path: &Path) -> PathBuf {
        match path.strip_prefix(&self.scratch_home) {
            Ok(rel) => self.home.join(rel),
            Err(_) => path.to_path_buf(),
        }
    }

    pub fn outside(&self, path: &Path) -> PathBuf {
        match path.strip_prefix(&self.home) {
            Ok(rel) => self.scratch_home.join(rel),
            Err(_) => path.to_path_buf(),
        }
    }

    pub fn argv(&self, tail: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = [
            "unshare",
            "--user",
            "--map-root-user",
            "--mount",
            "--uts",
            "--propagation",
            "private",
            "sh",
            "-c",
            PRELUDE,
            "sh",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        for p in [&self.stash, &self.parent] {
            out.push(p.display().to_string());
        }
        out.push(self.name.clone());
        out.push(self.scratch_home.display().to_string());
        out.push(self.hostname.clone());
        out.push(self.cwd.display().to_string());
        out.push(self.uid.to_string());
        out.push(self.gid.to_string());
        out.extend(tail.iter().map(|s| s.to_string()));
        out
    }

    pub fn probe(&self, path: &str, cwd: &Path) -> Result<(), String> {
        let argv = self.argv(&["true"]);
        let out = crate::paths::output(
            Command::new(&argv[0])
                .args(&argv[1..])
                .current_dir(cwd)
                .env_clear()
                .env("PATH", path),
        )
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "home needs unshare from util-linux on PATH".to_string()
            }
            _ => format!("home: unshare: {e}"),
        })?;
        if out.status.success() {
            return Ok(());
        }
        let said = String::from_utf8_lossy(&out.stderr);
        let why = said
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("it exited without a word");
        Err(format!(
            "home: the user and mount namespace can't be made here: {}",
            why.trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tmp/test-home")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scratch-home")).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn a_home_is_refused_off_linux() {
        let dir = test_dir("off-linux");
        let e = Ns::new(
            &dir.join("alpha").display().to_string(),
            "prec",
            &dir.join("scratch-home"),
            &dir.join("stash"),
        )
        .err()
        .unwrap();
        assert_eq!(e, "home needs Linux user namespaces");
    }

    #[cfg(target_os = "linux")]
    fn ns(dir: &Path, home: &str) -> Result<Ns, String> {
        Ns::new(
            home,
            "beta-box",
            &dir.join("scratch-home"),
            &dir.join("stash"),
        )
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_home_that_does_not_exist_under_a_real_folder_is_taken() {
        let dir = test_dir("taken");
        let home = dir.join("alpha");
        let n = ns(&dir, &home.display().to_string()).unwrap();
        assert_eq!(n.home, home);
        assert_eq!(n.cwd, home);
        assert_eq!(n.hostname, "beta-box");
        assert_eq!(n.parent, dir);
        assert_eq!(n.name, "alpha");
        assert_eq!(n.scratch_home, dir.join("scratch-home"));
        assert_eq!(n.stash, dir.join("stash"));
        let meta = std::fs::metadata(dir.join("scratch-home")).unwrap();
        assert_eq!((n.uid, n.gid), (meta.uid(), meta.gid()));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_home_that_exists_or_has_no_real_parent_or_last_part_is_refused() {
        let dir = test_dir("refused");
        std::fs::write(dir.join("file"), "x").unwrap();
        let shown = |p: &Path| p.display().to_string();
        let exists = shown(&dir.join("scratch-home"));
        assert_eq!(
            ns(&dir, &exists).err().unwrap(),
            format!("home {exists}: that path exists on this machine; name one that doesn't")
        );
        let dots = shown(&dir.join("missing/.."));
        assert_eq!(
            ns(&dir, &dots).err().unwrap(),
            format!("home {dots}: no last part")
        );
        for orphan in [
            shown(&dir.join("missing/alpha")),
            shown(&dir.join("file/alpha")),
            "prec-test-relative-home".to_string(),
        ] {
            assert_eq!(
                ns(&dir, &orphan).err().unwrap(),
                format!("home {orphan}: its parent is not a folder on this machine")
            );
        }
        let e = Ns::new(
            &shown(&dir.join("alpha")),
            "prec",
            &dir.join("no-scratch-home"),
            &dir.join("stash"),
        )
        .err()
        .unwrap();
        assert!(
            e.starts_with(&format!("{}: ", dir.join("no-scratch-home").display())),
            "{e}"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_path_maps_between_the_scratch_home_and_the_session_home() {
        let dir = test_dir("maps");
        let n = ns(&dir, &dir.join("alpha").display().to_string()).unwrap();
        let outside = dir.join("scratch-home/work/x");
        let inside = dir.join("alpha/work/x");
        assert_eq!(n.inside(&outside), inside);
        assert_eq!(n.outside(&inside), outside);
        assert_eq!(n.outside(&n.inside(&outside)), outside);
        assert_eq!(n.inside(&dir.join("scratch-home")), dir.join("alpha"));
        for other in [Path::new("/elsewhere/x"), &dir.join("scratch-homes/x")] {
            assert_eq!(n.inside(other), other);
            assert_eq!(n.outside(other), other);
        }
        assert_eq!(n.outside(&dir.join("alphabet")), dir.join("alphabet"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn the_session_runs_behind_the_prelude_with_its_eight_values_then_the_command() {
        let dir = test_dir("argv");
        let mut n = ns(&dir, &dir.join("alpha").display().to_string()).unwrap();
        n.cwd = dir.join("alpha/work");
        let argv = n.argv(&["bash", "-i"]);
        assert_eq!(
            argv[..11],
            [
                "unshare",
                "--user",
                "--map-root-user",
                "--mount",
                "--uts",
                "--propagation",
                "private",
                "sh",
                "-c",
                PRELUDE,
                "sh",
            ]
        );
        let meant = |name: &str| -> String {
            match name {
                "stash" => dir.join("stash").display().to_string(),
                "parent" => dir.display().to_string(),
                "name" => "alpha".to_string(),
                "home" => dir.join("scratch-home").display().to_string(),
                "host" => "beta-box".to_string(),
                "cwd" => dir.join("alpha/work").display().to_string(),
                "uid" => n.uid.to_string(),
                "gid" => n.gid.to_string(),
                other => panic!("the prelude names {other}, which no test value means"),
            }
        };
        let names = PRELUDE.lines().nth(1).unwrap();
        let mut seen = Vec::new();
        for field in names.split_whitespace() {
            let (name, at) = field.split_once("=$").unwrap();
            let at: usize = at.parse().unwrap();
            assert_eq!(argv[10 + at], meant(name), "${at} is {name}");
            seen.push(at);
        }
        seen.sort_unstable();
        assert_eq!(seen, (1..=8).collect::<Vec<_>>());
        assert!(PRELUDE.contains("\nshift 8\n"));
        assert_eq!(argv[19..], ["bash", "-i"]);
        assert_eq!(argv.len(), 21);
        assert!(PRELUDE.ends_with("--map-user=\"$uid\" --map-group=\"$gid\" \"$@\"\n"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_dangling_link_where_the_home_would_be_is_refused_as_existing() {
        let dir = test_dir("dangling");
        let home = dir.join("alpha");
        std::os::unix::fs::symlink(dir.join("nowhere"), &home).unwrap();
        assert!(!home.exists());
        let shown = home.display().to_string();
        assert_eq!(
            ns(&dir, &shown).err().unwrap(),
            format!("home {shown}: that path exists on this machine; name one that doesn't")
        );
    }

    #[test]
    fn the_prelude_is_valid_sh() {
        let out = crate::paths::output(Command::new("sh").args(["-n", "-c", PRELUDE])).unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[cfg(target_os = "linux")]
    fn stub(dir: &Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let unshare = bin.join("unshare");
        std::fs::write(&unshare, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&unshare, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin.display().to_string()
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_probe_without_unshare_on_path_says_what_to_install() {
        let dir = test_dir("probe-missing");
        std::fs::create_dir_all(dir.join("empty")).unwrap();
        let n = ns(&dir, &dir.join("alpha").display().to_string()).unwrap();
        assert_eq!(
            n.probe(&dir.join("empty").display().to_string(), &dir),
            Err("home needs unshare from util-linux on PATH".to_string())
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_probe_runs_true_through_the_prelude_with_only_path_and_tells_why_it_failed() {
        let dir = test_dir("probe-stub");
        let n = ns(&dir, &dir.join("alpha").display().to_string()).unwrap();
        let args = dir.join("args");
        let env = dir.join("env");
        let path = stub(
            &dir,
            &format!(
                "printf '%s\\0' \"$@\" > '{}'\n/usr/bin/env > '{}'\npwd >> '{}'\nexit 0",
                args.display(),
                env.display(),
                env.display()
            ),
        );
        assert_eq!(n.probe(&path, &dir.join("scratch-home")), Ok(()));
        let got: Vec<String> = std::fs::read_to_string(&args)
            .unwrap()
            .split_terminator('\0')
            .map(str::to_string)
            .collect();
        assert_eq!(got, n.argv(&["true"])[1..]);
        let env = std::fs::read_to_string(&env).unwrap();
        let names: Vec<&str> = env
            .lines()
            .filter_map(|l| l.split_once('=').map(|(k, _)| k))
            .filter(|k| !matches!(*k, "PWD" | "SHLVL" | "_" | "OLDPWD"))
            .collect();
        assert_eq!(names, ["PATH"], "{env}");
        assert!(env.contains(&format!("PATH={path}\n")), "{env}");
        assert!(
            env.ends_with(&format!("{}\n", dir.join("scratch-home").display())),
            "{env}"
        );
        let path = stub(
            &dir,
            "echo 'unshare: one' >&2\necho 'unshare: write failed /proc/self/uid_map: Operation not permitted' >&2\necho >&2\nexit 1",
        );
        assert_eq!(
            n.probe(&path, &dir),
            Err("home: the user and mount namespace can't be made here: unshare: write failed /proc/self/uid_map: Operation not permitted".to_string())
        );
        let path = stub(&dir, "exit 1");
        assert_eq!(
            n.probe(&path, &dir),
            Err(
                "home: the user and mount namespace can't be made here: it exited without a word"
                    .to_string()
            )
        );
    }
}
