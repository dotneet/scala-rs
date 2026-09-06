//! Recompiling an object into a classpath directory replaces its static mirror.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
#[test]
fn recompilation_preserves_main_forwarder() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-incremental-forwarder-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/incremental_forwarder");
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    for (producer, consumer) in [(false, false), (true, true), (false, true), (true, false)] {
        let out = root.join(format!("{producer}-{consumer}"));
        fs::create_dir_all(&out).unwrap();
        for round in 1..=2 {
            let ours = if round == 1 { producer } else { consumer };
            let mut cmd = if ours {
                let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                c.args(["compile", "--scala-library", jar]);
                c
            } else {
                Command::new("/tmp/scala-2.13.16/bin/scalac")
            };
            if round == 1 {
                cmd.arg(fixtures.join("Exts_1.scala"));
            }
            let result = cmd
                .arg(fixtures.join(format!("Main_{round}.scala")))
                .arg("-cp")
                .arg(&out)
                .arg("-d")
                .arg(&out)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "ours={ours}, round={round}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let result = Command::new("java")
                .args([
                    "-Xverify:all",
                    "-cp",
                    &format!("{}:{jar}", out.display()),
                    "Main",
                ])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "ours={ours}, round={round}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&result.stdout),
                if round == 1 { "moo!one\n" } else { "moo!two\n" },
                "ours={ours}, round={round}"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
