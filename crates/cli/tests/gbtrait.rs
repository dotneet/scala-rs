//! E2E tests for the `agent/gbtrait` slice: `new T()` where `T` is a trait
//! read from a class file, not from source.
//!
//! gitbucket's dependency closure has `org.scalatra.forms.Constraint`, a
//! trait compiled to a plain JVM interface with no `<init>` at all -- neither
//! its class file nor its `ScalaSignature` pickle mentions one (`javap -p`:
//! `interface … { … validate$(…); … $init$(…); }`, nothing named `<init>`).
//! SLS 5.1.2 makes a trait's parents mere constraints that the concrete class
//! running them, not the trait itself; nsc's own bytecode reflects exactly
//! that. A class read from source got a zero-argument `<init>` symbol
//! unconditionally (`check_namer::namer_class`, trait or not), so `new T()`
//! against a *source* trait type-checked; a binary one had no `<init>`
//! anywhere and every `new Constraint()` was "no matching overload for
//! constructor Constraint with arguments ()".
//!
//! Fixing only the typer half would have traded one bug for a worse one:
//! `parent_super_ctor` (`crates/backend/src/gen_desc.rs`) picked *any*
//! resolved constructor symbol's owner as the real superclass to
//! `invokespecial`, including a trait's own -- which is never emitted to
//! bytecode. That was a live, pre-existing bug for a *source* trait's `new
//! T() { … }` too: it type-checked, and the emitted class then failed the
//! JVM verifier (`VerifyError: Bad <init> method call`) the moment it was
//! run, which a check that only counts compiler diagnostics never catches.
//! `fixtures_gbtrait_trait_anon_runs` below runs the emitted class under
//! `-Xverify:all`, not just past the compiler.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with other
//! agents; see `.agent-brief.md`. All fixtures use the `gbtrait` prefix.

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
        "scala-rs-gbtrait-{tag}-{}-{nanos}-{seq}",
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
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    out
}

/// Compile `{lib}.scala` to class files, then hand them to the compile of
/// `{name}.scala` as a `-cp` entry -- exactly how gitbucket sees a jar's
/// trait: never as source, only as a class file (and, with `--scala-library`,
/// its pickle). Returns (use-output, lib-output).
fn compile_against_lib(name: &str, lib: &str, jar: &str) -> (PathBuf, PathBuf) {
    let lib_out = compile_fixture_with(lib, &["--scala-library", jar]);
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-cp",
            lib_out.to_str().unwrap(),
            "--scala-library",
            jar,
            "-d",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile {name} against {lib} failed");
    (out, lib_out)
}

fn compile_against_lib_errors(name: &str, lib: &str, jar: &str, needles: &[&str]) -> String {
    let lib_out = compile_fixture_with(lib, &["--scala-library", jar]);
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-bad"));
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-cp",
            lib_out.to_str().unwrap(),
            "--scala-library",
            jar,
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} against {lib} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for n in needles {
        assert!(
            err.contains(n),
            "expected {name} error to contain {n:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&lib_out);
    err
}

fn run_java(out: &Path, cp_extra: &[&str]) -> String {
    let mut cp = out.display().to_string();
    for e in cp_extra {
        cp.push(':');
        cp.push_str(e);
    }
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

/// The defect itself: `new GbTraitConstraint() { … }` against a trait read
/// only from a class file must compile *and* the emitted anonymous subclass
/// must actually run under the JVM verifier -- not merely fail to error at
/// compile time, which a codegen bug (calling the trait's own, unemitted
/// `<init>`) would still pass.
#[test]
fn fixtures_gbtrait_trait_anon_runs() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let (out, lib_out) = compile_against_lib("gbtrait_use", "gbtrait_lib", jar_s);
    if java_available() {
        assert_eq!(
            run_java(&out, &[lib_out.to_str().unwrap(), jar_s]),
            expected_stdout("gbtrait_use")
        );
    }
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&lib_out);
}

/// Neighbouring case 1: `new T(arg)` where `T` is a binary trait is still an
/// error, and the same one nsc reports -- a trait's constructor accepts
/// exactly the empty argument list.
#[test]
fn fixtures_gbtrait_trait_ctor_with_args_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let err = compile_against_lib_errors(
        "gbtrait_use_bad",
        "gbtrait_lib",
        jar_s,
        &["no matching overload for constructor GbTraitConstraint with arguments (\"x\")"],
    );
    assert!(
        err.contains("1 error(s)"),
        "expected exactly 1 error, got: {err}"
    );
}

/// Neighbouring case 2: `new C()` for a binary *abstract class* -- which does
/// have a real `<init>` in its class file -- must keep working.
#[test]
fn fixtures_gbtrait_abstract_class_anon_runs() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let (out, lib_out) = compile_against_lib("gbtrait_abs_use", "gbtrait_abs_lib", jar_s);
    if java_available() {
        assert_eq!(
            run_java(&out, &[lib_out.to_str().unwrap(), jar_s]),
            expected_stdout("gbtrait_abs_use")
        );
    }
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&lib_out);
}
