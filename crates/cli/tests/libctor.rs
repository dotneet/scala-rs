//! E2E tests for the `agent/libctor` slice: constructor call type-argument
//! inference for a type parameter that stays completely unconstrained (no
//! argument mentions it, no expected type reaches it).
//!
//! `docs/scala-library.md` estimated ~216 errors in `src/library` under this
//! one heading (`Vector2[Any]` / `Tree[A, …]` / `Array[Any]`). It turned out
//! to be **two** separate roots, not one:
//!
//! 1. `new C(args)` defaulted an unconstrained type parameter to `Type::Any`
//!    unconditionally. nsc's own default (`Infer.solvedTypes`) is
//!    variance-driven: the parameter's own lower bound (`Nothing` when
//!    unbounded) for a covariant or invariant parameter, its upper bound
//!    (`Any` when unbounded) for a contravariant one -- confirmed against
//!    real scalac with `-Xprint:typer`. This is `Vector.scala`'s
//!    `private[this] def copy(...) = new Vector2(...)`, whose value
//!    parameters never mention the element type at all
//!    (`scala/collection/immutable/Vector.scala`), and it accounted for all
//!    100 of the `Vector2[Any]`/…/`Vector6[Any]` errors.
//!
//! 2. A *self-recursive* generic method call (`def lookup[A, B](tree:
//!    Tree[A, B], x: A): Tree[A, B] = ... lookup(tree.left, x)`, in
//!    `scala/collection/immutable/RedBlackTree.scala`) legitimately solves
//!    its own type parameters to themselves -- the argument's type is
//!    written in terms of the very parameters being solved for, so `A := A`
//!    is the correct fixed point, not a failure to solve. The general
//!    method-call path then re-opened those parameters to their bounds
//!    anyway, because `open_tparams_of` decided whether a parameter was
//!    still "open" by checking whether the *substituted* parameter type
//!    still mentioned its symbol -- which an identity solution always does,
//!    indistinguishable from a parameter no argument pinned at all. Fixed by
//!    recording which of the callee's own type parameters this call's
//!    inference actually solved (`solved_own_tparams`), rather than
//!    re-deriving it from what the substituted type happens to mention.
//!
//! Measured on `tests/scalalib_measure.sh -no-specialization`:
//! `files=538 errors=1903 files_with_errors=172` before either fix,
//! `files=538 errors=1656 files_with_errors=171` after both.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`.
//!
//! Fixture prefix: the brief assigned this slice `lc_`, which
//! `crates/cli/tests/localconv.rs` already uses for an unrelated feature
//! ("local conversion", `lc_class.scala` etc., predating this slice on
//! `main`). To avoid colliding with those fixtures, this slice's own
//! fixtures use `lct_` instead.

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
        "scala-rs-libctor-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    out
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "lct.Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java lct.Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Private-runtime run (`--no-scala-library`).
fn check(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let got = run_java(&out, None);
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// library-ABI run (`--scala-library <jar>`).
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
    let got = run_java(&out, Some(jar_s));
    assert_eq!(
        got,
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_errors(name: &str, extra: &[&str]) -> String {
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
    let _ = fs::remove_dir_all(&out);
    err
}

/// `CoBox`/`InvBox`/`ContraBox`: a `private[this] def copy(...) = new
/// C(...)` whose value parameters never mention the class's own type
/// parameter, exercised for a covariant, an invariant and a contravariant
/// parameter -- against the private runtime, which needs none of `AnyRef`,
/// `Int` or `String` boxed specially.
///
/// `Node`/`lookup`: the self-recursive generic method call.
#[test]
fn fixtures_lct_ctorinfer() {
    check("lct_ctorinfer");
}

#[test]
fn fixtures_lct_ctorinfer_lib() {
    dual_run_fixture("lct_ctorinfer");
}

/// What the fix must not let through: ordinary contravariant-conformance
/// checking (`ContraBox[Int]` is not a `ContraBox[Any]`, unrelated to how an
/// unconstrained parameter defaults), and an explicit type argument on a
/// self-recursive call that disagrees with the argument. Real scalac
/// reports exactly 2 errors for this file.
#[test]
fn fixtures_lct_ctorinfer_bad_is_error() {
    let err = compile_errors("lct_ctorinfer_bad", &["--no-scala-library"]);
    assert!(
        err.contains("ContraBox[Int]") && err.contains("ContraBox[Any]"),
        "expected the contravariance mismatch, got: {err}"
    );
    assert!(
        err.contains("not an int"),
        "expected the disagreeing explicit type argument to be rejected, got: {err}"
    );
    assert!(
        err.contains("2 error(s)"),
        "expected exactly 2 errors, got: {err}"
    );
}

#[test]
fn fixtures_lct_ctorinfer_bad_is_error_lib() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let err = compile_errors(
        "lct_ctorinfer_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(
        err.contains("ContraBox[Int]") && err.contains("ContraBox[Any]"),
        "expected the contravariance mismatch, got: {err}"
    );
    assert!(
        err.contains("not an int"),
        "expected the disagreeing explicit type argument to be rejected, got: {err}"
    );
    assert!(
        err.contains("2 error(s)"),
        "expected exactly 2 errors, got: {err}"
    );
}
