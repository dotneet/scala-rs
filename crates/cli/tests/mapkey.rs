//! Map keys retain K independently of widening value type parameters.
use std::{fs, path::Path, process::Command};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
#[test]
fn map_key_acceptance_and_execution_match_scalac() {
    let root = std::env::temp_dir().join(format!(
        "mapkey-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let mut cases = vec![(fixtures.join("mapkey.scala"), true)];
    for (i, expression) in [
        "m.apply(1)",
        "m.get(1)",
        "m.contains(1)",
        "m.updated(1, 2)",
        "m.getOrElse(1, 2)",
    ]
    .iter()
    .enumerate()
    {
        let src = root.join(format!("bad{i}.scala"));
        let template = fs::read_to_string(fixtures.join("mapkey_bad.scala")).unwrap();
        fs::write(&src, template.replace("m.apply(1)", expression)).unwrap();
        cases.push((src, false));
    }
    for (i, (source, good)) in cases.iter().enumerate() {
        for oracle in [false, true] {
            let out = root.join(format!("{i}-{oracle}"));
            fs::create_dir(&out).unwrap();
            let mut c = if oracle {
                Command::new("/tmp/scala-2.13.16/bin/scalac")
            } else {
                let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                c.args(["compile", "--scala-library", JAR]);
                c
            };
            let r = c.arg(source).arg("-d").arg(&out).output().unwrap();
            assert_eq!(
                r.status.success(),
                *good,
                "case {i}, oracle {oracle}: {}",
                String::from_utf8_lossy(&r.stderr)
            );
            if *good {
                let r = Command::new("java")
                    .args([
                        "-Xverify:all",
                        "-cp",
                        &format!("{}:{JAR}", out.display()),
                        "Main",
                    ])
                    .output()
                    .unwrap();
                assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
                assert_eq!(
                    r.stdout,
                    fs::read(fixtures.join("expected/mapkey.txt")).unwrap()
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
