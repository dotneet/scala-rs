//! E2E tests for the `agent/sysout` slice: **a qualified `println` is a call
//! on its own receiver.**
//!
//! Defect 1 of the "Three defects found and not fixed here" list in the
//! `agent/libprelude` section of `docs/scala-library.md`:
//!
//! ```scala
//! package scala { object Predef { type String = java.lang.String } }
//! object Main { def main(a: Array[String]): Unit = java.lang.System.out.println("hello") }
//! ```
//!
//! compiles in `--scala-library` mode, passes `-Xverify:all`, and then throws
//! `NoSuchMethodError: 'void scala.Predef$.println(java.lang.Object)'`.
//!
//! The cause is in codegen, not in the import. `gen_apply` dispatched the
//! print intrinsic on the *name* alone --
//! `matches!(ic, Intrinsic::Println) || fun.name() == Some("println")` -- and
//! both emitters discard the qualifier: `gen_predef_println` loads
//! `scala/Predef$.MODULE$`, `gen_println` loads `java/lang/System.out`. Every
//! selection spelled `println` was therefore rewritten into a call on
//! `Predef`, whatever receiver the program wrote.
//!
//! Two consequences, and the second one is not about `Predef` at all:
//!
//! 1. With a source-defined `scala.Predef` the run emits its own
//!    `scala/Predef$.class`, which shadows the jar's on the classpath, so the
//!    hijacked call names a method that is not there. This is the reported
//!    defect, and it is why it needs the *combination*: without the source
//!    `Predef` the jar's `println` happens to exist and do the same thing, so
//!    the program prints the right text through the wrong method.
//! 2. `java.lang.System.err.println(x)` printed to **stdout**, in both modes,
//!    with or without a source `Predef`. It compiles, it verifies, and every
//!    compile-only check in this repository is green on it.
//!
//! The fix restricts the name-only fallback to a call with no symbol at all
//! (`gen_expr::unresolved_print`). `Intrinsic::Println` / `Intrinsic::Print`
//! are set exclusively on the prelude's own `Predef` members
//! (`prelude_predef2::add_predef_members`), in both `--scala-library` and
//! `--no-scala-library`, so the intrinsic already names the exact set that may
//! be rewritten.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.
//! Fixture prefix: `sysout_`.

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
        "scala-rs-sysout-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// Compile `src` (as a file of its own) and return the output directory.
///
/// Panics with the diagnostics on failure: every program here is one both
/// compilers accept, so a compile error is the test failing, not a case to
/// report.
fn compile(tag: &str, src: &str, extra: &[&str]) -> PathBuf {
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        file.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {tag} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// Run `Main` under the verifier and return `(stdout, stderr)` **separately**
/// -- which stream a line arrived on is the whole point of half these tests.
fn run_java(out: &Path, cp_extra: Option<&str>) -> (String, String) {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    (stdout, stderr)
}

// ---------------------------------------------------------------- the defect

/// The reported reproduction, verbatim, and it has to *run*.
///
/// Before the fix this compiled and verified and then threw
/// `NoSuchMethodError: 'void scala.Predef$.println(java.lang.Object)'`,
/// because the emitted `scala/Predef$.class` -- the one this very program
/// defines -- shadows the jar's and has no `println`.
#[test]
fn qualified_system_out_println_runs_with_a_source_predef() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile(
        "repro",
        r#"
package scala { object Predef { type String = java.lang.String } }

object Main {
  def main(args: Array[String]): Unit = java.lang.System.out.println("hello")
}
"#,
        &["--scala-library", jar_s],
    );
    let (stdout, _) = run_java(&out, Some(jar_s));
    assert_eq!(
        stdout, "hello\n",
        "the qualified call must reach System.out"
    );
    let _ = fs::remove_dir_all(out.parent().unwrap());
}

