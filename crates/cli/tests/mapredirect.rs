//! Explicit type arguments survive inherited factory lookup after delegate loading.
use std::{fs, path::PathBuf, process::Command};

#[test]
fn inherited_factory_redirect_matches_scalac() {
    let root = std::env::temp_dir().join(format!(
        "mapredirect-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let expected = fs::read(fixtures.join("expected/mapredirect.txt")).unwrap();
    for explicit in [false, true] {
        for bad in [false, true] {
            for oracle in [false, true] {
                let out = root.join(format!("{explicit}-{bad}-{oracle}"));
                fs::create_dir(&out).unwrap();
                let source = root.join(format!("Main-{explicit}-{bad}-{oracle}.scala"));
                let mut text = fs::read_to_string(fixtures.join("mapredirect.scala")).unwrap();
                if explicit {
                    text = text.replace(
                        "mutable.Map[String, Int]()",
                        "mutable.Map.apply[String, Int]()",
                    );
                }
                fs::write(&source, text).unwrap();
                let mut files = vec![source];
                if bad {
                    files.push(fixtures.join("mapredirect_bad.scala"));
                }
                let mut cmd = if oracle {
                    Command::new("/tmp/scala-2.13.16/bin/scalac")
                } else {
                    let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                    c.args(["compile", "--scala-library", jar]);
                    c
                };
                let result = cmd.args(files).arg("-d").arg(&out).output().unwrap();
                assert_eq!(
                    result.status.success(),
                    !bad,
                    "explicit={explicit} bad={bad} oracle={oracle}: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                if !bad {
                    let result = Command::new("java")
                        .args([
                            "-Xverify:all",
                            "-cp",
                            &format!("{}:{jar}", out.display()),
                            "Warm",
                        ])
                        .output()
                        .unwrap();
                    assert!(
                        result.status.success(),
                        "{}",
                        String::from_utf8_lossy(&result.stderr)
                    );
                    assert_eq!(result.stdout, expected);
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
