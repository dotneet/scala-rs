//! Java type completion is independent of preceding compilation units.
#[path = "support/temp_nonce.rs"]
mod temp_nonce;

use std::{fs, path::Path, process::Command};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
#[test]
fn java_types_complete_after_descriptor_discovery() {
    let root = std::env::temp_dir().join(format!(
        "jwarm-{}-{}",
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
    let lib = root.join("lib");
    fs::create_dir(&lib).unwrap();
    let mut javac = Command::new("javac");
    for name in ["Api", "Marker", "Carrier"] {
        javac.arg(fixtures.join(format!("java/jwarm/{name}.java")));
    }
    let r = javac.arg("-d").arg(&lib).output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let jar = root.join("lib.jar");
    assert!(Command::new("jar")
        .arg("cf")
        .arg(&jar)
        .arg("-C")
        .arg(&lib)
        .arg(".")
        .status()
        .unwrap()
        .success());
    for (index, source) in ["jwarm.scala", "jwarm_bad.scala"].iter().enumerate() {
        for reverse in [false, true] {
            for oracle in [false, true] {
                let out = root.join(format!("{index}-{reverse}-{oracle}"));
                fs::create_dir(&out).unwrap();
                let mut c = if oracle {
                    Command::new("/tmp/scala-2.13.16/bin/scalac")
                } else {
                    let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                    c.args(["compile", "--scala-library", JAR]);
                    c
                };
                let mut sources = vec![fixtures.join("jwarm_first.scala"), fixtures.join(source)];
                if reverse {
                    sources.reverse();
                }
                let cp = format!("{}:{JAR}", jar.display());
                let r = c
                    .args(sources)
                    .args(["-cp", &cp])
                    .arg("-d")
                    .arg(&out)
                    .output()
                    .unwrap();
                assert_eq!(
                    r.status.success(),
                    index == 0,
                    "{source}, reverse={reverse}, oracle={oracle}: {}",
                    String::from_utf8_lossy(&r.stderr)
                );
                if index == 0 {
                    let r = Command::new("java")
                        .args([
                            "-Xverify:all",
                            "-cp",
                            &format!("{}:{cp}", out.display()),
                            "Main",
                        ])
                        .output()
                        .unwrap();
                    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
                    assert_eq!(
                        r.stdout,
                        fs::read(fixtures.join("expected/jwarm.txt")).unwrap()
                    );
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
