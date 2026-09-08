//! `agent/lowerbound`: `[B >: A]` on a member whose only clause is implicit.
//!
//! `def sorted[B >: A](implicit ord: Ordering[B]): C` and its family (`max`,
//! `min`, `sum`, `product`). The lower bound does not *fix* `B`: an argument
//! supplied explicitly for the implicit clause determines it, and only when
//! the argument comes from implicit search does the bound decide. cats writes
//! the first form six times -- `create(toLazyList.sorted(AA.toOrdering))`
//! with `AA >: A` a parameter of the enclosing method -- and every one of
//! them was rejected on an unmodified build of the branch point with
//! `type mismatch; found: Ordering[AA] required: Ordering[A]`.
//!
//! There were two places the parameter was being thrown away, and they had to
//! be repaired together:
//!
//! * `pickle_supply::pin_undetermined_tparams` substituted `B := A` and
//!   dropped the parameter for *every* member whose type parameter no
//!   explicit parameter named. That is right for
//!   `Resource#allocated[B >: A](implicit F: MonadCancel[F, Throwable]):
//!   F[(B, F[Unit])]`, where nothing at the call site mentions `B` at all,
//!   and wrong for `sorted`, where the implicit clause does.
//! * `prelude_seq` declares `List`'s own `sorted` / `min` / `max` / `sum` /
//!   `product` by hand, and wrote the evidence as `Ordering[A]` with no type
//!   parameter at all. Every other collection reaches these through the
//!   pickle; `List` is the one class whose declaration is ours.
//!
//! Keeping the parameter then needs the *other* half of nsc's rule:
//! `adaptToImplicitMethod` runs `inferExprInstance` with
//! `keepNothings = false` before the witness search, so `xs.sorted` with no
//! argument is `B := String` and finds `Ordering.String`. Without it
//! `List("b", "a").sorted` searched for a bare `Ordering[B]` and found
//! nothing, and `List("a").sum` was left standing as an unapplied method
//! type -- accepted with no diagnostic, where scalac reports the missing
//! `Numeric[String]`. That instantiation must *not* run while the reference
//! is still the callee of an `Apply` or `TypeApply`: `c.sorted[AA]` says what
//! `B` is one line later.
//!
//! Fixture prefix: `sortedlb_`, plus `sortedlb.scala`.

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

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// Unique per call: these tests run concurrently and several compile the same
/// fixture, so a shared output directory would let one test's cleanup delete
/// another's class files.
fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-sortedlb-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Real scalac 2.13.16, when this machine has the checkout the measurement
/// scripts install. Every test that needs it skips loudly without it.
fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

struct Run {
    ok: bool,
    text: String,
}

fn out_of(output: std::process::Output) -> Run {
    Run {
        ok: output.status.success(),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn compile_rs(src: &Path, out: &Path, jar: &Path) -> Run {
    let mut cmd = Command::new(bin());
    cmd.arg("compile")
        .arg(src)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()]);
    out_of(cmd.output().expect("run scala-rs compile"))
}

fn compile_scalac(sc: &Path, src: &Path, out: &Path, jar: &Path) -> Run {
    let mut cmd = Command::new(sc);
    cmd.args([
        "-classpath",
        jar.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ])
    .arg(src);
    out_of(cmd.output().expect("run scalac"))
}

/// `-Xverify:all`, so a wrong inferred element type is a `VerifyError` rather
/// than a silent pass.
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

fn ok(run: Run, what: &str) {
    assert!(run.ok, "expected {what} to compile:\n{}", run.text);
}

fn rejected(run: Run, what: &str, needles: &[&str]) {
    assert!(
        !run.ok,
        "expected {what} to be rejected, but it compiled:\n{}",
        run.text
    );
    for n in needles {
        assert!(
            run.text.contains(n),
            "expected {what}'s rejection to mention {n:?}:\n{}",
            run.text
        );
    }
}

// ---------------------------------------------------------------------------
// The positive fixture, executed.

/// Every direction the parameter can be settled from, run: an explicit
/// argument that widens (`dogs.sorted(ByName.toOrdering)` on a `List[Dog]`
/// with an `Ordering[Animal]`), the same through a wrapper's own `[AA >: A]`
/// the way cats writes it, an explicit type application, and implicit search
/// with and without an expected type.
///
/// On an unmodified build of the branch point this fixture does not compile
/// at all: the first six lines of `main` are the `found: Ordering[AA]
/// required: Ordering[A]` shape.
#[test]
fn sortedlb_runs() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip sortedlb_runs: jar or java not present");
        return;
    };
    let dir = tmp_dir("run");
    ok(compile_rs(&fixture("sortedlb"), &dir, &jar), "sortedlb");
    assert_eq!(run_java(&dir, &jar), expected_stdout("sortedlb"));
    let _ = fs::remove_dir_all(&dir);
}

