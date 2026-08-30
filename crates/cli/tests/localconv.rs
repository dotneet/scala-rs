//! E2E tests for the `agent/localconv` slice: view search (implicit-class
//! extension methods and implicit-def conversions) did not look at local
//! scope, even though implicit-*parameter* search already did.
//!
//! Root causes fixed:
//!
//! 1. `Typer::type_def_sig` never copied `Flags::IMPLICIT` from the modifiers
//!    onto a freshly allocated method symbol. A class/module member is always
//!    pre-named by the namer (with the full flag set, `implicit` included)
//!    before `type_def_sig` runs, so this never showed up there; a *local*
//!    `def` inside a block has no such namer pass, so its symbol was
//!    allocated fresh, right there, with `Flags::EMPTY` -- silently dropping
//!    `implicit`. Every implicit search filters candidates on that flag
//!    (`Typer::implicits_in_scope`), so a local `implicit def` was correctly
//!    entered into the block's scope but never visible to any search at all.
//! 2. `implicit class C(x: P) { ... }` desugars to a synthetic
//!    `implicit def C(x: P): C = new C(x)`
//!    (`Typer::implicit_class_conversions`), which only ran for class/module
//!    *members*, never for a block's local statements -- so a local
//!    `implicit class` had no conversion method to find in the first place.
//! 3. `Typer::implicits_in_scope`'s scope-stack walk collected every implicit
//!    in every enclosing scope with no shadowing: a local `implicit def i2s`
//!    of the same name as an outer one was reported ambiguous against it
//!    instead of shadowing it, the way an ordinary unqualified reference to
//!    that name would (SLS 7.2's candidates are exactly such references).
//! 4. `crates/typer/src/lambda_lift.rs`'s free-variable analysis for a nested
//!    local `def` did not know that constructing a local class needing its
//!    own captured locals (`class F(...) { def m = ... factor ... }`) makes
//!    the *constructing* method need `factor` too. This is not
//!    implicit-conversion-specific -- any nested local `def` doing
//!    `new F(x)` hit it (`lc_capture` below exercises it through the
//!    `implicit class` desugar, which always routes construction through
//!    exactly such a synthetic nested method).
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All fixtures use
//! the `lc` prefix.

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
        "scala-rs-lc-{tag}-{}-{nanos}-{seq}",
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

// --------------------------------------------------------- control: params

/// A local implicit *val* filling a nested method's implicit *parameter*
/// already worked before this slice; kept as the baseline the view-search fix
/// has to match.
#[test]
fn fixtures_lc_param() {
    check("lc_param");
}

#[test]
fn fixtures_lc_param_lib() {
    dual_run_fixture("lc_param");
}

// ------------------------------------------------- 1. local `implicit class`

/// `implicit class` local to a method body, a nested `def`, and a lambda
/// body, each supplying an extension method scalac finds by the same
/// SLS 7.3 scope chain as an implicit parameter.
#[test]
fn fixtures_lc_class() {
    check("lc_class");
}

#[test]
fn fixtures_lc_class_lib() {
    dual_run_fixture("lc_class");
}

// --------------------------------------------------- 2. local `implicit def`

/// A local `implicit def` used as a plain conversion (assignment coercion)
/// and as the source of an extension method on a locally declared class.
#[test]
fn fixtures_lc_conv() {
    check("lc_conv");
}

#[test]
fn fixtures_lc_conv_lib() {
    dual_run_fixture("lc_conv");
}

// --------------------------------------------------------------- 3. shadowing

/// A local implicit shadows a same-named outer one -- ordinary unqualified
/// name resolution, not two candidates to disambiguate between.
#[test]
fn fixtures_lc_shadow() {
    check("lc_shadow");
}

#[test]
fn fixtures_lc_shadow_lib() {
    dual_run_fixture("lc_shadow");
}

// ----------------------------------------------------------------- 4. capture

/// A local `implicit class` closing over another local of the enclosing
/// method. Exercises the `lambda_lift` free-variable fix: the synthesized
/// conversion method (`implicit def F(x: P): F = new F(x)`) is itself a
/// nested local def, and `new F(x)` needs the *class*'s own captures
/// threaded into *that* def too.
#[test]
fn fixtures_lc_capture() {
    check("lc_capture");
}

#[test]
fn fixtures_lc_capture_lib() {
    dual_run_fixture("lc_capture");
}

// ------------------------------------------------------------- 5. bad: scope

/// An `implicit class` local to one method is not visible in a sibling
/// method -- same "not a member" scalac reports, in both modes.
#[test]
fn fixtures_lc_outofscope_bad_is_error() {
    compile_fails(
        "lc_outofscope_bad",
        &["--no-scala-library"],
        "value dbl is not a member of 3",
    );
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "lc_outofscope_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "value dbl is not a member of 3",
    );
}

// -------------------------------------------------------- 6. bad: ambiguous

/// Two equally specific local implicit conversions are ambiguous, same as
/// scalac -- not silently resolved and not "no implicit found".
#[test]
fn fixtures_lc_ambiguous_bad_is_error() {
    compile_fails(
        "lc_ambiguous_bad",
        &["--no-scala-library"],
        "ambiguous implicit",
    );
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "lc_ambiguous_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "ambiguous implicit",
    );
}
