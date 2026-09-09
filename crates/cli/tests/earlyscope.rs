//! Early completion must retain imports from the definition's own unit.
use std::{fs, path::PathBuf, process::Command};

#[test]
fn parent_anonymous_body_uses_forward_members_lexical_scope() {
    let root = std::env::temp_dir().join(format!(
        "earlyscope-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let expected = fs::read(fixtures.join("expected/earlyscope.txt")).unwrap();
    for reverse in [false, true] {
        for bad in [false, true] {
            for oracle in [false, true] {
                let out = root.join(format!("{reverse}-{bad}-{oracle}"));
                fs::create_dir(&out).unwrap();
                let mut files = vec![
                    fixtures.join("earlyscope_caller.scala"),
                    fixtures.join("earlyscope_directory.scala"),
                ];
                if reverse {
                    files.reverse();
                }
                if bad {
                    files.push(fixtures.join("earlyscope_bad.scala"));
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
                    "reverse={reverse} bad={bad} oracle={oracle}: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                if !bad {
                    let result = Command::new("java")
                        .args([
                            "-Xverify:all",
                            "-cp",
                            &format!("{}:{jar}", out.display()),
                            "SmallCaller",
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
