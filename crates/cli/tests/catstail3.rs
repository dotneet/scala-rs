//! E2E tests for the `agent/catstail3` slice: three roots from typelevel/cats'
//! flat tail, each of which turned out to cover several of the reported
//! symptoms.
//!
//! 1. **An inserted `apply` never had its own type parameters solved.**
//!    `cats.Parallel` reaches `FunctionK.apply[A](fa: F[A]): G[A]` through a
//!    parameterless `def sequential: F ~> M`, so the application path fails to
//!    resolve, `insert_apply_on_nullary` rewrites the callee, and the retry
//!    handed the declaration's result back raw. `P.sequential(fta)` was
//!    `M[A]` with `A` still `apply`'s own parameter --
//!    `found: M[A] required: M[T[A]]`, 24 of `Parallel.scala`'s 39 errors,
//!    plus every `NonEmptyParallel.F[A]` mismatch and all of `InjectK.scala`.
//! 2. **A parameterless collection member kept whichever `C` was asked for
//!    first.** `tail` / `init` are declared `C` and `zipWithIndex` / `flatten`
//!    are `CC[B]`; the application path already rebuilds those around the
//!    receiver (`returns_receiver_collection`), but a selection with no
//!    argument list never reached it. Member completion installs an inherited
//!    declaration on the class it was *asked* about, with that class's `C`
//!    substituted in, so once some `aSeq.tail` had put `IterableOps.tail` on
//!    `immutable.Seq` as `Seq[A]`, `aVector.tail` found it by inheritance and
//!    was a `Seq[A]`. Whether the program compiled depended on the order of
//!    two unrelated lines.
//! 3. **The override check assumed two type-parameter-mentioning parameters
//!    were the same type.** cats writes `compose` at every level of its
//!    type-class tower (`Functor`, `Apply`, `Applicative`, `Traverse`,
//!    `Reducible`, `Bitraverse`, ...) with no `override` on any of them,
//!    because the different implicit parameter makes each an *overload*.
//!    `robust` refuses to compare a type mentioning a type parameter, so all
//!    nine reported "`override` modifier required".
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `c3_` prefix.

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
        "scala-rs-catstail3-{tag}-{}-{nanos}-{seq}",
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

fn compile_fails(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail"
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

fn scalac_rejects(name: &str, needles: &[&str]) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir(&format!("scalac-{name}"));
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join(format!("{name}.scala")))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted {name}");
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected scalac's {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// `--scala-library` only: the fixture names `Vector`, `LazyList` and
/// `IterableOps`' inherited members, none of which the private runtime ships.
#[test]
fn fixtures_c3_parallel() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = tmp_dir("parallel");
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("c3_parallel.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile c3_parallel failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar_s, "Main"),
        expected_stdout("c3_parallel")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The expected output is nsc's: scalac 2.13.16 accepts every shape in the
/// fixture, so each one is a gap of ours and not a property of the code.
#[test]
fn scalac_agrees_c3_parallel_output() {
    let (Some(sc), true) = (scalac(), java_available()) else {
        eprintln!("skip: scalac or java not available");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let out = tmp_dir("scalac-parallel");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("c3_parallel.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected c3_parallel:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str().unwrap(), "Main"),
        expected_stdout("c3_parallel")
    );
    let _ = fs::remove_dir_all(&out);
}

/// Solving the inserted `apply`'s type parameters is not "believe the
/// expected type", and the collection rebuild puts the *receiver's* class
/// back, never a narrower one.
#[test]
fn fixtures_c3_parallel_bad_is_rejected() {
    compile_fails(
        "c3_parallel_bad",
        &[
            "found: M[List[X]]  required: M[X]",
            "found: Seq[Int]  required: Vector[Int]",
        ],
    );
}

#[test]
fn scalac_agrees_c3_parallel_bad_is_rejected() {
    scalac_rejects(
        "c3_parallel_bad",
        &["required: P.F[X]", "required: Vector[Int]"],
    );
}

/// Two parameter types that really are the same type are still an override.
/// In its own file because scalac's override check runs after `typer`, and so
/// never sees a unit that already has a type error.
#[test]
fn fixtures_c3_override_bad_is_rejected() {
    compile_fails(
        "c3_override_bad",
        &["`override` modifier required to override concrete member"],
    );
}

#[test]
fn scalac_agrees_c3_override_bad_is_rejected() {
    scalac_rejects(
        "c3_override_bad",
        &["`override` modifier required to override concrete member"],
    );
}
