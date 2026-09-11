//! Declaration shape, field storage and separately compiled Scala signatures.
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
        .join(format!("declbound_{name}.scala"))
}
fn root() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "declaration-boundaries-{}-{}-{}",
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
                    .join(format!("declbound_{name}.txt")),
            )
            .unwrap()
        });
        compare(&[name], expected.as_deref(), JAR);
    }
}
#[test]
fn field_storage_and_unit_statements() {
    matrix(&[
        ("ctor_valueclass", true),
        ("ctor_string", true),
        ("ctor_matrix", true),
        ("reference_valueclass", true),
        ("reference_members", true),
        ("byname_constructor", true),
        ("valueclass_self", true),
        ("unit_discard", true),
        ("unit_val", true),
        ("unit_lazy", true),
        ("unit_method", true),
        ("unit_matrix", true),
        ("unit_null", true),
        ("wrong_generic_bad", false),
    ]);
}
#[test]
fn value_class_accessor_collisions() {
    matrix(&[
        ("vc_object", false),
        ("vc_object_field", false),
        ("vc_lazy_bad", false),
        ("vc_param_bad", false),
        ("bridge_valid", true),
    ]);
}
#[test]
fn clause_and_identity_views() {
    matrix(&[
        ("empty_class_apply_bad", false),
        ("paramless_poly_array", true),
        ("flatmap_identity", true),
        ("flatten", true),
        ("flatten_matrix", true),
        ("array_views", true),
        ("array_view_bad", false),
        ("flatten_bad", false),
        ("wrong_array_bad", false),
    ]);
}
#[test]
fn separate_compilation_preserves_array_and_clause_types() {
    for base in ["array", "clause"] {
        for nsc in [true, false] {
            let root = root();
            let mut c = Command::new(if nsc {
                "/tmp/scala-2.13.16/bin/scalac"
            } else {
                env!("CARGO_BIN_EXE_scala-rs")
            });
            if !nsc {
                c.args(["compile", "--scala-library", JAR]);
            }
            let out = c
                .arg("-d")
                .arg(&root)
                .arg(fixture(&format!("{base}_base")))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "base={base} nsc={nsc}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let cp = format!("{}:{JAR}", root.display());
            let name = format!("{base}_client");
            let expected = fs::read(
                fixture(&name)
                    .parent()
                    .unwrap()
                    .join("expected")
                    .join(format!("declbound_{name}.txt")),
            )
            .unwrap();
            compare(&[&name], Some(&expected), &cp);
            compare(&[&format!("{base}_client_bad")], None, &cp);
            if base == "array" {
                compare(&["array_empty_bad"], None, &cp);
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
}
