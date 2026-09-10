//! Source-unit ownership and the declaration/use boundary of field accesses.
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
        .join(format!("fieldscope_{name}.scala"))
}
fn root() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "field-scope-{}-{}-{}",
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
                    .join(format!("fieldscope_{name}.txt")),
            )
            .unwrap()
        });
        compare(&[name], expected.as_deref(), JAR);
    }
}
#[test]
fn function_owners_are_source_local() {
    for variant in [
        "qualified",
        "qualified_bad_shape",
        "qualified_bad",
        "packages",
    ] {
        for order in [["Warm", "Live"], ["Live", "Warm"]] {
            let names: Vec<String> = order
                .iter()
                .chain(["Main"].iter())
                .map(|n| format!("{variant}_{n}"))
                .collect();
            compare(
                &names.iter().map(String::as_str).collect::<Vec<_>>(),
                (!variant.contains("bad")).then_some(b"2\n3\n".as_slice()),
                JAR,
            );
        }
    }
}
#[test]
fn generic_field_stores() {
    matrix(&[
        ("generic_var", true),
        ("generic_var_unit", true),
        ("var_matrix", true),
        ("var_unqualified", true),
        ("var_bound", true),
        ("generic_var_bad", false),
        ("var_bound_bad", false),
    ]);
}
#[test]
fn generic_lazy_loads() {
    matrix(&[
        ("lazy_array", true),
        ("lazy_string", true),
        ("lazy_matrix", true),
        ("lazy_retry", true),
        ("lazy_bad", false),
    ]);
}
#[test]
fn unqualified_abstract_loads() {
    matrix(&[
        ("unqualified_val", true),
        ("refined_unit", true),
        ("val_matrix", true),
        ("val_lazy", true),
        ("val_bad", false),
    ]);
}
#[test]
fn separately_compiled_field_boundaries() {
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
        let p = c
            .arg("-d")
            .arg(&root)
            .arg(fixture("binary_base"))
            .output()
            .unwrap();
        assert!(
            p.status.success(),
            "base nsc={nsc}: {}",
            String::from_utf8_lossy(&p.stderr)
        );
        let cp = format!("{}:{JAR}", root.display());
        compare(&["binary_client"], Some(b"7\n7\n7\n9\n()\n"), &cp);
        compare(&["binary_bad"], None, &cp);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn parameterless_receiver_application() {
    matrix(&[
        ("getter_apply", true),
        ("getter_apply_bad", false),
        ("java_empty_bad", false),
        ("empty_clause_bad", false),
        ("iterator_empty_bad", false),
    ]);
}
