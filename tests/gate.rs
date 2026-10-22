#![cfg(unix)]

mod common;

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const USAGE: &str = "usage: bin/gate [--fast|--render]\n";

const CARGO: &str = r#"#!/usr/bin/env bash
all="$*"
printf 'cargo %s\n' "$all" >> "$STUB_LOG"
printf '%s\n' "${CARGO_NET_GIT_FETCH_WITH_CLI-unset}" >> "$STUB_LOG.env"
if [ "$all" = "nextest --version" ] && [ -n "${STUB_NO_NEXTEST-}" ]; then
  exit 101
fi
if [ -n "${STUB_FAIL-}" ]; then
  case "$all" in
    "$STUB_FAIL"*) exit 7 ;;
  esac
fi
exit 0
"#;

const SHELLCHECK: &str = r#"#!/usr/bin/env bash
printf 'shellcheck %s\n' "$*" >> "$STUB_LOG"
if [ "${STUB_FAIL-}" = shellcheck ]; then
  exit 7
fi
exit 0
"#;

const FMT: &str = "cargo fmt --check";
const CLIPPY: &str = "cargo clippy --locked --all-targets -- -D warnings";
const PROBE: &str = "cargo nextest --version";
const NEXTEST: &str = "cargo nextest run --workspace --bins --no-tests=pass";
const TEST: &str = "cargo test --locked --bins";
const SHELLCHECK_RUN: &str = "shellcheck bin/gate bin/docs-bundle bin/marks";

fn found(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap();
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("{name} not on PATH"))
}

fn stub(dir: &Path, name: &str, body: &str) {
    let file = dir.join(name);
    std::fs::write(&file, body).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn lines(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

struct Run {
    out: Output,
    log: Vec<String>,
    env: Vec<String>,
}

impl Run {
    fn code(&self) -> Option<i32> {
        self.out.status.code()
    }

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.out.stdout).into_owned()
    }

    fn last_line(&self) -> Option<String> {
        self.stdout().lines().last().map(str::to_owned)
    }
}

fn gate(name: &str, args: &[&str], shellcheck: bool, env: &[(&str, &str)]) -> Run {
    let dir = common::scratch("gate", name);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let bash = found("bash");
    symlink(&bash, bin.join("bash")).unwrap();
    symlink(found("dirname"), bin.join("dirname")).unwrap();
    stub(&bin, "cargo", CARGO);
    if shellcheck {
        stub(&bin, "shellcheck", SHELLCHECK);
    }
    let log = dir.join("calls.log");
    let script = common::tree().join("bin/gate");
    let mut c = Command::new(&bash);
    c.arg(script)
        .args(args)
        .env_clear()
        .env("PATH", &bin)
        .env("STUB_LOG", &log);
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().unwrap();
    let env = lines(&dir.join("calls.log.env"));
    let log = lines(&log);
    Run { out, log, env }
}

fn refused(name: &str, args: &[&str]) {
    let run = gate(name, args, true, &[]);
    assert_eq!(run.code(), Some(2), "{args:?}");
    assert_eq!(String::from_utf8_lossy(&run.out.stderr), USAGE, "{args:?}");
    assert!(run.out.stdout.is_empty(), "{args:?}");
    assert!(run.log.is_empty(), "{args:?} ran {:?}", run.log);
}

#[test]
fn an_unknown_flag_is_refused_with_the_usage_line() {
    refused("unknown-bogus", &["--bogus"]);
    refused("unknown-f", &["-f"]);
    refused("unknown-fast", &["fast"]);
    refused("unknown-upper", &["--FAST"]);
}

#[test]
fn a_known_flag_with_anything_after_it_is_refused() {
    refused("extra-fast", &["--fast", "extra"]);
    refused("extra-render", &["--render", "--fast"]);
    refused("extra-twice", &["--fast", "--fast"]);
}

