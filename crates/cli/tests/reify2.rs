//! `reify { … }` over the typed body, and the type reifier behind the tag
//! materialiser. `docs/notes/reify-design.md`.
//!
//! Every fixture here is compiled and run by scala-rs *and* by real scalac
//! 2.13.16, and both runs must print the recorded output: a reified tree
//! that resolved a name in the wrong scope, or a tag built for the wrong
//! type, compiles and runs just as well as the right one, and only the
//! program's output tells them apart. The toolbox that evaluates the trees
//! needs scala-compiler.jar at run time, which is why this is not in
//! `e2e.rs` (the same reason `toolbox.rs` gives).

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

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-reify2-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn tool_available(what: &str) -> bool {
    Command::new(what)
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scala_reflect_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    p.is_file().then_some(p)
}

fn scala_compiler_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/lib/scala-compiler.jar");
    p.is_file().then_some(p)
}

fn find_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

/// scala-reflect.jar plus scala-compiler.jar: the toolbox lives in the
/// latter and is needed at both compile time and run time.
fn reflect_cp() -> String {
    format!(
        "{}:{}",
        scala_reflect_jar().unwrap().display(),
        scala_compiler_jar().unwrap().display()
    )
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
        .unwrap_or_else(|e| panic!("read expected/{name}.txt: {e}"))
}

fn prerequisites(tag: &str) -> bool {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("skip {tag}: java / javac not available");
        return false;
    }
    if scala_library_jar().is_none()
        || scala_reflect_jar().is_none()
        || scala_compiler_jar().is_none()
    {
        eprintln!("skip {tag}: scala-library / scala-reflect / scala-compiler not obtainable");
        return false;
    }
    true
}

/// Compile `<name>.scala` with scala-rs against the two jars.
fn compile(name: &str, out: &Path) -> std::process::Output {
    let jar = scala_library_jar().expect("scala-library");
    Command::new(bin())
        .args([
            "compile",
            fixture(name).to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            &reflect_cp(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile")
}

/// Compile `<name>.scala` with real scalac against the same jars.
fn scalac(name: &str, out: &Path) -> std::process::Output {
    Command::new(find_scalac().expect("scalac"))
        .args([
            "-cp",
            &reflect_cp(),
            "-d",
            out.to_str().unwrap(),
            fixture(name).to_str().unwrap(),
        ])
        .output()
        .expect("run scalac")
}

/// Run `Main` out of `dir` and return its stdout, asserting a clean exit.
fn run_main(dir: &Path, what: &str) -> String {
    let cp = format!(
        "{}:{}:{}",
        dir.display(),
        scala_library_jar().unwrap().display(),
        reflect_cp()
    );
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for {what}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// scala-rs compiles and runs `name`, printing the recorded output.
fn runs(name: &str) {
    if !prerequisites(name) {
        return;
    }
    let out_dir = tmp_dir(name);
    let out = compile(name, &out_dir);
    assert!(
        out.status.success(),
        "compile {name} failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&out_dir, name),
        expected_stdout(name),
        "stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// Real scalac compiles and runs `name`, printing the same recorded output.
/// This is what makes the recording mean something: the trees and types are
/// nsc's, not scala-rs's own invention.
fn matches_real_scalac(name: &str) {
    if !prerequisites(name) || find_scalac().is_none() {
        eprintln!("skip {name} scalac: scalac not available");
        return;
    }
    let out_dir = tmp_dir(&format!("{name}-scalac"));
    let out = scalac(name, &out_dir);
    assert!(
        out.status.success(),
        "scalac {name} failed: {}",
        diagnostics(&out)
    );
    assert_eq!(
        run_main(&out_dir, &format!("{name} (real scalac build)")),
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// Free terms for locals, parameters, local defs and lazy vals; members of
/// the enclosing object through `this`; closures, local classes and objects,
/// `while`, `try`, patterns, string interpolation, a nested `reify`, a `for`,
/// an anonymous class -- every one evaluated by the toolbox.
#[test]
fn reify2_free_runs() {
    runs("reify2_free");
}

#[test]
fn reify2_free_matches_real_scalac() {
    matches_real_scalac("reify2_free");
}

/// Free types (and the toolbox's report of them, naming where the type
/// parameter was defined), tags in scope, written types rebuilt by symbol.
#[test]
fn reify2_types_runs() {
    runs("reify2_types");
}

#[test]
fn reify2_types_matches_real_scalac() {
    matches_real_scalac("reify2_types");
}

/// The tag materialiser through the type reifier: nested classes,
/// singletons, aliases, free types, and `Predef.String` as the alias it is.
#[test]
fn reify2_tags_runs() {
    runs("reify2_tags");
}

#[test]
fn reify2_tags_matches_real_scalac() {
    matches_real_scalac("reify2_tags");
}

/// A class in a template-level block captures the block's locals -- the
/// shape the tree creator of a `reify` written there takes.
#[test]
fn reify2_capture_runs() {
    runs("reify2_capture");
}

#[test]
fn reify2_capture_matches_real_scalac() {
    matches_real_scalac("reify2_capture");
}

/// What is still refused, each named; never approximated.
#[test]
fn reify2_gaps_are_named() {
    if !prerequisites("reify2_bad") {
        return;
    }
    let out_dir = tmp_dir("reify2_bad");
    let out = compile("reify2_bad", &out_dir);
    assert!(!out.status.success(), "reify2_bad.scala should not compile");
    let text = diagnostics(&out);
    for want in [
        "an assignment to `w`, a `var` bound outside the reify body, is not reified yet",
        "`this` of `G`, a class with type parameters, is not reified yet",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    assert!(
        text.contains("cannot expand reify { ... }"),
        "the report should name reify:\n{text}"
    );
    let _ = fs::remove_dir_all(&out_dir);
}

/// Real scalac accepts `reify2_bad.scala`: the refusals above are
/// confessions, not rules.
#[test]
fn reify2_gaps_are_accepted_by_real_scalac() {
    if !prerequisites("reify2_bad scalac") || find_scalac().is_none() {
        eprintln!("skip reify2_bad scalac: scalac not available");
        return;
    }
    let out_dir = tmp_dir("reify2_bad-scalac");
    let out = scalac("reify2_bad", &out_dir);
    assert!(
        out.status.success(),
        "real scalac rejected reify2_bad.scala: {}",
        diagnostics(&out)
    );
    let _ = fs::remove_dir_all(&out_dir);
}
