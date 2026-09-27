//! An implicit found as a member of an enclosing class -- not only the
//! innermost one -- is selected on that class's `this`, as nsc does. It was
//! selected through a companion object that inherits the member: inside cats'
//! `EvalInstances`, an anonymous instance's `Functor[Eval]` read
//! `Eval.MODULE$`, still null while `object Eval`'s parent constructor runs,
//! and every cats program touching `Eval` failed in its initializer.
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
        "scala-rs-outer-this-implicit-{tag}-{}-{nanos}-{seq}",
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

fn run_java(out: &Path, jar: &Path) -> String {
    let o = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "outerimpl.Main",
        ])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "Main failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn an_enclosing_class_implicit_is_selected_on_its_this() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    let src = fixtures_dir().join("outer_this_implicit.scala");
    let ours = tmp_dir("ours");
    let o = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .arg("-d")
        .arg(&ours)
        .arg("--scala-library")
        .arg(&jar)
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let expected =
        fs::read_to_string(fixtures_dir().join("expected/outer_this_implicit.txt")).unwrap();
    assert_eq!(run_java(&ours, &jar), expected);
    let _ = fs::remove_dir_all(&ours);
    if let Some(scalac) = scalac() {
        let theirs = tmp_dir("scalac");
        let o = Command::new(scalac)
            .env("JAVA_OPTS", "-Xmx2g -Xss8m")
            .arg("-d")
            .arg(&theirs)
            .arg(&src)
            .output()
            .expect("run scalac");
        assert!(o.status.success(), "scalac rejected the fixture");
        assert_eq!(run_java(&theirs, &jar), expected);
        let _ = fs::remove_dir_all(&theirs);
    }
}
