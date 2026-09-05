//! E2E tests for the `agent/monadtrans` slice: the four things that stopped
//! typelevel/cats' monad transformers (`EitherT`, `IorT`, `OptionT`) from
//! type-checking, plus the compound-expected-type gap next to them.
//!
//! 1. **An `if`/`match`-bodied lambda could not decide the type parameter it
//!    was checked against.** An undetermined variable in the result of a
//!    function-typed parameter is opened to `Type::Wildcard`, so
//!    `F.flatMap(value) { case … }` is checked at `X => F[_]` -- and
//!    `pt_or_lub` adopted that `F[_]` as the answer, so the argument that was
//!    supposed to *decide* `flatMap`'s `B` said `B = _`. Every monad
//!    transformer method reported `no matching overload for
//!    (F[Either[A, B]])EitherT[F, A, B] with arguments (F[_])`; the same body
//!    written as a plain lambda type-checked.
//! 2. **Branches that disagree.** `EitherT.orElse` gives `F[Either[C, BB]]`
//!    from one branch and `F[Right[C, BB]]` from the other. nsc's `solve`
//!    takes the lub of a variable's *lower bounds*, not of the two whole
//!    types -- joining the applications cannot get past an abstract `F`'s
//!    invariance, and `SymbolTable::lub` had no arm for `Type::Applied` at
//!    all, so it walked out to `AnyRef`.
//! 3. **A type constructor named only by a compound result type.** cats'
//!    generated `NTupleUnorderedFoldableInstances` calls
//!    `private def instance[F[_]](…): Traverse[F] with Reducible[F]` with
//!    `F` stated nowhere else; `collect_expected` had no `Type::Refined` arm,
//!    so all 22 instances reported `value _1 is not a member of _[Any]` and
//!    `found: Traverse[F] with Reducible[F]`.
//! 4. **An extractor lined up with the scrutinee by position.** cats writes
//!    `final case class Right[+B](b: B) extends (Nothing Ior B)`, whose
//!    synthesized `unapply[B](x: Right[B])` was unified against a scrutinee
//!    `Ior[A, B]` argument-by-argument -- so `case Ior.Right(b)` bound
//!    `b: A`.
//!
//! A fifth, found on the way and in the same file: `Ior.Left(a)` was typed as
//! `scala.util.Left`, because the `Left.apply` / `Right.apply` shortcut keyed
//! off the owner module's *name* and then looked the class up by simple name
//! in scope. `cats/data/Ior.scala` went from 46 errors to 4 on that alone.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `mt_` prefix.

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
        "scala-rs-monadtrans-{tag}-{}-{nanos}-{seq}",
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

/// The fixture is `--scala-library` only: `Either`, `Tuple1` and
/// `Product with Serializable` come from the real library, and the private
/// runtime does not supply them (same as `c4_lang.scala`).
#[test]
fn fixtures_mt_transformer() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = tmp_dir("transformer");
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join("mt_transformer.scala")
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile mt_transformer failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar_s, "Main"),
        expected_stdout("mt_transformer")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The expected output is nsc's, not this compiler's idea of it: scalac
/// 2.13.16 accepts every shape in the fixture, so each one is a gap of ours
/// and not a property of the code.
#[test]
fn scalac_agrees_mt_transformer_output() {
    let (Some(sc), true) = (scalac(), java_available()) else {
        eprintln!("skip: scalac or java not available");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let out = tmp_dir("scalac-transformer");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("mt_transformer.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected mt_transformer:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str().unwrap(), "Main"),
        expected_stdout("mt_transformer")
    );
    let _ = fs::remove_dir_all(&out);
}

/// Filling an undetermined variable from the branches is not "believe the
/// branches": a branch that is not an application of the same constructor
/// decides nothing, an extractor aligned with its scrutinee still binds one
/// definite side, and a one-parameter `Left` in someone else's namespace is
/// not `scala.util.Left`.
#[test]
fn fixtures_mt_transformer_bad_is_rejected() {
    compile_fails(
        "mt_transformer_bad",
        &[
            "type mismatch",
            "found: B  required: A",
            "required: Either[String, Int]",
        ],
    );
}

/// Straight from scalac 2.13.16, so the negative fixture cannot drift into
/// asserting behaviour nsc does not have.
#[test]
fn scalac_agrees_mt_transformer_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-transformer-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("mt_transformer_bad.scala"))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted the bad fixture");
    for needle in [
        "required: F[Either[A,B]]",
        "required: A",
        "required: Either[String,Int]",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac's error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
