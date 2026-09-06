//! Compiler-defined Singleton conformance and JVM behavior against scalac.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn singleton_match_scalac() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-singleton-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let scalac = "/tmp/scala-2.13.16/bin/scalac";
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/singleton");
    for (name, accepted) in [
        ("literal", true),
        ("unit_bad", false),
        ("null", true),
        ("compound", true),
        ("selected", true),
        ("stable", true),
        ("fresh_bad", false),
        ("mutable_bad", false),
        ("method_bad", false),
        ("pattern", true),
        ("runtime", true),
        ("static_runtime", true),
        ("alias", true),
        ("bound", true),
        ("bound_stable", true),
        ("bound_mutable_bad", false),
        ("bound_bad", false),
        ("qualified", true),
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
            if name == "runtime" || name == "static_runtime" {
                let result = Command::new("java")
                    .arg("-Xverify:all")
                    .arg("-cp")
                    .arg(format!("{}:{jar}", out.display()))
                    .arg("Main")
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "ours={ours}: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&result.stdout),
                    if name == "runtime" {
                        "yes\nyes\nyes\nstable\n1\ntrue\ntrue\ntrue\neffect\nfalse\n"
                    } else {
                        "true\ntrue\nfalse\ntrue\nfalse\ntrue\nno\n"
                    },
                    "ours={ours}"
                );
            }
            if !accepted {
                let diagnostic = String::from_utf8_lossy(&result.stderr);
                assert!(
                    diagnostic.contains("type mismatch") || diagnostic.contains("type arguments"),
                    "{name}, ours={ours}: {diagnostic}"
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
