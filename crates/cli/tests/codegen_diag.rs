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
