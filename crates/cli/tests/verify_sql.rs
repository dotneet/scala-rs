//! Regression tests for JVM-valid parent constructor calls with defaults.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("scala-rs-vsql-{nanos}-{}", std::process::id()))
}

fn run_java(out: &Path) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("run java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn parent_default_constructor_is_verified() {
    let out = tmp_dir();
    fs::create_dir_all(&out).unwrap();
    let src = fixtures_dir().join("vsql_parent.scala");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out), "jdbc:test:user:password\n");
    let _ = fs::remove_dir_all(&out);
}
