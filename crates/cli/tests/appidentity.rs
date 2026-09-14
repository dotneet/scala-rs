//! Runtime identities, not simple names, select Scala initialization protocols.
use crate::support;

use std::{fs, path::Path, process::Command};
#[test]
fn shadowed_initialization_traits_match_scalac() {
    let toolchain = support::toolchain();
    let (Some(scalac), Some(jar), Some(java)) = (
        toolchain.scalac(),
        toolchain.scala_library(),
        toolchain.java(),
    ) else {
        eprintln!("skip appidentity differential test: Scala/JVM toolchain unavailable");
        return;
    };
    let root = support::TestDir::new("appidentity");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let expected = fs::read(fixtures.join("expected/appidentity.txt")).unwrap();
    for mode in ["nsc", "jar", "private"] {
        for bad in [false, true] {
            let out = root.join(format!("{mode}-{bad}"));
            fs::create_dir_all(&out).unwrap();
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
                .arg(fixtures.join(if bad {
                    "appidentity_bad.scala"
                } else {
                    "appidentity.scala"
                }))
                .arg("-d")
                .arg(&out)
                .output()
                .unwrap();
            assert_eq!(
                r.status.success(),
                !bad,
                "mode={mode} bad={bad}: {}",
                String::from_utf8_lossy(&r.stderr)
            );
            if !bad {
                let cp = if mode == "private" {
                    out.display().to_string()
                } else {
                    format!("{}:{}", out.display(), jar.display())
                };
                let r = Command::new(java)
                    .args(["-Xverify:all", "-cp", &cp, "Main"])
                    .output()
                    .unwrap();
                assert!(
                    r.status.success(),
                    "mode={mode}: {}",
                    String::from_utf8_lossy(&r.stderr)
                );
                assert_eq!(r.stdout, expected, "mode={mode}");
            }
        }
    }
}
