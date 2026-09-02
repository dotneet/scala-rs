//! E2E tests for the `agent/tail5` slice. Fixture prefix `t5`. Kept out of
//! `crates/cli/tests/e2e.rs` to avoid merge conflicts; see `.agent-brief.md`.
//!
//! Four small, independent slick symptoms, all traced to their own real
//! root cause (not the brief's guesses -- every one of those turned out
//! wrong or incomplete on inspection, as usual):
//!
//! # Named arguments through a qualified companion reference
//!
//! `pkg1.Bar(a = 1, b = "x")` (qualified) failed with "unimplemented
//! syntax: named arguments (method parameters not resolved)" while the
//! identical `Bar(a = 1, b = "x")` (bare) already worked. `fun.sym` for the
//! qualified form is the *module* `Bar`, not its `apply` method --
//! `rewrite_receiver_apply` deliberately leaves a qualified companion
//! reference unrewritten (codegen depends on this for `scala.Some(1)`) --
//! and a module carries no `paramss` for `first_clause_ids` to read.
//! `named_arg_param_ids` now looks the parameter names up on the module's
//! own `apply` member(s) when `fun.sym` is a `Module`, same as an
//! overloaded callee already does. Fixtures: `t5_named_qual(_bad)`.
//!
//! # `override def f = ...` inherits its result type
//!
//! `override def run(n: Node) = n match { case Wrap(x) => run(x) ... }`
//! reported "recursive method run needs result type" even when it
//! overrides `def run(n: Node): Any = ...` -- whose declared type SLS 6.1
//! says an override with no type of its own takes. The identical body on a
//! method that overrides nothing (`t5_override_infer_bad`) is correctly
//! still that error; only the override case was wrong.
//! `overridden_ret_type` (gated on the written `override` modifier) walks
//! the owner's ancestors for a same-name, same-arity member with an
//! already-known return type and borrows it -- only the return type; the
//! body is still checked/inferred exactly as written.
//!
//! Two more bugs sat behind the direct fix, both only found by measuring
//! end-to-end against slick rather than trusting the isolated repro:
//!
//!  * The borrowed type has to be read *as seen from* the overriding
//!    class's own type, not copied raw: `overridden_ret_type` initially
//!    returned the ancestor's declaration verbatim, which is only correct
//!    for a non-generic override. slick's `computeCapabilities` chain
//!    (`JdbcProfile` overriding `SqlProfile` overriding `RelationalProfile`
//!    overriding `BasicProfile`) stayed fine (monomorphic), but plenty of
//!    generic overrides elsewhere came out `type mismatch; found: T
//!    required: T` -- the same letter, two different symbols.
//!  * A method whose signature became "known" this way was still left
//!    registered as needing on-demand lazy completion, because that
//!    bookkeeping (`register_typed_sig`) only ever checked the parsed
//!    syntax (no written `: T`), never whether a type had already been
//!    produced another way. A self-reference inside such a method's own
//!    body then ran `complete_lazy_sig` on itself mid-typing, which locked
//!    the symbol and re-entered body-typing on a clone of the very body
//!    already in progress -- whose own self-reference then found the
//!    symbol locked and reported the cycle regardless. Fixed in
//!    `register_typed_sig`: a `DefDef` with an already-known return type is
//!    no longer lazy.
//!  * `overridden_ret_type` originally forced a still-pending ancestor
//!    candidate to complete on the spot (`complete_lazy_sig`) so its return
//!    type would be available. That ran the candidate's body -- and
//!    whatever forward references it makes -- before its *own* declaring
//!    file's top-down pass had registered that file's real scope
//!    (imports included), so a name only reachable through that file's own
//!    imports resolved against a bare "owner chain" fallback instead and
//!    came back "not found: value X" at a fabricated, unrelated span
//!    (measured against slick: `errors=155` became `errors=307`, mostly
//!    `not found: value Capability`/`DumpInfo` reported inside files that
//!    correctly import them). A still-pending candidate is now simply
//!    skipped -- exactly like not finding it -- and the walk continues to
//!    that candidate's own further ancestors, which is enough: every real
//!    case bottoms out at a member whose return type was written
//!    explicitly and needs no forcing.
//!
//! Fixtures: `t5_override_infer(_bad)`.
//!
//! # `recv.copy(...)` rebuilt a `new C(...)` by *name*, not by symbol
//!
//! `try_rewrite_case_copy` rewrites `recv.copy(f = v)` into `new C(...)`,
//! and built that `new`'s type head as a bare `Ident { name: "C" }` --
//! relying on ordinary lexical name lookup to re-resolve `C` when the
//! rebuilt tree was typed, even though the caller already had `C`'s real
//! `SymbolId` (from `class_sym_of` on the receiver's own type). A class
//! reached only through another file's inheritance chain, never imported
//! by simple name into the file doing the `.copy()`, has no reason to have
//! that name in scope there -- and this reported "not found: type C" with
//! no line/column at all (the synthesized tree carries no real span).
//! slick's `slick.jdbc.BaseResultConverter`, whose `override def
//! getDumpInfo = super.getDumpInfo.copy(...)` never imports
//! `slick.util.DumpInfo` itself, hit exactly this. Fixed by setting
//! `sym`/`ty` directly on the synthesized `Ident` from the already-resolved
//! `SymbolId`, and by teaching the `New`-typing code to use them instead of
//! re-resolving by name when they are already set. Fixtures:
//! `t5_case_copy_qual(_bad)`.
//!
//! # A function literal against a SAM (not literal `FunctionN`) parameter
//!
//! `Builder(sql, (u, pp) => ...)` against `case class Builder(sql: String,
//! setParameter: SetParameter[Unit])`, where `SetParameter[-T] extends
//! ((T, PositionedParameters) => Unit)`, scored no match at all: the
//! literal reached overload scoring as `(<notype>, <notype>) =>
//! <notype>` (pre-typing a literal against the callee's expected shape,
//! nsc's `pretypeArgs`, only ever ran for a genuine multi-alternative
//! `Overload`, and `Builder(...)` is a single synthesized `apply`, not
//! one), and even a correctly-typed literal would have scored nothing
//! against `SetParameter[Unit]` (`arg_score`'s function-parameter rule
//! only recognized a literal `scala.FunctionN`, not a trait that merely
//! extends one). slick's `SQLActionBuilder(sql, (u, pp) => ...)` against
//! `case class SQLActionBuilder(sql: String, setParameter:
//! SetParameter[Unit])` is the identical shape.
//!
//! The fix is entirely in `arg_score`: a class-shaped parameter that is
//! SAM-convertible (`SymbolTable::sam_sig`) is now compared as the
//! function type its abstract method describes, same as a literal
//! `FunctionN` already was -- and an untyped literal already scores as
//! compatible with *any* function-shaped parameter while its own
//! parameters are still open, so no separate pre-typing was needed once
//! scoring itself could see through the SAM. Widening
//! `agreed_lambda_params`'s own pre-typing to a single-candidate callee
//! was tried too and reverted: measured end-to-end against slick, it also
//! pre-typed a literal against still-type-parametric single-candidate
//! signatures elsewhere (cats-effect's `Async[F].uncancelable[A](body:
//! Poll[F] => F[A]): F[A]`) before the call's own inference had solved
//! `A`, regressing far more than the `arg_score` fix alone gained.
//! Fixtures: `t5_sam_ctor(_bad)`.
//!
//! `t5_sam_ctor` is checked only in `--scala-library` mode: `SetParameter`
//! extends `Function2`, and the private (`--no-scala-library`) runtime
//! currently emits only `scala.Function0`/`scala.Function1` -- a
//! pre-existing, unrelated gap (confirmed with a minimal repro that has
//! nothing to do with named arguments, overrides, or SAM conversion:
//! `val f: (Int, Int) => Int = (a, b) => a + b` alone crashes
//! `--no-scala-library` output with `NoClassDefFoundError: scala/Function2`),
//! flagged separately rather than folded into this slice.
//!
//! slick: `errors=155 files_with_errors=52` -> `errors=149
//! files_with_errors=49`.

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
        "scala-rs-tail5-{tag}-{}-{nanos}-{seq}",
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

