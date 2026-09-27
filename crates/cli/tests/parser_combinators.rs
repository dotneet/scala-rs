//! scala-parser-combinators names one thing `~` three ways: `case class ~`
//! declared in `trait Parsers` (class file `Parsers$$tilde`), its companion
//! (a member object, reached through the accessor `Parsers.$tilde()`), and
//! the method `Parser#~` that builds one. A grammar writes all of them:
//! `Parser[String ~ Int]`, `case a ~ op ~ b`, `P.~(a, b)`, `new P.~(a, b)`.
//! None of the first three resolved -- "not found: type ~", "not found:
//! value ~", "not found: extractor ~" -- because the class-file name was read
//! as a class `tilde` of a phantom `Parsers$`, and the pickle was asked about
//! the companion by its decoded name.
//!
//! The program is compiled by scalac and by scala-rs and run; the output has
//! to agree.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-parser-combinators-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn parser_combinators_jar() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let jar = home.join(
        "Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/scala-lang/modules/scala-parser-combinators_2.13/2.4.0/scala-parser-combinators_2.13-2.4.0.jar",
    );
    jar.is_file().then_some(jar)
}

fn run_java(out: &Path, cp: &str, main: &str) -> String {
    let o = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            main,
        ])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "{main} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Compile `fixture` with scala-rs (and with scalac when it is present)
/// against `cp`, run `main` from both, and compare the output with each
/// other and with `expected/<fixture>.txt`.
fn compiles_and_runs_like_scalac(fixture: &str, cp: Option<&Path>, main: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    let src = fixtures_dir().join(format!("{fixture}.scala"));
    let ours = tmp_dir("ours");
    let mut cmd = Command::new(bin());
    cmd.arg("compile").arg(&src).arg("-d").arg(&ours);
    if let Some(cp) = cp {
        cmd.arg("-cp").arg(cp);
    }
    let o = cmd
        .arg("--scala-library")
        .arg(&jar)
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "scala-rs rejected {fixture}: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let run_cp = match cp {
        Some(cp) => format!("{}:{}", cp.display(), jar.display()),
        None => jar.display().to_string(),
    };
    let actual = run_java(&ours, &run_cp, main);
    let _ = fs::remove_dir_all(&ours);
    let expected =
        fs::read_to_string(fixtures_dir().join(format!("expected/{fixture}.txt"))).unwrap();
    if let Some(scalac) = scalac() {
        let theirs = tmp_dir("scalac");
        let mut cmd = Command::new(scalac);
        cmd.env("JAVA_OPTS", "-Xmx2g -Xss8m");
        if let Some(cp) = cp {
            cmd.arg("-cp").arg(cp);
        }
        let o = cmd
            .arg("-d")
            .arg(&theirs)
            .arg(&src)
            .output()
            .expect("run scalac");
        assert!(o.status.success(), "scalac rejected {fixture}");
        assert_eq!(run_java(&theirs, &run_cp, main), expected);
        let _ = fs::remove_dir_all(&theirs);
    }
    assert_eq!(actual, expected);
}

#[test]
fn a_grammar_using_tilde_as_type_extractor_and_value_runs_like_scalac() {
    let Some(pc) = parser_combinators_jar() else {
        eprintln!("skip: scala-parser-combinators 2.4.0 not present");
        return;
    };
    compiles_and_runs_like_scalac("parser_combinators_tilde", Some(&pc), "pcg.Main");
}

/// Matching a parse result and passing strings to the by-name combinators,
/// as every grammar does. `case Success(v, _)` called `Parsers`' three-argument
/// `def Success` for the object (`VerifyError`) and typed `v` as `T`;
/// `NoSuccess`, `Success[_]` and `Error` as types were read as the standard
/// library's classes or as package-level stubs with no parents ("pattern type
/// is incompatible"); and `"(" ~> expr <~ ")"` handed the `String` itself to
/// `<~` (`ClassCastException`).
#[test]
fn matching_parse_results_and_string_combinators_run_like_scalac() {
    let Some(pc) = parser_combinators_jar() else {
        eprintln!("skip: scala-parser-combinators 2.4.0 not present");
        return;
    };
    compiles_and_runs_like_scalac("parser_combinators_results", Some(&pc), "pcr.Main");
}

/// The by-name half of the last case with no library involved: a view
/// applies to an argument of a by-name parameter whose type still mentions
/// the callee's type parameter, as it does for a strict one.
#[test]
fn a_by_name_argument_takes_a_view_like_a_strict_one() {
    compiles_and_runs_like_scalac("byname_view", None, "Main");
}
