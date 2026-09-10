//! Contextual method values, overload clauses, inferred overrides and parent varargs.
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
        .join(format!("ctxfollow_{name}.scala"))
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
        "contextual-followup-{}-{}-{}",
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
                            .join(format!("ctxfollow_{name}.txt")),
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
fn overload_domains_and_specificity() {
    matrix(
        &[
            ("value_reference_overloads", true),
            ("reference_domains", true),
            ("anyval_bad", false),
            ("anyref_bad", false),
            ("equal_domain_bad", false),
            ("reference_erasure_bad", false),
        ],
        false,
    );
}
#[test]
fn inferred_value_expectations() {
    matrix(
        &[
            ("inherited_val", true),
            ("val_narrow", true),
            ("val_var", true),
            ("val_missing_view_bad", false),
            ("val_explicit_bad", false),
            ("val_lambda_bad", false),
        ],
        false,
    );
}
#[test]
fn curried_constructor_clauses() {
    matrix(
        &[
            ("ctor_named", true),
            ("ctor_defaults", true),
            ("ctor_order", true),
            ("ctor_dependent_default", true),
            ("ctor_default_order", true),
            ("ctor_later_scope", true),
            ("ctor_three", true),
            ("ctor_clause_bad", false),
            ("ctor_wrong_bad", false),
            ("ctor_position_bad", false),
            ("ctor_same_name_bad", false),
            ("ctor_extra_bad", false),
        ],
        false,
    );
}
#[test]
fn real_twirl_overloads() {
    let base=Path::new("/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/gitbucket");
    let deps = fs::read_to_string(base.join("deps.cp")).unwrap();
    let cp = format!("{JAR}:{}", deps.trim());
    matrix_cp(&[("twirl", true), ("twirl_null_bad", false)], true, &cp);
}

#[test]
fn literal_identity_and_inherited_linearization() {
    matrix(
        &[
            ("symbol", true),
            ("symbol_bad", false),
            ("val_linearization", true),
            ("val_linearization_bad", false),
        ],
        false,
    );
}
#[test]
fn binary_constructor_clauses() {
    let root = std::env::temp_dir().join(format!(
        "contextual-followup-binary-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let p = Command::new("/tmp/scala-2.13.16/bin/scalac")
        .arg("-d")
        .arg(&root)
        .arg(fixture("binary_ctor_lib"))
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    matrix_cp(
        &[("binary_ctor", true), ("binary_ctor_bad", false)],
        false,
        &format!("{}:{JAR}", root.display()),
    );
    fs::remove_dir_all(root).unwrap();
}
