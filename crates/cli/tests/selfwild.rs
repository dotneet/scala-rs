//! E2E tests for the `ws` slice: `docs/gitbucket.md`'s "what would remove the
//! most next", entry 1 -- a self type that offers no members.
//!
//! Two roots, both in `crates/typer/src/check.rs`:
//!
//! 1. **A self type read from a class file offered nothing unqualified.**
//!    `bind_self_type` copies the self type's member list into the template
//!    scope, and a `-cp` class's member list is empty until something asks
//!    for a name. `expose_inherited_from_binary` already completed an
//!    *inherited* name on demand; it now does the same for the enclosing
//!    templates' self types. The wildcard in gitbucket's `self: Table[?] =>`
//!    is not what breaks it -- `Table[String]` failed identically, and a
//!    `Table` written in source worked with either spelling.
//! 2. **The signature pass's complaint about a self type was permanent.**
//!    A class header is typed by both passes, but a diagnostic is never
//!    retracted, so `self: Table[?] =>` under an `import profile.api._` whose
//!    prefix is another template's `val` reported `not found: type Table` on
//!    the signature pass and kept it, even though the body pass resolved it.
//!    `bind_self_type` now drops the signature pass's complaint, exactly as
//!    `type_parent_ctor_app` does for a parent's constructor arguments.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.
//! All fixtures use the `ws_` prefix.

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
        "scala-rs-ws-{tag}-{}-{nanos}-{seq}",
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

/// Compile `ws_selftype_lib.scala` to class files, then compile `name`
/// against them. Returns (use-output, lib-output).
fn compile_against_lib(name: &str, jar: &str) -> (PathBuf, PathBuf) {
    let lib_out = compile_fixture_with("ws_selftype_lib", &["--scala-library", jar]);
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
    assert!(status.success(), "compile {name} against the lib failed");
    (out, lib_out)
}

// --- (1) a self type read from a class file --------------------------------

/// `self: Table[?] =>`, `self: Table[String] =>`, a compound self type, and a
/// nested template's reference to the enclosing one's -- all against a
/// `Table` that arrives as a class file. Byte-for-byte what real scalac
/// 2.13.16 prints.
#[test]
fn fixtures_ws_selftype() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let (out, lib_out) = compile_against_lib("ws_selftype", jar_s);
    if java_available() {
        assert_eq!(
            run_java(&out, &[lib_out.to_str().unwrap(), jar_s]),
            expected_stdout("ws_selftype")
        );
    }
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&lib_out);
}

// --- (2) a self type named through an import on a later template -----------

/// `Provider` is written *after* the template that imports through it, which
/// is the order gitbucket's `BasicTemplate.scala` and `Profile.scala` sort in.
#[test]
fn fixtures_ws_selfimport() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let (out, lib_out) = compile_against_lib("ws_selfimport", jar_s);
    if java_available() {
        assert_eq!(
            run_java(&out, &[lib_out.to_str().unwrap(), jar_s]),
            expected_stdout("ws_selfimport")
        );
    }
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&lib_out);
}

// --- the diagnostics that must survive -------------------------------------

/// A member the self type does not have is still `not found`, a self type
/// that names nothing is still `not found: type`, and so is one written in a
/// *nested* template -- which is the position whose signature-pass diagnostic
/// is now dropped. Real scalac 2.13.16 reports the same three.
#[test]
fn fixtures_ws_selftype_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let lib_out = compile_fixture_with("ws_selftype_lib", &["--scala-library", jar_s]);
    let src = fixtures_dir().join("ws_selftype_bad.scala");
    let out = tmp_dir("ws_selftype_bad");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-cp",
            lib_out.to_str().unwrap(),
            "--scala-library",
            jar_s,
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of ws_selftype_bad to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for n in [
        "not found: value noSuchColumn",
        "not found: type Missing",
        "not found: type AlsoMissing",
    ] {
        assert!(err.contains(n), "expected {n:?}, got: {err}");
    }
    assert!(
        err.contains("3 error(s)"),
        "expected exactly 3 errors, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&lib_out);
}
