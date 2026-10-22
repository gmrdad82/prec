use std::path::Path;
use std::process::Command;

const USAGE: &str = "usage: bin/gate [--fast|--render]\n";

fn refused(args: &[&str]) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/gate");
    let out = Command::new("bash").arg(script).args(args).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{args:?}");
    assert_eq!(String::from_utf8_lossy(&out.stderr), USAGE, "{args:?}");
    assert!(out.stdout.is_empty(), "{args:?}");
}

#[test]
fn an_unknown_flag_is_refused_with_the_usage_line() {
    refused(&["--bogus"]);
    refused(&["-f"]);
    refused(&["fast"]);
    refused(&["--FAST"]);
}

#[test]
fn a_known_flag_with_anything_after_it_is_refused() {
    refused(&["--fast", "extra"]);
    refused(&["--render", "--fast"]);
    refused(&["--fast", "--fast"]);
}