/// The expected file is scalac's, checked by producing it again here. Without
/// this, `expected/sortedlb.txt` is only what this compiler happened to print
/// the day it was written.
#[test]
fn sortedlb_expected_output_is_scalacs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip sortedlb_expected_output_is_scalacs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("scalac");
    ok(
        compile_scalac(&sc, &fixture("sortedlb"), &dir, &jar),
        "sortedlb under scalac",
    );
    assert_eq!(run_java(&dir, &jar), expected_stdout("sortedlb"));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Negatives. Each is paired with the same file under real scalac, so the
// sentence being asserted is nsc's and not ours.

/// An argument that forces a `B` violating `B >: A` is still rejected, and at
/// the join nsc reports rather than at the bound: `Ordering.String` against a
/// `List[Int]` is `required: Ordering[Any]`, not `Ordering[Int]`.
#[test]
fn sortedlb_argument_violating_the_bound_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip sortedlb_argument_violating_the_bound_is_rejected: jar not present");
        return;
    };
    let dir = tmp_dir("bad");
    rejected(
        compile_rs(&fixture("sortedlb_bad"), &dir, &jar),
        "an Ordering whose element type is not a supertype of the receiver's",
        &["found: Ordering[String]", "required: Ordering[Any]"],
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sortedlb_bad_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip sortedlb_bad_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("badsc");
    rejected(
        compile_scalac(&sc, &fixture("sortedlb_bad"), &dir, &jar),
        "sortedlb_bad under scalac",
        &["required: Ordering[Any]"],
    );
    let _ = fs::remove_dir_all(&dir);
}

/// With no explicit argument the parameter *is* pinned at its lower bound, so
/// a missing witness for that bound is an error. The branch point accepted
/// this file: with the parameter dropped the receiver's own element type was
/// substituted in, and once it was kept but never instantiated the value was
/// left holding an unapplied method type. Both ways round, no diagnostic.
#[test]
fn sortedlb_missing_witness_at_the_bound_is_reported() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip sortedlb_missing_witness_at_the_bound_is_reported: jar not present");
        return;
    };
    let dir = tmp_dir("bad2");
    rejected(
        compile_rs(&fixture("sortedlb_bad2"), &dir, &jar),
        "`sum` on a List[String]",
        &["Numeric[String]"],
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sortedlb_bad2_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip sortedlb_bad2_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("bad2sc");
    rejected(
        compile_scalac(&sc, &fixture("sortedlb_bad2"), &dir, &jar),
        "sortedlb_bad2 under scalac",
        &["Numeric[String]"],
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The neighbouring pair that separates nsc's rule from one that merely fits
/// the cats call: `c.sorted[AA]` is taken, `c.sorted` is not. The bound is
/// what the second one is solved at, and the enclosing method's own witness
/// is for `AA`, not for `A`.
#[test]
fn sortedlb_explicit_type_argument_is_taken_and_its_absence_is_not() {
    let Some(jar) = scala_library_jar() else {
        eprintln!(
            "skip sortedlb_explicit_type_argument_is_taken_and_its_absence_is_not: jar not present"
        );
        return;
    };
    let dir = tmp_dir("bad3");
    let run = compile_rs(&fixture("sortedlb_bad3"), &dir, &jar);
    rejected(
        run,
        "`c.sorted` with the type argument left out",
        &["Order[A]"],
    );
    // Exactly one error: the `ok` method above it, which writes `[AA]`, must
    // compile.
    let again = compile_rs(&fixture("sortedlb_bad3"), &dir, &jar);
    assert_eq!(
        again.text.matches("error:").count(),
        1,
        "only the `bad` method should be rejected:\n{}",
        again.text
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sortedlb_bad3_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip sortedlb_bad3_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("bad3sc");
    let run = compile_scalac(&sc, &fixture("sortedlb_bad3"), &dir, &jar);
    rejected(
        run,
        "sortedlb_bad3 under scalac",
        &["could not find implicit value for parameter ord: Order[A]"],
    );
    let _ = fs::remove_dir_all(&dir);
}
