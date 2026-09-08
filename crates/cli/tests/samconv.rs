//! E2E tests for the `agent/samconv` slice: **SAM conversion onto a type that
//! came out of a library, rather than out of this run's own sources.**
//!
//! The brief for this slice named the root as "a function literal passed where
//! a single-abstract-method type is expected is not being converted". Measured,
//! that is not what was wrong. SAM conversion has worked for years for a
//! source-declared trait and for the three Java interfaces the prelude
//! hand-builds (`Runnable`, `java.util.Comparator`,
//! `java.util.function.Function`) -- `crates/typer/src/lib.rs`'s
//! `sam_runnable_and_comparator_typecheck` has pinned that all along. What did
//! not work was a SAM type the compiler *read* rather than parsed:
//!
//! ```scala
//! val e: scala.math.Equiv[Int] = (x, y) => x == y
//! // pre-fix: type mismatch; found: (Int, Int) => Boolean  required: Equiv[Int]
//! ```
//!
//! `SymbolTable::sam_sig` counts a class's abstract methods by looking for
//! `Flags::ABSTRACT`, and **neither of the two supplies that produce a library
//! class can set it**. `prelude::method` stamps every member `Flags::FINAL`,
//! and `PickleSupply` allocates members `Flags::EMPTY` -- both deliberately,
//! and both already written down in `override_check::modifiers_are_known`,
//! which withholds every modifier-shaped diagnostic for exactly those two
//! groups. So `scala.math.Equiv` reported **zero** abstract methods, not one,
//! and was not a SAM type at all. Same for `scala.math.Ordering` and
//! `scala.util.hashing.Hashing`, which are the other two sites cats writes.
//!
//! The fix records the fact on `Symbol::deferred_method` instead of widening
//! `Flags::ABSTRACT`, so it reaches SAM detection without turning on a
//! library's worth of `override`/`final` diagnostics as a side effect.
//!
//! **A second root, under the first.** `scala.math.Ordering` still did not
//! convert once the pickle's `DEFERRED` was carried across, because
//! `PickleSupply` installs library members one *name* at a time, on demand:
//! `Ordering` inherits a deferred `equiv` from `Equiv` (through
//! `PartialOrdering`) and overrides it concretely -- `javap -p
//! scala.math.Ordering` says `public default boolean equiv(T, T)` -- but that
//! override is not installed until something asks for it by name, so
//! `Ordering` read as having two abstract methods. `Checker::
//! complete_sam_overrides` asks for exactly the inherited deferred names
//! before deciding.
//!
//! **Two things this slice found and fixed that are not the root.**
//!
//! * A literal of the *wrong arity* claimed a SAM type: `type_function`'s last
//!   arm guarded on `param_tys.len() == pts.len()`, and `pts` had already been
//!   replaced with `vec![NoType; vparams.len()]` by the arity check above it,
//!   so the guard could not fail. `val e: Eq0[Int] = (x: Int) => x > 0`
//!   compiled. Pre-existing and reachable on the branch point through any
//!   source-declared SAM trait; `samconv_bad.scala` pins it.
//! * A **polymorphic** abstract method made a class a SAM. nsc's
//!   `definitions.samOf` requires `sam.typeParams.isEmpty`, and scalac 2.13.16
//!   rejects `trait Poly { def f[A](a: A): A }; val p: Poly = x => x` with
//!   `missing parameter type` and `found: ? => ?  required: Poly`. Also
//!   pre-existing, also pinned.
//!
//! **The emitted shape.** This compiler lowers a SAM literal to an anonymous
//! class and a `FunctionN` literal to `invokedynamic`; that split is
//! `crates/cli/tests/indy.rs`'s subject and this slice does not change it.
//! scalac 2.13.16 differs: for the same source it emits `invokedynamic` for
//! `Equiv`, `Hashing` and `Runnable` and an anonymous class
//! (`S$$anonfun$o$2`) for `Ordering` -- so one of the four is an anonymous
//! class there too. `samconv_emits_an_anonymous_class_per_sam_literal` asserts
//! what this compiler emits rather than pretending the two agree.
//!
//! **`agent/samfwd`'s concern is gone.** That branch (`03725d41`, from a base
//! 60-odd commits behind this one) added mixin forwarders to a SAM literal's
//! anonymous class because a trait's concrete methods lived in a `T$class`
//! static. This tree emits them as JVM `default` methods -- `javap -p Eq0`
//! says `public default boolean neqv(A, A)` -- so an anonymous class carrying
//! only the abstract method inherits them. Case 7 of the fixture calls
//! `neqv` on a SAM literal to keep that honest.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with the
//! other slices running in parallel; see `.agent-brief.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("{name}.scala"))
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/expected")
            .join(format!("{name}.txt")),
    )
    .unwrap()
}

