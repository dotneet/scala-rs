//! E2E tests for the `agent/hkselfalias` slice: a type member read through
//! the *self alias* of the class that declares it.
//!
//! slick's profile cake writes
//!
//! ```scala
//! trait Profile extends TypesComponent { self: Profile =>
//!   trait API { type ColumnType[T] = self.ColumnType[T] }
//! }
//! ```
//!
//! `API` is not itself a `Profile`, so `self` names the *enclosing* profile
//! instance and the concrete profile mixing `API` in is what settles
//! `ColumnType`. Until `agent/hkpath` the alias recorded the bare declaration
//! and the ordinary name walk in `expand_type_members` reduced it; with a
//! prefix on it that walk refuses to look, and `api.ColumnType[Int]` stayed
//! the abstract `self.ColumnType[Int]` (`--test tmember` went red on `main`).
//!
//! The reduction is driven by the *written prefix*, never by the class doing
//! the reading: `Mem.api.ColumnType[Int]` inside `object Jdbc` is still
//! `MemType[Int]`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `hkself` prefix.

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
        "scala-rs-hkselfalias-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: a member reduced through the wrong profile would erase to
/// the wrong class, and that shows up here rather than as a silent difference.
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

#[test]
fn fixtures_hkself_member_private() {
    let out = compile_fixture_with("hkself_member", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None, "Main"),
            expected_stdout("hkself_member"),
            "stdout mismatch for hkself_member (private runtime)"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_hkself_member_dual_run() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("hkself_member", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s), "Main"),
        expected_stdout("hkself_member"),
        "stdout mismatch for library dual-run hkself_member"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The fixture is only worth its runtime if real scalac accepts it too, and
/// prints exactly what `expected/hkself_member.txt` records.
#[test]
fn scalac_agrees_hkself_member_runs() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-hkself-member");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("hkself_member.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected hkself_member.scala: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        expected_stdout("hkself_member"),
        "scalac's own run does not match the recorded output"
    );
    let _ = fs::remove_dir_all(&out);
}

// --------------------------------------- negative: the prefix still decides

/// Two profiles settle `ColumnType` differently and a profile that leaves it
/// deferred settles nothing, so the reduction is not a widening. The first is
/// the one that says the *prefix* decides: `Mem.api.ColumnType[Int]` is
/// written inside `object Jdbc`, whose own `ColumnType` is `JdbcType`.
#[test]
fn fixtures_hkself_member_bad_is_rejected() {
    compile_fails(
        "hkself_member_bad",
        &["--no-scala-library"],
        &[
            "type mismatch; found: JdbcType[Int]  required: MemType[Int]",
            "hkself_member_bad.scala:29",
            "type mismatch; found: JdbcType[Int]  required: self.ColumnType[Int]",
            "hkself_member_bad.scala:41",
        ],
    );
}

/// The same two, straight from scalac, at the same lines and against the same
/// types, so the fixture cannot drift into asserting a distinction nsc does
/// not make -- nor into asserting one it makes for a different reason.
#[test]
fn scalac_agrees_hkself_member_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-hkself-member-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("hkself_member_bad.scala"))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted it: {err}");
    for needle in [
        "hkself_member_bad.scala:29: error: type mismatch",
        "required: Mem.api.ColumnType[Int]",
        "(which expands to)  MemType[Int]",
        "hkself_member_bad.scala:41: error: type mismatch",
        "required: OpenProfile.this.api.ColumnType[Int]",
        "(which expands to)  OpenProfile.this.ColumnType[Int]",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac output to contain {needle:?}, got: {err}"
        );
    }
    // scalac accepts the two legal reads (lines 27 and 34) and reports
    // nothing else. On the pre-fix binary those two failed as well, so a
    // count is what tells "rejected for the right reason" from "rejected".
    assert!(
        err.contains("2 errors"),
        "expected exactly two scalac errors, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
