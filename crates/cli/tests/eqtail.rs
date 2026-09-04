//! Summoning `Equiv[T]` (left over from `agent/ordsummon`; `agent/eqtail`).
//!
//! Real scalac 2.13.16 accepts `implicitly[Equiv[Int]]` / `Equiv[Int]`, but scala-rs
//! failed them with `could not find implicit value`. Two causes:
//!
//! 1. The prelude did not wire up the hierarchy edges of
//!    `Ordering[T] <: PartialOrdering[T] <: Equiv[T]` (real ABI: `javap -p -s
//!    scala.math.Ordering` / `PartialOrdering` / `Equiv`). A weakening assignment
//!    such as `val e: Equiv[Int] = Ordering.Int` came out as `type mismatch`.
//! 2. `object Equiv` carried no `Int` / `Long` / ... implicit instance at all. Real
//!    scalac picks the Equiv-specific instance (`Equiv$Int$`) for
//!    `implicitly[Equiv[Int]]`, not something derived through `Ordering.Int` -- as
//!    confirmed with `implicitly[Equiv[Int]].getClass.getName`.
//!
//! The fix is `crates/typer/src/prelude_eqtail.rs` (a new module). Like the other
//! `scala.math` type classes (`Ordering` / `Numeric` / `Integral` / `Fractional`),
//! `Equiv` / `PartialOrdering` get their own class plus companion module built and
//! entered into the current scope at prelude time, without waiting for the jar to be
//! lazily loaded.
//!
//! Real scalac has no instance for `implicitly[PartialOrdering[Int]]` either, so
//! adding the hierarchy edges must not make it summonable (checked by
//! `eq2_summon_bad`).
//!
//! The same branch (left over from `agent/parentcheck`) also fixes `new T` (a type
//! parameter) and `new A` (an unaliased abstract type member). Real scalac rejects
//! them with `class type required but T found` (type parameter) and `class type
//! required but X.this.A found` (abstract type member, with a `this` qualifier that
//! needs no implementation), while scala-rs let them through silently. The `Ident`
//! branch of the `new` expression in `check.rs` gains a check that fires only when
//! the target is resolved and is not a class (`new_alias_target` tries dealiasing
//! first and then looks only at symbols still left as `TypeParam` / `TypeMember`, so
//! a type alias from the jar is not misjudged).

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
        "scala-rs-eqtail-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: the value `implicitly[Equiv[Int]]` returns really is an
/// `Equiv` instance, not e.g. an `Equiv$` module reference.
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

// ------------------------------------------------------------------ fixtures

#[test]
fn eq2_summon_scala_library() {
    jar_run("eq2_summon");
}

#[test]
fn eq2_summon_matches_real_scalac() {
    matches_real_scalac("eq2_summon");
}

