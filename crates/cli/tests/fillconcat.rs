//! Factory results used where the collection type itself is wanted.
//!
//! `object Main { def main(a: Array[String]) = println(List.fill(2)(5) ::: List(9)) }`
//! type-checked and then died with
//! `VerifyError: Bad type on operand stack: 'scala/collection/SeqOps' is not
//! assignable to 'scala/collection/immutable/List'`.
//!
//! `List$.fill` really is `(ILscala/Function0;)Lscala/collection/SeqOps;` in
//! the jar -- `StrictOptimizedSeqFactory[+CC[_] <: SeqOps[…]]` erases `CC[A]`
//! to its bound -- and scalac emits exactly that call followed by
//! `checkcast scala/collection/immutable/List`. We emitted the call and no
//! cast, because the rule for "the descriptor returns something wider than the
//! typer's result type" only fired when the prelude's own class hierarchy
//! could show that the result type reaches the declared one, and the `…Ops`
//! traits are deliberately not in that hierarchy.
//!
//! So it was never about `List.fill`, nor about `:::` (which is
//! right-associative, so the factory result is the *argument* there): every
//! `fill`/`tabulate`/`concat`/`iterate`/`empty`/`unfold` on
//! `List`/`Vector`/`Seq`/`Set`/`ArrayBuffer`/`ListBuffer` was affected, in
//! receiver position as much as in argument position.
//!
//! Each fixture is compiled against the real `scala-library` jar, run under
//! `-Xverify:all`, and diffed against what real scalac 2.13.16 prints.

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
        "scala-rs-fillconcat-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn compile(out: &Path, name: &str, extra: &[&str]) -> (bool, String) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let output = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .args(["-d", out.to_str().unwrap()])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

/// `-Xverify:all` so the bug this file is about is a test failure, not a
/// silent pass on a JVM that skips verification.
fn run_main(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn jar_run(name: &str) {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip {name}: jar or java not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    assert_eq!(
        run_main(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for {name} (scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The recorded expectation has to be what real scalac 2.13.16 prints.
fn matches_real_scalac(name: &str) {
    let (Some(scalac), Some(jar), true) = (find_scalac(), scala_library_jar(), java_available())
    else {
        eprintln!("skip real-scalac diff {name}: scalac, jar or java not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-nsc"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    assert_eq!(
        run_main(&ref_out, Some(&jar)),
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, extra);
    assert!(!ok, "expected compile of {name} to fail, got:\n{msgs}");
    for needle in needles {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Compile a snippet against the jar and report the diagnostics.
fn compile_src(src: &str, tag: &str) -> (bool, String) {
    let Some(jar) = scala_library_jar() else {
        return (true, String::new());
    };
    let out = tmp_dir(tag);
    let path = out.join("Snippet.scala");
    fs::write(&path, src).unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(&path)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let ok = output.status.success();
    let _ = fs::remove_dir_all(&out);
    (ok, msgs)
}

// ------------------------------------------------------------------ fixtures

#[test]
fn fc_factory_scala_library() {
    jar_run("fc_factory");
}

#[test]
fn fc_factory_matches_real_scalac() {
    matches_real_scalac("fc_factory");
}

#[test]
fn fc_ops_scala_library() {
    jar_run("fc_ops");
}

#[test]
fn fc_ops_matches_real_scalac() {
    matches_real_scalac("fc_ops");
}

#[test]
fn fc_local_scala_library() {
    jar_run("fc_local");
}

#[test]
fn fc_local_matches_real_scalac() {
    matches_real_scalac("fc_local");
}

/// The inserted cast must not paper over a real type error.
#[test]
fn fc_factory_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip fc_factory_bad: jar not present");
        return;
    };
    compile_fails(
        "fc_factory_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "required: Vector[Int]",
            "type mismatch",
            "::: is not a member of Vector[Int]",
        ],
    );
}

/// The private runtime (`--no-scala-library`) has no `IterableFactory`, so
/// these factories have to be *diagnosed* there, never quietly emitted.
#[test]
fn factories_are_diagnosed_without_the_jar() {
    let out = tmp_dir("fc-private");
    let (ok, msgs) = compile(&out, "fc_factory", &["--no-scala-library"]);
    assert!(
        !ok,
        "expected --no-scala-library to reject fc_factory, got:\n{msgs}"
    );
    assert!(
        msgs.contains("fill is not a member of List$"),
        "expected a diagnostic naming `fill`, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------------ snippets

/// The reported reproducer, verbatim.
#[test]
fn minimal_repro_verifies() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        return;
    };
    let out = tmp_dir("fc-min");
    let path = out.join("Main.scala");
    fs::write(
        &path,
        "object Main { def main(a: Array[String]): Unit = println(List.fill(2)(5) ::: List(9)) }\n",
    )
    .unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(&path)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(run_main(&out, Some(&jar)), "List(5, 5, 9)\n");
    let _ = fs::remove_dir_all(&out);
}

/// `List.range` needs an `Integral[Int]`, which the prelude did not model:
/// this used to assert the `no implicit` diagnostic instead.
/// `agent/integral` closed the gap for real -- `Integral[T] <: Numeric[T]` is
/// now in the hierarchy and `Numeric.IntIsIntegral` carries its true type
/// `Integral[Int]` -- so the snippet compiles. See `crates/cli/tests/integral.rs`
/// for the full coverage, including the negative cases that make sure this is
/// not the "silently pick the `Numeric[Int]` instance" fix this project forbids.
#[test]
fn range_resolves_the_integral() {
    let (ok, msgs) = compile_src(
        "object Snippet { def main(a: Array[String]): Unit = println(List.range(0, 3)) }\n",
        "fc-range",
    );
    // With no jar on this machine `compile_src` reports `(true, "")`, which
    // passes both assertions below without pretending anything was checked.
    assert!(ok, "expected List.range to compile, got:\n{msgs}");
    assert!(
        !msgs.contains("Integral[Int]"),
        "expected no Integral[Int] diagnostic, got:\n{msgs}"
    );
}
