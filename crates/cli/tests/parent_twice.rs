//! nsc `Typers.validateParentClasses`: a class, trait or object that names
//! the same class or trait twice among its parents is rejected with
//! `<kind> <name> is inherited twice`, at every parent that has a twin.
//! scala-rs accepted it.
//!
//! The legal neighbour is `fixDuplicateSyntheticParents`: `Product`,
//! `ProductN` and `Serializable` written twice are silently kept once. That
//! fixture runs, and scala-rs's class files used to list
//! `java/io/Serializable` twice, which the JVM refuses to load.
//!
//! Fixtures: `tests/fixtures/parent_twice_{neg,pos}.scala`; the expected text
//! is scalac 2.13.16's console output (`--diagnostics=scalac`).

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
        "scala-rs-parent-twice-{tag}-{}-{nanos}-{seq}",
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

/// Compile a fixture by its bare name from the fixtures directory, so both
/// compilers print the same file name. Returns success and all output.
fn compile(compiler: &Path, scala_rs: bool, fixture: &str, out: &Path) -> (bool, String) {
    let mut cmd = Command::new(compiler);
    cmd.current_dir(fixtures_dir());
    if scala_rs {
        let jar = scala_library_jar().expect("jar");
        cmd.arg("compile")
            .arg("--diagnostics=scalac")
            .arg("--scala-library")
            .arg(jar);
    } else {
        cmd.env("JAVA_OPTS", "-Xmx2g -Xss8m");
    }
    cmd.arg("-d").arg(out).arg(fixture);
    let o = cmd.output().expect("run compiler");
    (
        o.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
    )
}

fn expected() -> String {
    fs::read_to_string(fixtures_dir().join("expected/parent_twice_neg.txt")).unwrap()
}

fn run_main(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn parents_inherited_twice_are_rejected_like_scalac() {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    let out = tmp_dir("neg");
    let (ok, text) = compile(&bin(), true, "parent_twice_neg.scala", &out);
    let _ = fs::remove_dir_all(&out);
    assert!(!ok, "parent_twice_neg.scala compiled:\n{text}");
    assert_eq!(text, expected());
}

#[test]
fn parents_inherited_twice_expected_is_scalacs() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let out = tmp_dir("neg-scalac");
    let (ok, text) = compile(&sc, false, "parent_twice_neg.scala", &out);
    let _ = fs::remove_dir_all(&out);
    assert!(!ok, "scalac accepted parent_twice_neg.scala");
    assert_eq!(text, expected());
}

#[test]
fn repeated_synthetic_parents_compile_and_run_like_scalac() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let ours = tmp_dir("pos");
    let (ok, text) = compile(&bin(), true, "parent_twice_pos.scala", &ours);
    assert!(
        ok,
        "parent_twice_pos.scala must compile, as under scalac:\n{text}"
    );
    let actual = run_main(&ours, &jar);
    let _ = fs::remove_dir_all(&ours);
    let expected = match scalac() {
        Some(sc) => {
            let theirs = tmp_dir("pos-scalac");
            let (ok, text) = compile(&sc, false, "parent_twice_pos.scala", &theirs);
            assert!(ok, "scalac rejected parent_twice_pos.scala:\n{text}");
            let out = run_main(&theirs, &jar);
            let _ = fs::remove_dir_all(&theirs);
            out
        }
        // scalac 2.13.16's output for the fixture.
        None => "List(7, 7, 0, 7, 1, 3)\nList(true, true, true)\n".to_string(),
    };
    assert_eq!(actual, expected);
}
