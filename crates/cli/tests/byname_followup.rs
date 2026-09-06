//! Regression coverage for forwarding an existing by-name identifier through
//! multiple Apply nodes.
//!
//! The tailrec thunk fix keeps the argument tree's `ByName(T)` type so erasure
//! can forward the original thunk. Overload applicability must still compare
//! the value yielded by that thunk with a by-name formal. This is the shape of
//! cats' `Monad.Ops.untilM` and `untilM_` methods.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/byname_followup.scala")
}

fn output_dir(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "scala-rs-byname-followup-{tag}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn byname_identifier_matches_after_multiple_applies() {
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let scalac = "/tmp/scala-2.13.16/bin/scalac";
    if !PathBuf::from(jar).is_file() || !PathBuf::from(scalac).is_file() {
        eprintln!("skip by-name differential test: Scala 2.13.16 toolchain required");
        return;
    }

    let ours = output_dir("ours");
    let output = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            fixture().to_str().unwrap(),
            "-d",
            ours.to_str().unwrap(),
            "--scala-library",
            jar,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "scala-rs rejected the by-name Apply chain:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let reference = output_dir("scalac");
    let output = Command::new(scalac)
        .args([
            fixture().to_str().unwrap(),
            "-d",
            reference.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "scalac rejected the by-name Apply chain: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(ours);
    let _ = fs::remove_dir_all(reference);
}
