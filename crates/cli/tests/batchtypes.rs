//! Function Unit arity and the real Either companion retain Scala identities.
use crate::support;

use std::{fs, path::Path, process::Command};
fn check_cases(cases: &[(&str, bool)]) {
    let toolchain = support::toolchain();
    let (Some(scalac), Some(jar), Some(_java)) = (
        toolchain.scalac(),
        toolchain.scala_library(),
        toolchain.java(),
    ) else {
        eprintln!("skip batchtypes differential test: Scala/JVM toolchain unavailable");
        return;
    };
    let root = support::TestDir::new("batchtypes");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    for &(name, good) in cases {
        for mode in ["nsc", "jar", "private"] {
            if mode == "private" && name.starts_with("either") {
                continue;
            }
            let out = root.join(format!("{name}-{mode}"));
            fs::create_dir(&out).unwrap();
            let mut c = if mode == "nsc" {
                Command::new(scalac)
            } else {
                let mut c = Command::new(support::scala_rs());
                c.arg("compile");
                if mode == "private" {
                    c.arg("--no-scala-library");
                } else {
                    c.args(["--scala-library", jar.to_str().unwrap()]);
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
                    format!("{}:{}", out.display(), jar.display())
                };
                let r = Command::new(toolchain.java().unwrap())
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
}

#[test]
fn unit_function_arity_matches_scalac() {
    check_cases(&[("unitfn", true), ("unitfn_bad", false)]);
}
#[test]
fn either_companion_matches_scalac() {
    check_cases(&[("eitherobject", true), ("eitherobject_bad", false)]);
}
