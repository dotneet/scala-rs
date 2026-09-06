//! Regression tests for backend limitations that must become compile errors.

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
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("scala-rs-codegen-diag-{tag}-{nanos}"));
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

fn has_class_files(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let p = entry.path();
        if p.is_dir() {
            has_class_files(&p)
        } else {
            p.extension().is_some_and(|ext| ext == "class")
        }
    })
}

fn compile(args: &[&Path], out: &Path) -> std::process::Output {
    let mut command = Command::new(bin());
    command.arg("compile").arg("--no-scala-library");
    for arg in args {
        command.arg(arg);
    }
    command.args(["-d", out.to_str().expect("output path")]);
    command.output().expect("run scala-rs compile")
}

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn scalac() -> Option<PathBuf> {
    let scalac = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    scalac.is_file().then_some(scalac)
}

#[test]
fn generic_array_clone_is_a_source_diagnostic_without_classfiles() {
    let source = fixtures_dir().join("codegen_diag_clone.scala");
    let out = tmp_dir("clone");
    let output = compile(&[&source], &out);
    assert!(
        !output.status.success(),
        "unsupported clone unexpectedly compiled"
    );
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        rendered.contains("codegen_diag_clone.scala:4:"),
        "{rendered}"
    );
    assert!(rendered.contains("generic Array"), "{rendered}");
    assert!(
        !has_class_files(&out),
        "backend error published class files"
    );
    let _ = fs::remove_dir_all(out);
}

#[test]
fn backend_span_keeps_the_failing_unit_with_multiple_sources() {
    let dir = tmp_dir("multi");
    let bad = dir.join("bad.scala");
    let good = dir.join("good.scala");
    fs::write(
        &bad,
        "object Bad {\n  def dup[T](a: Array[T]): Array[T] = a.clone()\n}\n",
    )
    .expect("write bad source");
    fs::write(&good, "object Good {\n  def value: Int = 1\n}\n").expect("write good source");
    let out = dir.join("classes");
    let output = compile(&[&good, &bad], &out);
    assert!(
        !output.status.success(),
        "unsupported clone unexpectedly compiled"
    );
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(rendered.contains("bad.scala:2:"), "{rendered}");
    assert!(!rendered.contains("good.scala:2:"), "{rendered}");
    assert!(
        !has_class_files(&out),
        "backend error published class files"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn super_accessor_uses_the_selected_target_across_source_units() {
    let base = fixtures_dir().join("codegen_diag_super_base.scala");
    let layer = fixtures_dir().join("codegen_diag_super_layer.scala");
    let main = fixtures_dir().join("codegen_diag_super_main.scala");
    let out = tmp_dir("super");
    let output = compile(&[&base, &layer, &main], &out);
    assert!(
        output.status.success(),
        "valid super helper failed to compile: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        has_class_files(&out),
        "successful compile produced no classfiles"
    );

    let cp = out.to_str().expect("output path");
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("run generated super helper");
    assert!(
        run.status.success(),
        "generated super helper failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "base\n");
    let _ = fs::remove_dir_all(out);
}

#[test]
fn super_accessor_metadata_supports_a_scalac_consumer() {
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip scalac consumer: Scala 2.13.16 tools are unavailable");
        return;
    };
    let dir = tmp_dir("super-consumer");
    let base = fixtures_dir().join("codegen_diag_super_base.scala");
    let layer = fixtures_dir().join("codegen_diag_super_layer.scala");
    let provider = dir.join("provider");
    let output = compile(&[&base, &layer], &provider);
    assert!(
        output.status.success(),
        "provider failed to compile: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let client = dir.join("Client.scala");
    fs::write(
        &client,
        "class Client extends Layer\nobject ClientMain { def main(args: Array[String]): Unit = println(new Client().helper) }\n",
    )
    .expect("write scalac consumer");
    let consumer = dir.join("consumer");
    fs::create_dir_all(&consumer).expect("create consumer directory");
    let cp = format!("{}:{}", provider.display(), jar.display());
    let scalac_output = Command::new(&scalac)
        .args(["-cp", &cp, "-d", consumer.to_str().expect("consumer path")])
        .arg(&client)
        .output()
        .expect("run scalac consumer");
    assert!(
        scalac_output.status.success(),
        "scalac consumer failed: {}",
        String::from_utf8_lossy(&scalac_output.stderr)
    );

    let java_cp = format!(
        "{}:{}:{}",
        consumer.display(),
        provider.display(),
        jar.display()
    );
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &java_cp, "ClientMain"])
        .output()
        .expect("run scalac consumer");
    assert!(
        run.status.success(),
        "scalac consumer failed at runtime: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "base\n");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn super_accessors_keep_overload_descriptors_distinct() {
    let source = fixtures_dir().join("codegen_diag_super_overload.scala");
    let out = tmp_dir("super-overload");
    let output = compile(&[&source], &out);
    assert!(
        output.status.success(),
        "overloaded super helper failed to compile: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let cp = out.to_str().expect("output path");
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "OverloadMain"])
        .output()
        .expect("run overloaded super helper");
    assert!(
        run.status.success(),
        "overloaded super helper failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "int\nstring\n");
    let _ = fs::remove_dir_all(out);
}

#[test]
fn super_accessor_skips_a_wrong_overload_in_linearization() {
    let source = fixtures_dir().join("codegen_diag_super_linearization.scala");
    let out = tmp_dir("super-linearization");
    let output = compile(&[&source], &out);
    assert!(
        output.status.success(),
        "linearized overload fixture failed to compile: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let cp = out.to_str().expect("output path");
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "LinearMain"])
        .output()
        .expect("run linearized overload fixture");
    assert!(
        run.status.success(),
        "linearized overload fixture failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let ours = String::from_utf8_lossy(&run.stdout).into_owned();
    assert_eq!(ours, "base-int\n");

    // Keep a scalac 2.13.16 oracle for the exact linearization. The wrong
    // same-name fallback returns `middle-string` (or fails verification),
    // while scalac reaches Base.foo(Int).
    if let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) {
        let reference_out = tmp_dir("super-linearization-scalac");
        let scalac_output = Command::new(&scalac)
            .args([
                source.to_str().expect("source path"),
                "-d",
                reference_out.to_str().expect("reference path"),
            ])
            .output()
            .expect("run scalac linearized overload fixture");
        assert!(
            scalac_output.status.success(),
            "scalac oracle failed: {}",
            String::from_utf8_lossy(&scalac_output.stderr)
        );
        let reference_cp = format!("{}:{}", reference_out.display(), jar.display());
        let reference_run = Command::new("java")
            .args(["-Xverify:all", "-cp", &reference_cp, "LinearMain"])
            .output()
            .expect("run scalac linearized overload fixture");
        assert!(
            reference_run.status.success(),
            "scalac oracle failed at runtime: {}",
            String::from_utf8_lossy(&reference_run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&reference_run.stdout), ours);
        let _ = fs::remove_dir_all(reference_out);
    }
    let _ = fs::remove_dir_all(out);
}

#[test]
fn qualified_and_nested_super_calls_keep_their_own_scope() {
    let source = fixtures_dir().join("codegen_diag_super_scope.scala");
    let out = tmp_dir("super-scope");
    let output = compile(&[&source], &out);
    assert!(
        output.status.success(),
        "qualified/nested super fixture failed to compile: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let cp = out.to_str().expect("output path");
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "ScopeMain"])
        .output()
        .expect("run qualified/nested super fixture");
    assert!(
        run.status.success(),
        "qualified/nested super fixture failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "base\nbase\n");
    let _ = fs::remove_dir_all(out);
}
