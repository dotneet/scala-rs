//! Three codegen defects that only a *running* program shows.
//!
//! None of them is caught by the type checker, the four compile measures, or
//! `tests/slick_subset.sh` (`Class.forName(initialize = false)` parses the
//! constant pool and never links a method body). Two are caught by the JVM
//! verifier and one is not caught by anything short of execution.
//!
//! **1. `Predef.identity` / `locally` / `implicitly`'s erased result was not
//! coerced.** All three are `(A)A`, so with the real library on the classpath
//! they go through `Predef$.<name>(Ljava/lang/Object;)Ljava/lang/Object;`.
//! `gen_predef_poly` (`crates/backend/src/gen_call.rs`) emitted that call and
//! stopped, leaving a bare `Object` where the tree's own type said otherwise:
//!
//! ```text
//! VerifyError: Bad type on operand stack
//!   Type 'java/lang/Object' is not assignable to 'Shape'
//! ```
//!
//! It survived because **the JVM verifier does not check interface types**:
//! `implicitly[SomeTrait].member` linked and ran with no cast at all, and the
//! failure only appears once the result is a *class* -- a `putfield` into a
//! `Shape`-typed field, or an `invokevirtual` on one. The fix routes the
//! result through `maybe_unbox_erased_result`, the same coercion every other
//! erased call site gets, so a primitive is unboxed, a `String` cast, and a
//! class-typed result cast; a bare type parameter still gets nothing, because
//! there is no class to name.
//!
//! **2. `case Wrapped(x)` on a value-class scrutinee did not verify.** A
//! value class is held unboxed wherever its static type says so, so `w:
//! Wrapped` is an `int` in its slot -- but `gen_ctor_fields_pattern`
//! (`crates/backend/src/gen_match.rs`) lowered the pattern to `instanceof` /
//! `checkcast` / `getfield` against it as though it were a boxed instance:
//!
//! ```text
//! VerifyError: Bad local variable type
//!   Type integer (current frame, locals[3]) is not assignable to reference
//! ```
//!
//! A box is always a *reference*, so a scrutinee of primitive sort here is
//! provably the underlying value: the class is final and the static type
//! already names it, so the test is vacuous and the pattern is a plain
//! binding -- which is exactly what nsc emits for the same source (`iload;
//! istore`, no `instanceof`). A boxed scrutinee (`case Wrapped(x)` on an
//! `Any`, or inside an `Option`) is of reference sort and keeps the test.
//!
//! **3. `Wrapped.unapply(w)` named explicitly threw `NoSuchMethodError`.**
//! `emit_case_unapply` (`crates/backend/src/gen_object.rs`) deliberately
//! emitted no method for a value class's companion. The reason recorded at
//! the time was that the *pattern* path handed the extractor a boxed
//! instance, so a method with nsc's descriptor would have linked and then
//! answered `Some(Wrapped(w))` where scalac says `Some(w)` -- silently wrong,
//! which is worse than a `NoSuchMethodError`. Fixing 2 removed that path: a
//! value-class pattern no longer calls `unapply` at all. The method is now
//! emitted in nsc's own shape -- `Wrapped$.unapply(int): Option`, answering
//! `Some(u)` with the underlying value boxed, never a `Wrapped` (confirmed
//! with `javap -c` against 2.13.16) -- which is the very descriptor our call
//! sites were already emitting, as the `NoSuchMethodError` itself said.
//!
//! Both fixtures are run in `--scala-library` *and* `--no-scala-library` mode
//! and checked byte for byte against what real scalac 2.13.16 prints.
//!
//! Found in passing and **not** fixed here, both pre-existing and reproducing
//! on an unpatched binary (see `docs/notes/known-gaps-backlog.md`):
//!
//! * A value class over a *reference* type (`case class WS(s: String) extends
//!   AnyVal`) is broken well before pattern matching -- `WS("a")` passed as an
//!   argument is a `ClassCastException` at the call site. Only value classes
//!   over primitives are covered here.
//! * An extractor sub-pattern whose ascription cannot hold the field's type
//!   (`case P(s: String)` where `P`'s field is an `Int`) is accepted; scalac
//!   says `scrutinee is incompatible with pattern type`. Not specific to
//!   value classes.
//! * The private runtime's `Some` has no case-class `toString`, so
//!   `println(Some(3))` prints `scala.Some@…` there. The fixture prints
//!   `.get` rather than the `Option` for that reason.

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
        "scala-rs-implicitcast-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all` so a missing cast or a bad `StackMapTable` is a failure
/// rather than a silent pass.
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

