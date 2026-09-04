//! E2E tests for the `agent/tail4` slice. Fixture prefix `t4`. Kept out of
//! `crates/cli/tests/e2e.rs` to avoid merge conflicts; see `.agent-brief.md`.
//!
//! # `new C(a)(b)` is one constructor, not a call on the instance
//!
//! `agent/tail3` left a note that a curried `new` "does overload resolution
//! per `Apply` layer" and reported `ambiguous overload` on slick's
//! `slick/lifted/SimpleFunction.scala:74`, `new SimpleLiteral(name)(tpe)`.
//! What was actually wrong was one layer earlier, in the parser: `parse_new`
//! took the parent tree (`Apply(Apply(C, a), b)`), destructured *one* `Apply`
//! layer, and wrapped its `fun` in `New` -- so `New`'s "type" was `C(a)`, an
//! application. Typing that head went through the ordinary expression path
//! and looked up `apply`: with a companion that declares one (`SimpleLiteral`
//! declares `apply[T](name: String)(implicit tpe: TypedType[T])`) the report
//! was `ambiguous overload for apply`; without one it was `no matching
//! overload for constructor apply`. `parse_new` now peels the whole chain and
//! puts `New` on its head, and `Typer::flatten_curried_new` folds the clauses
//! into the single flat argument list that `pick_ctor` (and the JVM) already
//! expect -- the same thing `type_parent_ctor_app_in` has always done for
//! `extends A(1)(2)` -- but only as far as the constructor the *first* list
//! picks has room for. `new Foo(1)(2)` on a one-parameter `Foo` with an
//! `apply` is `(new Foo(1)).apply(2)`, and folding the two lists together
//! would build a two-argument `Foo` instead, silently where the class has one
//! (`Ov` in the fixture).
//!
//! Two further bugs were behind that one, both only reachable once a curried
//! `new` typechecked at all:
//!
//!  * **The picked constructor was read at the class's own type parameters.**
//!    `new TypedCase[B, P](ConstArray(cond, res.toNode))(bType,
//!    om.liftedType(bType))` (`slick/lifted/Case.scala:21`) passes a
//!    `BaseTypedType[B]` where the clause declares `TypedType[B]` -- and
//!    that conformance only holds once the clause is read at the `[B, P]` the
//!    `new` was written with. `pick_ctor_at` has taken those arguments all
//!    along for `extends A(1)(2)`; the `new` path called `pick_ctor`, which
//!    passes none.
//!  * **An explicitly given implicit clause was searched for again.**
//!    Constructor arguments reach `fill_defaults_and_implicits` already
//!    flattened, but the constructor *symbol* still has two clauses, so the
//!    second one looked unfilled and a searched argument was appended after
//!    the one the user wrote. `new K[B]("s")(tb)` typechecked and then died
//!    in the verifier with three arguments for a two-parameter constructor --
//!    a silent miscompilation, not a diagnostic. The fill now runs only when
//!    the call really is short.
//!
//! Fixing the `new` path let `try_rewrite_case_copy_curried` stop going
//! through the companion's `apply`: `copy()(x)` is `new C(…)(x)` in nsc, and
//! the two are only the same method when the companion is synthetic. A
//! companion that declares any `apply` of its own gets no synthetic one
//! emitted, so slick's `SimpleLiteral.rebuild = copy()(buildType)` compiled
//! to a call to a method that is not in the classfile
//! (`NoSuchMethodError: SimpleLiteral$.apply(String, Type)`).
//!
//! Named arguments in a later clause (`new C(1)(c = 3, b = 2)`) fall out of
//! the same flattening: `reorder_named_ctor_args` now looks a name up across
//! every clause of the sole constructor, since that is the shape the
//! arguments arrive in. README's "Not implemented" list had this as
//! unsupported.
//!
//! # `lub` walked past the right class at the wrong arguments
//!
//! `value getOrElse is not a member of Product` (4 occurrences in
//! `slick/jdbc/PositionedResult.scala`) is the symptom `agent/tail1`,
//! `mismatch10`, `mismatch11` and `tail3` all failed to reduce, each
//! recording that it "depends on the state of all 184 slick files". It does
//! not depend on slick at all: it depends on how much of *scala-library* the
//! run happens to have read.
//!
//! `SymbolTable::lub` walked `a`'s base type sequence and returned the first
//! entry `b` conforms to. For `if (rs.wasNull) None else Some(r)` that
//! sequence is `None.type`, `Option[Nothing]`, then whatever `Option`'s own
//! parents are. `Some[Blob] <: Option[Nothing]` is false, so the answer was
//! the next candidate -- and `scala/Option`'s classfile declares `implements
//! scala.Product`, so once anything in the run had made that parent visible,
//! `Product` was already an upper bound and `lub` stopped there. Walking
//! `b`'s sequence, which the function does next, would have found
//! `Option[Blob]`.
//!
//! What the walk passed over was the *right class at the wrong arguments*:
//! the two sequences meet at `Option`, just at `Nothing` on one side and at
//! `Blob` on the other. `lub` now joins the arguments when that happens
//! (through its own same-class arm, which already knew how) and stops there,
//! so the answer is `Option[Blob]` and never depends on how far the library
//! has been read. Ranking candidates by specificity instead was tried and is
//! wrong: `Product` and `Shape` are both minimal for `lub(Circle, Rect)`, and
//! `Product <: Equals` makes `Product` look like the more specific of the two.
//!
//! `t4_lub_bases.scala` writes the shape out in user code -- `case object Nn
//! extends Opt[Nothing]`, `case class Sm[+A](v: A) extends Opt[A]`,
//! `Opt[+A] extends Marker` -- so it needs no library state and fails on
//! plain `main` as `value get is not a member of Product`.
//!
//! slick: `errors=177 files_with_errors=57` -> `errors=166
//! files_with_errors=53`.

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
        "scala-rs-tail4-{tag}-{}-{nanos}-{seq}",
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
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
    let p = Command::new("scalac").arg("-version").output().ok()?;
    (p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty())
        .then_some(PathBuf::from("scalac"))
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
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
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
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `--no-scala-library` (private runtime) check.
fn check(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let got = run_java(&out, None);
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// `--scala-library` (real jar) dual-run, under `-Xverify:all`.
fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run {name}: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    let got = run_java(&out, Some(jar_s));
    assert_eq!(
        got,
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The fixture is legitimate Scala, and the recorded expectation is what real
/// scalac 2.13.16 prints.
fn real_scalac_check(name: &str) {
    if !java_available() {
        return;
    }
    let Some(scalac) = scalac() else {
        eprintln!("skip real-scalac check {name}: scalac not obtainable");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip real-scalac check {name}: scala-library jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let out = run_java(&ref_out, Some(jar.to_str().unwrap()));
    assert_eq!(
        out,
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

fn compile_bad(name: &str) -> String {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    if let Some(jar) = scala_library_jar() {
        cmd.args(["--scala-library", jar.to_str().unwrap()]);
    } else {
        cmd.arg("--no-scala-library");
    }
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "{name} should not compile, got:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&out);
    msgs
}

// --- `new C(a)(b)` -------------------------------------------------------

#[test]
fn fixtures_t4_curried_new() {
    check("t4_curried_new");
}

#[test]
fn scala_library_dual_run_t4_curried_new() {
    dual_run_fixture("t4_curried_new");
}

#[test]
fn real_scalac_accepts_t4_curried_new() {
    real_scalac_check("t4_curried_new");
}

// --- the join of two branches is the *least* common upper bound ----------

#[test]
fn fixtures_t4_lub_bases() {
    check("t4_lub_bases");
}

#[test]
fn scala_library_dual_run_t4_lub_bases() {
    dual_run_fixture("t4_lub_bases");
}

#[test]
fn real_scalac_accepts_t4_lub_bases() {
    real_scalac_check("t4_lub_bases");
}

/// nsc 2.13.16 reports the same three causes on this file: `Lit does not take
/// parameters` (the third list is an application of the instance, and `Lit`
/// has no `apply`), `found String("b") required Int`, and `could not find
/// implicit value for parameter b`.
#[test]
fn t4_curried_new_bad_is_still_rejected() {
    let msgs = compile_bad("t4_curried_new_bad");
    for needle in [
        "value apply is not a member of Lit",
        "type mismatch; found: \"b\"  required: Int",
        "could not find implicit value of type TT[B]",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics, got:\n{msgs}"
        );
    }
}
