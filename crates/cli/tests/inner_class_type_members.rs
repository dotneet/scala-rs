//! An inner class of a trait written in the trait's abstract type member
//! (`trait CompA { type KA; final class InnerA(val k: KA) }`), instantiated
//! and read in a class that fixes the member (`type KA = Int`) -- the cake
//! pattern's shape. `new InnerA(1)` wanted a `CompA.KA`, and `AppA.mkI(3).get`
//! was one: the inner class was read through its prefix for the enclosing
//! class's type parameters but not for its type aliases.
//!
//! Both programs are compiled by scalac and by scala-rs and run; the output
//! has to agree.

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
        "scala-rs-inner-class-type-members-{tag}-{}-{nanos}-{seq}",
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

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn run_scalac(scalac: &Path, cp: &str, out: &Path, src: &Path) {
    let mut cmd = Command::new(scalac);
    cmd.env("JAVA_OPTS", "-Xmx2g -Xss8m").arg("-nowarn");
    if !cp.is_empty() {
        cmd.args(["-cp", cp]);
    }
    let o = cmd
        .arg("-d")
        .arg(out)
        .arg(src)
        .output()
        .expect("run scalac");
    assert!(
        o.status.success(),
        "scalac failed on {}: {}",
        src.display(),
        String::from_utf8_lossy(&o.stdout)
    );
}

fn run_ours(cp: &str, jar: &Path, out: &Path, src: &Path) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile").arg(src).arg("-d").arg(out);
    if !cp.is_empty() {
        cmd.args(["-cp", cp]);
    }
    let o = cmd
        .arg("--scala-library")
        .arg(jar)
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "scala-rs rejected {}: {}{}",
        src.display(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

fn run_java(cp: &str, main: &str) -> String {
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, main])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn an_inner_class_reads_the_enclosing_aliases_like_scalac() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    let src = fixtures_dir().join("inner_class_type_members.scala");
    let ours = tmp_dir("src");
    run_ours("", &jar, &ours, &src);
    let actual = run_java(&format!("{}:{}", ours.display(), jar.display()), "Main");
    let _ = fs::remove_dir_all(&ours);
    if let Some(scalac) = scalac() {
        let theirs = tmp_dir("src-scalac");
        run_scalac(&scalac, "", &theirs, &src);
        let reference = run_java(&format!("{}:{}", theirs.display(), jar.display()), "Main");
        let _ = fs::remove_dir_all(&theirs);
        assert_eq!(reference, expected("inner_class_type_members"));
    }
    assert_eq!(actual, expected("inner_class_type_members"));
}

#[test]
fn an_inner_class_from_a_class_file_reads_the_aliases_too() {
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not present");
        return;
    };
    let lib = tmp_dir("lib");
    run_scalac(
        &scalac,
        "",
        &lib,
        &fixtures_dir().join("inner_class_type_members_lib.scala"),
    );
    let client = fixtures_dir().join("inner_class_type_members_client.scala");
    let theirs = tmp_dir("client-scalac");
    run_scalac(&scalac, &lib.to_string_lossy(), &theirs, &client);
    let ours = tmp_dir("client");
    run_ours(&lib.to_string_lossy(), &jar, &ours, &client);
    let cp = |out: &Path| format!("{}:{}:{}", out.display(), lib.display(), jar.display());
    let reference = run_java(&cp(&theirs), "Client");
    assert_eq!(reference, expected("inner_class_type_members_client"));
    assert_eq!(run_java(&cp(&ours), "Client"), reference);
    for d in [&lib, &theirs, &ours] {
        let _ = fs::remove_dir_all(d);
    }
}
