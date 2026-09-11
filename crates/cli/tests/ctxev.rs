//! Contextual prototypes, lexical imports and value-class declaration storage.
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
        .join(format!("ctxev_{name}.scala"))
}
fn root() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "contextual-evidence-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    root
}
fn compare(names: &[&str], expected: Option<&[u8]>, cp: &str) {
    let root = root();
    let mut oracle = None;
    for nsc in [true, false] {
        let out = root.join(if nsc { "nsc" } else { "rs" });
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
        for name in names {
            c.arg(fixture(name));
        }
        let p = c.output().unwrap();
        assert_eq!(
            p.status.success(),
            expected.is_some(),
            "{names:?} nsc={nsc}: {}",
            String::from_utf8_lossy(&p.stderr)
        );
        if let Some(expected) = expected {
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
                "{names:?} nsc={nsc}: {}",
                String::from_utf8_lossy(&p.stderr)
            );
            assert_eq!(p.stdout, expected, "{names:?} nsc={nsc}");
            if let Some(ref oracle) = oracle {
                assert_eq!(&p.stdout, oracle);
            } else {
                oracle = Some(p.stdout);
            }
        } else {
            assert!(String::from_utf8_lossy(&p.stderr).contains("error"));
        }
    }
    fs::remove_dir_all(root).unwrap();
}
fn matrix(names: &[(&str, bool)]) {
    for &(name, accepted) in names {
        let expected = accepted.then(|| {
            fs::read(
                fixture(name)
                    .parent()
                    .unwrap()
                    .join("expected")
                    .join(format!("ctxev_{name}.txt")),
            )
            .unwrap()
        });
        compare(&[name], expected.as_deref(), JAR);
    }
}
#[test]
fn parent_and_function_inference() {
    matrix(&[
        ("contextual", true),
        ("parent_implicitly_bad", false),
        ("parent_bad", false),
        ("function_bad", false),
    ]);
}
#[test]
fn source_order_import_signatures() {
    matrix(&[
        ("imports", true),
        ("import_after_signature_bad", false),
        ("import_escape_bad", false),
    ]);
}
#[test]
fn invariant_lower_bound_prototypes() {
    matrix(&[("fallback", true), ("getorelse_wrong_bad", false)]);
}
#[test]
fn value_class_storage_boundaries() {
    matrix(&[("valueclass", true)]);
}

#[test]
fn deferred_lambda_results_and_invariant_context() {
    matrix(&[
        ("recovery_inference", true),
        ("recovery_hk_wrong_bad", false),
        ("recovery_fold_wrong_bad", false),
        ("recovery_function_domain_bad", false),
        ("recovery_explicit_result_bad", false),
        ("recovery_fold_invariant_bad", false),
    ]);
}
#[test]
fn rigid_factory_evidence() {
    matrix(&[
        ("recovery_factory", true),
        ("recovery_factory_wrong_bad", false),
        ("recovery_factory_missing_bad", false),
    ]);
}
#[test]
fn secondary_constructor_and_written_copy() {
    matrix(&[
        ("recovery_copy", true),
        ("recovery_constructor_wrong_bad", false),
        ("recovery_local_copy_bad", false),
        ("recovery_private_parent_copy_bad", false),
        ("recovery_curried_copy_bad", false),
        ("recovery_own_copy_bad", false),
        ("recovery_inherited_copy_bad", false),
        ("recovery_copy_default_bad", false),
    ]);
}
#[test]
fn provisional_values_preserve_written_constraints() {
    matrix(&[("recovery_value_hints", true)]);
}
