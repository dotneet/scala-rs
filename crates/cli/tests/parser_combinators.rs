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

fn run_java(out: &Path, cp: &str) -> String {
    let o = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            "pcg.Main",
        ])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "pcg.Main failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn a_grammar_using_tilde_as_type_extractor_and_value_runs_like_scalac() {
    let (Some(jar), Some(pc)) = (scala_library_jar(), parser_combinators_jar()) else {
        eprintln!("skip: scala-library or scala-parser-combinators 2.4.0 not present");
        return;
    };
    let src = fixtures_dir().join("parser_combinators_tilde.scala");
    let ours = tmp_dir("ours");
    let o = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .arg("-d")
        .arg(&ours)
        .arg("-cp")
        .arg(&pc)
        .arg("--scala-library")
        .arg(&jar)
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "scala-rs rejected the grammar: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let run_cp = format!("{}:{}", pc.display(), jar.display());
    let actual = run_java(&ours, &run_cp);
    let _ = fs::remove_dir_all(&ours);
    let expected =
        fs::read_to_string(fixtures_dir().join("expected/parser_combinators_tilde.txt")).unwrap();
    if let Some(scalac) = scalac() {
        let theirs = tmp_dir("scalac");
        let o = Command::new(scalac)
            .env("JAVA_OPTS", "-Xmx2g -Xss8m")
            .arg("-cp")
            .arg(&pc)
            .arg("-d")
            .arg(&theirs)
            .arg(&src)
            .output()
            .expect("run scalac");
        assert!(o.status.success(), "scalac rejected the grammar");
        assert_eq!(run_java(&theirs, &run_cp), expected);
        let _ = fs::remove_dir_all(&theirs);
    }
    assert_eq!(actual, expected);
}
