//! Stack-map frames for locals that are merged at a loop head or a branch join,
//! and for a `try` reached with a non-empty operand stack.
//!
//! # The loop-head merge
//!
//! ```scala
//! var c: Option[Int] = Some(1)
//! while (c.isDefined) { c = None }
//! ```
//!
//! type-checked, and then failed to *load*:
//!
//! ```text
//! java.lang.VerifyError: Bad type on operand stack
//!   Type 'java/lang/Object' (current frame, stack[0]) is not assignable to 'scala/Option'
//! ```
//!
//! The slot holds a `scala/Some` on entry and a `scala/None$` on the back edge,
//! and the assembler merged two unrelated classes to `java/lang/Object` -- a
//! well-formed frame, but too weak for the `invokevirtual scala/Option.isDefined`
//! that reads the slot.
//!
//! `javap -v -c` on real scalac 2.13.16 answers what belongs there:
//!
//! ```text
//!   StackMapTable: number_of_entries = 2
//!     frame_type = 252 /* append */
//!       offset_delta = 12
//!       locals = [ class scala/Option ]
//!   LocalVariableTable:
//!      Start  Length  Slot  Name   Signature
//!         12      23     2     c   Lscala/Option;
//! ```
//!
//! `class scala/Option` -- the slot's **declared** erased type, the same type
//! its `LocalVariableTable` entry has. scalac computes no least upper bound of
//! `Some` and `None$` at all: a local has one declared type for its whole
//! lifetime and every frame repeats it. The declared type is by construction an
//! upper bound of everything the source can store there, so recording it needs
//! no class hierarchy and never widens further than it must.
//!
//! That is what this assembler now does, at *every* store into the slot rather
//! than only at the merge -- it emits its frames in a single forward pass, so a
//! frame written before the back edge is seen would otherwise keep the entry
//! type. `var a: Any = 1; while (…) { a = "s" }` showed that: the loop head
//! merged to `java/lang/Object` correctly, but the frames already emitted
//! inside the condition still said `java/lang/Integer`
//! (`VerifyError: Inconsistent stackmap frames`). `java/lang/Object` therefore
//! counts as a declared class like any other.
//!
//! # `try` on a non-empty operand stack
//!
//! A separate root cause, recorded in the README as a remaining item of
//! `agent/localtrait`: `println(try { "y" } catch { … })` was a
//! `VerifyError: Inconsistent stackmap frames` in the private runtime.
//!
//! The JVM clears the operand stack when it enters an exception handler (JVMS
//! 4.10.1.6). Whatever was pending before the `try` -- the `Predef$` receiver, an
//! earlier argument, the uninitialized reference a `new` left behind -- is gone
//! on the catch path, so the join after the `try` has a stack of depth *n* on one
//! side and 0 on the other. Only the shape where the pending value happened to
//! be pushed *after* the argument (the `swap` the jar mode uses for `println`)
//! escaped it; `two("p", try …)` failed in both modes.
//!
//! `javap -c` on scalac shows its answer: a synthetic
//! `private static final java.lang.String liftedTree1$1()` holding the `try`,
//! called from the argument position (nsc's `LiftTry` phase). We park the
//! pending values in locals for the duration of the guarded region instead,
//! which needs no extra method and works for the uninitialized `new` result too.

use std::collections::BTreeSet;
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
        "scala-rs-loopframe-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: the whole point here is that the frames are accepted, and a
/// program this small would otherwise run fine on a class the verifier only
/// checks lazily.
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

// ------------------------------------------------------- StackMapTable reading

