//! Cycle detection in type resolution.
//!
//! Before this suite existed, a cyclic type reference did not produce a
//! diagnostic: the compiler recursed until its 512 MB stack ran out and the
//! process aborted, so the run reported `errors=0` and no classfiles — which
//! reads like success (see the `classes=0` note in `docs/scala-corpus.md`).
//! scala/scala's own corpus has eight such programs.
//!
//! The two halves are:
//!
//! * `cyclic::bound_cycles` — nsc's `checkNonCyclic` for bounds. A bound may
//!   name the type it bounds as an *argument* (`trait Ord[A <: Ord[A]]`); it
//!   may not be that type (`A <: A`, `A[X] <: A[X]`, `type X <: Y; type Y <: X`).
//! * `cyclic::value_class_wraps_value_class` — nsc's
//!   `validateDerivedValueClass`. A value class erases to what it wraps, so a
//!   pair that wraps each other has no erasure at all.
//!
//! Every message and every accept/reject decision below was read off real
//! scalac 2.13.16 at `/tmp/scala-2.13.16/bin/scalac`, not guessed. The
//! `accepts` cases matter more than the `rejects` ones: this slice adds
//! rejection rules, and a rule that fires on a legal program is far worse
//! than one that misses.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-cyclic-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

/// Compile one snippet and return the whole diagnostic stream plus whether the
/// compiler exited cleanly. A stack overflow shows up here as a `None`: the
/// process is killed by a signal, so there is no status code to read.
fn compile_source(tag: &str, src: &str) -> (Option<i32>, String) {
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        file.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    match scala_library_jar() {
        Some(jar) => {
            cmd.arg("--scala-library").arg(jar.to_str().unwrap());
        }
        None => {
            cmd.arg("--no-scala-library");
        }
    }
    let output = cmd.output().expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&dir);
    (output.status.code(), text)
}

#[track_caller]
fn rejects(tag: &str, src: &str, needle: &str) {
    let (code, text) = compile_source(tag, src);
    assert_eq!(
        code,
        Some(1),
        "expected {tag} to be rejected with an exit status, got {code:?}: {text}"
    );
    assert!(
        text.contains(needle),
        "expected {needle:?} in the diagnostics for {tag}, got: {text}"
    );
}

#[track_caller]
fn accepts(tag: &str, src: &str) {
    let (code, text) = compile_source(tag, src);
    assert_eq!(
        code,
        Some(0),
        "expected {tag} to compile, got {code:?}: {text}"
    );
    assert!(
        !text.contains("error"),
        "expected no diagnostics for {tag}, got: {text}"
    );
}

// ---------------------------------------------------------------------------
// Rejected: a bound that is the type it bounds.
// ---------------------------------------------------------------------------

#[test]
fn self_bounded_type_parameter_is_cyclic() {
    rejects(
        "selfbound",
        "object T { def f[A <: A](x: A): A = x }",
        "cyclic aliasing or subtyping involving type A",
    );
}

/// scala/scala `neg/t2918`, the two-line program the corpus survey named.
#[test]
fn self_bounded_higher_kinded_parameter_is_cyclic() {
    rejects(
        "hkselfbound",
        "object T { def g[X, A[X] <: A[X]](x: A[X]): A[X] = x }",
        "cyclic aliasing or subtyping involving type A",
    );
}

/// scala/scala `neg/t5093`, the same shape reached through a wildcard.
#[test]
fn self_bounded_higher_kinded_parameter_on_class_is_cyclic() {
    rejects(
        "hkclassbound",
        "class C { def f[D[X] <: D[X]](l: D[Int]): D[Int] = l }",
        "cyclic aliasing or subtyping involving type D",
    );
}

#[test]
fn self_bounded_type_member_is_cyclic() {
    rejects(
        "selfmember",
        "trait C { type T <: T }",
        "cyclic aliasing or subtyping involving type T",
    );
}

#[test]
fn mutually_bounded_type_members_are_cyclic() {
    rejects(
        "mutualmember",
        "trait A { type X <: Y; type Y <: X }",
        "cyclic aliasing or subtyping involving type",
    );
}

#[test]
fn self_bounded_higher_kinded_member_is_cyclic() {
    rejects(
        "hkmember",
        "trait E { type V[X] <: V[X] }",
        "cyclic aliasing or subtyping involving type V",
    );
}

// ---------------------------------------------------------------------------
// Rejected: a value class that wraps another one.
// ---------------------------------------------------------------------------

/// scala/scala `neg/t5878`.
#[test]
fn value_class_wrapping_value_class_is_rejected() {
    rejects(
        "vcpair",
        "class Foo(val x: Bar) extends AnyVal\nclass Bar(val x: Foo) extends AnyVal\n",
        "value class may not wrap another user-defined value class",
    );
}

/// A value class inside a compound counts, whichever side it is on -- both
/// halves checked against real scalac.
#[test]
fn value_class_wrapping_a_compound_containing_one_is_rejected() {
    rejects(
        "vccompound",
        "class VA(val i: Int) extends AnyVal\ntrait Tr\nclass VB(val x: Tr with VA) extends AnyVal\n",
        "value class may not wrap another user-defined value class",
    );
}

