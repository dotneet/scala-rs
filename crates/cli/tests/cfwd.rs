//! E2E tests for the `agent/catstail` slice: which *declaration* of a library
//! member a receiver gets, when the member reaches the typer twice.
//!
//! A concrete class carries a mixin forwarder for every default method it
//! inherits from a trait, and scalac writes a `Signature` for it -- but a JVM
//! generic signature cannot write `[B >: A]` and has only one argument list.
//! `scala.collection.AbstractIterable`'s copy of `IterableOnceOps.reduceLeft[B
//! >: A](op: (B, A) => B): B` is therefore `<B> B reduceLeft(Function2<B, A,
//! B>)`, and its copy of `foldLeft[B](z: B)(op: (B, A) => B): B` is
//! `foldLeft(Object, Function2)`. Both copies were installed, overload
//! resolution had two alternatives where nsc has one, and picking the
//! forwarder left `B` with nothing but `Any` to be. `Check::
//! drop_classfile_forwarders` drops a copy that says exactly what the
//! declaration says minus those two losses, and keeps every other class-file
//! member -- `immutable.List`'s own `++(IterableOnce): List[B]`, the erased
//! `Any` key of `immutable.Map#getOrElse`, and `SetOps.++(that:
//! IterableOnce[A]): C`, which is a genuinely different overload.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `cfw` prefix, and both need the
//! real scala-library: `AbstractIterable` is where the forwarders are, and
//! the private runtime does not carry it.

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
        "scala-rs-cfwd-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: a constructor captured with the wrong arguments would
/// erase to the wrong class and show up here, not as a silent difference.
fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
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

fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s), "Main"),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
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
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------- positive: it runs

/// Every line is checked by what it prints, so an instantiation that merely
/// compiles cannot pass: `fold` accumulates a `String` over `Int`s, which
/// needs `B` to be the caller's own `B` and needs the second clause to exist,
/// and `reduce` needs `B >: A` to come back as `A`.
///
/// On the pre-fix binary the fixture does not compile: five errors,
/// `found: (A, A) => A  required: Function2[Any, A, Any]` and
/// `value apply is not a member of B` among them.
#[test]
fn fixtures_cfw_forwarder() {
    dual_run_fixture("cfw_forwarder");
}

/// The fixture is only worth its runtime if real scalac accepts it too -- and
/// prints exactly what `expected/cfw_forwarder.txt` records.
#[test]
fn scalac_agrees_cfw_forwarder_runs() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-cfw-forwarder");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("cfw_forwarder.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected cfw_forwarder.scala: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        expected_stdout("cfw_forwarder"),
        "scalac's own run does not match the recorded output"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------ negative: it still binds

/// Dropping the class file's copy must leave the declaration in force.
/// `bad1` needs `B >: Int`, which the forwarder's bound-less `<B>` would have
/// let through; `bad2` passes both of `foldLeft`'s clauses as one argument
/// list, which is exactly what the forwarder's flattened signature accepted --
/// **the pre-fix binary compiles that line**, and scalac does not.
#[test]
fn fixtures_cfw_forwarder_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not available");
        return;
    };
    compile_fails(
        "cfw_forwarder_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "cfw_forwarder_bad.scala:25",
            "found: (String, Int) => String  required: (Any, Int) => Any",
            "cfw_forwarder_bad.scala:29",
        ],
    );
}

/// The same two, straight from scalac, at the same lines, so the fixture
/// cannot drift into asserting a restriction nsc does not have.
#[test]
fn scalac_agrees_cfw_forwarder_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-cfw-forwarder-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("cfw_forwarder_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(
        !output.status.success(),
        "scalac accepted cfw_forwarder_bad.scala"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "cfw_forwarder_bad.scala:25: error: type mismatch",
        "required: (Any, Int) => Any",
        "cfw_forwarder_bad.scala:29: error: missing argument list for method foldLeft",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac to report {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