/// The negative fixture must still be rejected, and for the reasons recorded
/// beside it -- a coercion that is only ever *added* must not make anything
/// type-check that did not before.
fn rejected(name: &str, wanted: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: jar not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(!ok, "{name} should not compile, but did:\n{msgs}");
    for w in wanted {
        assert!(msgs.contains(w), "{name}: expected {w:?} in:\n{msgs}");
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn ic_implicitly_scala_library() {
    jar_run("ic_implicitly");
}

#[test]
fn ic_implicitly_private_runtime() {
    private_run("ic_implicitly");
}

#[test]
fn ic_implicitly_matches_real_scalac() {
    matches_real_scalac("ic_implicitly");
}

#[test]
fn ic_implicitly_bad_is_rejected() {
    rejected(
        "ic_implicitly_bad",
        &[
            "could not find implicit value of type Cell[Boolean]",
            "type mismatch; found: String  required: Int",
            "value show is not a member of Cell[String]",
        ],
    );
}

#[test]
fn ic_vcmatch_scala_library() {
    jar_run("ic_vcmatch");
}

#[test]
fn ic_vcmatch_private_runtime() {
    private_run("ic_vcmatch");
}

#[test]
fn ic_vcmatch_matches_real_scalac() {
    matches_real_scalac("ic_vcmatch");
}

#[test]
fn ic_vcmatch_bad_is_rejected() {
    rejected(
        "ic_vcmatch_bad",
        &[
            "extractor Wrapped expects 1 argument(s), found 2",
            "no matching overload for (Wrapped)Option[Int] with arguments (3)",
        ],
    );
}

/// The shape of what `gen_predef_poly` now emits: the erased `Predef` call
/// followed by the cast the result needs. Reading it off the classfile keeps
/// the fix honest even if the fixture's own output stopped depending on it.
#[test]
fn implicitly_result_is_cast_to_its_class() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip implicitly_result_is_cast_to_its_class: jar not present");
        return;
    };
    let out = tmp_dir("javap");
    let (ok, msgs) = compile(
        &out,
        "ic_implicitly",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(ok, "compile ic_implicitly failed:\n{msgs}");
    let javap = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), "Main$"])
        .output();
    let Ok(javap) = javap else {
        eprintln!("skip implicitly_result_is_cast_to_its_class: no javap");
        return;
    };
    if !javap.status.success() {
        eprintln!("skip implicitly_result_is_cast_to_its_class: javap failed");
        return;
    }
    let text = String::from_utf8_lossy(&javap.stdout).into_owned();
    // The field initialisers live in `<init>`; each one is the `implicitly`
    // call and then the cast that the `putfield` needs.
    let init = text
        .split("private Main$();")
        .nth(1)
        .expect("Main$ constructor in javap output");
    let init = init.split("\n\n").next().unwrap_or(init);
    assert!(
        init.contains("Predef$.implicitly:(Ljava/lang/Object;)Ljava/lang/Object;"),
        "expected the erased Predef.implicitly call:\n{init}"
    );
    assert!(
        init.contains("checkcast") && init.contains("class Cell"),
        "expected a checkcast to the result's own class:\n{init}"
    );
    assert!(
        init.contains("class Shape"),
        "expected the wildcard-typed result to be cast too:\n{init}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// nsc's own shape for a value class's `unapply`: it takes the *underlying*
/// value, not the class, and there is no `unapply(LWrapped;)` beside it.
#[test]
fn value_class_unapply_takes_the_underlying_value() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip value_class_unapply_takes_the_underlying_value: jar not present");
        return;
    };
    let out = tmp_dir("javap-vc");
    let (ok, msgs) = compile(
        &out,
        "ic_vcmatch",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(ok, "compile ic_vcmatch failed:\n{msgs}");
    let javap = Command::new("javap")
        .args(["-p", "-cp", out.to_str().unwrap(), "Wrapped$"])
        .output();
    let Ok(javap) = javap else {
        eprintln!("skip value_class_unapply_takes_the_underlying_value: no javap");
        return;
    };
    if !javap.status.success() {
        eprintln!("skip value_class_unapply_takes_the_underlying_value: javap failed");
        return;
    }
    let text = String::from_utf8_lossy(&javap.stdout).into_owned();
    assert!(
        text.contains("unapply(int)"),
        "Wrapped$ should carry nsc's erased unapply:\n{text}"
    );
    assert!(
        !text.contains("unapply(Wrapped)"),
        "Wrapped$ must not also carry a boxed unapply:\n{text}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A value-class pattern is a binding, not a type test: the lowering must not
/// emit an `instanceof` against a scrutinee the JVM holds as an `int`.
#[test]
fn value_class_pattern_emits_no_type_test() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip value_class_pattern_emits_no_type_test: jar not present");
        return;
    };
    let out = tmp_dir("javap-pat");
    let (ok, msgs) = compile(
        &out,
        "ic_vcmatch",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(ok, "compile ic_vcmatch failed:\n{msgs}");
    let javap = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), "Main$"])
        .output();
    let Ok(javap) = javap else {
        eprintln!("skip value_class_pattern_emits_no_type_test: no javap");
        return;
    };
    if !javap.status.success() {
        eprintln!("skip value_class_pattern_emits_no_type_test: javap failed");
        return;
    }
    let text = String::from_utf8_lossy(&javap.stdout).into_owned();
    let direct = text
        .split("public int direct(int);")
        .nth(1)
        .expect("Main$.direct(int) in javap output");
    let direct = direct.split("\n\n").next().unwrap_or(direct);
    assert!(
        !direct.contains("instanceof"),
        "an unboxed value-class scrutinee needs no type test:\n{direct}"
    );
    assert!(
        !direct.contains("getfield"),
        "an unboxed value-class scrutinee has no field to read:\n{direct}"
    );
    // The boxed case still does test.
    let from_any = text
        .split("public int fromAny(java.lang.Object);")
        .nth(1)
        .expect("Main$.fromAny in javap output");
    let from_any = from_any.split("\n\n").next().unwrap_or(from_any);
    assert!(
        from_any.contains("instanceof"),
        "a boxed value-class scrutinee still needs the test:\n{from_any}"
    );
    let _ = fs::remove_dir_all(&out);
}
