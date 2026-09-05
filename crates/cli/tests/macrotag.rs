//! Passing a class **this run is compiling** to a macro's type tag
//! (`docs/macros.md` §5.1).
//!
//! The JVM bridge builds a `WeakTypeTag` inside `scala.reflect.runtime`'s
//! universe, whose mirror resolves a class *by name against the macro
//! classpath*. A class the run is still typing has no class file there, so it
//! could not be passed at all: gitbucket writes
//! `lazy val Issues = TableQuery[Issues]` 35 times, and every one of them was
//! the diagnostic "the type argument `Issues` is not on the classpath".
//!
//! Such a type now travels as a **placeholder**: a symbol built in the runtime
//! universe carrying the class's full name and no info at all. scala-rs
//! recognises the name in the tree that comes back and puts its own type
//! there, so the expansion is typed against the symbol the typer already has
//! rather than one resolved again by name -- which matters, because a class
//! nested in a trait has no name a mirror could resolve even after it is
//! compiled.
//!
//! The placeholder is empty on purpose. scala-rs cannot describe the class
//! truthfully at that moment: while `lazy val rows = MgQuery[Row]` is being
//! typed, the members of `class Row` are still un-inferred. So an
//! implementation that asks the placeholder what the class *is* -- slick's
//! `mapToImpl` opens with `if (!rSym.asClass.isCaseClass) c.abort(...)` -- is
//! answering about a symbol it was never shown, and its verdict is not
//! repeated to the user. `mg_inspect_bad.scala` pins both halves of that: the
//! placeholder's verdict is refused, a real classpath class's is reported.
//!
//! The library half (`mg_lib.scala`) is compiled by **real scalac**, because
//! only nsc writes the `MACRO` flag and the `@macroImpl` annotation a macro
//! def survives in. `mg_use.scala` is then compiled by scala-rs, run, and
//! compared against the same file compiled and run by real scalac: a macro
//! that expands to a *different* tree still compiles, so only the output can
//! say the expansion was right.

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
        "scala-rs-macrotag-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn tool_runs(name: &str) -> bool {
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

/// Everything these tests need, or a named skip.
fn prerequisites(tag: &str) -> bool {
    if !tool_runs("java") || !tool_runs("javac") {
        eprintln!("skip {tag}: java / javac not available");
        return false;
    }
    if scala_library_jar().is_none() || scala_reflect_jar().is_none() || find_scalac().is_none() {
        eprintln!("skip {tag}: the 2.13.16 toolchain is not obtainable");
        return false;
    }
    true
}

/// Compile `mg_lib.scala` with real scalac. Only nsc writes the `MACRO` flag
/// and the `@macroImpl` annotation, which is the whole record of a macro def.
fn build_library() -> PathBuf {
    let scalac = find_scalac().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let out = tmp_dir("lib");
    let res = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            fixtures_dir().join("mg_lib.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        res.status.success(),
        "real scalac rejected mg_lib.scala: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    out
}

fn compile_with_scala_rs(name: &str, out: &Path, lib: &Path) -> std::process::Output {
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            &format!("{}:{}", lib.display(), reflect.display()),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile")
}

fn run_main(cp: &str, what: &str) -> String {
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for {what}: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// A class this run is compiling -- top level, and nested in a trait -- passed
/// to a macro's type tag, expanded and run.
#[test]
fn mg_local_class_type_argument_expands_and_runs() {
    if !prerequisites("mg_use") {
        return;
    }
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let lib = build_library();
    let uses = tmp_dir("use");

    let out = compile_with_scala_rs("mg_use", &uses, &lib);
    assert!(
        out.status.success(),
        "compile mg_use failed: {}",
        diagnostics(&out)
    );
    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        lib.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "mg_use"),
        expected_stdout("mg_use"),
        "stdout mismatch for mg_use"
    );
    let _ = fs::remove_dir_all(&uses);
    let _ = fs::remove_dir_all(&lib);
}

/// The same file through real scalac. This is what makes the recorded
/// expectation mean anything: an expansion to a *different* tree would still
/// compile and still run.
#[test]
fn mg_local_class_expansion_matches_real_scalac() {
    if !prerequisites("mg_use scalac diff") {
        return;
    }
    let scalac = find_scalac().unwrap();
    let jar = scala_library_jar().unwrap();
    let reflect = scala_reflect_jar().unwrap();
    let lib = build_library();
    let uses = tmp_dir("use-scalac");

    let out = Command::new(&scalac)
        .args([
            "-cp",
            &format!("{}:{}", lib.display(), reflect.display()),
            "-d",
            uses.to_str().unwrap(),
            fixtures_dir().join("mg_use.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "real scalac rejected mg_use.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cp = format!(
        "{}:{}:{}:{}",
        uses.display(),
        lib.display(),
        reflect.display(),
        jar.display()
    );
    assert_eq!(
        run_main(&cp, "mg_use (real scalac build)"),
        expected_stdout("mg_use"),
        "recorded expectation for mg_use does not match real scalac"
    );
    let _ = fs::remove_dir_all(&uses);
    let _ = fs::remove_dir_all(&lib);
}

/// An implementation that asks the placeholder what the class *is* gets an
/// answer about a symbol it was never shown, so its verdict is refused and the
/// call site says why. The same implementation's verdict on a class that
/// really is on the macro classpath is reported as itself.
#[test]
fn mg_placeholder_verdict_is_not_reported_as_the_programs_error() {
    if !prerequisites("mg_inspect_bad") {
        return;
    }
    let lib = build_library();
    let out_dir = tmp_dir("inspect");
    let out = compile_with_scala_rs("mg_inspect_bad", &out_dir, &lib);
    assert!(
        !out.status.success(),
        "expected mg_inspect_bad to fail: {}",
        diagnostics(&out)
    );
    let err = diagnostics(&out);
    assert!(
        err.contains("the type argument `MgPlain` is a class this run is compiling")
            && err.contains("placeholder symbol carrying only its name"),
        "the placeholder's verdict was not refused: {err}"
    );
    assert!(
        !err.contains("error: MgPlain must be a case class"),
        "the placeholder's verdict was reported as the program's error: {err}"
    );
    // The control: a class the mirror really can find gets the real symbol, so
    // the implementation's `abort` is its own judgement and is reported.
    assert!(
        err.contains("error: java.lang.String must be a case class"),
        "a real class's abort was not reported as itself: {err}"
    );
    let _ = fs::remove_dir_all(&out_dir);
    let _ = fs::remove_dir_all(&lib);
}
