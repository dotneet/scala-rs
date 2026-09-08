//! E2E tests for the `agent/sortedmap` slice: a sorted collection keeps its
//! ordering through `map` / `flatMap` / `collect`.
//!
//! `SortedMapOps` overloads all three with an `(implicit Ordering[K2])` clause
//! returning the *sorted* collection, next to the `MapOps` ones it inherits.
//! The two have the same *explicit* parameters, so `PickleSupply::install`
//! keeps only one of them, and nsc's `isAsSpecific` looks through an implicit
//! clause, so specificity does not separate them either -- only their owners
//! do, and the one kept was whichever the pickle walk offered first, which is
//! `MapOps`. `PickleSupply` now lets a declaration that *adds an implicit
//! clause* to one it inherits supersede it: that clause is the `Ordering`
//! witness, and it is the only way the result can be sorted.
//!
//! Nothing diagnosed the old answer: `aSortedMap.map(f)` compiled and returned
//! an unordered `Map`. So `sm_ordering.scala` **runs**, under a *reversed*
//! `Ordering[Int]`, and prints iteration order at every step; the expected file
//! is what real scalac 2.13.16 prints for the same source, and
//! `real_scalac_dual_run_sm_ordering` pins it to scalac's own run.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `sm` prefix.

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
        "scala-rs-sortedmap-{tag}-{}-{nanos}-{seq}",
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
    if cached.is_file() {
        return Some(cached);
    }
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// `-Xverify:all`: which overload the call site takes decides the owner and
/// descriptor codegen writes, so a wrong pick shows up here rather than as a
/// silent difference.
fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_fails(name: &str, extra: &[&str], needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-bad"));
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} (extra={extra:?}) to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {name} error to contain {needle:?}, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The regression itself, run rather than merely compiled: the ordering is
/// reversed, so a result built through `MapOps` cannot print these lines.
#[test]
fn fixtures_sm_ordering() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("sm_ordering", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("sm_ordering"),
        "stdout mismatch for library dual-run sm_ordering"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through **real scalac**, so the expected output is pinned
/// to what Scala 2.13.16 prints and not just to our own.
#[test]
fn real_scalac_dual_run_sm_ordering() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip real-scalac dual-run: jar or scalac not obtainable");
        return;
    };
    let dir = tmp_dir("sm_ordering-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("sm_ordering.scala"))
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac rejected sm_ordering:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run_java(&dir, Some(jar.to_str().unwrap())),
        expected_stdout("sm_ordering"),
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The private runtime has no `scala.collection` pickle, so there is nothing
/// to complete and no overload to order. It must say so rather than quietly
/// accept the program.
#[test]
fn sm_ordering_without_the_library_is_diagnosed() {
    compile_fails(
        "sm_ordering",
        &["--no-scala-library"],
        "value SortedMap is not a member of package scala.collection.immutable",
    );
}

/// The restriction: the sorted overload wins because it is more derived, not
/// unconditionally. With no `Ordering[Key]` its implicit clause cannot be
/// filled and the program is an error -- at scalac's own lines 24 and 26.
#[test]
fn fixtures_sm_ordering_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    compile_fails(
        "sm_ordering_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "could not find implicit value of type Ordering[Key]",
    );
}

/// scalac rejects the `_bad` fixture too, and on the same two lines.
#[test]
fn real_scalac_rejects_sm_ordering_bad() {
    let Some(scalac) = scalac() else {
        eprintln!("skip: scalac not obtainable");
        return;
    };
    let dir = tmp_dir("sm_ordering_bad-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("sm_ordering_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(!out.status.success(), "scalac accepted sm_ordering_bad");
    let err = String::from_utf8_lossy(&out.stderr);
    for line in ["sm_ordering_bad.scala:24", "sm_ordering_bad.scala:26"] {
        assert!(err.contains(line), "scalac did not reject {line}: {err}");
    }
    assert!(
        err.contains("No implicit Ordering[Key] found"),
        "unexpected scalac error: {err}"
    );
    let _ = fs::remove_dir_all(&dir);
}