#[test]
fn fast_runs_the_steps_in_order_with_nextest_and_ends_green() {
    let run = gate("fast-green", &["--fast"], true, &[]);
    assert_eq!(run.code(), Some(0), "{}", run.stdout());
    assert_eq!(
        run.log,
        [SHELLCHECK_RUN, FMT, CLIPPY, PROBE, NEXTEST],
        "{:?}",
        run.log
    );
    assert_eq!(run.last_line().as_deref(), Some("fast gate: green"));
}

#[test]
fn fast_falls_back_to_cargo_test_without_nextest() {
    let run = gate(
        "fast-no-nextest",
        &["--fast"],
        true,
        &[("STUB_NO_NEXTEST", "1")],
    );
    assert_eq!(run.code(), Some(0), "{}", run.stdout());
    assert_eq!(run.log, [SHELLCHECK_RUN, FMT, CLIPPY, PROBE, TEST]);
    assert_eq!(run.last_line().as_deref(), Some("fast gate: green"));
}

#[test]
fn fast_skips_shellcheck_when_it_is_not_installed() {
    let run = gate("fast-no-shellcheck", &["--fast"], false, &[]);
    assert_eq!(run.code(), Some(0), "{}", run.stdout());
    assert_eq!(run.log, [FMT, CLIPPY, PROBE, NEXTEST]);
    assert_eq!(run.last_line().as_deref(), Some("fast gate: green"));
}

#[test]
fn fast_exports_git_fetch_with_cli_to_every_cargo_step() {
    let run = gate("fast-env", &["--fast"], false, &[]);
    assert_eq!(run.code(), Some(0));
    assert_eq!(run.env, ["true"; 4]);
}

#[test]
fn a_bare_gate_leaves_git_fetch_with_cli_alone() {
    let run = gate("bare-env", &[], false, &[]);
    assert_eq!(run.code(), Some(0));
    assert_eq!(run.env, ["unset"; 3]);
}

#[test]
fn fast_stops_at_the_first_failing_step_with_its_status() {
    for (name, fail, ran) in [
        ("fail-shellcheck", "shellcheck", &[SHELLCHECK_RUN][..]),
        ("fail-fmt", "fmt", &[SHELLCHECK_RUN, FMT][..]),
        ("fail-clippy", "clippy", &[SHELLCHECK_RUN, FMT, CLIPPY][..]),
        (
            "fail-nextest",
            "nextest run",
            &[SHELLCHECK_RUN, FMT, CLIPPY, PROBE, NEXTEST][..],
        ),
    ] {
        let run = gate(name, &["--fast"], true, &[("STUB_FAIL", fail)]);
        assert_eq!(run.code(), Some(7), "{fail}");
        assert_eq!(run.log, ran, "{fail}");
        assert!(!run.stdout().contains("fast gate: green"), "{fail}");
    }
}

#[test]
fn fast_fallback_failure_keeps_its_status() {
    let run = gate(
        "fail-fallback",
        &["--fast"],
        true,
        &[("STUB_NO_NEXTEST", "1"), ("STUB_FAIL", "test")],
    );
    assert_eq!(run.code(), Some(7));
    assert_eq!(run.log, [SHELLCHECK_RUN, FMT, CLIPPY, PROBE, TEST]);
    assert!(!run.stdout().contains("fast gate: green"));
}

#[test]
fn a_bare_gate_keeps_the_cargo_test_unit_path() {
    let run = gate("bare", &[], true, &[]);
    assert_eq!(run.code(), Some(0));
    assert_eq!(run.log, [SHELLCHECK_RUN, FMT, CLIPPY, TEST]);
    assert!(!run.stdout().contains("fast gate: green"));
}

#[test]
fn a_bare_gate_stops_at_a_failing_step() {
    let run = gate("bare-fail", &[], true, &[("STUB_FAIL", "clippy")]);
    assert_eq!(run.code(), Some(7));
    assert_eq!(run.log, [SHELLCHECK_RUN, FMT, CLIPPY]);
}

#[test]
fn render_runs_only_the_integration_tests() {
    let run = gate("render", &["--render"], true, &[]);
    assert_eq!(run.code(), Some(0));
    assert_eq!(run.log, ["cargo test --locked --tests"]);
    assert!(!run.stdout().contains("fast gate: green"));
}