/// Unique per call: these tests run concurrently and several compile the same
/// fixture, so a shared output directory would let one test's cleanup delete
/// another's class files.
fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-samconv-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Real scalac 2.13.16, when this machine has the checkout the measurement
/// scripts install. Every test that needs it skips loudly without it.
fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

struct Run {
    ok: bool,
    text: String,
}

fn out_of(output: std::process::Output) -> Run {
    Run {
        ok: output.status.success(),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn compile_rs(src: &Path, out: &Path, jar: &Path) -> Run {
    let mut cmd = Command::new(bin());
    cmd.arg("compile")
        .arg(src)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()]);
    out_of(cmd.output().expect("run scala-rs compile"))
}

fn compile_scalac(sc: &Path, src: &Path, out: &Path, jar: &Path) -> Run {
    let mut cmd = Command::new(sc);
    cmd.args([
        "-classpath",
        jar.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ])
    .arg(src);
    out_of(cmd.output().expect("run scalac"))
}

/// `-Xverify:all`, so a SAM anonymous class that does not really implement the
/// interface is a `VerifyError` rather than a silent pass.
fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn ok(run: Run, what: &str) {
    assert!(run.ok, "expected {what} to compile:\n{}", run.text);
}

fn rejected(run: Run, what: &str, needles: &[&str]) {
    assert!(
        !run.ok,
        "expected {what} to be rejected, but it compiled:\n{}",
        run.text
    );
    for n in needles {
        assert!(
            run.text.contains(n),
            "expected {what}'s rejection to mention {n:?}:\n{}",
            run.text
        );
    }
}

// ---------------------------------------------------------------------------
// The positive fixture, executed.
//
// On an unmodified build of the branch point this does not compile at all:
// the six literals whose expected type is `Equiv`, `Ordering` or `Hashing`
// are each `type mismatch; found: (A, A) => Boolean  required: Equiv[A]`.
// ---------------------------------------------------------------------------

#[test]
fn samconv_runs() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip samconv_runs: jar or java not present");
        return;
    };
    let dir = tmp_dir("run");
    ok(compile_rs(&fixture("samconv"), &dir, &jar), "samconv");
    assert_eq!(run_java(&dir, &jar), expected_stdout("samconv"));
    let _ = fs::remove_dir_all(&dir);
}

/// The expected file is scalac's, checked by producing it again here. Without
/// this, `expected/samconv.txt` is only what this compiler happened to print
/// the day it was written.
#[test]
fn samconv_expected_output_is_scalacs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip samconv_expected_output_is_scalacs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("scalac");
    ok(
        compile_scalac(&sc, &fixture("samconv"), &dir, &jar),
        "samconv under scalac",
    );
    assert_eq!(run_java(&dir, &jar), expected_stdout("samconv"));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The negatives, paired with real scalac on the same file.
//
// These are the two neighbouring cases that separate the right rule from one
// that merely fits cats, plus the arity hole the widening exposed. Both
// compilers report five errors on this file.
// ---------------------------------------------------------------------------

