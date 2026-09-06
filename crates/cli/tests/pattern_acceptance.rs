//! Audit valid programs hidden by fatal-warning corpus classifications.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn pattern_acceptance_match_scalac() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-pattern-acceptance-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let scalac = "/tmp/scala-2.13.16/bin/scalac";
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/pattern_acceptance");
    for (name, accepted) in [
        ("t7984", true),
        ("t8597", true),
        ("list_subtype", true),
        ("list_explicit_bad", false),
        ("some_explicit_bad", false),
        ("map_explicit_bad", false),
        ("vector_explicit_bad", false),
    ] {
        let source = fs::read_to_string(fixtures.join(format!("{name}.scala"))).unwrap();
        let src = root.join(format!("{name}.scala"));
        fs::write(&src, source).unwrap();
        for ours in [false, true] {
            let out = root.join(format!("{name}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let mut cmd = if ours {
                let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                c.args(["compile", "--scala-library", jar]);
                c
            } else {
                Command::new(scalac)
            };
            let result = cmd.arg(&src).arg("-d").arg(&out).output().unwrap();
            assert_eq!(
                result.status.success(),
                accepted,
                "{name}, ours={ours}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            if !accepted {
                let diagnostic = String::from_utf8_lossy(&result.stderr);
                assert!(
                    diagnostic.contains("type mismatch"),
                    "{name}, ours={ours}: {diagnostic}"
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
