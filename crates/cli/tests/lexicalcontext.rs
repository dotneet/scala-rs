//! Lexical type context, field identity, function invocation and capture boundaries.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("lexctx_{name}.scala"))
}
fn matrix(names: &[(&str, bool)], warm: bool) {
    matrix_cp(names, warm, JAR);
}
fn matrix_cp(names: &[(&str, bool)], warm: bool, cp: &str) {
    matrix_ordered(names, warm.then_some("warm"), cp);
}
fn matrix_ordered(names: &[(&str, bool)], warm: Option<&str>, cp: &str) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "lexical-context-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    for &(name, accepted) in names {
        let mut oracle = None;
        for nsc in [true, false] {
            for order in 0..if warm.is_some() { 3 } else { 1 } {
                let out = root.join(format!("{name}-{nsc}-{order}"));
                fs::create_dir(&out).unwrap();
                let mut c = Command::new(if nsc {
                    "/tmp/scala-2.13.16/bin/scalac"
                } else {
                    env!("CARGO_BIN_EXE_scala-rs")
                });
                if !nsc {
                    c.args(["compile", "--scala-library", JAR]);
                }
                c.args(["-cp", cp, "-d"]).arg(&out);
                if order == 1 {
                    c.arg(fixture(warm.unwrap()));
                }
                c.arg(fixture(name));
                if order == 2 {
                    c.arg(fixture(warm.unwrap()));
                }
                let p = c.output().unwrap();
                assert_eq!(
                    p.status.success(),
                    accepted,
                    "{name} nsc={nsc} order={order}: {}",
                    String::from_utf8_lossy(&p.stderr)
                );
                if accepted {
                    let p = Command::new("java")
                        .args([
                            "-Xverify:all",
                            "-cp",
                            &format!("{}:{cp}", out.display()),
                            "Main",
                        ])
                        .output()
                        .unwrap();
                    assert!(
                        p.status.success(),
                        "{name} nsc={nsc} order={order}: {}",
                        String::from_utf8_lossy(&p.stderr)
                    );
                    let expected = fs::read(
                        fixture(name)
                            .parent()
                            .unwrap()
                            .join("expected")
                            .join(format!("lexctx_{name}.txt")),
                    )
                    .unwrap();
                    assert_eq!(p.stdout, expected, "{name} nsc={nsc} order={order}");
                    if let Some(ref oracle) = oracle {
                        assert_eq!(&p.stdout, oracle);
                    } else {
                        oracle = Some(p.stdout);
                    }
                } else {
                    assert!(String::from_utf8_lossy(&p.stderr).contains("error"));
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lexical_type_context() {
    matrix(
        &[
            ("nested_alias", true),
            ("nested_alias_renamed", true),
            ("outer_alias", true),
            ("inherited_alias", true),
            ("path_alias", true),
            ("outer_alias_bad", false),
        ],
        false,
    );
}
#[test]
fn lambda_capture_context() {
    matrix(
        &[
            ("capture_object", true),
            ("capture_class", true),
            ("capture_method", true),
            ("capture_named", true),
            ("capture_block", true),
            ("capture_trait", true),
            ("capture_return", true),
            ("capture_bad", false),
        ],
        false,
    );
}
#[test]
fn nullary_function_invocation() {
    matrix(
        &[
            ("nullary", true),
            ("nullary_order", true),
            ("nullary_bad", false),
        ],
        false,
    );
}
#[test]
fn java_field_identity() {
    let root = std::env::temp_dir().join(format!(
        "lexical-java-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let source = fixture("marker")
        .parent()
        .unwrap()
        .join("lexctx_ObjectFields.java");
    let p = Command::new("javac")
        .args(["--release", "8", "-d"])
        .arg(&root)
        .arg(source)
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    matrix_cp(
        &[
            ("marker", true),
            ("marker_bad", false),
            ("java_fields", true),
            ("java_generic_bad", false),
            ("java_final_bad", false),
            ("scala_anyref_bad", false),
            ("java_array", true),
            ("java_array_write", true),
            ("java_array_bad", false),
            ("java_static_import", true),
            ("java_generic_primitive", true),
        ],
        false,
        &format!("{}:{JAR}", root.display()),
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn abstract_val_and_var_bridges() {
    matrix(
        &[
            ("bridge_unit", true),
            ("bridge_var", true),
            ("bridge_function", true),
            ("bridge_valueclass", true),
            ("bridge_def", true),
            ("bridge_array", true),
            ("bridge_bad", false),
        ],
        false,
    );
}
