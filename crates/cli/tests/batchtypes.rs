//! Function Unit arity and the real Either companion retain Scala identities.
#[path = "support/temp_nonce.rs"]
mod temp_nonce;

use std::{fs, path::Path, process::Command};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
fn check_cases(cases: &[(&str, bool)]) {
    let root = std::env::temp_dir().join(format!(
        "batchtypes-{}-{}",
        std::process::id(),
        temp_nonce::unique_stamp(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    ));
    fs::create_dir(&root).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    for &(name, good) in cases {
        for mode in ["nsc", "jar", "private"] {
            if mode == "private" && name.starts_with("either") {
                continue;
            }
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
fn unit_function_arity_matches_scalac() {
    check_cases(&[("unitfn", true), ("unitfn_bad", false)]);
}
#[test]
fn either_companion_matches_scalac() {
    check_cases(&[("eitherobject", true), ("eitherobject_bad", false)]);
}