fn bad_scalac_also_rejects(name: &str) {
    if !java_available() {
        return;
    }
    let Some(scalac) = scalac() else {
        eprintln!("skip real-scalac rejection check {name}: scalac not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(
        !status.success(),
        "{name} is meant to be invalid Scala, but real scalac accepted it"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

// --- named arguments through a qualified companion reference --------------

#[test]
fn fixtures_t5_named_qual() {
    check("t5_named_qual");
}

#[test]
fn scala_library_dual_run_t5_named_qual() {
    dual_run_fixture("t5_named_qual");
}

#[test]
fn real_scalac_accepts_t5_named_qual() {
    real_scalac_check("t5_named_qual");
}

#[test]
fn t5_named_qual_bad_is_still_rejected() {
    let msgs = compile_bad("t5_named_qual_bad");
    assert!(
        msgs.contains("unknown parameter name: c"),
        "expected \"unknown parameter name: c\" in diagnostics, got:\n{msgs}"
    );
    bad_scalac_also_rejects("t5_named_qual_bad");
}

// --- `override def f = ...` inherits its result type -----------------------

#[test]
fn fixtures_t5_override_infer() {
    check("t5_override_infer");
}

#[test]
fn scala_library_dual_run_t5_override_infer() {
    dual_run_fixture("t5_override_infer");
}

#[test]
fn real_scalac_accepts_t5_override_infer() {
    real_scalac_check("t5_override_infer");
}

#[test]
fn t5_override_infer_bad_is_still_rejected() {
    let msgs = compile_bad("t5_override_infer_bad");
    assert!(
        msgs.contains("recursive method run needs result type"),
        "expected \"recursive method run needs result type\" in diagnostics, got:\n{msgs}"
    );
    bad_scalac_also_rejects("t5_override_infer_bad");
}

// --- `recv.copy(...)` rebuilds `new C(...)` by symbol, not by name --------

#[test]
fn fixtures_t5_case_copy_qual() {
    check("t5_case_copy_qual");
}

#[test]
fn scala_library_dual_run_t5_case_copy_qual() {
    dual_run_fixture("t5_case_copy_qual");
}

#[test]
fn real_scalac_accepts_t5_case_copy_qual() {
    real_scalac_check("t5_case_copy_qual");
}

#[test]
fn t5_case_copy_qual_bad_is_still_rejected() {
    let msgs = compile_bad("t5_case_copy_qual_bad");
    assert!(
        msgs.contains("unknown parameter name: nope"),
        "expected \"unknown parameter name: nope\" in diagnostics, got:\n{msgs}"
    );
    bad_scalac_also_rejects("t5_case_copy_qual_bad");
}

// --- a function literal against a SAM (not literal `FunctionN`) parameter -

/// `--scala-library` only: `SetParameter` extends `Function2`, and the
/// private runtime does not emit `scala.Function2` yet (a separate,
/// pre-existing gap -- see the module doc comment). No `fixtures_`/`check`
/// (`--no-scala-library`) counterpart for this one fixture.
#[test]
fn scala_library_dual_run_t5_sam_ctor() {
    dual_run_fixture("t5_sam_ctor");
}

#[test]
fn real_scalac_accepts_t5_sam_ctor() {
    real_scalac_check("t5_sam_ctor");
}

#[test]
fn t5_sam_ctor_bad_is_still_rejected() {
    let msgs = compile_bad("t5_sam_ctor_bad");
    assert!(
        msgs.contains("no matching overload"),
        "expected a \"no matching overload\" diagnostic, got:\n{msgs}"
    );
    bad_scalac_also_rejects("t5_sam_ctor_bad");
}
