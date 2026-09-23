//! A Java type stub created while eagerly reading classpath descriptors must
//! have its real parent hierarchy loaded before typed-pattern compatibility is
//! decided.
//!
//! `PatternErrorHolder.identity` mentions `AbstractMethodError` in its JVM
//! descriptor, so the directory scan creates that exact symbol before source
//! lookup.  Its temporary parent is only `AnyRef`.  Slick subsequently uses
//! `case _: AbstractMethodError` in a `catch`, whose scrutinee is `Throwable`;
//! the pattern is legal because the JDK hierarchy is
//! `AbstractMethodError <: Error <: Throwable`.

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

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-java-pattern-parent-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn descriptor_stub_keeps_java_error_pattern_compatible_with_throwable() {
    let library = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !library.is_file() {
        eprintln!("skip Java pattern parent regression: scala-library jar unavailable");
        return;
    }

    let root = temp_dir("run");
    let java_out = root.join("java");
    let scala_out = root.join("scala");
    fs::create_dir_all(&java_out).unwrap();
    fs::create_dir_all(&scala_out).unwrap();

    let javac = Command::new("javac")
        .args([
            "-d",
            java_out.to_str().unwrap(),
            fixtures_dir()
                .join("PatternErrorHolder.java")
                .to_str()
                .unwrap(),
        ])
        .output()
        .expect("run javac");
    assert!(
        javac.status.success(),
        "javac failed:\n{}{}",
        String::from_utf8_lossy(&javac.stderr),
        String::from_utf8_lossy(&javac.stdout)
    );

    let compiled = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join("java_pattern_parent.scala")
                .to_str()
                .unwrap(),
            "-cp",
            java_out.to_str().unwrap(),
            "--scala-library",
            library.to_str().unwrap(),
            "-d",
            scala_out.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs");
    assert!(
        compiled.status.success(),
        "scala-rs rejected the valid Java error pattern:\n{}{}",
        String::from_utf8_lossy(&compiled.stderr),
        String::from_utf8_lossy(&compiled.stdout)
    );

    let cp = format!(
        "{}:{}:{}",
        scala_out.display(),
        java_out.display(),
        library.display()
    );
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        run.status.success(),
        "java failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "abstract\n");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn throwable_can_match_an_unrelated_open_trait() {
    let library = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !library.is_file() {
        return;
    }
    let root = temp_dir("trait");
    let source = fixtures_dir().join("throwable_trait_pattern.scala");
    for (name, compiler) in [
        ("scala-rs", bin()),
        ("scalac", PathBuf::from("/tmp/scala-2.13.16/bin/scalac")),
    ] {
        if !compiler.is_file() {
            continue;
        }
        let out = root.join(name);
        fs::create_dir_all(&out).unwrap();
        let mut command = Command::new(compiler);
        if name == "scala-rs" {
            command.arg("compile").arg("--scala-library").arg(&library);
        }
        let compiled = command
            .arg(&source)
            .arg("-d")
            .arg(&out)
            .output()
            .expect("compile trait pattern");
        assert!(
            compiled.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let cp = format!("{}:{}", out.display(), library.display());
        let run = Command::new("java")
            .args(["-Xverify:all", "-cp", &cp, "Main"])
            .output()
            .expect("run trait pattern");
        assert!(
            run.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"7\n", "{name}");
    }
    let _ = fs::remove_dir_all(root);
}
