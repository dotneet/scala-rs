//! Regression coverage for forwarding an existing by-name identifier through
//! multiple Apply nodes.
//!
//! The tailrec thunk fix keeps the argument tree's `ByName(T)` type so erasure
//! can forward the original thunk. Overload applicability must still compare
//! the value yielded by that thunk with a by-name formal. This is the shape of
//! cats' `Monad.Ops.untilM` and `untilM_` methods.

use crate::support;

use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/byname_followup.scala")
}

#[test]
fn byname_identifier_matches_after_multiple_applies() {
    let toolchain = support::toolchain();
    let (Some(jar), Some(scalac)) = (toolchain.scala_library(), toolchain.scalac()) else {
        eprintln!("skip by-name differential test: Scala 2.13.16 toolchain required");
        return;
    };

    let root = support::TestDir::new("byname-followup");
    let ours = root.join("ours");
    std::fs::create_dir_all(&ours).unwrap();
    let output = support::CompileCommand::new(fixture(), &ours)
        .scala_library(jar)
        .run();
    assert!(
        output.success(),
        "scala-rs rejected the by-name Apply chain:\n{}{}",
        String::from_utf8_lossy(output.stdout()),
        String::from_utf8_lossy(output.stderr())
    );

    let reference = root.join("scalac");
    std::fs::create_dir_all(&reference).unwrap();
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
}
