//! E2E tests for the `agent/unitpop` slice: a `Unit`-valued `Predef` intrinsic
//! and the statement-position discard disagreeing about the operand stack.
//!
//! ```scala
//! println(identity(()))
//! ```
//!
//! compiled, and threw `java.lang.VerifyError: Operand stack underflow` at run
//! time. `gen_call::gen_predef_poly` ended with
//!
//! ```text
//! if is_unit_like(result_ty) { asm.pop(); } else { maybe_unbox_erased_result(…) }
//! ```
//!
//! and `identity(())`'s result type *is* `Unit`, so the `pop` always fired.
//! Dropping the value is right in **statement** position and wrong when the
//! value is an **argument**: `gen_predef_println` had already been told by
//! `unit_leaves_boxed_ref` that its argument left a reference behind, so it
//! emitted no `BoxedUnit` of its own and `Predef$.println` was handed an empty
//! stack. nsc keeps the value (`javap`: `invokevirtual identity; invokevirtual
//! println`) and lets the generic statement-position discard remove it; so does
//! this now, through `gen_expr::discarded_predef_poly`.
//!
//! The private runtime was failing the *mirror* image of the same disagreement
//! and is fixed by the same predicate: it inlines the intrinsic, erasure boxes
//! a `Unit` argument, and `unit_stat_leaves_ref` refuses every symbol carrying
//! an `Intrinsic` — so a discarded `identity(())` left a `BoxedUnit` nobody
//! popped. Straight-line code merely leaked a slot; `if (b) identity(()) else
//! side()` was `VerifyError: Inconsistent stackmap frames`.
//!
//! **This class of defect is invisible to every check but running the
//! program.** Both shapes compile clean, and only `-Xverify:all` or execution
//! says otherwise — which is why the case sat commented out in
//! `crates/cli/tests/intrinsicqual.rs` with a note naming it. Every assertion
//! here therefore runs `java -Xverify:all`, in both linking modes, against
//! output taken from real scalac 2.13.16.
//!
//! Fixture prefix: `unitpop_`.

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
        "scala-rs-unitpop-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile(tag: &str, src: &str, extra: &[&str]) -> PathBuf {
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
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {tag} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn compile_fixture(name: &str, extra: &[&str]) -> PathBuf {
    let out = tmp_dir(name);
    let src = fixtures_dir().join(format!("{name}.scala"));
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
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// `-Xverify:all` is the point of this file, not decoration: the defect
/// produces a classfile the default verifier setting still rejects only when
/// the method is actually reached, and nothing before execution notices.
fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "java -Xverify:all {main} failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(stderr, "", "nothing belongs on stderr:\n{stderr}");
    stdout
}

/// Both linking modes. The defect proper lived inside `if ctx.library_abi`,
/// but the private runtime carried the mirror image of it, so neither half is
/// optional here.
fn both_modes() -> Vec<(&'static str, Vec<String>, Option<String>)> {
    let mut modes: Vec<(&'static str, Vec<String>, Option<String>)> =
        vec![("private", vec!["--no-scala-library".into()], None)];
    if let Some(jar) = scala_library_jar() {
        let jar_s = jar.to_str().unwrap().to_string();
        modes.push((
            "jar",
            vec!["--scala-library".into(), jar_s.clone()],
            Some(jar_s),
        ));
    }
    modes
}

/// The fixture, in both modes, against real scalac 2.13.16's own output.
///
/// It holds both positions in one program on purpose. A fix that merely
/// stopped popping would pass the argument half and leave a stray reference in
/// statement position — the verifier failing the other way round, which is
/// exactly what the private runtime was already doing. An unmodified build of
/// the branch point compiles this file in both modes and fails to run it in
/// both: `Operand stack underflow` at `Main$.a3` with the jar,
/// `Inconsistent stackmap frames` at `Main$.s7` without it.
#[test]
fn the_unitpop_fixture_runs_and_matches_scalac() {
    if !java_available() {
        return;
    }
    let name = "unitpop_intrinsic";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile_fixture(name, &flags);
        let stdout = run_java(&out, cp.as_deref(), "unitpoppkg.Main");
        assert_eq!(stdout, expected_stdout(name), "[{tag}] stdout for {name}");
        let _ = fs::remove_dir_all(&out);
    }
}

/// The one-line reproducer from the report, on its own, so a regression names
/// itself without reading the fixture.
#[test]
fn a_unit_intrinsic_in_argument_position_keeps_its_value() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    println(identity(()))
  }
}
"#;
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("unitpop-arg-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), "()\n", "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// The other half, on its own: the discarded value has to go, or the first
/// control-flow join after it is `Inconsistent stackmap frames`.
#[test]
fn a_unit_intrinsic_in_statement_position_drops_its_value() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  def side(s: String): Unit = println(s)
  def f(b: Boolean): Unit = if (b) identity(()) else side("f")
  def g(b: Boolean): Unit = { locally(()); if (b) side("gt") else side("gf") }
  def main(args: Array[String]): Unit = {
    f(true); f(false); g(true); g(false)
  }
}
"#;
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("unitpop-stat-{tag}"), src, &flags);
        assert_eq!(
            run_java(&out, cp.as_deref(), "Main"),
            "f\ngt\ngf\n",
            "[{tag}]"
        );
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}
