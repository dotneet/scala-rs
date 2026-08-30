//! `Unit` in a *value* position erases to `scala/runtime/BoxedUnit`.
//!
//! `Unit` is `V` only as a method **result**. As a parameter, a field, an
//! array element or a type argument nsc erases it to `scala/runtime/BoxedUnit`
//! and the single value `()` is the `UNIT` singleton. Emitting `V` there is
//! not merely different, it is not a well-formed descriptor:
//!
//! ```text
//! java.lang.ClassFormatError: Method "f" in class Main has illegal signature
//!   "(V)Ljava/lang/String;"
//! ```
//!
//! and the *whole class* fails to load. `def f(x: Unit)`, `class C(val u:
//! Unit)`, `var w: Unit`, `case class K(k: Unit, …)` and `Array[Unit]` were
//! all unloadable.
//!
//! Read off `javap -v -p` on real scalac 2.13.16:
//!
//!  * `def f(x: Unit): String` is `(Lscala/runtime/BoxedUnit;)Ljava/lang/String;`.
//!  * `f(())` pushes `getstatic scala/runtime/BoxedUnit.UNIT`.
//!  * `f(g())` for `def g(): Unit` calls `g()V` *and then* pushes `UNIT` — the
//!    call leaves nothing, the argument still has to be there.
//!  * `val u: Unit = ()` takes a real slot, `Lscala/runtime/BoxedUnit;`.
//!  * `List((), ())` builds a `[Lscala/runtime/BoxedUnit;` and wraps it with
//!    `ScalaRunTime.wrapUnitArray`; `Array[Unit]` *is* `[Lscala/runtime/BoxedUnit;`.
//!  * `val any: Any = ()` stores `BoxedUnit.UNIT`, which is why `println`
//!    prints `()`.
//!
//! The private runtime (`--no-scala-library`) had no `BoxedUnit` at all and
//! boxed `()` as `null`, so `(x: Any)` printed `null` and a `case () =>`
//! pattern also matched `null`. It now emits its own `scala/runtime/BoxedUnit`
//! and both modes agree with scalac.
//!
//! Every fixture is run three ways — private runtime, real `scala-library`
//! jar, and real scalac — and all three have to print the same thing.

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
        "scala-rs-unitbox-{tag}-{}-{nanos}-{seq}",
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
/// than a silent pass — the whole point here is that the classfile loads.
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

