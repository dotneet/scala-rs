//! A cake's type members: a base trait leaves `type Session` deferred and a
//! derived trait fixes it. nsc resolves such a name in linearisation order,
//! where a concrete definition overrides a deferred one; reading the deferred
//! declaration instead leaves an opaque type with no members.
//!
//! The library case is slick's: `slick.basic.BasicBackend` declares
//! `type Session` / `type Database` / `type DatabaseFactory` and
//! `slick.jdbc.JdbcBackend` fixes each of them. Which of the two was in the
//! symbol table first depended only on what an earlier file had completed, so
//! `slick.jdbc.JdbcBackend#Session` came out as `BasicBackend.Session` and
//! `session.conn` was "not a member". See `docs/gitbucket.md`.
//!
//! The negative fixture is the other half: a member nothing fixes has to stay
//! abstract, and a member that is genuinely absent has to stay diagnosed.

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
        "scala-rs-bt-{tag}-{}-{nanos}-{seq}",
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
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn run_java(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile(name: &str, extra: &[&str]) -> PathBuf {
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
        "compile {name} failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// The private runtime: no jar, so this also proves the rule is in the typer
/// and not in anything the library ABI supplies.
#[test]
fn cake_type_members_run_under_the_private_runtime() {
    if !java_available() {
        return;
    }
    let out = compile("bt_backendtypes", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        expected_stdout("bt_backendtypes"),
        "stdout mismatch under the private runtime"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture against the real scala-library 2.13.16 ABI. The expected
/// file is real scalac's own output for this source.
#[test]
fn cake_type_members_run_against_the_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip bt_backendtypes library run: jar not present");
        return;
    };
    let out = compile(
        "bt_backendtypes",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout("bt_backendtypes"),
        "stdout mismatch against the library ABI"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A member nothing fixes stays abstract, and a member that is not there is
/// still reported. Real scalac rejects the same two lines.
#[test]
fn opaque_and_absent_members_are_still_diagnosed() {
    let src = fixtures_dir().join("bt_backendtypes_bad.scala");
    let out = tmp_dir("bt_backendtypes_bad");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "bt_backendtypes_bad compiled; it must not"
    );
    let err = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(
        err.contains("value length is not a member of"),
        "the deferred member was not left opaque: {err}"
    );
    assert!(
        err.contains("value rollback is not a member of SessionDef"),
        "the absent member was not diagnosed on the fixed type: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