#[test]
fn samconv_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip samconv_bad_is_rejected: jar not present");
        return;
    };
    let dir = tmp_dir("bad");
    rejected(
        compile_rs(&fixture("samconv_bad"), &dir, &jar),
        "samconv_bad",
        &[
            // two abstract methods
            "required: Two",
            // a polymorphic abstract method
            "required: Poly",
            // the right SAM, the wrong arity
            "found: (Int) => Boolean  required: Equiv[Int]",
        ],
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn samconv_bad_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip samconv_bad_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("badsc");
    rejected(
        compile_scalac(&sc, &fixture("samconv_bad"), &dir, &jar),
        "samconv_bad under scalac",
        &[
            "required: Two",
            "required: Poly",
            "required: scala.math.Equiv[Int]",
        ],
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The emitted shape.
//
// Running green is necessary but not sufficient. A SAM literal must produce a
// class that really implements the interface and really defines the erased
// abstract method -- an anonymous class that declares the *unerased*
// descriptor passes the verifier and throws `AbstractMethodError` at the call
// site instead.
// ---------------------------------------------------------------------------

#[test]
fn samconv_emits_an_anonymous_class_per_sam_literal() {
    let (Some(jar), true) = (scala_library_jar(), javap_available()) else {
        eprintln!("skip samconv_emits_an_anonymous_class_per_sam_literal: jar or javap absent");
        return;
    };
    let dir = tmp_dir("shape");
    ok(compile_rs(&fixture("samconv"), &dir, &jar), "samconv");

    // Each of the three conversions in `object Conv` -- the placeholder
    // spelling cats writes -- lowers to one anonymous class implementing the
    // library interface, whose single method is the SAM at its *erased*
    // descriptor.
    for (class, iface, member) in [
        (
            "Conv$$$anonfun$0",
            "implements scala.math.Equiv",
            "public boolean equiv(java.lang.Object, java.lang.Object);",
        ),
        (
            "Conv$$$anonfun$1",
            "implements scala.math.Ordering",
            "public int compare(java.lang.Object, java.lang.Object);",
        ),
        (
            "Conv$$$anonfun$2",
            "implements scala.util.hashing.Hashing",
            "public int hash(java.lang.Object);",
        ),
    ] {
        let o = Command::new("javap")
            .args(["-p", "-cp", dir.to_str().unwrap(), class])
            .output()
            .expect("javap");
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(
            o.status.success() && text.contains(iface),
            "{class} should be `{iface}`:\n{text}{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(
            text.contains(member),
            "{class} should define `{member}`:\n{text}"
        );
    }

    // `Ordering`'s inherited concrete members are the library's own JVM
    // `default` methods, so the anonymous class must *not* carry copies of
    // them -- it defines `compare` and nothing else. This is the assertion
    // that would fail if `complete_sam_overrides` had installed `equiv` as
    // something to implement rather than as an override already present.
    let o = Command::new("javap")
        .args(["-p", "-cp", dir.to_str().unwrap(), "Conv$$$anonfun$1"])
        .output()
        .expect("javap");
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(
        !text.contains(" equiv("),
        "the Ordering SAM class must not declare `equiv`; the library's default method is:\n{text}"
    );

    // A trait declared in this run gets JVM `default` methods for its
    // concrete members, which is why the SAM literal's class needs no
    // forwarder for `neqv` (and why `agent/samfwd`'s mixin-forwarder fix is
    // no longer the shape of this problem).
    let o = Command::new("javap")
        .args(["-p", "-cp", dir.to_str().unwrap(), "Eq0"])
        .output()
        .expect("javap");
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(
        text.contains("public abstract boolean eqv(A, A);")
            && text.contains("public default boolean neqv(A, A);"),
        "Eq0 should declare `eqv` abstract and `neqv` default:\n{text}"
    );

    let _ = fs::remove_dir_all(&dir);
}
