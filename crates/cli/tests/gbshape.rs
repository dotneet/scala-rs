//! E2E tests for the `agent/gbshape` slice.
//!
//! Two roots, both reached from gitbucket's
//! `no matching overload for (Boolean)Boolean with arguments (Rep[Boolean])`:
//!
//! 1. **A pending signature kept a stale scope snapshot.** The second
//!    signature round rebuilds a `def` only when the first round reported
//!    something or left an unresolved name in its type (`sig_rerun_safe`), and
//!    rebuilding is the only thing that re-registers it. A member whose own
//!    signature has nothing wrong with it therefore kept the *first* round's
//!    scope -- taken before an `import` through another unit's abstract `val`
//!    could resolve -- and an on-demand completion of its body then ran with
//!    the imported implicits invisible. `Typer::refresh_pending_scope`
//!    refreshes the snapshot without rebuilding the signature.
//!    `gbshape_lazyscope.scala`.
//!
//! 2. **`===` was read as an op-assignment.** nsc's `isOpAssignmentName`
//!    requires the operator's first character to be an operator character and
//!    not `=`; ours required neither, so `a === b` was rewritten into
//!    `a = (a == b)` whenever the `===` member could not be found on the
//!    receiver. On a `var` of the operand's type that *type-checks*, so a
//!    program real scalac rejects was accepted silently.
//!    `gbshape_opassign.scala` / `gbshape_opassign_bad.scala`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with other
//! agents; see `.agent-brief.md`. All fixtures use the `gbshape` prefix.

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
        "scala-rs-gbshape-{tag}-{}-{nanos}-{seq}",
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

/// Every error `scala-rs` reports for `name`, as `(line, text)`.
fn compile_errors(name: &str, extra: &[&str]) -> Vec<(u32, String)> {
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
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    let mut errs = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("error: ") {
            pending = Some(rest.trim().to_string());
            continue;
        }
        if let (Some(msg), Some(pos)) = (pending.clone(), line.trim_start().strip_prefix("--> ")) {
            let mut parts = pos.rsplitn(3, ':');
            let _col = parts.next();
            if let Some(l) = parts.next().and_then(|n| n.parse::<u32>().ok()) {
                errs.push((l, msg));
            }
            pending = None;
        }
    }
    errs
}

/// The definition's body sees the enclosing template's `import`, even when the
/// completion that types it is forced from above the definition.
#[test]
fn fixtures_gbshape_lazyscope_runs() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip gbshape_lazyscope: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("gbshape_lazyscope", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("gbshape_lazyscope"),
        "stdout mismatch for gbshape_lazyscope"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac 2.13.16: the recorded expectation,
/// scalac's stdout and ours all have to agree.
#[test]
fn real_scalac_dual_run_gbshape_lazyscope() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff gbshape_lazyscope: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("gbshape_lazyscope.scala");
    let ref_out = tmp_dir("gbshape_lazyscope-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(
        status.success(),
        "real scalac failed to compile gbshape_lazyscope"
    );
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout("gbshape_lazyscope"),
        "recorded expectation for gbshape_lazyscope does not match real scalac"
    );
    let out = compile_fixture_with("gbshape_lazyscope", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        reference,
        "stdout differs from real scalac for gbshape_lazyscope"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// Every real op-assignment still rewrites, and a member named `===` is still
/// called rather than rewritten.
#[test]
fn fixtures_gbshape_opassign_runs() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip gbshape_opassign: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("gbshape_opassign", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("gbshape_opassign"),
        "stdout mismatch for gbshape_opassign"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_gbshape_opassign() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff gbshape_opassign: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("gbshape_opassign.scala");
    let ref_out = tmp_dir("gbshape_opassign-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(
        status.success(),
        "real scalac failed to compile gbshape_opassign"
    );
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout("gbshape_opassign"),
        "recorded expectation for gbshape_opassign does not match real scalac"
    );
    let out = compile_fixture_with("gbshape_opassign", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        reference,
        "stdout differs from real scalac for gbshape_opassign"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// The load-bearing half. Real scalac 2.13.16 rejects lines 18, 23, 27 and 31
/// and no others; before this slice line 18 -- `b === false` on a
/// `var b: Boolean` -- **compiled**, as an assignment of the comparison's
/// result, and line 23 reported a type mismatch instead of the missing member.
#[test]
fn fixtures_gbshape_opassign_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let errs = compile_errors(
        "gbshape_opassign_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    let lines: Vec<u32> = errs.iter().map(|(l, _)| *l).collect();
    assert_eq!(lines, vec![18, 23, 27, 31], "errors were: {errs:?}");
    assert!(
        errs[0].1.contains("value === is not a member of Boolean"),
        "line 18: {:?}",
        errs[0].1
    );
    assert!(
        errs[1].1.contains("value === is not a member of Int"),
        "line 23: {:?}",
        errs[1].1
    );
    assert!(
        errs[2].1.contains("value === is not a member of Box"),
        "line 27: {:?}",
        errs[2].1
    );
    assert!(
        errs[3].1.contains("value x_= is not a member of P"),
        "line 31: {:?}",
        errs[3].1
    );
}
