//! Dependent results, implicit overrides, self types and Java SAM adaptation.
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
        .join(format!("depadapt_{name}.scala"))
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
        "dependent-adaptation-{}-{}-{}",
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
                            .join(format!("depadapt_{name}.txt")),
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
fn pickled_repeated_overloads() {
    matrix(
        &[
            ("catching", true),
            ("catch_qualified", true),
            ("catching_class_control", true),
            ("exception_family", true),
            ("catching_value_bad", false),
        ],
        true,
    );
}

#[test]
fn java_sam_loading_and_wildcard_bounds() {
    matrix(
        &[
            ("consumer", true),
            ("consumer_typed", true),
            ("consumer_direct", true),
            ("sam_primitives", true),
            ("sam_generic", true),
        ],
        true,
    );
}

#[test]
fn java_sam_rejects_narrow_and_wrong_parameters() {
    matrix(
        &[
            ("consumer_bad", false),
            ("sam_narrow_bad", false),
            ("sam_lower_bad", false),
            ("sam_generic_bad", false),
        ],
        true,
    );
}

#[test]
fn dependent_builder_and_singleton_results() {
    matrix(
        &[
            ("builder", true),
            ("singletons", true),
            ("singleton_fresh", true),
            ("builder_bad", false),
            ("singleton_bad", false),
        ],
        true,
    );
}

#[test]
fn nonimplicit_overrides_hide_inherited_evidence() {
    matrix(
        &[
            ("eval_evidence", true),
            ("parent_implicit", true),
            ("implicit_hidden_bad", false),
        ],
        false,
    );
}

#[test]
fn explicit_self_type_requirements() {
    matrix(
        &[
            ("self_intersection", true),
            ("self_instantiation", true),
            ("self_bad", false),
            ("self_new_bad", false),
            ("self_new_parent_bad", false),
            ("self_new_generic_bad", false),
        ],
        false,
    );
}

#[test]
fn function_valued_results_keep_inferred_type() {
    matrix(
        &[
            ("deferred", true),
            ("deferred_function1", true),
            ("deferred_bad", false),
            ("deferred_function1_bad", false),
            ("deferred_known_bad", false),
        ],
        true,
    );
}

#[test]
fn polymorphic_apply_keeps_receiver_substitution() {
    matrix(&[("functionk", true), ("functionk_bad", false)], true);
}
