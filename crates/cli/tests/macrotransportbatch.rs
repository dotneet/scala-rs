//! Macro AST transport and separately compiled macro declaration access.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const REFLECT: &str = "/tmp/scala-2.13.16/lib/scala-reflect.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn compile(name: &str, nsc: bool, out: &Path, cp: &str, accepted: bool) {
    fs::create_dir(out).unwrap();
    let mut cmd = Command::new(if nsc {
        NSC
    } else {
        env!("CARGO_BIN_EXE_scala-rs")
    });
    if !nsc {
        cmd.args(["compile", "--scala-library", JAR]);
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("{name}.scala"));
    let r = cmd
        .arg(source)
        .args(["-cp", cp, "-d"])
        .arg(out)
        .output()
        .unwrap();
    assert_eq!(
        r.status.success(),
        accepted,
        "{name} nsc={nsc}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    if name == "macrotransport_owner_use" {
        let stdout = String::from_utf8_lossy(&r.stdout);
        let stderr = String::from_utf8_lossy(&r.stderr);
        for owner in ["field", "local", "definitions"] {
            assert!(
                stdout.contains(&format!("owner 日本語 {owner}")),
                "{stdout}"
            );
            assert!(
                stderr.contains(&format!("owner stderr 日本語 {owner}")),
                "{stderr}"
            );
        }
    }
    if !accepted {
        let error = String::from_utf8_lossy(&r.stderr);
        assert!(
            if name == "macrotransport_bad" {
                error.contains("cannot be accessed")
            } else if name == "macrotransport_owner_bad" {
                error.contains("recursive value field needs type")
                    && error.contains("recursive value local needs type")
            } else if name == "macrotransport_fields_bad" {
                error.matches("integer storage is forbidden").count() == 4
            } else {
                error.contains("type mismatch") || error.contains("no matching overload")
            },
            "wrong rejection: {error}"
        );
    }
}
fn run(out: &Path, cp: &str) -> Vec<u8> {
    let r = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            "Main",
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    r.stdout
}
fn root() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "macrotransport-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}
#[test]
fn bidirectional_macro_transport_and_access() {
    let root = root();
    let implementation = root.join("implementation");
    let base_cp = format!("{JAR}:{REFLECT}");
    compile("macrotransport_impl", true, &implementation, &base_cp, true);
    let impl_cp = format!("{}:{base_cp}", implementation.display());
    let expected = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/expected/macrotransport_use.txt"),
    )
    .unwrap();
    let mut reference = None;
    for producer in [true, false] {
        let api = root.join(format!("api-{producer}"));
        compile("macrotransport_api", producer, &api, &impl_cp, true);
        let cp = format!("{}:{impl_cp}", api.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_use", consumer, &output, &cp, true);
            let stdout = run(&output, &cp);
            assert_eq!(stdout, expected);
            let nsc_stdout = reference.get_or_insert_with(|| stdout.clone());
            assert_eq!(&stdout, nsc_stdout);
            for bad in ["macrotransport_bad", "macrotransport_arg_bad"] {
                compile(
                    bad,
                    consumer,
                    &root.join(format!("{bad}-{producer}-{consumer}")),
                    &cp,
                    false,
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn formerly_refused_shapes_execute_once_and_match_scalac() {
    let root = root();
    let expected = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/expected/macrotransport_legacy.txt"),
    )
    .unwrap();
    for producer in [true, false] {
        let base_cp = format!("{JAR}:{REFLECT}");
        let eg = root.join(format!("eg-{producer}"));
        let ex = root.join(format!("ex-{producer}"));
        compile("eg_impl", producer, &eg, &base_cp, true);
        compile("ex_impl", producer, &ex, &base_cp, true);
        let cp = format!("{}:{}:{base_cp}", eg.display(), ex.display());
        // API roundtrips are checked above. Explicit dependent signatures of
        // scala-rs-produced implementation methods are not nsc-readable yet;
        // here compare executable transport using each supported producer.
        for consumer in [true, false].into_iter().filter(|&nsc| producer || !nsc) {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_legacy", consumer, &output, &cp, true);
            assert_eq!(run(&output, &cp), expected);
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inferred_implementation_materializes_qualified_context_tag() {
    let root = root();
    for producer in [true, false] {
        let cp = format!("{JAR}:{REFLECT}");
        let implementation = root.join(format!("implementation-{producer}"));
        compile(
            "macrotransport_inferred",
            producer,
            &implementation,
            &cp,
            true,
        );
        let cp = format!("{}:{cp}", implementation.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_inferred_use", consumer, &output, &cp, true);
            assert_eq!(run(&output, &cp), b"42\n");
            compile(
                "macrotransport_inferred_bad",
                consumer,
                &root.join(format!("bad-{producer}-{consumer}")),
                &cp,
                false,
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn separate_expansions_keep_distinct_local_class_identities() {
    let root = root();
    for producer in [true, false] {
        let cp = format!("{JAR}:{REFLECT}");
        let implementation = root.join(format!("implementation-{producer}"));
        compile(
            "macrotransport_class_identity",
            producer,
            &implementation,
            &cp,
            true,
        );
        let cp = format!("{}:{cp}", implementation.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile(
                "macrotransport_class_identity_use",
                consumer,
                &output,
                &cp,
                true,
            );
            assert_eq!(run(&output, &cp), b"true\ntrue\n");
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reflected_storage_fields_drive_macro_rejection() {
    let root = root();
    for producer in [true, false] {
        let cp = format!("{JAR}:{REFLECT}");
        let implementation = root.join(format!("implementation-{producer}"));
        compile(
            "macrotransport_fields",
            producer,
            &implementation,
            &cp,
            true,
        );
        let cp = format!("{}:{cp}", implementation.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_fields_use", consumer, &output, &cp, true);
            assert_eq!(run(&output, &cp), b"true\ntrue\ntrue\ntrue\nok\nok\n");
            compile(
                "macrotransport_fields_bad",
                consumer,
                &root.join(format!("bad-{producer}-{consumer}")),
                &cp,
                false,
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_ownership_and_macro_console_match_scalac() {
    let root = root();
    let implementation = root.join("implementation");
    let cp = format!("{JAR}:{REFLECT}");
    compile(
        "macrotransport_owner_impl",
        true,
        &implementation,
        &cp,
        true,
    );
    let cp = format!("{}:{cp}", implementation.display());
    let expected = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/expected/macrotransport_owner_use.txt"),
    )
    .unwrap();
    for producer in [true, false] {
        let api = root.join(format!("api-{producer}"));
        compile("macrotransport_owner_api", producer, &api, &cp, true);
        let cp = format!("{}:{cp}", api.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_owner_use", consumer, &output, &cp, true);
            assert_eq!(run(&output, &cp), expected);
            compile(
                "macrotransport_owner_bad",
                consumer,
                &root.join(format!("bad-{producer}-{consumer}")),
                &cp,
                false,
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}
