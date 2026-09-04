//! E2E tests for the `agent/kernel` slice: ten things typelevel/cats' `kernel`
//! module writes that this compiler rejected. `CATS_MODULES=kernel
//! tests/cats_measure.sh` went from 84 errors in 23 of 95 files to 18 in 9.
//!
//! Every one of them is plain Scala 2.13 -- cats-kernel names no
//! kind-projector construct anywhere -- so each test has a twin that runs the
//! same fixture through real scalac 2.13.16 and compares the output.
//!
//! See `docs/cats.md` for the roots and for what is still reported.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `k1` prefix.

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
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-kernel-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn run_java(out: &Path, cp_extra: &str, main: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `k1_kernel.scala` names `scala.concurrent.duration`, `immutable.BitSet` and
/// `scala.math.BigDecimal`, none of which the private runtime supplies, so it
/// is `--scala-library` only.
#[test]
fn fixtures_k1_kernel() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = tmp_dir("kernel");
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("k1_kernel.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile k1_kernel failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(run_java(&out, jar_s, "Main"), expected_stdout("k1_kernel"));
    let _ = fs::remove_dir_all(&out);
}

/// The expected output is nsc's, not this compiler's idea of it.
#[test]
fn scalac_agrees_k1_kernel_output() {
    let (Some(sc), true) = (scalac(), java_available()) else {
        eprintln!("skip: scalac or java not available");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let out = tmp_dir("scalac-kernel");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("k1_kernel.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected k1_kernel:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str().unwrap(), "Main"),
        expected_stdout("k1_kernel")
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_bad() -> String {
    let jar = scala_library_jar().expect("checked by the caller");
    let out = tmp_dir("bad");
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("k1_kernel_bad.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "k1_kernel_bad was accepted:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    err
}

/// Widening a bound is only right if it stops where nsc stops.
#[test]
fn k1_kernel_bad_is_rejected() {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    let err = compile_bad();
    assert!(
        err.contains("Annotation needs to be a trait to be mixed in"),
        "`Annotation` is a class and cannot be a second parent:\n{err}"
    );
    assert!(
        err.contains("found: Int") && err.contains("required: String"),
        "`Tuple1[Int]#_1` is an `Int`:\n{err}"
    );
    assert!(
        err.contains("found: BitSet"),
        "`BitSet` is a `Set[Int]`, not a `Set[String]`:\n{err}"
    );
}

/// Real scalac 2.13.16 rejects the same file, so the three diagnostics above
/// are nsc's answer and not this compiler's invention.
#[test]
fn scalac_agrees_k1_kernel_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("k1_kernel_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(
        !output.status.success(),
        "scalac accepted k1_kernel_bad:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
}

/// nsc emits `class a extends Annotation with StaticAnnotation` -- and
/// `class a extends StaticAnnotation` alone -- as `extends Annotation
/// implements StaticAnnotation`. Checked here because the interface/superclass
/// split is exactly what making `StaticAnnotation` a trait changed.
#[test]
fn k1_annotation_classes_have_nscs_shape() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let out = tmp_dir("annot");
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("k1_kernel.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(output.status.success());
    let javap = Command::new("javap")
        .args(["-p", "-classpath", out.to_str().unwrap()])
        .args(["Main$marker", "Main$staticOnly"])
        .output()
        .expect("javap");
    let text = String::from_utf8_lossy(&javap.stdout).into_owned();
    for name in ["Main$marker", "Main$staticOnly"] {
        assert!(
            text.contains(&format!(
                "class {name} extends scala.annotation.Annotation implements scala.annotation.StaticAnnotation"
            )),
            "{name} does not have nsc's shape:\n{text}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
