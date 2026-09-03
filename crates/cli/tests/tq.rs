//! E2E tests for the `agent/tq` slice: slick's `lifted/TableQuery.scala`,
//! `lifted/Compiled.scala` and `relational/RelationalProfile.scala`.
//!
//! Three roots, none of them where the diagnostic pointed:
//!
//! * a wildcard type argument did not accept the application of an *abstract*
//!   type constructor. `Query[+E, U, C[_]]` inherits `Rep[C[U]]`, and slick's
//!   `StreamingExecutable.apply[T <: Rep[_], TU, EU]` asks whether that is a
//!   `Rep[_]`; the invariant-argument rule reduces it to `C[BU] <: _`, and the
//!   `Type::Applied` arm of `is_sub_type` -- which precedes the wildcard arms
//!   and matches every right-hand side -- knew how to follow only a
//!   `TypeMember`'s bound, never a type *parameter*. Reported as
//!   "type arguments […] do not conform to method apply's type parameter
//!   bounds", i.e. as a bound check, which it was not;
//!
//! * the callee of a `TypeApply` was typed in *value* position, so an
//!   overloaded reference had already collapsed to its parameterless
//!   alternative before the enclosing `Apply` saw the arguments. slick's
//!   `object TableQuery` has `def apply[E]: TableQuery[E]` (a macro) next to
//!   `def apply[E](cons: Tag => E)`, so `TableQuery.apply[E](cons)` became a
//!   `TableQuery[E]` applied to an argument: "value apply is not a member of
//!   TableQuery[E]". Nothing to do with macro expansion -- the same two
//!   alternatives without a macro in sight reproduce it;
//!
//! * an implicit whose own type parameters only its own implicit clause can
//!   pin down was dropped. `Compiled.apply[V, C <: Compiled[V]](raw: V)
//!   (implicit c: Compilable[V, C], …): C` leaves `C` undetermined, so
//!   `function1IsCompilable[A, B, P, U]` is unified against
//!   `Compilable[Rep[P] => Query[T, U, Seq], ?C]`, which settles `A` and `B`
//!   and leaves `P` and `U` open -- `aShape` and `bExe` are what say what they
//!   are. Reported as slick's own `@implicitNotFound`, "Computation of type …
//!   cannot be compiled (as type C)".
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with other
//! agents; see `.agent-brief.md`. All fixtures use the `tq` prefix.

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
        "scala-rs-tq-{tag}-{}-{nanos}-{seq}",
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

/// `tq.scala` uses `Seq` as a type constructor argument and `Array[String]`,
/// both from the real scala-library.
#[test]
fn fixtures_tq_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run tq: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("tq", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("tq"),
        "stdout mismatch for library-ABI tq"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded
/// expectation, scalac's stdout and ours all have to agree.
#[test]
fn real_scalac_dual_run_tq() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff tq: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("tq.scala");
    let ref_out = tmp_dir("tq-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile tq");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout("tq"),
        "recorded expectation for tq does not match real scalac"
    );
    let out = compile_fixture_with("tq", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        reference,
        "stdout differs from real scalac for tq"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// The private runtime has no `Seq`; the fixture has to be diagnosed there,
/// not quietly accepted.
#[test]
fn fixtures_tq_without_library_is_error() {
    compile_fails("tq", &["--no-scala-library"], "not found: type Seq");
}

/// A wildcard type argument accepting `C[BU]` does not make the *bound*
/// vacuous: `String` is no `BRep[_]`, and nsc rejects it too.
#[test]
fn fixtures_tq_bad_bound_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "tq_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "type arguments [String,Any,Any] do not conform to method apply's type parameter bounds",
    );
}

/// Keeping the overload set alive through the `TypeApply` does not make a
/// call that fits no alternative legal.
#[test]
fn fixtures_tq_bad_overload_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "tq_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "no matching overload for <overload ((Int) => E)BTQ[E] | BTQ[E]> with arguments",
    );
}

/// Completing a candidate's own type parameters from its own implicit clause
/// does not invent a witness: `BExe[Long, U]` has none, so there is no
/// `BCompilable` either.
#[test]
fn fixtures_tq_bad_open_implicit_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "tq_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "could not find implicit value of type BCompilable[(Int) => Long, C]",
    );
}
