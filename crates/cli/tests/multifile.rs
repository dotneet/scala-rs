//! Whole-run compilation: several files in one invocation, referring to each
//! other across packages.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn multi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-multi-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Files naming each other across packages compile in one run and run.
#[test]
fn cross_file_references_resolve() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    if Command::new("java").arg("-version").output().is_err() {
        return;
    }
    let dir = multi_dir();
    let out = tmp_dir("cross");
    let status = Command::new(bin())
        .args([
            "compile",
            dir.join("main.scala").to_str().unwrap(),
            dir.join("lib_a.scala").to_str().unwrap(),
            dir.join("lib_b.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs");
    assert!(status.success(), "multi-file compile failed");
    let output = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "Main",
        ])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let got = String::from_utf8_lossy(&output.stdout).into_owned();
    let want = "a\n6\n5\n";
    assert!(
        got.starts_with(want),
        "unexpected output: {got:?} (wanted it to start with {want:?})"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A name in an enclosing package is visible without an import.
#[test]
fn enclosing_package_names_are_visible() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    if Command::new("java").arg("-version").output().is_err() {
        return;
    }
    let dir = multi_dir();
    let out = tmp_dir("pkg");
    let status = Command::new(bin())
        .args([
            "compile",
            dir.join("pkg_inner.scala").to_str().unwrap(),
            dir.join("pkg_outer.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs");
    assert!(status.success(), "compile failed");
    let output = Command::new("java")
        .args([
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "top.inner.Main",
        ])
        .output()
        .expect("java");
    assert!(output.status.success(), "run failed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n");
    let _ = fs::remove_dir_all(&out);
}
