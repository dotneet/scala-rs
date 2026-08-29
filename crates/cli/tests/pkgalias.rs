//! Type aliases declared by a **jar package object**.
//!
//! `scala/package$.class` carries no member for `type NoSuchElementException =
//! java.util.NoSuchElementException`: scalac writes package-object aliases only
//! into the `ScalaSignature` pickle. Reading the classfile therefore leaves
//! them out, and every mention of one is a "not found". These tests pin the
//! pickle-driven path that supplies them.

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
    let p = std::env::temp_dir().join(format!(
        "scala-rs-pkgalias-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Compile `name.scala` with the given extra flags, returning the output dir.
fn compile(name: &str, out: &Path, extra: &[&str]) -> std::process::Output {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    cmd.output().expect("run scala-rs compile")
}

fn diagnostics(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stderr),
        String::from_utf8_lossy(&o.stdout)
    )
}

/// `pkgalias.scala` uses only names the `scala` package object declares as
/// aliases, and must run against the real jar with scalac's own output.
#[test]
fn scala_library_dual_run_pkgalias() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip pkgalias dual-run: jar not obtainable");
        return;
    };
    let out = tmp_dir("pkgalias");
    let o = compile(
        "pkgalias",
        &out,
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(
        o.status.success(),
        "compile pkgalias failed: {}",
        diagnostics(&o)
    );
    assert!(out.join("Main.class").is_file());
    assert!(out.join("Main$.class").is_file());
    let cp = format!("{}:{}", out.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all failed for pkgalias: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected_stdout("pkgalias"),
        "stdout mismatch for library dual-run pkgalias"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A name the package object does not declare stays an error: the pickle
/// supplies the aliases that are there, not ones that are not.
#[test]
fn fixtures_pkgalias_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip pkgalias_bad: jar not obtainable");
        return;
    };
    let out = tmp_dir("pkgalias-bad");
    let o = compile(
        "pkgalias_bad",
        &out,
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!o.status.success(), "expected pkgalias_bad to fail");
    let err = diagnostics(&o);
    assert!(
        err.contains("not found: type NoSuchAliasHere"),
        "expected the missing alias to be diagnosed, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime has no `scala/package$` pickle, so it cannot supply
/// these aliases. It must say so rather than quietly accept the name.
#[test]
fn pkgalias_without_library_is_diagnosed() {
    let out = tmp_dir("pkgalias-norl");
    let o = compile("pkgalias", &out, &["--no-scala-library"]);
    assert!(
        !o.status.success(),
        "expected pkgalias to fail without the library jar"
    );
    let err = diagnostics(&o);
    assert!(
        err.contains("not found: value NoSuchElementException"),
        "expected a diagnostic for the unsupplied alias, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}
