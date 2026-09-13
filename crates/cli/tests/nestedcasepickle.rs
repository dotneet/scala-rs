//! Separate-compilation regression for named arguments on a nested case class.
//!
//! A source-level `Outer.Status(...)` goes through `Status$`'s synthetic
//! companion `apply`, so the consumer must recover its parameter symbols from
//! the library pickle rather than from the source unit that defined `Status`.

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

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-nestedcasepickle-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn compile(src: &Path, out: &Path, jar: &Path, classpath: Option<&Path>) {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    if let Some(cp) = classpath {
        cmd.args(["-cp", cp.to_str().unwrap()]);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {} failed:\n{}{}",
        src.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn separate_compilation_recovers_nested_case_apply_parameter_names() {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !jar.is_file() {
        eprintln!("skip nested case pickle regression: scala-library jar not obtainable");
        return;
    }

    let root = tmp_dir("named");
    let lib_out = root.join("lib");
    let use_out = root.join("use");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&use_out).unwrap();

    compile(
        &fixtures_dir().join("nestedcasepickle_lib.scala"),
        &lib_out,
        &jar,
        None,
    );
    compile(
        &fixtures_dir().join("nestedcasepickle_use.scala"),
        &use_out,
        &jar,
        Some(&lib_out),
    );

    let cp = format!(
        "{}:{}:{}",
        use_out.display(),
        lib_out.display(),
        jar.display()
    );
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "everyone:1\n");

    let _ = fs::remove_dir_all(root);
}