/// Real scalac rejects the same 3 lines for the same reasons: `PartialOrdering[Int]`
/// has no instance (a hierarchy edge creates no new summonable instance), an
/// `Equiv[Int]` is not an `Ordering[Int]` (the weakening only goes towards Equiv),
/// and the companion object itself is not an `Equiv`.
#[test]
fn eq2_summon_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip eq2_summon_bad: jar not present");
        return;
    };
    let out = tmp_dir("eq2_summon_bad");
    let (ok, msgs) = compile(
        &out,
        "eq2_summon_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected eq2_summon_bad to be rejected, got:\n{msgs}");
    for needle in [
        "could not find implicit value of type PartialOrdering[Int]",
        "type mismatch; found: Equiv[Int]  required: Ordering[Int]",
        "type mismatch; found: Equiv$  required: Equiv[Int]",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for eq2_summon_bad, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime (`--no-scala-library`) has no `scala/math/Equiv` classfile.
/// `prelude_eqtail` is gated on `library_abi`, so a diagnostic comes out rather than
/// silent acceptance.
#[test]
fn summon_is_diagnosed_without_the_jar() {
    let out = tmp_dir("eq2-private");
    let (ok, msgs) = compile(&out, "eq2_summon", &["--no-scala-library"]);
    assert!(
        !ok,
        "expected --no-scala-library to reject eq2_summon, got:\n{msgs}"
    );
    assert!(
        msgs.contains("not found: type Equiv"),
        "expected `Equiv` to stay unknown without the jar, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- eq2_compare
//
// `Ordering[T]#compare` was hand-written in the prelude as `(Any, Any):
// Int`, so `Ordering[String].compare(1, 2)` type-checked when real scalac
// rejects it (`found: Int(1) required: String`). Fixed by giving `compare`
// the class's own type parameter `T` instead of `Any` -- `lt` / `gt` /
// `lteq` / `gteq` / `equiv` / `max` / `min` were never hand-written (they
// come from `pickle_supply` on demand) and were already `(T, T)`.

#[test]
fn eq2_compare_scala_library() {
    jar_run("eq2_compare");
}

#[test]
fn eq2_compare_matches_real_scalac() {
    matches_real_scalac("eq2_compare");
}

/// real scalac rejects every line here: `Ordering[T]` is generic in `T`,
/// not `Any`.
#[test]
fn eq2_compare_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip eq2_compare_bad: jar not present");
        return;
    };
    let out = tmp_dir("eq2_compare_bad");
    let (ok, msgs) = compile(
        &out,
        "eq2_compare_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected eq2_compare_bad to be rejected, got:\n{msgs}");
    // The first two pins moved with `agent/tail2`: supplying a jar class's
    // implicit members put a second `compare` candidate next to the prelude's,
    // so the single-candidate "type mismatch; found: 1 required: T" (scalac's
    // wording) became "no matching overload" over the pair. The *rejection*
    // is intact -- these pin that it stays one diagnostic per line -- but the
    // wording now diverges from scalac; recorded in README Remaining.
    for needle in [
        "no matching overload for (String, String)Int with arguments (1, 2)",
        "no matching overload for (Int, Int)Int with arguments (\"a\", \"b\")",
        "do not conform to method max's type parameter bounds [U <: T]",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for eq2_compare_bad, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- eq2_newtype
//
// `new T` (a type parameter) and `new A` (an unaliased abstract type
// member) both type-checked in scala-rs; real scalac rejects both with
// "class type required but ... found". Unaffected: `new C[T](...)` (a real
// class applied to a type parameter *argument*, not the `new` target
// itself) and `new A` where `type A = SomeClass` is a genuine alias
// (`new_alias_target` still handles that first).

/// This program only exercises basics (constructors, an abstract-type
/// alias) that do not need the scala-library jar, so it runs identically
/// under both modes -- unlike `eq2_summon` / `eq2_compare`, which need real
/// `Ordering` / `Equiv` instances.
fn run_main_private(out: &Path) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {} Main failed: {}",
        out.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn eq2_newtype_private_runtime() {
    if !java_available() {
        return;
    }
    let out = tmp_dir("eq2_newtype-priv");
    let (ok, msgs) = compile(&out, "eq2_newtype", &["--no-scala-library"]);
    assert!(ok, "compile eq2_newtype (private) failed:\n{msgs}");
    assert_eq!(
        run_main_private(&out),
        expected_stdout("eq2_newtype"),
        "stdout mismatch for eq2_newtype (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn eq2_newtype_scala_library() {
    jar_run("eq2_newtype");
}

#[test]
fn eq2_newtype_matches_real_scalac() {
    matches_real_scalac("eq2_newtype");
}

/// real scalac rejects both lines, in both modes: `Named.this.Self` (the
/// abstract member, referenced from inside the trait that declares it, with
/// no `=`) and `T` (a method type parameter) are neither one a class type.
#[test]
fn eq2_newtype_bad_is_rejected_private_runtime() {
    let out = tmp_dir("eq2_newtype_bad-priv");
    let (ok, msgs) = compile(&out, "eq2_newtype_bad", &["--no-scala-library"]);
    assert!(
        !ok,
        "expected eq2_newtype_bad to be rejected (private runtime), got:\n{msgs}"
    );
    assert_newtype_bad_diagnostics(&msgs);
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn eq2_newtype_bad_is_rejected_scala_library() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip eq2_newtype_bad: jar not present");
        return;
    };
    let out = tmp_dir("eq2_newtype_bad-lib");
    let (ok, msgs) = compile(
        &out,
        "eq2_newtype_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(
        !ok,
        "expected eq2_newtype_bad to be rejected (scala-library), got:\n{msgs}"
    );
    assert_newtype_bad_diagnostics(&msgs);
    let _ = fs::remove_dir_all(&out);
}

fn assert_newtype_bad_diagnostics(msgs: &str) {
    for needle in [
        "class type required but Named.this.Self found",
        "class type required but T found",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for eq2_newtype_bad, got:\n{msgs}"
        );
    }
}
