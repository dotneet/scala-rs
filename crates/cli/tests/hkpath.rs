//! E2E tests for the `agent/hkpath` slice: a *higher-kinded* type member read
//! through an instance prefix.
//!
//! `agent/projection` made a path-dependent member a symbol allocated per
//! (path, declaration) pair, and deliberately stopped at first-order members.
//! cats is written the other way round: `trait NonEmptyParallel[M[_]] { type
//! F[_]; def parallel: M ~> F }`, every instance refines `type F[_]`, and
//! `P.F` on a particular instance came out as the declaration's own unrefined
//! member. The two sides then printed almost identically and compared unequal:
//!
//! ```text
//! found:    Applicative[[γ$24$]Nested[NonEmptyParallel.F, [β$5$]Validated[E, β$5$], γ$24$]]
//! required: Applicative[[γ$29$]Nested[$anon$1667.F,      [β$28$]Validated[E, β$28$], γ$29$]]
//! ```
//!
//! Two things had to change. The restriction to first-order members is gone
//! from `can_be_path_member`; and the dependent-method-type substitution now
//! runs on an *inserted implicit argument* and reaches inside a type lambda,
//! whose body lives beside its symbol rather than in the type.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `hkp` prefix.

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
        "scala-rs-hkpath-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: a path-dependent member that erased differently from the
/// declaration it stands for would show up here as a verification failure
/// rather than as a silent difference in the output.
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

fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None, "Main"),
            expected_stdout(name),
            "stdout mismatch for {name} (private runtime)"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

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
    assert_eq!(
        run_java(&out, Some(jar_s), "Main"),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
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

/// The expected output is what real scalac 2.13.16 prints for the same source.
#[test]
fn fixtures_hkp_member() {
    dual_run_fixture("hkp_member");
}

#[test]
fn fixtures_hkp_member_private() {
    check_private("hkp_member");
}

/// The fixture is only worth its runtime if real scalac accepts it too -- and
/// prints exactly what `expected/hkp_member.txt` records.
#[test]
fn scalac_agrees_hkp_member_runs() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-hkp-member");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("hkp_member.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected hkp_member.scala: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        expected_stdout("hkp_member"),
        "scalac's own run does not match the recorded output"
    );
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------- negative: paths stay apart

/// Two instances' `F` are two different type constructors, in both
/// directions, and a name the prefix's class does not declare is still not a
/// member. Before this slice the first two compiled: `p.F` and `q.F` were both
/// the declaration `Par.F`.
#[test]
fn fixtures_hkp_member_bad_is_rejected() {
    compile_fails(
        "hkp_member_bad",
        &["--no-scala-library"],
        &[
            "type mismatch; found: p.F[Int]  required: q.F[Int]",
            "type mismatch; found: q.F[Int]  required: p.F[Int]",
            "type G is not a member of Par[M]",
            "hkp_member_bad.scala:19",
            "hkp_member_bad.scala:22",
            "hkp_member_bad.scala:25",
        ],
    );
}

/// The same three, straight from scalac, at the same lines, so the fixture
/// cannot drift into asserting a distinction nsc does not make.
#[test]
fn scalac_agrees_hkp_member_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-hkp-member-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("hkp_member_bad.scala"))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted it: {err}");
    for needle in [
        "hkp_member_bad.scala:19: error: type mismatch",
        "underlying type p.F[Int]",
        "required: q.F[Int]",
        "hkp_member_bad.scala:22: error: type mismatch",
        "underlying type q.F[Int]",
        "required: p.F[Int]",
        "hkp_member_bad.scala:25: error: type G is not a member of Par[M]",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac output to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
