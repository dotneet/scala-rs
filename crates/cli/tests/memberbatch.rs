//! Batched member, evidence, catch and standard-library result type regressions.
use std::{fs, path::Path, process::Command};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
fn check_cases(cases: &[(&str, bool)]) {
    static NEXT_DIR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "memberbatch-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    for &(name, good) in cases {
        for mode in ["nsc", "jar"] {
            let out = root.join(format!("{name}-{mode}"));
            fs::create_dir(&out).unwrap();
            let mut c = if mode == "nsc" {
                Command::new("/tmp/scala-2.13.16/bin/scalac")
            } else {
                let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                c.arg("compile");
                if mode == "private" {
                    c.arg("--no-scala-library");
                } else {
                    c.args(["--scala-library", JAR]);
                }
                c
            };
            let r = c
                .arg(fixtures.join(format!("{name}.scala")))
                .arg("-d")
                .arg(&out)
                .output()
                .unwrap();
            assert_eq!(
                r.status.success(),
                good,
                "{name}/{mode}: {}",
                String::from_utf8_lossy(&r.stderr)
            );
            if good {
                let cp = if mode == "private" {
                    out.display().to_string()
                } else {
                    format!("{}:{JAR}", out.display())
                };
                let r = Command::new("java")
                    .args(["-Xverify:all", "-cp", &cp, "Main"])
                    .output()
                    .unwrap();
                assert!(
                    r.status.success(),
                    "{name}/{mode}: {}",
                    String::from_utf8_lossy(&r.stderr)
                );
                assert_eq!(
                    r.stdout,
                    fs::read(fixtures.join(format!("expected/{name}.txt"))).unwrap()
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn option_try_signatures_match_scalac() {
    check_cases(&[
        ("opttrybatch", true),
        ("optflatbatch_bad", false),
        ("inferredimplicitbatch_bad", false),
        ("optzipbatch_bad", false),
        ("optzipboundbatch_bad", false),
        ("optcollectbatch_bad", false),
        ("optcollectargbatch_bad", false),
        ("tryflatbatch_bad", false),
        ("trytransformbatch_bad", false),
        ("tryorelsebatch_bad", false),
        ("tryrecoverbatch_bad", false),
        ("tryrecoverwithbatch_bad", false),
        ("trycollectbatch_bad", false),
    ]);
}
#[test]
fn members_and_catch_match_scalac() {
    check_cases(&[
        ("memberbatch", true),
        ("viewgenericbatch", true),
        ("arraymapbatch_bad", false),
        ("arraymapresultbatch_bad", false),
        ("wildmemberbatch_bad", false),
        ("wildlowerbatch_bad", false),
        ("viewcurriedbatch_bad", false),
        ("catchbatch_bad", false),
    ]);
}
