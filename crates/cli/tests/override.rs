//! SLS 5.1.4 "Overriding" and SLS 5.2.6 "needs to be abstract".
//!
//! Before this slice scala-rs had **no** conformance check on an override:
//!
//! ```scala
//! trait It[A] { def next(): A }
//! val i = new It[Int] { def next(): String = "x" }
//! println(i.next())          // ClassCastException at the caller's unbox
//! ```
//!
//! type-checked, and a class that forgot to implement an abstract member
//! compiled and threw `AbstractMethodError`.
//!
//! Every diagnostic asserted here was read off **real scalac 2.13.16** running
//! on the very same fixture, not off `javap` and not from memory. For each
//! fixture scalac reports exactly *one* error, and so must scala-rs: a rule
//! that fires twice, or that fires in only one of the two library modes, is as
//! wrong as one that does not fire at all. `compile_fails_both` pins both.
//!
//! Kept in its own file so it does not collide with parallel work in `e2e.rs`.

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
        "scala-rs-ovr-{tag}-{}-{nanos}-{seq}",
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
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Private-runtime mode: `--no-scala-library`.
fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout(name),
            "stdout mismatch for private-runtime {name}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Library-ABI mode: linked against the real scala-library 2.13.16 jar. The
/// expected file is real scalac's own output for the same source.
fn check_library(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn diagnostics(name: &str, extra: &[&str]) -> String {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    err
}

/// Both modes must reject it, with the wording and the **count** real scalac
/// produced for the same file. A rule that fires only with the jar on the
/// classpath would let the private runtime miscompile in silence; one that
/// fires twice would be a cascade scalac does not report.
fn rejected_once(name: &str, needles: &[&str]) {
    let mut modes: Vec<Vec<String>> = vec![vec!["--no-scala-library".to_string()]];
    if let Some(jar) = scala_library_jar() {
        modes.push(vec![
            "--scala-library".to_string(),
            jar.to_str().unwrap().to_string(),
        ]);
    }
    for m in &modes {
        let args: Vec<&str> = m.iter().map(|s| s.as_str()).collect();
        let err = diagnostics(name, &args);
        for needle in needles {
            assert!(
                err.contains(needle),
                "expected {needle:?} in diagnostics for {name} ({args:?}), got {err:?}"
            );
        }
        let n = err.lines().filter(|l| l.starts_with("error:")).count();
        assert_eq!(
            n, 1,
            "scalac 2.13.16 reports exactly one error for {name}; scala-rs \
             reported {n} ({args:?}):\n{err}"
        );
    }
}

// ---------------------------------------------------------------- happy path

/// Every *legal* override shape in one file. This is the guard against the new
/// checks over-rejecting: each of these compiled before the checks existed and
/// still has to, in both modes, with scalac's own output.
#[test]
fn fixtures_ov_ok_private_runtime() {
    check_private("ov_ok");
}

#[test]
fn fixtures_ov_ok_scala_library() {
    check_library("ov_ok");
}

/// An inherited method bound is read through the owner instantiation:
/// `BoundApply[CharSequence]` makes the base bound `A <: CharSequence`.
#[test]
fn ov_owner_bound_private_runtime() {
    check_private("ov_owner_bound");
}

#[test]
fn ov_owner_bound_scala_library() {
    check_library("ov_owner_bound");
}

// ----------------------------------- the discarded `Unit` result that follows

/// `agent/anonbridge`'s other leftover, and the reason the two live in one
/// slice: the *result* of a member erased through `Object` is not popped when
/// it is discarded. A **nilary** `def` reached the statement discard as a bare
/// `Select` with no `Apply` above it, so `b.get` on a `Box[Unit]` left a
/// reference behind; the next branch that needs a stackmap frame — the `try`
/// in the fixture — then failed the verifier with `Inconsistent stackmap
/// frames`. `java -Xverify:all` in both modes is the assertion.
#[test]
fn fixtures_ov_unitpop_private_runtime() {
    check_private("ov_unitpop");
}

#[test]
fn fixtures_ov_unitpop_scala_library() {
    check_library("ov_unitpop");
}

/// The `pop` is there in the bytecode, not merely tolerated by this JVM: the
/// bare `Select` of `Box.get` must be followed by one.
#[test]
fn ov_nilary_unit_select_is_popped() {
    let out = compile_fixture_with("ov_unitpop", &["--no-scala-library"]);
    let output = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), "Main$"])
        .output()
        .expect("javap");
    let text = String::from_utf8_lossy(&output.stdout);
    let body: Vec<&str> = text
        .lines()
        .skip_while(|l| !l.contains("viaNilarySelect"))
        .take(12)
        .collect();
    let joined = body.join("\n");
    let call = body
        .iter()
        .position(|l| l.contains("Box.get:()Ljava/lang/Object;"))
        .unwrap_or_else(|| panic!("no erased Box.get call in viaNilarySelect:\n{joined}"));
    assert!(
        body[call + 1].contains(": pop"),
        "the discarded Box.get result is not popped:\n{joined}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------- 1. result type covariance

/// The report that started the slice. scalac 2.13.16 echoes the overridden
/// member *as the overriding class sees it* — `def next(): Int`, not
/// `def next(): A` — so that is what is pinned here.
#[test]
fn ov_result_type_must_conform() {
    rejected_once(
        "ov_result_bad",
        &[
            "incompatible type in overriding",
            "def next(): Int (defined in trait It);",
            " found   : (): String",
            " required: (): Int",
        ],
    );
}

// --------------------------------------------- 2. parameter types invariant

/// A different parameter type makes it an overload, so `override` refers to
/// nothing. scalac appends the note listing what the super classes do offer.
#[test]
fn ov_parameter_types_are_invariant() {
    rejected_once(
        "ov_param_bad",
        &[
            "method f overrides nothing.",
            "Note: the super classes of class D contain the following, non final members named f:",
            "def f(x: Int): Int",
        ],
    );
}

// ------------------------------------------------------ 3. override modifier

#[test]
fn ov_override_modifier_required() {
    rejected_once(
        "ov_modreq_bad",
        &[
            "`override` modifier required to override concrete member:",
            "def f(x: Int): Int (defined in class B)",
        ],
    );
}

#[test]
fn ov_override_modifier_refers_to_nothing() {
    rejected_once("ov_modnone_bad", &["method h overrides nothing"]);
}

// --------------------------------- 4. a deferred re-declaration un-implements

/// Only an implementation *more derived* than the declaration grounds it:
/// `class B { def f = 1 }`, `abstract class M extends B { override def f: Int }`,
/// `class C extends M` is an error even though `B.f` has a body.
#[test]
fn ov_deferred_redeclaration_needs_reimplementing() {
    rejected_once(
        "ov_deferred_bad",
        &[
            "class C needs to be abstract.",
            "No implementation found in a subclass for deferred declaration",
            "override def f: Int (defined in class M)",
        ],
    );
}

// ----------------------------------------------------------------- 5. final

#[test]
fn ov_cannot_override_final() {
    rejected_once(
        "ov_final_bad",
        &[
            "cannot override final member:",
            "final def f: Int (defined in class B)",
        ],
    );
}

// ------------------------------------------------------------ 6. visibility

#[test]
fn ov_access_may_not_narrow() {
    rejected_once(
        "ov_access_bad",
        &[
            "weaker access privileges in overriding",
            "def f: Int (defined in class B)",
            "  override should be public",
        ],
    );
}

// -------------------------------------------------------- 7. val / var / def

#[test]
fn ov_def_may_not_override_val() {
    rejected_once(
        "ov_valdef_bad",
        &[
            "stable, immutable value required to override:",
            "val v: Int (defined in class B)",
        ],
    );
}

/// scalac echoes the *accessor*: a `var`'s getter is not stable, so it prints
/// as `def v: Int` and not as `var v: Int`.
#[test]
fn ov_concrete_var_is_not_overridable() {
    rejected_once(
        "ov_var_bad",
        &[
            "mutable variable cannot be overridden:",
            "def v: Int (defined in class B)",
        ],
    );
}

// ------------------------------------------------------- 8. type parameters

#[test]
fn ov_type_parameter_count_is_part_of_the_signature() {
    rejected_once(
        "ov_tparam_bad",
        &["method f overrides nothing.", "def f[A](x: A): A"],
    );
}

/// `[A]` may override `[A <: AnyRef]`; the reverse refuses arguments the base
/// accepts, and scalac reports it as a type incompatibility.
#[test]
fn ov_type_parameter_bounds_may_only_widen() {
    rejected_once(
        "ov_bound_bad",
        &[
            "incompatible type in overriding",
            "def f[A](x: A): A (defined in class B);",
        ],
    );
}

/// A child bound that is narrower than an instantiated owner bound is still
/// rejected after the base bound is read through the receiver.
#[test]
fn ov_owner_bound_may_not_narrow() {
    rejected_once(
        "ov_owner_bound_bad",
        &[
            "incompatible type in overriding",
            "def apply[A <: AnyRef](a: A): A (defined in trait OwnerBound);",
        ],
    );
}

// -------------------------------------------- 9. unimplemented abstract members

#[test]
fn ov_class_needs_to_be_abstract() {
    rejected_once(
        "ov_abstract_bad",
        &[
            "class D needs to be abstract.",
            "Missing implementation for member of class B:",
            "def f: Int = ???",
        ],
    );
}

/// Two missing members are still **one** diagnostic, listed together — the
/// count is what `rejected_once` pins.
#[test]
fn ov_several_missing_members_are_one_diagnostic() {
    rejected_once(
        "ov_abstract2_bad",
        &[
            "class D needs to be abstract.",
            "Missing implementations for 2 members of trait T.",
            "def f: Int = ???",
            "val v: String = ???",
        ],
    );
}

#[test]
fn ov_object_creation_impossible() {
    rejected_once(
        "ov_object_bad",
        &[
            "object creation impossible.",
            "Missing implementation for member of trait T:",
            "def f: Int = ???",
        ],
    );
}

/// The anonymous class of a `new T {}` is reported the same way an `object` is.
#[test]
fn ov_anonymous_class_creation_impossible() {
    rejected_once(
        "ov_anon_bad",
        &[
            "object creation impossible.",
            "Missing implementation for member of trait T:",
        ],
    );
}
