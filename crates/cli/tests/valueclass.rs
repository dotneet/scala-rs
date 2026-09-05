//! E2E tests for the value-class restrictions (SLS 5.1.7 / SIP-15), nsc's
//! `Typers.validateDerivedValueClass`. Fixture prefix `vc_`.
//!
//! None of these rules existed. `test/files/neg/valueclasses.scala` — thirty
//! lines that are nothing but violations — compiled to 33 class files with
//! zero diagnostics, and stayed that way until `@specialized` stopped being a
//! parse error and took the one wall on line 29 with it.
//!
//! The implementation is `crates/typer/src/valueclass.rs`; its module header
//! carries the reasoning, including why a `trait` reports one message and not
//! two.
//!
//! Both fixtures were run against real scalac 2.13.16 before being written
//! down. `vc_bad.scala` gets these thirteen from
//! `/tmp/scala-2.13.16/bin/scalac`, and scala-rs reproduces every message at
//! every line:
//!
//! ```text
//! 10: only classes (not traits) are allowed to extend AnyVal
//! 13: value class may not be a member of another class
//! 15: value class may not be a local class
//! 20: value class needs to have exactly one val parameter
//! 21: value class needs to have exactly one val parameter
//! 22: value class needs to have exactly one val parameter
//! 23: value class needs to have exactly one val parameter
//! 24: value class parameter must not be a var
//! 25: value class parameter must be a val and not be private[this]
//! 26: value class parameter must be a val and not be private[this]
//! 27: value class parameter must not be protected[this]
//! 30: field definition is not allowed in value class
//! 33: type parameter of value class may not be specialized
//! ```
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.

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
        "scala-rs-valueclass-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn compile_fixture(name: &str, extra: &[&str]) -> PathBuf {
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

fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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

/// `<message>` together with the `--> …:<line>:` line that follows it, so a
/// message reported at the wrong place does not pass.
fn errors_with_lines(out: &str) -> Vec<(String, u32)> {
    let lines: Vec<&str> = out.lines().collect();
    let mut got = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(msg) = l.strip_prefix("error: ") else {
            continue;
        };
        let at = lines
            .get(i + 1)
            .and_then(|n| n.trim().strip_prefix("--> "))
            .and_then(|n| n.rsplit(':').nth(1))
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(0);
        got.push((msg.trim().to_string(), at));
    }
    got
}

/// The legal shapes must keep compiling and keep running, against the private
/// runtime and against the real jar. This is the half a rejection rule gets
/// wrong: `neg/valueclasses.scala`'s own "okay, wasn't allowed in 2.10.x"
/// lines, a `case class` whose bare parameter is a public `val` anyway, a
/// value class that is a member of an *object* (static, so legal), and one
/// that mixes in a universal trait.
#[test]
fn fixtures_vc_ok() {
    let out = compile_fixture("vc_ok", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None, "vc.Main"),
            expected_stdout("vc_ok"),
            "stdout mismatch for vc_ok"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_vc_ok_lib() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip vc_ok dual-run: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture("vc_ok", &["--scala-library", jar_s]);
    if java_available() {
        assert_eq!(
            run_java(&out, Some(jar_s), "vc.Main"),
            expected_stdout("vc_ok"),
            "stdout mismatch for vc_ok against the real library"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Every restriction, at scalac's own line, and **nothing else**: the count is
/// asserted too, because nsc's reporter drops a second error at a position
/// that already has one and this check reproduces that (a `trait` gets the
/// trait message only, not the parameter message as well).
#[test]
fn fixtures_vc_bad() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip vc_bad: scala-library jar not present (the context bound needs Ordering)");
        return;
    };
    let err = compile_errors("vc_bad", &["--scala-library", jar.to_str().unwrap()]);
    let want: &[(&str, u32)] = &[
        ("only classes (not traits) are allowed to extend AnyVal", 10),
        ("value class may not be a member of another class", 13),
        ("value class may not be a local class", 15),
        ("value class needs to have exactly one val parameter", 20),
        ("value class needs to have exactly one val parameter", 21),
        ("value class needs to have exactly one val parameter", 22),
        ("value class needs to have exactly one val parameter", 23),
        ("value class parameter must not be a var", 24),
        (
            "value class parameter must be a val and not be private[this]",
            25,
        ),
        (
            "value class parameter must be a val and not be private[this]",
            26,
        ),
        ("value class parameter must not be protected[this]", 27),
        ("field definition is not allowed in value class", 30),
        ("type parameter of value class may not be specialized", 33),
    ];
    let got = errors_with_lines(&err);
    let want: Vec<(String, u32)> = want.iter().map(|(m, l)| ((*m).to_string(), *l)).collect();
    assert_eq!(got, want, "vc_bad diagnostics differ\n{err}");
}
