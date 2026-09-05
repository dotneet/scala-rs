//! E2E tests for the `Type::TypeMember` "no prefix" fix (cats' `Newtype`
//! implicit-scope gap): a still-abstract type member `type Type[A] <: Base
//! with Tag`, declared once on a shared `Newtype` trait and inherited
//! (never overridden) by an object that also declares an implicit
//! conversion out of it. `Type::TypeMember` carries only the defining
//! symbol, never the prefix (the object) a qualified `p.Type[A]` selected
//! it through, so implicit search used to see only the upper bound's
//! (empty) companion scope and every method the conversion's target class
//! adds reported "value ... is not a member of Newtype.Type[A]" -- see
//! `docs/cats.md`'s "`Type::TypeMember` has no prefix" note, and the
//! `Checker::with_prefix_if_type_member` / `Typer::type_member_prefixes`
//! side table that fixes it for this qualified-module-prefix shape.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts
//! with other agents working the same file; see `.agent-brief.md`. All
//! fixtures use the `tm_` prefix.

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
        "scala-rs-tm-{tag}-{}-{nanos}-{seq}",
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
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
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
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        Some(cached)
    } else {
        None
    }
}

/// Private-runtime check (`--no-scala-library`).
fn check(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let got = run_java(&out, None);
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// library-ABI check (`--scala-library`), against the real 2.13.16 jar. The
/// expected file is real scalac's own stdout for the same source.
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

/// `WidgetImpl extends Newtype` inherits `type Type[A] <: Base with Tag`
/// without overriding it, and `Widget[A] = WidgetImpl.Type[A]` (reached
/// through a package object that inherits the alias from a parent class, so
/// the object and the alias-bearing package object are declared in
/// genuinely different namer scopes -- the same indirection
/// `nel_newtype.scala` uses). `WidgetImpl.widgetOps` is the only place
/// `WidgetOps`'s conversion is declared, reachable solely through the
/// prefix `Type` was selected through.
#[test]
fn fixtures_tm_newtype() {
    check("tm_newtype");
}

#[test]
fn fixtures_tm_newtype_lib() {
    dual_run_fixture("tm_newtype");
}

/// The fix must not loosen arity checking: `Widget` still takes exactly one
/// type parameter, and nsc rejects `Widget[Int, String]` too ("wrong number
/// of type arguments for tm.data.Widget, should be 1").
#[test]
fn fixtures_tm_newtype_bad_is_rejected() {
    compile_fails(
        "tm_newtype_bad",
        &["--no-scala-library"],
        "too many type arguments",
    );
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "tm_newtype_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "too many type arguments",
    );
}
