//! Case-local type binders retain scope, kinds, bounds and runtime branch types.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn pattern_type_binders_match_scalac() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-pattern-type-binders-{}-{}",
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/pattern_type_binders");
    for (name, accepted) in [
        ("refined", true),
        ("abstract", true),
        ("shadow", true),
        ("declared_bound", true),
        ("higher_kind", true),
        ("covariant_good", true),
        ("bound_covariant", true),
        ("runtime", true),
        ("t6275", true),
        ("bad_value", false),
        ("scope", false),
        ("quoted", false),
        ("quoted_missing", false),
        ("duplicate", false),
        ("covariant", false),
        ("contravariant_bad", false),
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
            if accepted && name == "runtime" {
                let cp = format!("{}:{jar}", out.display());
                let ran = Command::new("java")
                    .args(["-Xverify:all", "-cp", &cp, "Main"])
                    .output()
                    .unwrap();
                assert!(
                    ran.status.success(),
                    "{}",
                    String::from_utf8_lossy(&ran.stderr)
                );
                assert_eq!(String::from_utf8_lossy(&ran.stdout), "42\nbound\nother\n");
            }
            if !accepted {
                let diagnostic = String::from_utf8_lossy(&result.stderr);
                assert!(
                    diagnostic.contains(if name == "scope" || name == "quoted_missing" {
                        "not found: type t"
                    } else if name == "quoted" {
                        "incompatible"
                    } else if name == "duplicate" {
                        "already defined"
                    } else {
                        "type mismatch"
                    }),
                    "{name}, ours={ours}: {diagnostic}"
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
