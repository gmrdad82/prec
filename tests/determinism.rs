mod common;

use std::path::Path;
use std::process::{Child, Command, Stdio};

fn allowed_cpus() -> (String, String) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let all = status
        .lines()
        .find_map(|l| l.strip_prefix("Cpus_allowed_list:"))
        .unwrap()
        .trim()
        .to_string();
    let first = all.split([',', '-']).next().unwrap().to_string();
    (first, all)
}

fn start(cpus: &str, args: &[&str], work: &Path) -> Child {
    let mut c = Command::new("taskset");
    c.args(["-c", cpus, env!("CARGO_BIN_EXE_prec")]).args(args);
    common::confine(&mut c, &common::tree(), &work.join("archive"));
    c.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

fn finish(child: Child) -> String {
    let out = child.wait_with_output().unwrap();
    common::expect_ok(&out, "prec");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_cast_replays_to_the_same_bytes_on_one_cpu_or_many() {
    let work = common::scratch("determinism", "work");
    let (one, many) = allowed_cpus();
    let cast = finish(start(
        &many,
        &[
            "record",
            "demo",
            "typing",
            "--app-dir",
            "examples/demo",
            "--out",
            "tmp/determinism/work/cast",
        ],
        &work,
    ));
    let both: Vec<Child> = [("one", &one), ("many", &many)]
        .into_iter()
        .map(|(out, cpus)| {
            start(
                cpus,
                &[
                    "render",
                    &cast,
                    "--size",
                    "640x360",
                    "--fps",
                    "30",
                    "--out",
                    &format!("tmp/determinism/work/{out}"),
                ],
                &work,
            )
        })
        .collect();
    for child in both {
        finish(child);
    }
    for name in [
        "demo-clip-typing-640x360-30fps.master.mp4",
        "demo-clip-typing-640x360-30fps.poster.png",
    ] {
        let read = |out: &str| std::fs::read(work.join(out).join("demo/clip").join(name)).unwrap();
        assert!(
            read("one") == read("many"),
            "{name} differs between CPU {one} and CPUs {many}"
        );
    }
}
