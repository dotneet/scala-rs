//! Programs we compiled, loaded and ran — and then got a different answer to.
//!
//! The third pass over that pile (`docs/scala-corpus.md`, "The 113 that ran and
//! printed the wrong answer"). Both roots here are invisible to every other
//! check in the battery: the classfiles load, verify and lint exactly as
//! before, and only running the program against scalac's own output shows the
//! difference.
//!
//! * **`c.prefix` is a typed tree.** A bare name that resolves to a member of
//!   an enclosing template reaches a macro implementation as
//!   `Main.this.macros`; the bridge sent the source `Ident`.
//! * **What our `ScalaSignature` says about constructors.** A constructor's
//!   result type carries the prefix an instance is reached through, and an
//!   `object`'s module class has a primary constructor at all.
//!
//! Every assertion is dual-run: real scalac 2.13.16 compiles the same fixtures
//! and its output is compared against ours, so the recorded expectation is nsc's
//! answer and not ours.

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
        "scala-rs-rw3-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scala_reflect_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
        .unwrap_or_else(|e| panic!("read expected/{name}.txt: {e}"))
}

/// Everything these tests need. `javac` too: the macro bridge builds its engine.
fn prerequisites(tag: &str) -> bool {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("skip {tag}: java / javac not available");
        return false;
    }
    if scala_library_jar().is_none() || scala_reflect_jar().is_none() {
        eprintln!("skip {tag}: scala-library / scala-reflect not obtainable");
        return false;
    }
    true
}

fn joined(paths: &[&Path]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// Compile one fixture with scala-rs, asserting it succeeded.
fn compile_ours(name: &str, out: &Path, cp: &[&Path]) {
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let mut all: Vec<&Path> = vec![reflect.as_path()];
    all.extend_from_slice(cp);
    let o = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            &joined(&all),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "scala-rs rejected {name}.scala:\n{}{}",
        String::from_utf8_lossy(&o.stderr),
        String::from_utf8_lossy(&o.stdout)
    );
}

/// The same fixture through real scalac 2.13.16.
fn compile_scalac(scalac: &Path, name: &str, out: &Path, cp: &[&Path]) {
    let reflect = scala_reflect_jar().unwrap();
    let mut all: Vec<&Path> = vec![reflect.as_path()];
    all.extend_from_slice(cp);
    let o = Command::new(scalac)
        .args([
            "-cp",
            &joined(&all),
            "-d",
            out.to_str().unwrap(),
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
        ])
        .output()
        .expect("run scalac");
    assert!(
        o.status.success(),
        "real scalac rejected {name}.scala:\n{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// Run `Main` out of `cp` and return its stdout.
fn run_main(cp: &[&Path], what: &str) -> String {
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &joined(cp), "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java Main failed for {what}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// `c.prefix` reaches the implementation with the qualifier nsc gives it.
///
/// Three call sites in one fixture: a `val` of the enclosing object, a path
/// through one, and a *local* `val`. Only the first two are members of a
/// template, so only those two are qualified — the third is what says the
/// bridge did not simply prepend `this` to every name it saw.
#[test]
fn rw3_macro_prefix_is_a_typed_tree() {
    if !prerequisites("rw3_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("impl");
    let uses = tmp_dir("use");

    compile_ours("rw3_impl", &impls, &[]);
    compile_ours("rw3_use", &uses, &[&impls]);

    assert_eq!(
        run_main(&[&uses, &impls, &reflect, &jar], "rw3_use"),
        expected_stdout("rw3_use")
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// The same two files through real scalac, which is what makes the recorded
/// expectation nsc's answer rather than ours.
#[test]
fn rw3_macro_prefix_matches_real_scalac() {
    if !prerequisites("rw3_use scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rw3_use scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let impls = tmp_dir("impl-scalac");
    let uses = tmp_dir("use-scalac");

    compile_scalac(&scalac, "rw3_impl", &impls, &[]);
    compile_scalac(&scalac, "rw3_use", &uses, &[&impls]);

    assert_eq!(
        run_main(&[&uses, &impls, &reflect, &jar], "rw3_use scalac"),
        expected_stdout("rw3_use")
    );
    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
}

/// What runtime reflection reads back out of the `ScalaSignature` we write:
/// a constructor's result-type prefix, and the primary constructor of an
/// `object`'s module class.
#[test]
fn rw3_pickled_constructors_read_back_as_nsc_writes_them() {
    if !prerequisites("rw3_reflect") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let out = tmp_dir("reflect");

    compile_ours("rw3_reflect", &out, &[]);

    assert_eq!(
        run_main(&[&out, &reflect, &jar], "rw3_reflect"),
        expected_stdout("rw3_reflect")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture compiled by real scalac. Its `ScalaSignature` is the one
/// ours is being held to, so if this line ever disagrees with the expectation
/// it is the expectation that is wrong.
#[test]
fn rw3_pickled_constructors_match_real_scalac() {
    if !prerequisites("rw3_reflect scalac diff") {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip rw3_reflect scalac diff: scalac not obtainable");
        return;
    };
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let out = tmp_dir("reflect-scalac");

    compile_scalac(&scalac, "rw3_reflect", &out, &[]);

    assert_eq!(
        run_main(&[&out, &reflect, &jar], "rw3_reflect scalac"),
        expected_stdout("rw3_reflect")
    );
    let _ = fs::remove_dir_all(&out);
}
