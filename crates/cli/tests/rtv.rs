//! Programs that compiled but crashed at run time (scala/scala `run` tests
//! that failed with `VerifyError`, `ClassCastException`,
//! `NotSerializableException`, … under the real scala-library).
//!
//! Each fixture collects one family of miscompilations; the comments in the
//! fixtures name the corpus tests they came from. Every fixture is run twice:
//! compiled by scala-rs, and compiled by scalac 2.13.16, against the same
//! expected output, with `-Xverify:all`.
//!
//! Fixture prefix: `rtv_`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-rtv-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `name` with scala-rs (`scalac_path` `None`) or scalac, run it,
/// and compare with the expected output.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-nowarn",
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    };
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

fn check_scalac(name: &str) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs(name, Some(&sc));
}

#[test]
fn rtv_unbox_null_runs() {
    check_runs("rtv_unbox_null", None);
}

#[test]
fn scalac_agrees_rtv_unbox_null() {
    check_scalac("rtv_unbox_null");
}

#[test]
fn rtv_calls_runs() {
    check_runs("rtv_calls", None);
}

#[test]
fn scalac_agrees_rtv_calls() {
    check_scalac("rtv_calls");
}

#[test]
fn rtv_ctor_runs() {
    check_runs("rtv_ctor", None);
}

#[test]
fn scalac_agrees_rtv_ctor() {
    check_scalac("rtv_ctor");
}

#[test]
fn rtv_serial_runs() {
    check_runs("rtv_serial", None);
}

#[test]
fn scalac_agrees_rtv_serial() {
    check_scalac("rtv_serial");
}

#[test]
fn rtv_shapes_runs() {
    check_runs("rtv_shapes", None);
}

#[test]
fn scalac_agrees_rtv_shapes() {
    check_scalac("rtv_shapes");
}

/// A class with more than one bootstrap-argument group of serializable
/// lambdas (nsc splits them at 251) reads each one back through the
/// `IllegalArgumentException` chain in `$deserializeLambda$` (`run/t10232`).
#[test]
fn rtv_many_lambdas_deserialize() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("many");
    let src = dir.join("Many.scala");
    let mut s =
        String::from("import java.io._\nobject Main {\n  val fs: List[Int => Int] = List(\n");
    for i in 0..300 {
        s.push_str(&format!("    (x: Int) => x + {i},\n"));
    }
    s.push_str(
        "  )\n  def rt[A](a: A): A = {\n    val bos = new ByteArrayOutputStream\n    \
         val o = new ObjectOutputStream(bos); o.writeObject(a); o.close()\n    \
         new ObjectInputStream(new ByteArrayInputStream(bos.toByteArray)).readObject().asInstanceOf[A]\n  }\n  \
         def main(args: Array[String]): Unit = println(fs.map(f => rt(f)(1)).sum)\n}\n",
    );
    fs::write(&src, s).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // sum over i of (1 + i) for i in 0..300
    assert_eq!(run_java(&out, &jar), "45150\n");
    let _ = fs::remove_dir_all(&dir);
}