fn compile_fails(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: jar not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(!ok, "expected compile of {name} to fail, got:\n{msgs}");
    for needle in needles {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The descriptor a `Unit` parameter is actually emitted with. Reads the
/// classfile rather than the program's output: `(V)…` made the class
/// unloadable, so a run alone cannot tell `BoxedUnit` from a lucky accident.
fn method_descriptors(class_file: &Path) -> Vec<String> {
    let out = Command::new("javap")
        .args(["-p", class_file.to_str().unwrap()])
        .output()
        .expect("javap");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .collect()
}

// ------------------------------------------------------------------ fixtures

#[test]
fn ub_param_private_runtime() {
    private_run("ub_param");
}

#[test]
fn ub_param_scala_library() {
    jar_run("ub_param");
}

#[test]
fn ub_param_matches_real_scalac() {
    matches_real_scalac("ub_param");
}

#[test]
fn ub_field_private_runtime() {
    private_run("ub_field");
}

#[test]
fn ub_field_scala_library() {
    jar_run("ub_field");
}

#[test]
fn ub_field_matches_real_scalac() {
    matches_real_scalac("ub_field");
}

#[test]
fn ub_case_private_runtime() {
    private_run("ub_case");
}

#[test]
fn ub_case_scala_library() {
    jar_run("ub_case");
}

#[test]
fn ub_case_matches_real_scalac() {
    matches_real_scalac("ub_case");
}

#[test]
fn ub_boxed_private_runtime() {
    private_run("ub_boxed");
}

#[test]
fn ub_boxed_scala_library() {
    jar_run("ub_boxed");
}

#[test]
fn ub_boxed_matches_real_scalac() {
    matches_real_scalac("ub_boxed");
}

#[test]
fn ub_mixin_private_runtime() {
    private_run("ub_mixin");
}

#[test]
fn ub_mixin_scala_library() {
    jar_run("ub_mixin");
}

#[test]
fn ub_mixin_matches_real_scalac() {
    matches_real_scalac("ub_mixin");
}

#[test]
fn ub_call_private_runtime() {
    private_run("ub_call");
}

#[test]
fn ub_call_scala_library() {
    jar_run("ub_call");
}

#[test]
fn ub_call_matches_real_scalac() {
    matches_real_scalac("ub_call");
}

#[test]
fn ub_super_private_runtime() {
    private_run("ub_super");
}

#[test]
fn ub_super_scala_library() {
    jar_run("ub_super");
}

#[test]
fn ub_super_matches_real_scalac() {
    matches_real_scalac("ub_super");
}

/// `List[Unit]` / `Array[Unit]` / `Option[Unit]`: jar only, because the
/// private runtime has no varargs `List.apply` or `Array.apply` at all —
/// nothing to do with `Unit`.
#[test]
fn ub_typearg_scala_library() {
    jar_run("ub_typearg");
}

#[test]
fn ub_typearg_matches_real_scalac() {
    matches_real_scalac("ub_typearg");
}

#[test]
fn ub_param_bad_is_rejected() {
    compile_fails("ub_param_bad", &["error"]);
}

/// The descriptors themselves, against what `javap -v -p` shows for scalac.
#[test]
fn ub_param_descriptors_use_boxed_unit() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: jar not present");
        return;
    };
    let out = tmp_dir("ub_param-desc");
    let (ok, msgs) = compile(
        &out,
        "ub_param",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(ok, "compile ub_param failed:\n{msgs}");
    let lines = method_descriptors(&out.join("Main$.class"));
    let joined = lines.join("\n");
    assert!(
        !joined.contains("(void"),
        "a `Unit` parameter is still `V`:\n{joined}"
    );
    for want in [
        "public java.lang.String f(scala.runtime.BoxedUnit);",
        "public java.lang.String middle(int, scala.runtime.BoxedUnit, java.lang.String);",
        "public void high(scala.runtime.BoxedUnit);",
        // `Nothing` has the same result/value split.
        "public int never(scala.runtime.Nothing$);",
    ] {
        assert!(lines.iter().any(|l| l == want), "missing {want}:\n{joined}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// A `Unit` member read back from a **separately compiled** classfile. The
/// descriptor there says `Lscala/runtime/BoxedUnit;`, so the classfile reader
/// has to map it back to `Unit`: without that, `case class LK(k: Unit, n:
/// Int)` came back as `apply(BoxedUnit, Int)` and `LK((), 2)` no longer
/// type-checked against our own output.
#[test]
fn ub_separate_compilation_reads_boxed_unit_back() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip ub_sepuse: jar or java not present");
        return;
    };
    let lib = tmp_dir("ub_sepdef");
    let (ok, msgs) = compile(
        &lib,
        "ub_sepdef",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(ok, "compile ub_sepdef failed:\n{msgs}");

    let out = tmp_dir("ub_sepuse");
    let (ok, msgs) = compile(
        &out,
        "ub_sepuse",
        &[
            "-cp",
            lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ],
    );
    assert!(ok, "compile ub_sepuse against -cp failed:\n{msgs}");

    let cp = format!("{}:{}:{}", out.display(), lib.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("ub_sepuse"),
        "stdout mismatch for ub_sepuse"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&lib);
}

/// The private runtime has to ship `scala/runtime/Nothing$` as well: the
/// verifier resolves a parameter's class even for a method nobody can call.
#[test]
fn private_runtime_emits_nothing_class() {
    let out = tmp_dir("ub_param-nothing");
    let (ok, msgs) = compile(&out, "ub_param", &["--no-scala-library"]);
    assert!(ok, "compile ub_param failed:\n{msgs}");
    assert!(
        out.join("scala/runtime/Nothing$.class").is_file(),
        "private runtime did not emit scala/runtime/Nothing$"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `Array[Unit]` is `[Lscala/runtime/BoxedUnit;`, not `[V`.
#[test]
fn ub_typearg_array_descriptor() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: jar not present");
        return;
    };
    let out = tmp_dir("ub_typearg-desc");
    let (ok, msgs) = compile(
        &out,
        "ub_typearg",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(ok, "compile ub_typearg failed:\n{msgs}");
    let lines = method_descriptors(&out.join("Main$.class"));
    assert!(
        lines
            .iter()
            .any(|l| l == "public scala.runtime.BoxedUnit[] arr();"),
        "Array[Unit] is not BoxedUnit[]:\n{}",
        lines.join("\n")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime has to ship a `scala/runtime/BoxedUnit` of its own,
/// otherwise `()` in an `Any` is `null` there and `case () =>` catches `null`.
#[test]
fn private_runtime_emits_boxed_unit() {
    let out = tmp_dir("ub_boxed-runtime");
    let (ok, msgs) = compile(&out, "ub_boxed", &["--no-scala-library"]);
    assert!(ok, "compile ub_boxed failed:\n{msgs}");
    let cls = out.join("scala/runtime/BoxedUnit.class");
    assert!(
        cls.is_file(),
        "private runtime did not emit {}",
        cls.display()
    );
    let lines = method_descriptors(&cls);
    let joined = lines.join("\n");
    for want in [
        "public java.lang.String toString();",
        "public boolean equals(java.lang.Object);",
        "public int hashCode();",
    ] {
        assert!(lines.iter().any(|l| l == want), "missing {want}:\n{joined}");
    }
    assert!(
        joined.contains("UNIT"),
        "BoxedUnit has no UNIT singleton:\n{joined}"
    );
    let _ = fs::remove_dir_all(&out);
}
