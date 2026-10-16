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
        if home.exists() {
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
