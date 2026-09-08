//! E2E tests for the `agent/pkgobjdup` slice: one entity supplied twice, by a
//! package and by that package's package object.
//!
//! A package object's members *are* its package's members (SLS 9.3), so two
//! entries under one name are not an overload set. The owners are a package
//! and a package-object class, which stand in no `extends` relation, so no
//! ordering rule in `Check::drop_overridden` can apply and none should: the
//! set is not mis-*reduced*, it should never have had two members. That is why
//! the display repeats a type -- `<overload Nil$ | Nil$>` on every `Nil` in
//! scala/scala's `src/library`, where the prelude's `scala.Nil` stood beside
//! `scala/package.scala`'s `val Nil = scala.collection.immutable.Nil`.
//!
//! nsc's `openPackageModule` resolves it in the package object's favour, in
//! two passes: first every name the package object declares unlinks the
//! package's entry of that name *in that name's own namespace*, then the
//! package object's members are entered. Both halves are load-bearing and both
//! were measured; see `SymbolTable::fold_package_object_members`.
//!
//! That the package object wins, and that the two namespaces are separate, is
//! [`scalac_agrees_pkgobjdup_runs`] rather than an argument: real scalac
//! 2.13.16 compiles the fixture and its output is what
//! `tests/fixtures/expected/pkgobjdup_pkgobject.txt` holds.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. Both fixtures use the `pkgobjdup` prefix and are checked
//! in both modes.

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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-pkgobjdup-{tag}-{}-{nanos}-{seq}",
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
        "compile of {name} (extra={extra:?}) failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
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

/// On the pre-fix binary this fixture does not compile: `value eq is not a
/// member of <overload Payload$ | Payload$>` at the term route and `type
/// mismatch; found: Box  required: Box` at the type route -- the two halves of
/// the shape `Iterable.scala` 215 and `StdIn.scala` 222 reported against the
/// real standard library's own sources.
#[test]
fn fixtures_pkgobjdup_pkgobject_runs() {
    if !java_available() {
        eprintln!("skip fixtures_pkgobjdup_pkgobject_runs: no java");
        return;
    }
    let out = compile_fixture_with("pkgobjdup_pkgobject", &["--no-scala-library"]);
    assert_eq!(run_java(&out, None), expected_stdout("pkgobjdup_pkgobject"));
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_pkgobjdup_pkgobject_runs_against_the_library() {
    let (true, Some(jar)) = (java_available(), scala_library_jar()) else {
        eprintln!("skip fixtures_pkgobjdup_pkgobject_runs_against_the_library: no java or jar");
        return;
    };
    let out = compile_fixture_with(
        "pkgobjdup_pkgobject",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap())),
        expected_stdout("pkgobjdup_pkgobject")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through real scalac 2.13.16, run and compared. The expected
/// output is scalac's answer and not a transcription of ours, including the
/// three lines that say which route won: `inner.Payload` and `true` for the
/// term, `Box(7)` for the type, and `outer's own object Thing` for the object
/// the package object shadowed in the *type* namespace only.
#[test]
fn scalac_agrees_pkgobjdup_runs() {
    let (true, Some(sc), Some(jar)) = (java_available(), scalac(), scala_library_jar()) else {
        eprintln!("skip scalac_agrees_pkgobjdup_runs: scalac, jar or java not available");
        return;
    };
    let theirs_out = tmp_dir("scalac");
    let run = Command::new(sc)
        .args([
            "-classpath",
            jar.to_str().unwrap(),
            "-d",
            theirs_out.to_str().unwrap(),
        ])
        .arg(fixtures_dir().join("pkgobjdup_pkgobject.scala"))
        .output()
        .expect("run scalac");
    assert!(
        run.status.success(),
        "scalac 2.13.16 rejected pkgobjdup_pkgobject: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let theirs = run_java(&theirs_out, Some(jar.to_str().unwrap()));
    let ours_out = compile_fixture_with("pkgobjdup_pkgobject", &["--no-scala-library"]);
    assert_eq!(
        theirs,
        run_java(&ours_out, None),
        "package-object supply differs from scalac 2.13.16"
    );
    assert_eq!(theirs, expected_stdout("pkgobjdup_pkgobject"));
    let _ = fs::remove_dir_all(&theirs_out);
    let _ = fs::remove_dir_all(&ours_out);
}

// ------------------------------------------------- negative: the restrictions

#[test]
fn fixtures_pkgobjdup_pkgobject_bad_is_rejected() {
    for mode in [
        vec!["--no-scala-library"],
        match scala_library_jar() {
            Some(jar) => vec![
                "--scala-library",
                Box::leak(jar.display().to_string().into()),
            ],
            None => vec!["--no-scala-library"],
        },
    ] {
        compile_fails(
            "pkgobjdup_pkgobject_bad",
            &mode,
            &[
                "recursive value Loop needs type",
                "value tag is not a member",
            ],
        );
    }
}

/// Real scalac 2.13.16 rejects the negative fixture too, and for the same two
/// reasons. A rule that made the package's `object Payload` reachable again
/// would compile line 40, and one that entered the package object's `val Loop`
/// beside the object rather than in its place would not see the recursion.
#[test]
fn scalac_rejects_pkgobjdup_bad() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip scalac_rejects_pkgobjdup_bad: scalac or jar not available");
        return;
    };
    let out = tmp_dir("scalac-bad");
    let run = Command::new(sc)
        .args([
            "-classpath",
            jar.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .arg(fixtures_dir().join("pkgobjdup_pkgobject_bad.scala"))
        .output()
        .expect("run scalac");
    let err = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        !run.status.success(),
        "scalac 2.13.16 accepted pkgobjdup_pkgobject_bad"
    );
    assert!(
        err.contains("recursive value Loop needs type"),
        "scalac's first error changed: {err}"
    );
    assert!(
        err.contains("value tag is not a member of object inner.Payload"),
        "scalac's second error changed: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