/// Every class named in a `locals = [ … ]` line of `method`'s `StackMapTable`.
///
/// A run alone cannot tell a *right* frame from a *lucky* one: `java/lang/Object`
/// verifies wherever the code only ever passes the value on as an `Object`, and
/// the same frame breaks as soon as the value is used at its own type. So read
/// the frames back out and compare them with what scalac wrote.
///
/// `javap` prints locals the same way for scalac's `append` frames and for the
/// `full_frame`s this assembler emits, so the two are directly comparable --
/// except that a `full_frame` repeats the slots an `append` inherits from the
/// implicit initial frame (`this` and the parameters), which is why the
/// assertions below are about *which* classes appear rather than about the
/// exact lists.
fn stackmap_local_classes(class_file: &Path, method: &str) -> BTreeSet<String> {
    let out = Command::new("javap")
        .args(["-v", "-p", "-c", class_file.to_str().unwrap()])
        .output()
        .expect("javap -v");
    assert!(
        out.status.success(),
        "javap -v {} failed: {}",
        class_file.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut classes = BTreeSet::new();
    let mut in_method = false;
    let mut in_table = false;
    for line in text.lines() {
        let t = line.trim();
        // Method headers sit at one level of indentation inside the class body.
        if line.starts_with("  ") && !line.starts_with("   ") && t.ends_with(';') {
            in_method = t.contains(method);
            in_table = false;
            continue;
        }
        if !in_method {
            continue;
        }
        if t.starts_with("StackMapTable:") {
            in_table = true;
            continue;
        }
        // Any other attribute of the same Code ends the table.
        if in_table && (t.starts_with("LineNumberTable:") || t.starts_with("LocalVariableTable:")) {
            in_table = false;
        }
        if !in_table || !t.starts_with("locals = [") {
            continue;
        }
        let inner = t
            .trim_start_matches("locals = [")
            .trim_end_matches(']')
            .trim();
        for item in inner.split(',') {
            let item = item.trim();
            if let Some(name) = item.strip_prefix("class ") {
                classes.insert(name.trim().trim_matches('"').to_string());
            }
        }
    }
    assert!(
        !classes.is_empty(),
        "no StackMapTable locals found for {method} in {}",
        class_file.display()
    );
    classes
}

fn compiled_class(name: &str, class: &str, extra: &[&str]) -> (PathBuf, PathBuf) {
    let out = tmp_dir(&format!("{name}-frames"));
    let (ok, msgs) = compile(&out, name, extra);
    assert!(ok, "compile {name} failed:\n{msgs}");
    (out.join(format!("{class}.class")), out)
}

fn scalac_class(name: &str, class: &str) -> (PathBuf, PathBuf) {
    let scalac = find_scalac().expect("scalac");
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-nsc-frames"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    (out.join(format!("{class}.class")), out)
}

// ------------------------------------------------------------------ fixtures

#[test]
fn lf_frame_private_runtime() {
    private_run("lf_frame");
}

#[test]
fn lf_frame_scala_library() {
    jar_run("lf_frame");
}

#[test]
fn lf_frame_matches_real_scalac() {
    matches_real_scalac("lf_frame");
}

/// The frames themselves, against real scalac's. scalac's `main` mentions
/// exactly one class in its frames -- `scala/Option`, the declared type of `c`
/// -- and nothing about `Some`, `None$` or `Object`. Ours has to name
/// `scala/Option` too, and must not have escaped to `java/lang/Object` nor
/// pinned the slot to either branch's own class.
#[test]
fn lf_frame_stackmap_matches_scalac() {
    let (Some(jar), Some(_), true) = (scala_library_jar(), find_scalac(), java_available()) else {
        eprintln!("skip lf_frame_stackmap_matches_scalac: scalac, jar or java not present");
        return;
    };
    let (nsc_class, nsc_dir) = scalac_class("lf_frame", "Main$");
    let nsc = stackmap_local_classes(&nsc_class, "main(java.lang.String[])");
    assert_eq!(
        nsc,
        BTreeSet::from(["scala/Option".to_string()]),
        "real scalac 2.13.16 no longer writes just scala/Option here"
    );

    let (ours_class, ours_dir) = compiled_class(
        "lf_frame",
        "Main$",
        &["--scala-library", jar.to_str().unwrap()],
    );
    let ours = stackmap_local_classes(&ours_class, "main(java.lang.String[])");
    for want in &nsc {
        assert!(
            ours.contains(want),
            "our StackMapTable for lf_frame does not carry {want}: {ours:?}"
        );
    }
    for loose in ["java/lang/Object", "scala/Some", "scala/None$"] {
        assert!(
            !ours.contains(loose),
            "our StackMapTable for lf_frame degraded the loop-carried local to {loose}: {ours:?}"
        );
    }
    let _ = fs::remove_dir_all(&nsc_dir);
    let _ = fs::remove_dir_all(&ours_dir);
}

/// The same check for the private runtime, which has its own `scala/Option`.
#[test]
fn lf_frame_stackmap_private_runtime() {
    if !java_available() {
        return;
    }
    let (ours_class, dir) = compiled_class("lf_frame", "Main$", &["--no-scala-library"]);
    let ours = stackmap_local_classes(&ours_class, "main(java.lang.String[])");
    assert!(
        ours.contains("scala/Option"),
        "private-runtime StackMapTable for lf_frame does not carry scala/Option: {ours:?}"
    );
    for loose in ["java/lang/Object", "scala/Some", "scala/None$"] {
        assert!(
            !ours.contains(loose),
            "private-runtime StackMapTable for lf_frame degraded the local to {loose}: {ours:?}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn lf_loopvar_scala_library() {
    jar_run("lf_loopvar");
}

#[test]
fn lf_loopvar_matches_real_scalac() {
    matches_real_scalac("lf_loopvar");
}

/// Every loop-carried local in `lf_loopvar` keeps its declared class in the
/// frames: `scala/Option` and `scala/collection/immutable/List`, never the
/// `Some` / `None$` / `Nil$` / `$colon$colon` a branch happened to store.
#[test]
fn lf_loopvar_stackmap_keeps_declared_classes() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip lf_loopvar_stackmap_keeps_declared_classes: jar not present");
        return;
    };
    let (class, dir) = compiled_class(
        "lf_loopvar",
        "Main$",
        &["--scala-library", jar.to_str().unwrap()],
    );
    let ours = stackmap_local_classes(&class, "main(java.lang.String[])");
    for want in ["scala/Option", "scala/collection/immutable/List"] {
        assert!(
            ours.contains(want),
            "lf_loopvar frames do not carry {want}: {ours:?}"
        );
    }
    for loose in [
        "scala/Some",
        "scala/None$",
        "scala/collection/immutable/Nil$",
        "scala/collection/immutable/$colon$colon",
    ] {
        assert!(
            !ours.contains(loose),
            "lf_loopvar pinned a loop-carried local to {loose}: {ours:?}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn lf_loopany_private_runtime() {
    private_run("lf_loopany");
}

#[test]
fn lf_loopany_scala_library() {
    jar_run("lf_loopany");
}

#[test]
fn lf_loopany_matches_real_scalac() {
    matches_real_scalac("lf_loopany");
}

/// `var a: Any` really is declared `java/lang/Object`, so here the frames *have*
/// to say `Object` and must not pin the slot to `java/lang/Integer` or
/// `java/lang/String`.
#[test]
fn lf_loopany_stackmap_uses_object() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip lf_loopany_stackmap_uses_object: jar not present");
        return;
    };
    let (class, dir) = compiled_class(
        "lf_loopany",
        "Main$",
        &["--scala-library", jar.to_str().unwrap()],
    );
    let ours = stackmap_local_classes(&class, "main(java.lang.String[])");
    assert!(
        ours.contains("java/lang/Object"),
        "lf_loopany frames do not carry java/lang/Object for `var a: Any`: {ours:?}"
    );
    assert!(
        !ours.contains("java/lang/Integer"),
        "lf_loopany pinned `var a: Any` to its first value's class: {ours:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn lf_trystack_private_runtime() {
    private_run("lf_trystack");
}

#[test]
fn lf_trystack_scala_library() {
    jar_run("lf_trystack");
}

#[test]
fn lf_trystack_matches_real_scalac() {
    matches_real_scalac("lf_trystack");
}

#[test]
fn lf_ctorframe_private_runtime() {
    private_run("lf_ctorframe");
}

#[test]
fn lf_ctorframe_scala_library() {
    jar_run("lf_ctorframe");
}

#[test]
fn lf_ctorframe_matches_real_scalac() {
    matches_real_scalac("lf_ctorframe");
}

/// After `invokespecial B.<init>`, `this` is a `C` -- the class being verified
/// -- not a `B` (JVMS 4.10.1.9). The frames of `C.<init>` must say so.
#[test]
fn lf_ctorframe_stackmap_names_the_subclass() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip lf_ctorframe_stackmap_names_the_subclass: jar not present");
        return;
    };
    let (class, dir) = compiled_class(
        "lf_ctorframe",
        "C",
        &["--scala-library", jar.to_str().unwrap()],
    );
    let ours = stackmap_local_classes(&class, "C(int)");
    assert!(
        ours.contains("C"),
        "C.<init> frames do not describe `this` as a C: {ours:?}"
    );
    assert!(
        !ours.contains("B"),
        "C.<init> frames describe `this` as its superclass B: {ours:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn lf_loopvar_bad_is_rejected() {
    compile_fails("lf_loopvar_bad", &["type mismatch"]);
}
