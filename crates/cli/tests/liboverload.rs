//! E2E tests for the `agent/liboverload` slice: a re-declaration of an
//! inherited member, and which of the two `drop_overridden` keeps.
//!
//! `scala.collection.LinearSeqOps` re-declares the `tail` that `IterableOps`
//! defines and `Stream` re-declares it again. Two of `drop_overridden`'s
//! rules answered that pair in opposite directions -- the
//! declaration/definition rule dropped the deferred member, the owner rule
//! dropped the one whose owner is above the other's -- so the candidate set
//! eliminated itself, `kept` came out empty, and the `kept.is_empty()`
//! fallback handed the caller back the whole unreduced set. Every selection
//! on it then read `value <member> is not a member of <overload Iterable[A] |
//! Stream[A] | Stream[A]>`, and every use of it as a value was a type
//! mismatch: 50 errors in the scala library measure.
//!
//! Both rules now ask `definition_outranks_declaration`, so exactly one of
//! them fires. A declaration that only *restates* the definition at its own
//! prefix is one member with the derived spelling; one that says something
//! different leaves the definition in force, which is nsc's `findMember`
//! behaviour and is what `libov_reabstract_bad.scala` pins.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. Both fixtures use the `libov` prefix and neither needs
//! the real library -- they are checked in both modes.

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
        "scala-rs-libov-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    None
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
        "compile {name} failed extra={extra:?}:\n{}{}",
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
        "java -Xverify:all {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
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
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------- positive: it runs

/// The restating chain, in both modes. Every line is checked by its value, so
/// an alternative that merely type-checks cannot pass: `second`, `third` and
/// `lastOne` all walk `tail`, which has to stay a `Str[A]`.
///
/// On the pre-fix binary this fixture does not compile: four errors, three of
/// them `value <member> is not a member of <overload Iter[A] | Str[A] |
/// Str[A]>` -- the scala library's own spelling.
#[test]
fn fixtures_libov_reabstract_runs_on_private_runtime() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with("libov_reabstract", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None, "Main"),
        expected_stdout("libov_reabstract"),
        "stdout mismatch for private-runtime run"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_libov_reabstract_runs_against_the_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("libov_reabstract", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s), "Main"),
        expected_stdout("libov_reabstract"),
        "stdout mismatch for library dual-run"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The fixture is only worth its runtime if real scalac accepts it too -- and
/// prints exactly what `expected/libov_reabstract.txt` records.
#[test]
fn scalac_agrees_libov_reabstract_runs() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-libov-reabstract");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("libov_reabstract.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected libov_reabstract.scala: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        expected_stdout("libov_reabstract"),
        "scalac's own run does not match the recorded output"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------ negative: it still binds

/// `bad1` and `bad2` are the two the rule decides, and both keep nsc's
/// receiver: `Ops` where the derived declaration says `Box`, and `Any` where
/// it says `NarrowBox`. On the pre-fix binary both name the unreduced
/// candidate set instead. `bad3` keeps an inherited alternative that the
/// subclass does not override, and `bad4` keeps a real ambiguity ambiguous.
#[test]
fn fixtures_libov_reabstract_bad_is_rejected() {
    for mode in [
        vec!["--no-scala-library"],
        vec![
            "--scala-library",
            "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
        ],
    ] {
        if mode[0] == "--scala-library" && scala_library_jar().is_none() {
            continue;
        }
        compile_fails(
            "libov_reabstract_bad",
            &mode,
            &[
                "value tag is not a member of Ops",
                "value note is not a member of Any",
                "no matching overload for <overload (Int)String | (String)String>",
                "ambiguous overload for g",
            ],
        );
    }
}

/// Real scalac 2.13.16 rejects the same four lines, at 25, 35, 45 and 54.
/// Without this the fixture would only say what this compiler does.
#[test]
fn scalac_rejects_libov_reabstract_bad_at_the_same_lines() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-libov-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("libov_reabstract_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(!output.status.success(), "scalac accepted the bad fixture");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "libov_reabstract_bad.scala:25: error: value tag is not a member of Ops",
        "libov_reabstract_bad.scala:35: error: value note is not a member of Any",
        "libov_reabstract_bad.scala:45: error: overloaded method f with alternatives",
        "libov_reabstract_bad.scala:54: error: ambiguous reference to overloaded definition",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac to report {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
