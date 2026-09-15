//! Regression tests for generic lambda results nested below invariant types.
//!
//! `def flatMap[B](f: A => Rep[Option[B]])` leaves `B` open while the lambda
//! is typed. The provisional wildcard is not an expected result type: using
//! it for the body made the invariant `Rep[Option[String]]` fail against
//! `Rep[Option[_]]` before the body could infer `B`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-nested-lambda-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn compile(name: &str) -> std::process::Output {
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    let _ = fs::remove_dir_all(out);
    output
}

#[test]
fn infers_nested_invariant_lambda_result() {
    let output = compile("nested_lambda_result");
    assert!(
        output.status.success(),
        "nested result fixture failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn rejects_inferred_result_that_disagrees_with_expected_type() {
    let output = compile("nested_lambda_result_bad");
    assert!(
        !output.status.success(),
        "bad nested result unexpectedly compiled"
    );
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        diagnostics.contains("NestedResultBadRep[Option[String]]")
            || diagnostics.contains("NestedResultBadRep[Option[Int]]"),
        "unexpected nested result diagnostic:\n{diagnostics}"
    );
}
