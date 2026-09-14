//! Classfile round-trip regression for a path-dependent DDL extension.
//!
//! The library fixture is compiled first, then consumed from its emitted
//! classfiles. This is the supply path used by slick-testkit and catches
//! missing ScalaSignature type members that a same-invocation source compile
//! cannot expose.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/{name}"))
}

fn tmp_dir() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-slickddl-classpath-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn compile(args: &[&str]) -> (bool, String) {
    let output = Command::new(bin())
        .args(args)
        .output()
        .expect("run scala-rs compile");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), diagnostics)
}

#[test]
fn schema_description_extension_survives_classfile_round_trip() {
    let scala_library = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !scala_library.is_file() {
        eprintln!("skip slickddl_classpath: scala-library not cached");
        return;
    }
    let out = tmp_dir();
    let library_out = out.join("library");
    let consumer_out = out.join("consumer");
    fs::create_dir_all(&library_out).unwrap();
    fs::create_dir_all(&consumer_out).unwrap();

    let library_fixture = fixture("slickddl_classpath_lib.scala");
    let library_args = [
        "compile",
        library_fixture.to_str().unwrap(),
        "-d",
        library_out.to_str().unwrap(),
        "--scala-library",
        scala_library.to_str().unwrap(),
    ];
    let (ok, diagnostics) = compile(&library_args);
    assert!(
        ok,
        "classfile fixture library failed to compile:\n{diagnostics}"
    );

    let consumer_fixture = fixture("slickddl_classpath.scala");
    let consumer_args = [
        "compile",
        consumer_fixture.to_str().unwrap(),
        "-d",
        consumer_out.to_str().unwrap(),
        "-cp",
        library_out.to_str().unwrap(),
        "--scala-library",
        scala_library.to_str().unwrap(),
    ];
    let (ok, diagnostics) = compile(&consumer_args);
    assert!(
        ok && !diagnostics.contains("error:"),
        "classfile fixture consumer failed to compile:\n{diagnostics}"
    );
    assert!(
        fs::read_dir(&consumer_out).unwrap().next().is_some(),
        "classfile fixture emitted no consumer classes"
    );
    let _ = fs::remove_dir_all(out);
}
