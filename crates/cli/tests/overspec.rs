//! E2E tests for the `agent/overspec` slice: overload specificity (SLS 6.26.3,
//! nsc's `Infer.isAsSpecific` / `isStrictlyMoreSpecific`) and the shape
//! pre-selection that runs before it (`Typers.preSelectOverloaded`).
//!
//! The defect was `pf.andThen(s => s.length)` -- `ambiguous overload for
//! andThen with arguments ((<notype>) => <notype>)` -- and it turned out to be
//! three separate roots, not one:
//!
//! 1. **Specificity was not strict.** `arg_score` deliberately lets a
//!    one-parameter function type inhabit a `PartialFunction[A, B]` formal, so
//!    that a `{ case … }` literal (which reaches overload resolution as a
//!    plain one-parameter function) can be passed to `collect` / `recover`.
//!    That is the *literal* being adapted, which is `typedFunction`'s job in
//!    nsc; specificity compares two declared signatures with `isCompatible`,
//!    which has no function-to-`PartialFunction` coercion at all
//!    (`PartialFunction` declares two abstract members, so it is not a SAM
//!    type either, and scalac 2.13.16 rejects `f(g)` for a `g: Int => Int`
//!    against a `PartialFunction[Int, Int]` formal). Scoring a match made
//!    `andThen[C](k: B => C)` as specific as `andThen[C](k:
//!    PartialFunction[B, C])` and vice versa, so nothing separated them.
//!    Fixed by gating that one rule on `spec_probe`.
//!
//! 2. **The shape type was only being used for arity.** nsc's
//!    `preSelectOverloaded` throws out alternatives using `shapeType`, and the
//!    shape of `x => e` is `FunctionN[Any, …, Nothing]` while the shape of
//!    `{ case … }` is `PartialFunction[Any, Nothing]`. Only the first rules
//!    out a `PartialFunction` formal, and that is the whole difference between
//!    `pf.andThen(s => …)` (keeps the receiver's domain) and
//!    `pf.andThen { case … }` (composes the two domains) -- a *run-time*
//!    difference in `isDefinedAt`, not a compile error. `narrow_by_lambda_shape`
//!    compared arities only, so this was invisible; the argument trees are now
//!    summarised as `ArgShape`s and passed down to it.
//!
//! 3. **`isInProperSubClassOf` was blind to a function-typed parent.**
//!    `class AndThen[-T, +R] extends (T => R)` (cats `data/AndThen.scala`)
//!    records `Type::Function` as its parent, and both `class_reaches` and
//!    `base_type_instance` stop dead there. So the `override def andThen` /
//!    `compose` it declares and the `Function1` members they override were two
//!    equally specific alternatives with no owner relation to separate them.
//!    `class_has_base` reads such a parent back as `FunctionN`.
//!
//! Measured on `tests/cats_measure.sh`: the 13 `andThen` and 3 `compose`
//! ambiguities are gone. The remaining 4 (`lazyZip`) are a different root --
//! `IterableOps.lazyZip` adopted twice, once under `Seq` and once under
//! `AbstractIterable`, with unrelated owners and identical parameter types, so
//! no specificity rule can separate them and nsc never sees two members at
//! all. Left alone deliberately; it belongs to the member-supply seam.
//!
//! Both positive fixtures *execute* and their expected output is real scalac
//! 2.13.16's, so an alternative we pick that nsc would not is a red test.
//! `Function1.andThen` / `compose` exist only in the real library ABI (the
//! private runtime's `Function1` declares neither), so these are dual-run
//! against the jar and skipped when it is not there.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.

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
        "scala-rs-overspec-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "ovspec.Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java ovspec.Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile against the real library ABI and check the program's *output*.
/// Which overload was picked is only visible at run time here.
fn dual_run_fixture(name: &str) {
    if !java_available() {
        eprintln!("skip {name}: no java");
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let got = run_java(&out, jar_s);
    assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    let _ = fs::remove_dir_all(&out);
}

fn compile_errors(name: &str) -> String {
    let Some(jar) = scala_library_jar() else {
        return String::new();
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-bad"));
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
    err
}

/// `PartialFunction.andThen`'s two alternatives, chosen five different ways.
/// Each `isDefinedAt` in the expected output says which one ran.
#[test]
fn fixtures_ovspec_pf() {
    dual_run_fixture("ovspec_pf");
}

/// `isInProperSubClassOf` through a parent written as a function type, and
/// through one written as `Function1[T, R]`.
#[test]
fn fixtures_ovspec_sub() {
    dual_run_fixture("ovspec_sub");
}

/// A call that is genuinely ambiguous for scalac 2.13.16 stays ambiguous, on
/// the same line scalac reports.
#[test]
fn fixtures_ovspec_pf_bad() {
    if scala_library_jar().is_none() {
        eprintln!("skip ovspec_pf_bad: scala-library jar not obtainable");
        return;
    }
    let err = compile_errors("ovspec_pf_bad");
    assert!(
        err.contains("ambiguous overload for m"),
        "expected an ambiguity for `m`, got:\n{err}"
    );
    assert!(
        err.contains("ovspec_pf_bad.scala:20:"),
        "expected the diagnostic on line 20 (where scalac reports it), got:\n{err}"
    );
}
