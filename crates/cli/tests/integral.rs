//! `Integral` / `Fractional` in the `Numeric` type-class hierarchy.
//!
//! `println(List.range(0, 5))` -- and `Vector.range` / `Seq.range` with it --
//! reported `no implicit: could not find implicit value of type Integral[Int]`.
//! `IterableFactory#range[A](start: A, end: A)(implicit ord: Integral[A])` is
//! the real signature, and the prelude had two gaps under it:
//!
//! 1. `Integral` and `Fractional` were not in the symbol table when the
//!    prelude ran. A pickle stub was raised the moment source named one, but
//!    its pickled parent (`Numeric`) was attached only on a *failed member
//!    lookup*, far too late for subtyping. `Integral[T] <: Numeric[T]` was
//!    therefore never true.
//! 2. `object Numeric`'s implicit instances were typed `Numeric[Int]`. The jar
//!    declares them one level down: `Numeric$IntIsIntegral$` implements
//!    `Numeric$IntIsIntegral extends Integral<Object>`.
//!
//! The risky part is that `Numeric[T] extends Ordering[T]`, so a new
//! `Integral[Int]` candidate could have made `Ordering[Int]` ambiguous. It
//! does not: the implicit scope of `Ordering[Int]` (SLS 7.2) is the companion
//! of `Ordering`, of its base classes, and of `Int` -- `Numeric`'s companion
//! is not among them. `ambiguity_did_not_increase` pins that down, and
//! `ig_hier` records `implicitly[Ordering[Int]].getClass.getName` so the
//! chosen instance is compared against real scalac's, not merely asserted to
//! be unique.
//!
//! Every fixture is compiled against the real `scala-library` jar, run under
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
        "scala-rs-integral-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: an implicit instance handed to a parameter whose descriptor
/// is `Lscala/math/Integral;` has to satisfy the verifier, not just the typer.
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
fn ig_hier_scala_library() {
    jar_run("ig_hier");
}

#[test]
fn ig_hier_matches_real_scalac() {
    matches_real_scalac("ig_hier");
}

/// The hierarchy must not turn into a rubber stamp: `Integral[Double]` has no
/// instance in the real library either, and `Numeric[T] <: Ordering[T]` must
/// not be usable in reverse. Real scalac rejects this file with the same six
/// errors, on the same six lines.
#[test]
fn ig_hier_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip ig_hier_bad: jar not present");
        return;
    };
    let out = tmp_dir("ig_hier_bad");
    let (ok, msgs) = compile(
        &out,
        "ig_hier_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected ig_hier_bad to be rejected, got:\n{msgs}");
    for needle in [
        "type mismatch; found: Numeric[Int]  required: Integral[Int]",
        "type mismatch; found: Ordering[Int]  required: Numeric[Int]",
        "could not find implicit value of type Integral[Double]",
        "could not find implicit value of type Fractional[Int]",
        "could not find implicit value of type Integral[String]",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for ig_hier_bad, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime (`--no-scala-library`) emits no `scala/math/Integral`
/// and no `Numeric$IntIsIntegral$`, so `prelude_numhier` deliberately does
/// nothing there. `List.range` has to be *diagnosed*, never quietly emitted
/// against classes that would not load.
#[test]
fn range_is_diagnosed_without_the_jar() {
    let out = tmp_dir("ig-private");
    let (ok, msgs) = compile(&out, "ig_hier", &["--no-scala-library"]);
    assert!(
        !ok,
        "expected --no-scala-library to reject ig_hier, got:\n{msgs}"
    );
    assert!(
        msgs.contains("range is not a member of List$"),
        "expected a diagnostic naming `range`, got:\n{msgs}"
    );
    assert!(
        msgs.contains("not found: type Integral"),
        "expected `Integral` to stay unknown without the jar, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------------ snippets

/// The reported reproducer, verbatim.
#[test]
fn range_compiles_and_runs() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        return;
    };
    let out = tmp_dir("ig-min");
    let path = out.join("Main.scala");
    fs::write(
        &path,
        "object Main {\n  def main(a: Array[String]): Unit = {\n    println(List.range(0, 5))\n    println(Vector.range(0, 3))\n    println(Seq.range(0, 3))\n  }\n}\n",
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
    assert_eq!(
        run_main(&out, Some(&jar)),
        "List(0, 1, 2, 3, 4)\nVector(0, 1, 2)\nNumericRange 0 until 3\n"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Introducing `Integral[Int]` puts a second `Ordering[Int]`-conforming value
/// into the world. It must not reach `Ordering[Int]`'s implicit scope: nothing
/// here may report `ambiguous`.
#[test]
fn ambiguity_did_not_increase() {
    let (ok, msgs) = compile_src(
        "object Snippet {\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   println(implicitly[Numeric[Int]])\n\
         \x20   println(implicitly[Ordering[Int]])\n\
         \x20   println(implicitly[Integral[Int]])\n\
         \x20   println(implicitly[Fractional[Double]])\n\
         \x20   println(implicitly[Numeric[Double]])\n\
         \x20   println(implicitly[Ordering[Double]])\n\
         \x20   println(implicitly[Ordering[Long]])\n\
         \x20   println(implicitly[Ordering[Byte]])\n\
         \x20   println(implicitly[Ordering[Short]])\n\
         \x20   println(implicitly[Ordering[Char]])\n\
         \x20   println(implicitly[Ordering[Float]])\n\
         \x20   println(List(1, 2, 3).sum)\n\
         \x20   println(List(1, 2, 3).product)\n\
         \x20   println(List(3, 1, 2).sorted)\n\
         \x20   println(List(3, 1, 2).max)\n\
         \x20   println(List(3, 1, 2).min)\n\
         \x20   println(List((1, 2), (1, 1)).sorted)\n\
         \x20 }\n\
         }\n",
        "ig-ambig",
    );
    if msgs.is_empty() {
        return; // no jar on this machine
    }
    assert!(
        !msgs.contains("ambiguous"),
        "an `ambiguous` diagnostic appeared, got:\n{msgs}"
    );
    assert!(ok, "expected the snippet to compile, got:\n{msgs}");
}

/// `Ordering.Option` -- the other prelude hole in this corner, an
/// `implicit def` rather than an `implicit object`.
#[test]
fn ordering_of_option_is_derived() {
    let (ok, msgs) = compile_src(
        "object Snippet { def main(a: Array[String]): Unit = println(List(Some(2), None, Some(1)).sorted) }\n",
        "ig-optord",
    );
    if msgs.is_empty() {
        return; // no jar on this machine
    }
    assert!(
        ok,
        "expected Ordering[Option[Int]] to resolve, got:\n{msgs}"
    );
}
