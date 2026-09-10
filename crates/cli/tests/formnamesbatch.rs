//! Nested generic inference and source/JVM/ScalaSignature name boundaries.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const REFLECT: &str = "/tmp/scala-2.13.16/lib/scala-reflect.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}
fn root() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let r = std::env::temp_dir().join(format!(
        "formnames-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&r).unwrap();
    r
}
fn compile(name: &str, nsc: bool, out: &Path, cp: &str, accepted: bool) {
    fs::create_dir(out).unwrap();
    let mut c = Command::new(if nsc {
        NSC
    } else {
        env!("CARGO_BIN_EXE_scala-rs")
    });
    if !nsc {
        c.args(["compile", "--scala-library", JAR]);
    }
    let p = c
        .arg(fixtures().join(format!("{name}.scala")))
        .args(["-cp", cp, "-d"])
        .arg(out)
        .output()
        .unwrap();
    assert_eq!(
        p.status.success(),
        accepted,
        "{name} nsc={nsc} out={}: {}",
        out.display(),
        String::from_utf8_lossy(&p.stderr)
    );
    if !accepted {
        let err = String::from_utf8_lossy(&p.stderr);
        assert!(err.contains("error"), "missing rejection diagnostic: {err}");
    }
}
fn run(out: &Path, cp: &str) -> Vec<u8> {
    run_main(out, cp, "Main")
}
fn run_main(out: &Path, cp: &str, main: &str) -> Vec<u8> {
    let p = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            main,
        ])
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    p.stdout
}
fn expected(name: &str) -> Vec<u8> {
    fs::read(fixtures().join("expected").join(format!("{name}.txt"))).unwrap()
}

#[test]
fn nested_actual_base_types_match_scalac() {
    let r = root();
    for nsc in [true, false] {
        let out = r.join(format!("nested-{nsc}"));
        compile("formnames_nested", nsc, &out, JAR, true);
        assert_eq!(run(&out, JAR), expected("formnames_nested"));
        for name in ["formnames_nested_bad", "formnames_nested_bad_result"] {
            compile(name, nsc, &r.join(format!("{name}-{nsc}")), JAR, false);
        }
    }
    fs::remove_dir_all(r).unwrap();
}

#[test]
fn real_forms_inferred_subclasses_execute_mapping() {
    let r = root();
    let cache = PathBuf::from(std::env::var("HOME").unwrap())
        .join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/scalatra");
    let mut cp = JAR.to_string();
    for module in [
        "scalatra-javax",
        "scalatra-forms-javax",
        "scalatra-common-javax",
        "scalatra-compat-javax",
    ] {
        let jar = cache.join(format!("{module}_2.13/3.2.1/{module}_2.13-3.2.1.jar"));
        assert!(jar.is_file(), "missing {}", jar.display());
        cp.push(':');
        cp.push_str(jar.to_str().unwrap());
    }
    for nsc in [true, false] {
        for name in ["formnames_forms", "formnames_forms_named"] {
            let out = r.join(format!("{name}-{nsc}"));
            compile(name, nsc, &out, &cp, true);
            assert_eq!(run(&out, &cp), expected("formnames_forms"));
        }
        compile(
            "formnames_forms_bad",
            nsc,
            &r.join(format!("bad-{nsc}")),
            &cp,
            false,
        );
    }
    fs::remove_dir_all(r).unwrap();
}

fn binary_matrix(api_name: &str, use_name: &str, bad: &[&str]) {
    let r = root();
    let cp = format!("{JAR}:{REFLECT}");
    for producer in [true, false] {
        let api = r.join(format!("api-{producer}"));
        compile(api_name, producer, &api, &cp, true);
        let cp = format!("{}:{cp}", api.display());
        for consumer in [true, false] {
            let out = r.join(format!("use-{producer}-{consumer}"));
            compile(use_name, consumer, &out, &cp, true);
            assert_eq!(run(&out, &cp), expected(use_name));
            for name in bad {
                compile(
                    name,
                    consumer,
                    &r.join(format!("{name}-{producer}-{consumer}")),
                    &cp,
                    false,
                );
            }
        }
    }
    fs::remove_dir_all(r).unwrap();
}

#[test]
fn member_names_and_literal_payloads_cross_both_compilers() {
    binary_matrix(
        "formnames_api",
        "formnames_use",
        &[
            "formnames_bad_argument",
            "formnames_bad_alias",
            "formnames_bad_literal",
        ],
    );
}

#[test]
fn encoded_operator_classes_keep_nesting_boundaries() {
    binary_matrix(
        "formnames_classes_api",
        "formnames_classes_use",
        &["formnames_classes_bad"],
    );
}

#[test]
fn malformed_backquoted_names_are_independently_rejected() {
    let r = root();
    for nsc in [true, false] {
        for name in ["formnames_bad_escape", "formnames_bad_empty"] {
            compile(name, nsc, &r.join(format!("{name}-{nsc}")), JAR, false);
        }
    }
    fs::remove_dir_all(r).unwrap();
}
