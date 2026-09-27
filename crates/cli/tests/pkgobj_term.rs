//! A package object that declares a type alias and an object of the same name
//! (cats' `package object data`: `type State[S, A]` and `object State`,
//! `type Reader[A, B]` and `object Reader`). A term selection written out in
//! full, `cats.data.State.modify(f)`, has to reach the object. Once the alias
//! had been read -- any mention of the type does that -- the selection stopped
//! at it, typed the alias as a value, and selected the aliased class's
//! *instance* members: `IndexedStateT#modify`, whose lambda parameter is the
//! unsolved `SB`, so `m.updated(...)` was "not a member of S".
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
        "scala-rs-pkgobj-term-{tag}-{}-{nanos}-{seq}",
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

fn coursier_jar(group: &str, artifact: &str, version: &str) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let jar = home
        .join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2")
        .join(group)
        .join(version)
        .join(format!("{artifact}-{version}.jar"));
    jar.is_file().then_some(jar)
}

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn run_scalac(scalac: &Path, cp: &str, out: &Path, src: &Path) {
    let mut cmd = Command::new(scalac);
    cmd.env("JAVA_OPTS", "-Xmx2g -Xss8m");
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
    let o = Command::new(bin())
        .arg("compile")
        .arg(src)
        .arg("-d")
        .arg(out)
        .args(["-cp", cp])
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
fn full_path_to_a_package_object_member_beside_its_alias_is_the_object() {
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not present");
        return;
    };
    let lib = tmp_dir("lib");
    run_scalac(
        &scalac,
        "",
        &lib,
        &fixtures_dir().join("pkgobj_term_lib.scala"),
    );
    let client = fixtures_dir().join("pkgobj_term_client.scala");
    let theirs = tmp_dir("client-scalac");
    run_scalac(&scalac, &lib.to_string_lossy(), &theirs, &client);
    let ours = tmp_dir("client");
    run_ours(&lib.to_string_lossy(), &jar, &ours, &client);
    let cp = |out: &Path| format!("{}:{}:{}", out.display(), lib.display(), jar.display());
    let reference = run_java(&cp(&theirs), "Client");
    assert_eq!(reference, expected("pkgobj_term_client"));
    assert_eq!(run_java(&cp(&ours), "Client"), reference);
    for d in [&lib, &theirs, &ours] {
        let _ = fs::remove_dir_all(d);
    }
}

#[test]
fn cats_state_and_reader_written_out_in_full_run_like_scalac() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    let (Some(core), Some(kernel)) = (
        coursier_jar("org/typelevel/cats-core_2.13", "cats-core_2.13", "2.13.0"),
        coursier_jar(
            "org/typelevel/cats-kernel_2.13",
            "cats-kernel_2.13",
            "2.13.0",
        ),
    ) else {
        eprintln!("skip: cats 2.13.0 jars not in the Coursier cache");
        return;
    };
    let cats = format!("{}:{}", core.display(), kernel.display());
    let src = fixtures_dir().join("pkgobj_term_cats.scala");
    let ours = tmp_dir("cats");
    run_ours(&cats, &jar, &ours, &src);
    let cp = |out: &Path| format!("{}:{}:{}", out.display(), cats, jar.display());
    let actual = run_java(&cp(&ours), "tf.Main");
    let _ = fs::remove_dir_all(&ours);
    let reference = match scalac() {
        Some(scalac) => {
            let theirs = tmp_dir("cats-scalac");
            run_scalac(&scalac, &cats, &theirs, &src);
            let out = run_java(&cp(&theirs), "tf.Main");
            let _ = fs::remove_dir_all(&theirs);
            assert_eq!(out, expected("pkgobj_term_cats"));
            out
        }
        None => expected("pkgobj_term_cats"),
    };
    assert_eq!(actual, reference);
}
