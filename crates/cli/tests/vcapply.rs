//! A value class case class's companion `apply` invoked with the wrong
//! descriptor in `--scala-library` mode.
//!
//! `emit_case_apply` (`crates/backend/src/gen.rs`) always wrote the erased
//! descriptor a value class's companion `apply` needs (`(I)I`, not
//! `(I)LWrapped;` -- a value class erases to its single field's type). But
//! the *symbol table's* stored type for that same method disagreed: the
//! companion module extends `AbstractFunctionN` so it can serve as a
//! function value, so the typer's erasure pass (`erase_overriding_method` in
//! `crates/typer/src/erasure.rs`) saw `apply` as overriding
//! `AbstractFunctionN.apply` and applied its "our primitive narrows the
//! overridden's `Object`" rule -- meant for a genuine covariant-return
//! override needing a bridge -- which widened `apply`'s own *stored* return
//! type back to `Object`. Every call site in the same compilation read that
//! widened, wrong descriptor off the symbol and invoked
//! `Wrapped$.apply` accordingly, which the classfile does not have:
//!
//! ```text
//! Exception in thread "main" java.lang.NoSuchMethodError:
//!   'java.lang.Object Wrapped$.apply(int)'
//! ```
//!
//! invisible to every static check (the verifier does not resolve method
//! descriptors against anything but the constant pool) and to
//! `tests/slick_subset.sh` (`Class.forName(initialize = false)` does not
//! link). Only running the program catches it.
//!
//! `--no-scala-library` was unaffected: the private runtime's case companion
//! never extends a real `AbstractFunctionN`, so `find_overridden_method`
//! never fired and the symbol's stored type stayed the narrow, correct one.
//!
//! The fix excludes a method whose own declared return type directly names a
//! value class from the override-widening rule, since nsc itself keeps that
//! method's descriptor at the erased underlying type and reaches the
//! `AbstractFunctionN` override through a *separate* bridge method
//! (`emit_case_apply_bridge`), not by widening the primary method.
//!
//! `tests/fixtures/va_apply.scala` exercises a `case class ... extends
//! AnyVal`'s companion `apply`, `new`, `==`, and `toString`, plus a
//! non-case-class value class's `new` and its own extension method -- run in
//! both `--scala-library` and `--no-scala-library` modes and checked against
//! real scalac 2.13.16's own recorded output.
//!
//! Two adjacent, pre-existing defects are deliberately **not** covered here:
//!
//! * `Wrapped.unapply(w)` called explicitly still fails with
//!   `NoSuchMethodError: 'scala.Option Wrapped$.unapply(int)'` in both
//!   modes -- the companion has no real `unapply` body for a value class.
//!   A previous slice tried emitting one in nsc's own shape and reverted it:
//!   the *caller's* erasure still hands the pattern the boxed instance, so
//!   the extractor would have silently rewrapped it (`Some(Wrap(w))` where
//!   scalac says `Some(w)`) instead of failing loudly. `NoSuchMethodError` is
//!   the intentional, accepted state; this slice did not touch it.
//! * `w match { case Wrapped(x) => ... }` (the pattern-match sugar, as
//!   opposed to naming `unapply` directly) fails with a `VerifyError` in
//!   *both* modes, unrelated to `--scala-library`/`--no-scala-library` and
//!   unrelated to this fix (reproduces identically against an unpatched
//!   binary): the match-lowering emits `instanceof`/`checkcast`/`getfield`
//!   against the scrutinee as though it were a real boxed instance, while the
//!   scrutinee's local slot actually holds the erased `int`
//!   (`aload_3`/`istore_3` on the same slot). This is a separate,
//!   pre-existing bug in pattern-match codegen for a value-class scrutinee,
//!   not in erasure of the companion `apply`; it is flagged separately
//!   rather than fixed here.

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
        "scala-rs-vcapply-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all` so a bad descriptor or `StackMapTable` is a failure rather
/// than a silent pass.
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

fn private_run(name: &str) {
    if !java_available() {
        return;
    }
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--no-scala-library"]);
    assert!(ok, "compile {name} --no-scala-library failed:\n{msgs}");
    assert_eq!(
        run_main(&out, None),
        expected_stdout(name),
        "stdout mismatch for {name} (private runtime)"
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

#[test]
fn va_apply_scala_library() {
    jar_run("va_apply");
}

#[test]
fn va_apply_private_runtime() {
    private_run("va_apply");
}

#[test]
fn va_apply_matches_real_scalac() {
    matches_real_scalac("va_apply");
}

/// The descriptor `emit_case_apply` writes on the classfile and the one call
/// sites use now agree: `apply` stays at the value class's erased underlying
/// type, and the `AbstractFunctionN` override goes through the separate
/// `Object`-typed bridge, not a widened `apply` itself.
#[test]
fn companion_apply_keeps_the_erased_descriptor() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip companion_apply_keeps_the_erased_descriptor: jar not present");
        return;
    };
    let out = tmp_dir("javap");
    let (ok, msgs) = compile(
        &out,
        "va_apply",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(ok, "compile va_apply failed:\n{msgs}");
    let javap = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), "Wrapped$"])
        .output();
    let Ok(javap) = javap else {
        eprintln!("skip companion_apply_keeps_the_erased_descriptor: no javap");
        return;
    };
    if !javap.status.success() {
        eprintln!("skip companion_apply_keeps_the_erased_descriptor: javap failed");
        return;
    }
    let text = String::from_utf8_lossy(&javap.stdout).into_owned();
    assert!(
        text.contains("public int apply(int)"),
        "Wrapped$ should keep the narrow, erased apply:\n{text}"
    );
    assert!(
        text.contains("public java.lang.Object apply(java.lang.Object)"),
        "Wrapped$ should still carry the AbstractFunction1 bridge:\n{text}"
    );
    let _ = fs::remove_dir_all(&out);
}
