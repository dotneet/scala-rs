//! Regression tests for the `agent/final1` slice. Collects the roots of 6 of the 7
//! slick errors "around collection arguments".
//!
//! * The self alias in `class C { self => … self(i) … }` is `C.this.type`. Only the
//!   `Select` side widened that to the class; the application side
//!   (`resolve_overload`) stopped at `_ => None` and reported
//!   `value apply is not a member of C.this.type`
//!   (slick `util/ConstArray.scala:276`).
//! * Even where there is no expected type, the undetermined type variables of an
//!   expression with nothing left but an implicit clause are settled first, as in
//!   nsc's `adaptToImplicitMethod`. Those with a lower bound become that bound (the
//!   ones that would become `Nothing` stay open), so `toArray[R >: T : ClassTag]`
//!   becomes `Array[String]` and the two overloads of
//!   `withPreparedInsertStatement` can be told apart
//!   (slick `jdbc/JdbcActionComponent.scala:725`).
//! * `typing_call_args` ("typing an argument") is a flag on the typer, not on the
//!   expression, so lazy signature completion running mid-argument inherited it. The
//!   *inferred result type* of a forward-referenced `def … = ….map(…).flatten` thus
//!   came out as the unapplied method type `((Option[X]) => IterableOnce[B])Seq[B]`
//!   (slick `jdbc/JdbcModelBuilder.scala:159`; line 93 is the cascade from this).
//! * When two arguments contribute to the same type parameter, and when joining with
//!   a declared lower bound, an argument's own undetermined variables are lowered to
//!   their lower bound first. `m.getOrElse(k, Seq.empty)` was coming out as
//!   `Seq[AnyRef]` (slick `compiler/MergeToComprehensions.scala:218`).
//! * A non-case class matches through the extractor rather than as a constructor
//!   pattern when its companion has one (SLS 8.1.6/8.1.7). `ConstArray(disc, map)`
//!   was binding `Array[Any]` and `Int`
//!   (slick `compiler/ExpandSums.scala:245`).
//! * The undetermined variables the receiver brings in are also subject to the
//!   expected type being stronger than the argument in an *invariant* result
//!   position. `Set() ++ opt` stayed `Set[SqlType]` and the invariant `Set` then
//!   rejected the expected type (half of slick `jdbc/JdbcModelBuilder.scala:279`).
//! * Conversion search's `open_conversion_fit` let `Unify` decide even when neither
//!   side had a variable left to solve. Since a wildcard unifies with anything,
//!   `Option.option2Iterable` claimed to be `Option[Default[_]] =>
//!   IterableOnce[ColumnOption[Nothing]]`, which made the monomorphic
//!   `Set#++(IterableOnce[A]): Set[A]` applicable and collapsed the
//!   `Set() ++ … ++ dflt` chain to `Set[ColumnOption[Nothing]]`
//!   (the other half of that same 279).
//!
//! As the brief asks, the fixture is a single file (a real scalac run costs 1.8 s).
//! It uses `Set` / `Map` / `ClassTag` / `IndexedSeq`, so `--scala-library` mode only.
//! The helpers follow `crates/cli/tests/ovl4.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-final1-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: codegen that picks the wrong overload and so gets the erased
/// descriptor wrong shows up as a `VerifyError`, not as an output difference.
fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
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

fn compile_diagnostics(name: &str) -> Option<String> {
    let jar = scala_library_jar()?;
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
    let _ = fs::remove_dir_all(&out);
    Some(err)
}

#[test]
fn fixtures_final1_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip final1: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("final1", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, jar_s),
        expected_stdout("final1"),
        "stdout mismatch for library-ABI final1"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Run the same fixture through real scalac 2.13.16 and check that the recorded
/// expectation, scalac's output and ours all three agree.
#[test]
fn real_scalac_dual_run_final1() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff final1: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("final1.scala");
    let ref_out = tmp_dir("final1-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile final1");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, jar_s);
    assert_eq!(
        reference,
        expected_stdout("final1"),
        "recorded expectation for final1 does not match real scalac"
    );

    let out = compile_fixture_with("final1", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, jar_s),
        reference,
        "stdout differs from real scalac for final1"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// The far side of what we relaxed. Real scalac 2.13.16 rejects these three too
/// (`Main.NoApply does not take parameters` /
/// `found: Some[String] required: IterableOnce[Int]` /
/// `found: Option[Main.DefaultOpt[_]] required: IterableOnce[Main.ColOpt[Nothing]]`).
#[test]
fn final1_bad_is_still_rejected() {
    let Some(err) = compile_diagnostics("final1_bad") else {
        eprintln!("skip: scala-library jar not available");
        return;
    };
    assert!(
        err.contains("value apply is not a member of NoApply.this.type"),
        "a self alias whose class has no `apply` must still be reported: {err}"
    );
    assert!(
        err.contains("found: Set[String]  required: Set[Int]"),
        "the expected type must not override an argument solution that does not \
         conform to it: {err}"
    );
    assert!(
        err.contains("found: Option[DefaultOpt[_]]  required: IterableOnce[ColOpt[Nothing]]"),
        "`option2Iterable` must not answer a view whose result does not actually \
         conform: {err}"
    );
}
