//! Runtime identities, not simple names, select Scala initialization protocols.
#[path = "support/temp_nonce.rs"]
mod temp_nonce;

use std::{fs, path::Path, process::Command};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
#[test]
fn shadowed_initialization_traits_match_scalac() {
    let root = std::env::temp_dir().join(format!(
        "appidentity-{}-{}",
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
    let expected = fs::read(fixtures.join("expected/appidentity.txt")).unwrap();
    for mode in ["nsc", "jar", "private"] {
        for bad in [false, true] {
            let out = root.join(format!("{mode}-{bad}"));
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
                    format!("{}:{JAR}", out.display())
                };
                let r = Command::new("java")
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
    fs::remove_dir_all(root).unwrap();
}