/// scala/scala `neg/t10530`'s last line: the parameter is a type parameter,
/// and it is its *bound* that is the value class.
#[test]
fn value_class_wrapping_a_bounded_parameter_is_rejected() {
    rejects(
        "vcbound",
        "class A(val a: Int) extends AnyVal\nclass B[T <: A](val a: T) extends AnyVal\n",
        "value class may not wrap another user-defined value class",
    );
}

// ---------------------------------------------------------------------------
// Accepted. These are the ones that matter: the rules above must not fire.
// ---------------------------------------------------------------------------

/// F-bounded polymorphism. The bound names the parameter, as an *argument*.
#[test]
fn f_bounded_polymorphism_stays_legal() {
    accepts("fbound", "trait Ord[A <: Ord[A]] { def cmp(o: A): Int }");
}

#[test]
fn member_bounded_by_a_class_applied_to_itself_stays_legal() {
    accepts("listbound", "trait B { type X <: List[X] }");
}

#[test]
fn member_bounded_through_an_enclosing_this_stays_legal() {
    accepts(
        "outerthis",
        "trait Outer { type T; trait Inner { type T <: Outer.this.T } }",
    );
}

/// The same shape with a differently named member: `Outer.this.T` has to
/// resolve to the *outer* member even when nothing shadows it.
#[test]
fn enclosing_this_type_member_resolves_to_the_outer_member() {
    accepts(
        "outerthis2",
        "trait Outer { type T; trait Inner2 { type U <: Outer.this.T } }",
    );
}

/// scala/scala `pos/contrib701`, the whole file. An *applied* self-reference
/// is a cycle in an upper bound and not in a lower one; reading the two the
/// same way turned this passing test into a failure, which is how the
/// asymmetry was found.
#[test]
fn applied_self_reference_in_a_lower_bound_stays_legal() {
    accepts("contrib701", "trait B { type A[T] >: A[A[T]] }");
}

/// The bare lower-bound self-reference, which scalac *does* reject — with its
/// other message.
#[test]
fn bare_self_reference_in_a_lower_bound_is_cyclic() {
    rejects(
        "barelower",
        "trait B { type A >: A }",
        "illegal cyclic reference involving type A",
    );
}

#[test]
fn mutually_lower_bounded_members_are_cyclic() {
    rejects(
        "mutuallower",
        "trait B { type X >: Y; type Y >: X }",
        "illegal cyclic reference involving type",
    );
}

#[test]
fn value_classes_over_ordinary_types_stay_legal() {
    accepts(
        "vcok",
        "class VI(val i: Int) extends AnyVal\n\
         class VS(val s: String) extends AnyVal\n\
         class VL(val l: List[Int]) extends AnyVal\n\
         class VG[A](val a: A) extends AnyVal\n\
         trait Tr\n\
         class VTr(val t: Tr) extends AnyVal\n\
         class VD(val x: Tr with Int) extends AnyVal\n",
    );
}

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

#[track_caller]
fn fixture_rejects(name: &str, needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    match scala_library_jar() {
        Some(jar) => {
            cmd.arg("--scala-library").arg(jar.to_str().unwrap());
        }
        None => {
            cmd.arg("--no-scala-library");
        }
    }
    let output = cmd.output().expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail, got: {text}"
    );
    assert!(
        text.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got: {text}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixture_cyclic_bounds_bad() {
    fixture_rejects(
        "cyclic_bounds_bad",
        "cyclic aliasing or subtyping involving type A",
    );
}

#[test]
fn fixture_cyclic_hk_bad() {
    fixture_rejects(
        "cyclic_hk_bad",
        "cyclic aliasing or subtyping involving type A",
    );
}

#[test]
fn fixture_cyclic_member_bad() {
    fixture_rejects(
        "cyclic_member_bad",
        "cyclic aliasing or subtyping involving type",
    );
}

#[test]
fn fixture_cyclic_valueclass_bad() {
    fixture_rejects(
        "cyclic_valueclass_bad",
        "value class may not wrap another user-defined value class",
    );
}

/// The positive fixture, run against the real scala-library and compared with
/// `tests/fixtures/expected/cyclic_ok.txt` (which is what scalac 2.13.16
/// prints for the same source).
#[test]
fn fixture_cyclic_ok_runs() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip fixture_cyclic_ok_runs: scala-library jar not available");
        return;
    };
    let src = fixtures_dir().join("cyclic_ok.scala");
    let out = tmp_dir("ok");
    let status = Command::new(bin())
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
        status.status.success(),
        "cyclic_ok.scala should compile: {}{}",
        String::from_utf8_lossy(&status.stderr),
        String::from_utf8_lossy(&status.stdout)
    );
    let run = Command::new("java")
        .arg("-cp")
        .arg(format!("{}:{}", out.display(), jar.display()))
        .arg("Main")
        .output()
        .expect("run java");
    assert!(
        run.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let expected = fs::read_to_string(fixtures_dir().join("expected/cyclic_ok.txt")).unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = fs::remove_dir_all(&out);
}
