//! Module apply alternatives and bounded polymorphic overloads match scalac.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn overload_module_matches_scalac() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-overload-module-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    for case in [
        "equal_bad",
        "bounded_bad",
        "all_bad",
        "method_narrow",
        "object_narrow",
        "method_only",
        "object_only",
        "unbounded",
        "receiver",
        "collections",
        "set_any",
        "any_bad",
        "int_bound_bad",
        "anyref_bound_bad",
        "widen_bad",
    ] {
        let name = format!("overload_module_{case}");
        for ours in [false, true] {
            let out = root.join(format!("{case}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let mut cmd = if ours {
                let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                c.args(["compile", "--scala-library", jar]);
                c
            } else {
                Command::new("/tmp/scala-2.13.16/bin/scalac")
            };
            let result = cmd
                .arg(fixtures.join(format!("{name}.scala")))
                .arg("-d")
                .arg(&out)
                .output()
                .unwrap();
            let diagnostic = String::from_utf8_lossy(&result.stderr);
            assert_eq!(
                result.status.success(),
                !case.ends_with("bad"),
                "{case}, ours={ours}: {diagnostic}"
            );
            if case.ends_with("bad") {
                assert!(
                    diagnostic.contains(if case == "widen_bad" {
                        "type mismatch"
                    } else {
                        "ambiguous"
                    }),
                    "{case}, ours={ours}: {diagnostic}"
                );
                if case == "all_bad" {
                    for line in [11, 13, 15, 17] {
                        assert!(
                            diagnostic.contains(&format!(".scala:{line}:")),
                            "missing diagnostic at {line}, ours={ours}: {diagnostic}"
                        );
                    }
                }
            } else {
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
                    "{case}, ours={ours}: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(
                    result.stdout,
                    fs::read(fixtures.join("expected").join(format!("{name}.txt"))).unwrap(),
                    "{case}, ours={ours}"
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
