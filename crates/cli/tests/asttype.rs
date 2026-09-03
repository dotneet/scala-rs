//! E2E tests for the `agent/asttype` slice: slick's `ast/Type.scala` and
//! `compiler/RewriteJoins.scala`.
//!
//! Five roots:
//!
//! * `Array` was not counted as a type constructor (`SymbolTable::kind_arity`
//!   read `scala.Array`'s symbol, which carries no type parameter because
//!   source `Array[T]` becomes `Type::Array`), so
//!   `TypedCollectionTypeConstructor[Array]` was a kind error, and the
//!   `Class { array_sym, [T] }` spelling that substitution then produces had
//!   to be made interchangeable with `Type::Array` in subtyping and erasure,
//! * a wildcard type argument was checked against the parameter's kind even
//!   in a type *pattern*, where nsc lets it stand for a type constructor
//!   (`case o: TypedCollectionTypeConstructor[?]`),
//! * `@tailrec` counted only `Apply` nodes, so a *parameterless* recursive
//!   call -- a bare `Select`, as in `NominalType.sourceNominalType` -- was
//!   "no recursive calls",
//! * `Ordering.ordered` was in no implicit scope (a companion object's
//!   *inherited* implicits were never supplied from the pickle) and
//!   `Predef.$conforms` was in none either (it is added to `Predef` after the
//!   base scope has already imported its members), which together are the
//!   only route to an `Ordering[Null]`,
//! * a Scala class file's mixin forwarders (no `Signature` attribute, so all
//!   that survives is the erasure) hid the properly typed declarations their
//!   parents pickle: `immutable.HashMap#filter` read as `(Any) => Any` made
//!   `foundRefs.filter(_._2._2.isEmpty)` report `value _2 is not a member of
//!   Any`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with other
//! agents; see `.agent-brief.md`. All fixtures use the `at` prefix.

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
        "scala-rs-asttype-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
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
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

/// `-Xverify:all`: the bridge `sizeOf(Object)I` emitted for `sizeOf(c: C[Int])`
/// at `C = Array` needs a `checkcast [I`, and without one this is a
/// `VerifyError` rather than a silent difference in the output.
fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(e) => format!("{}:{}", out.display(), e),
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

/// Everything in `at.scala` -- `@tailrec`, `Ordering`, `<:<`,
/// `immutable.HashMap` -- comes from the real scala-library.
#[test]
fn fixtures_at_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run at: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("at", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("at"),
        "stdout mismatch for library-ABI at"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded
/// expectation, scalac's stdout and ours all have to agree.
#[test]
fn real_scalac_dual_run_at() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff at: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("at.scala");
    let ref_out = tmp_dir("at-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile at");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout("at"),
        "recorded expectation for at does not match real scalac"
    );
    let out = compile_fixture_with("at", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        reference,
        "stdout differs from real scalac for at"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// `scala.annotation.tailrec` has no private-runtime backing: the fixture has
/// to be diagnosed there, not quietly accepted.
#[test]
fn fixtures_at_without_library_is_error() {
    compile_fails(
        "at",
        &["--no-scala-library"],
        "value tailrec is not a member of package scala.annotation",
    );
}

/// A proper type is still not a type constructor.
#[test]
fn fixtures_at_bad_kind_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "at_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "kinds of the type arguments (Int) do not conform to the expected kinds of the type parameters of TC",
    );
}

/// The wildcard takes its parameter's kind only inside a type *pattern*;
/// written as an ordinary type it is an existential over a proper type, which
/// nsc rejects too (`_$1 takes no type parameters, expected: 1`).
#[test]
fn fixtures_at_bad_wildcard_outside_pattern_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "at_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "kinds of the type arguments (_) do not conform to the expected kinds of the type parameters of TC",
    );
}

/// Counting a bare `Select` as a recursive call does not make every `@tailrec`
/// legal: `def loop: Int = loop + 1` is still not in tail position.
#[test]
fn fixtures_at_bad_tailrec_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "at_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "could not optimize @tailrec annotated method: it contains a recursive call not in tail position",
    );
}
