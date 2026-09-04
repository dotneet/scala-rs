//! The `Ordering` companion and summoning (`Ordering[T]`).
//!
//! All three reported shapes are accepted by real scalac 2.13.16:
//!
//! ```scala
//! Ordering.Int.reverse.compare(1, 2)   // error: value Int is not a member of Ordering[Option[AnyRef]]
//! Ordering[String].compare("a", "b")   // typechecks, then ClassCastException at run time
//! Ordering[Int].reverse.compare(1, 2)  // likewise
//! ```
//!
//! There is a single cause, and it is **not** a regression from `agent/integral`
//! (the `59d967a` binary reports `value Int is not a member of Ordering` and the
//! `ClassCastException` too).
//!
//! All `prelude::add_scala_aliases` installed was what nsc's `package object scala`
//! calls `type Ordering[T] = scala.math.Ordering[T]`; there was no
//! `val Ordering = scala.math.Ordering`. `Ordering` in **term** position therefore
//! resolved to the trait itself, and
//!
//! - `Ordering.Int` looked for a member of the trait and failed (fully qualified as
//!   `scala.math.Ordering.Int` it did work). The
//!   `implicit def Option[T](implicit ord: Ordering[T])` that `agent/integral` added
//!   was picked up as a view by the implicit-conversion search, which is why only
//!   the receiver in the error text turned into `Ordering[Option[AnyRef]]`.
//! - `Ordering[String]` went through **silently** as "a type application of a trait
//!   in term position", and codegen checkcast `Ordering$.MODULE$` to `Ordering`.
//!
//! Three places were fixed:
//! 1. `prelude_ordsummon`: put the companion module into the term namespace too
//!    (`Integral` / `Fractional` had no module at all, so it is created).
//! 2. The `Module[T]` -> `Module.apply[T]` redirect in `check.rs`: supply `apply`
//!    from the pickle before looking it up. It now also works through a package
//!    object's accessor (`def Equiv(): Equiv$`).
//! 3. `implicits.rs`: a method whose first parameter list is implicit is not a view
//!    (SLS 7.3). `val o: Ordering[Option[Int]] = Ordering.Int` was going through
//!    silently.
//!
//! Everything is run against the jar with `-Xverify:all` and checked against real
//! scalac's stdout.

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
        "scala-rs-ordsummon-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: make the verifier agree that what `Ordering[String]` returns is
/// really an instance of `Ordering` and not of `Ordering$`.
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

/// The expectation has to be what real scalac 2.13.16 prints.
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

/// Compile one snippet against the jar and return the diagnostics.
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
fn os2_summon_scala_library() {
    jar_run("os2_summon");
}

#[test]
fn os2_summon_matches_real_scalac() {
    matches_real_scalac("os2_summon");
}

