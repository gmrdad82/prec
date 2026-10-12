mod common;

use std::path::Path;

fn refused(cwd: &Path, args: &[&str]) -> String {
    let o = common::run(cwd, &cwd.join("archive"), args);
    assert!(!o.status.success(), "prec {} succeeded", args.join(" "));
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn an_app_renders_from_its_own_tree_and_nowhere_else() {
    let caller = common::caller("source", "own");
    common::demo_app(&caller, "");
    std::fs::rename(caller.join("app/render"), caller.join("render")).unwrap();
    let o = common::run(&caller, &caller.join("archive"), &["shots", "demo"]);
    common::expect_ok(&o, "prec shots demo from the app's own tree");
    let listed = String::from_utf8_lossy(&o.stdout);
    assert!(listed.contains("typing"), "{listed}");

    let other = common::tree().join("examples/demo");
    let err = refused(
        &caller,
        &["shots", "demo", "--app-dir", other.to_str().unwrap()],
    );
    assert!(err.contains("outside this repository"), "{err}");
    let err = refused(
        &caller,
        &[
            "clip",
            "demo",
            "typing",
            "--app-dir",
            other.to_str().unwrap(),
            "--rev",
            "HEAD",
        ],
    );
    assert!(err.contains("outside this repository"), "{err}");
    assert!(!caller.join("tmp/prec/run").exists());

    for retired in [
        &["clip", "demo", "typing", "--git", "x", "--rev", "v1"][..],
        &["clip", "demo", "typing", "--publish"][..],
        &["still", "demo", "palette", "--assets", "x"][..],
    ] {
        let err = refused(&caller, retired);
        assert!(err.contains("unexpected argument"), "{err}");
    }
}