/// The other half of the same root, and it needs no `Predef` at all: a
/// qualified call on the *error* stream was emitted on `Predef` / `System.out`
/// and printed to stdout. Both modes, because both emitters had the defect --
/// `gen_predef_println` under `--scala-library`, `gen_println` without it.
#[test]
fn system_err_println_goes_to_stderr_in_both_modes() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    java.lang.System.err.println("to-stderr")
    java.lang.System.err.print("err-no-newline")
    java.lang.System.out.println("to-stdout")
  }
}
"#;
    let mut modes: Vec<(&str, Vec<String>, Option<String>)> =
        vec![("private", vec!["--no-scala-library".into()], None)];
    if let Some(jar) = scala_library_jar() {
        let jar_s = jar.to_str().unwrap().to_string();
        modes.push((
            "jar",
            vec!["--scala-library".into(), jar_s.clone()],
            Some(jar_s),
        ));
    }
    for (tag, extra, cp) in modes {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("err-{tag}"), src, &flags);
        let (stdout, stderr) = run_java(&out, cp.as_deref());
        assert_eq!(stdout, "to-stdout\n", "[{tag}] stdout");
        assert_eq!(stderr, "to-stderr\nerr-no-newline", "[{tag}] stderr");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// The fixture: both halves in one program, dual-run against real scalac
/// 2.13.16 compiling the same file against the same jar.
///
/// It pins the fix from swinging the other way as well. The *unqualified*
/// `println` must still resolve through the source `scala.Predef` that
/// `crates/typer/src/predef_reimport.rs` put in scope, and must call that
/// object's own method -- the `P:` / `p:` prefixes in the expected output are
/// how the source `Predef` is told apart from the jar's.
#[test]
fn the_fixture_matches_scalac_on_both_streams() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let name = "sysout_predef";
    let jar_s = jar.to_str().unwrap();
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let (stdout, stderr) = run_java(&out, Some(jar_s));
    assert_eq!(stdout, expected_stdout(name), "stdout mismatch for {name}");
    assert_eq!(
        stderr, "qualified-err\n",
        "the System.err call must reach stderr, and nothing else may"
    );
    assert!(
        !stdout.contains("qualified-err"),
        "the System.err call must not reach stdout:\n{stdout}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------- what must not have moved

/// `Predef.println` is still the intrinsic, spelled either way, in both modes.
///
/// The fix keys on `Intrinsic::Println`, which `prelude_predef2` sets on the
/// prelude's own members; a fix that keyed on the tree shape instead would
/// lose `scala.Predef.println(x)`, and one that keyed on the owner's name
/// would lose nothing here but would claim a source `Predef` too.
#[test]
fn predef_println_still_works_unqualified_and_qualified() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  def main(args: Array[String]): Unit = {
    println("bare")
    print("bare-print")
    println()
    scala.Predef.println("qualified-predef")
    println(42)
    println(())
  }
}
"#;
    let expect = "bare\nbare-print\nqualified-predef\n42\n()\n";
    let mut modes: Vec<(&str, Vec<String>, Option<String>)> =
        vec![("private", vec!["--no-scala-library".into()], None)];
    if let Some(jar) = scala_library_jar() {
        let jar_s = jar.to_str().unwrap().to_string();
        modes.push((
            "jar",
            vec!["--scala-library".into(), jar_s.clone()],
            Some(jar_s),
        ));
    }
    for (tag, extra, cp) in modes {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("predef-{tag}"), src, &flags);
        let (stdout, stderr) = run_java(&out, cp.as_deref());
        assert_eq!(stdout, expect, "[{tag}] stdout");
        assert_eq!(stderr, "", "[{tag}] nothing belongs on stderr");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// A user-defined method that happens to be called `println` is that method,
/// not `Predef`'s. Same root, and the one shape a reader is most likely to
/// write by accident.
#[test]
fn a_user_defined_println_is_not_predefs() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile(
        "userdef",
        r#"
class Logger(prefix: String) {
  def println(x: Any): Unit = java.lang.System.out.println(prefix + x.toString)
  def print(x: Any): Unit = java.lang.System.out.print(prefix + x.toString)
}

object Main {
  def main(args: Array[String]): Unit = {
    val log = new Logger("[log] ")
    log.println("one")
    log.print("two")
    println("")
  }
}
"#,
        &["--scala-library", jar_s],
    );
    let (stdout, _) = run_java(&out, Some(jar_s));
    assert_eq!(
        stdout, "[log] one\n[log] two\n",
        "the user's method must run"
    );
    let _ = fs::remove_dir_all(out.parent().unwrap());
}