/// Letting the companion stand in term position must not turn into "anything goes".
/// Real scalac rejects these 5 lines too, and rejects the same 5.
#[test]
fn os2_summon_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip os2_summon_bad: jar not present");
        return;
    };
    let out = tmp_dir("os2_summon_bad");
    let (ok, msgs) = compile(
        &out,
        "os2_summon_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected os2_summon_bad to be rejected, got:\n{msgs}");
    for needle in [
        "type mismatch; found: Ordering$  required: Ordering[Int]",
        "type mismatch; found: Ordering[Int]  required: Ordering[Option[Int]]",
        "value Foo is not a member of Ordering$",
        "value Int is not a member of Numeric$",
        "could not find implicit value of type Ordering[AnyRef]",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for os2_summon_bad, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime (`--no-scala-library`) has neither a `scala/math/Ordering`
/// classfile nor `Ordering$`. `prelude_ordsummon` is gated on `library_abi`, so a
/// diagnostic comes out rather than silent acceptance.
#[test]
fn summon_is_diagnosed_without_the_jar() {
    let out = tmp_dir("os2-private");
    let (ok, msgs) = compile(&out, "os2_summon", &["--no-scala-library"]);
    assert!(
        !ok,
        "expected --no-scala-library to reject os2_summon, got:\n{msgs}"
    );
    assert!(
        msgs.contains("not found: value Ordering"),
        "expected `Ordering` to stay unknown without the jar, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------------ snippets

/// The three reported shapes, verbatim. The `ClassCastException` came after
/// typechecking, so compiling is not enough -- we run them to be sure.
#[test]
fn the_three_reported_forms_run() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        return;
    };
    let out = tmp_dir("os2-repro");
    let path = out.join("Main.scala");
    fs::write(
        &path,
        "object Main {\n  def main(a: Array[String]): Unit = {\n\
         \x20   println(Ordering.Int.reverse.compare(1,2))\n\
         \x20   println(Ordering[String].compare(\"a\",\"b\"))\n\
         \x20   println(Ordering[Int].reverse.compare(1,2))\n\
         \x20 }\n}\n",
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
    assert_eq!(run_main(&out, Some(&jar)), "1\n-1\n1\n");
    let _ = fs::remove_dir_all(&out);
}

/// `Ordering.Option` is a derivation rule, not a view. `sorted` must still derive as
/// before (the same thing as `agent/integral`'s
/// `ordering_of_option_is_derived`).
#[test]
fn option_ordering_is_still_derived_but_is_not_a_view() {
    let (ok, msgs) = compile_src(
        "object Snippet { def main(a: Array[String]): Unit = {\n\
         \x20 println(List(Some(2), None, Some(1)).sorted)\n\
         \x20 println(implicitly[Ordering[Option[Int]]].compare(Some(1), None))\n\
         } }\n",
        "os2-optord",
    );
    if msgs.is_empty() {
        return; // no jar on this machine
    }
    assert!(
        ok,
        "expected Ordering[Option[Int]] to resolve, got:\n{msgs}"
    );
    let (bad_ok, bad_msgs) = compile_src(
        "object Snippet { val o: Ordering[Option[Int]] = Ordering.Int }\n",
        "os2-optview",
    );
    assert!(
        !bad_ok,
        "an implicit *clause* must not act as a view, got:\n{bad_msgs}"
    );
    assert!(
        bad_msgs.contains("type mismatch"),
        "expected a type mismatch, got:\n{bad_msgs}"
    );
}

/// Since `agent/integral`, `Integral[Int]` had the trait itself standing in term
/// position and went through **silently**, giving a run-time
/// `ClassCastException: scala.math.Integral$ cannot be cast to scala.math.Integral`
/// (`59d967a` gave a type error).
#[test]
fn integral_and_fractional_summon() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        return;
    };
    let out = tmp_dir("os2-integral");
    let path = out.join("Main.scala");
    fs::write(
        &path,
        "object Main {\n  def main(a: Array[String]): Unit = {\n\
         \x20   val i: Integral[Int] = Integral[Int]\n\
         \x20   val f: Fractional[Double] = Fractional[Double]\n\
         \x20   println(i.quot(7, 2))\n\
         \x20   println(f.div(1.0, 4.0))\n\
         \x20 }\n}\n",
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
    assert_eq!(run_main(&out, Some(&jar)), "3\n0.25\n");
    let _ = fs::remove_dir_all(&out);
}

/// The existing `Module[T]` redirect (`List[Int]()` and the like) is not broken.
#[test]
fn module_apply_redirect_still_works() {
    let (ok, msgs) = compile_src(
        "object Snippet { def main(a: Array[String]): Unit = {\n\
         \x20 println(List[Int](1, 2))\n\
         \x20 println(Vector[String](\"a\"))\n\
         \x20 println(Option[Int](3))\n\
         \x20 println(Map[String, Int](\"a\" -> 1))\n\
         } }\n",
        "os2-modapply",
    );
    if msgs.is_empty() {
        return; // no jar on this machine
    }
    assert!(ok, "expected the module factories to compile, got:\n{msgs}");
}

/// Resolving the alias to the module changes which path `BigDecimal(3L)`
/// takes: the term is no longer a *class*, so `widen_with_companion` -- the
/// recovery that used to hand the companion's `apply` overloads over -- does
/// not apply, and the module class carries only the three the prelude writes
/// by hand. `widen_module_from_pickle` reads the rest. This is the regression
/// the first version of this slice was reverted for; `oshadow` covers the same
/// program end to end, and this pins the alias path itself.
#[test]
fn alias_module_keeps_the_pickled_overloads() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        return;
    };
    let out = tmp_dir("os2-bigdec");
    let path = out.join("Main.scala");
    fs::write(
        &path,
        "object Main {\n  def main(a: Array[String]): Unit = {\n\
         \x20   println(BigDecimal(3L))\n\
         \x20   println(BigDecimal(2))\n\
         \x20   println(BigDecimal(\"4.25\"))\n\
         \x20   println(BigDecimal(BigInt(6)))\n\
         \x20   println(BigDecimal(0.5))\n\
         \x20   println(BigInt(\"7\"))\n\
         \x20 }\n}\n",
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
    assert_eq!(run_main(&out, Some(&jar)), "3\n2\n4.25\n6\n0.5\n7\n");
    let _ = fs::remove_dir_all(&out);
}
