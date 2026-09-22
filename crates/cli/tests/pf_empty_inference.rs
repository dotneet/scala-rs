//! An empty polymorphic value inside a partial-function result keeps its
//! lower-bound type while the enclosing method type parameter is inferred.

use crate::support;

use std::{path::PathBuf, process::Command};

#[test]
fn pf_empty_result_preserves_the_receiver_lower_bound() {
    let toolchain = support::toolchain();
    let (Some(scalac), Some(jar)) = (toolchain.scalac(), toolchain.scala_library()) else {
        eprintln!("skip PF empty-result inference differential test: Scala toolchain unavailable");
        return;
    };

    let root = support::TestDir::new("pf-empty-inference");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/pf_empty_inference.scala");
    let ours = support::CompileCommand::new(&source, &root.join("ours"))
        .scala_library(&jar)
        .run();
    assert!(
        ours.success(),
        "scala-rs rejected the fixture:\n{}",
        ours.diagnostics()
    );

    let theirs_out = root.join("scalac");
    std::fs::create_dir_all(&theirs_out).unwrap();
    let theirs = Command::new(scalac)
        .arg(&source)
        .arg("-d")
        .arg(&theirs_out)
        .output()
        .unwrap();
    assert!(
        theirs.status.success(),
        "scalac rejected the fixture:\n{}{}",
        String::from_utf8_lossy(&theirs.stderr),
        String::from_utf8_lossy(&theirs.stdout),
    );
}
