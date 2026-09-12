//! Taking scala/scala's own `src/library` from 41 errors to 4 (`agent/libzero2`).
//!
//! Fourteen roots, each reduced to a standalone program and compared with real
//! scalac 2.13.16 in **both** directions. Fixture prefix: `lz2_`.
//!
//! Inference and conformance (`lz2_infer`):
//!
//!  1. A method's type argument read off an expected type that is a *function*,
//!     through a **base class** of the result. `Predef.$conforms[A]: A => A =
//!     <:<.refl` calls `def refl[A]: A =:= A`, and `=:=` reaches `Function1`
//!     two classes up; the expected type is kept structural, so nothing matched
//!     the pair and `A` was minimised to `Nothing` (`Predef.scala:510`).
//!  2. A constructor **self-call**'s arguments typed against the formals of the
//!     one constructor the arity can mean -- plainly
//!     (`concurrent/TrieMap.scala:711`) and with the clauses folded
//!     (`mutable/TreeMap.scala:48`, `mutable/TreeSet.scala:48`). With every
//!     clause written, nothing is filled in a second time: the emitted `<init>`
//!     used to push `(tree, ord, ord)` for a two-parameter descriptor.
//!  3. A subclass method and an inherited one of the same name whose parameters
//!     name **unrelated** classes are two alternatives, not one member
//!     (`collection/Factory.scala:53`, `b ++= it` on a `mutable.StringBuilder`).
//!  4. A **compound** receiver has the base types of its components, so an
//!     implicit conversion's type parameter is solved from them -- and a
//!     conversion whose own parameter is compound is solved against the
//!     component that mentions them
//!     (`collection/convert/StreamExtensions.scala:190-213`, eight errors).
//!  5. A compound on the right of `<:` is **every** one of its components, even
//!     when the left side is an abstract constructor applied to arguments
//!     (`collection/BuildFrom.scala:48,55,98`).
//!  6. A closed argument position of an implicit candidate is a *conformance*
//!     question, read at that position's variance
//!     (`concurrent/Future.scala:813,832`).
//!  7. A type parameter nothing in a call determines, carried out through a
//!     **selection** on that call's result and settled by the expected type
//!     (`concurrent/Future.scala:651`).
//!  8. A SAM whose abstract method returns `Unit` **discards** the function
//!     literal's value (`concurrent/impl/FutureConvertersImpl.scala:57`).
//!  9. Applicability between two implicit conversions is **weak** conformance,
//!     so the narrowest of three numeric conversions wins
//!     (`math/BigDecimal.scala:555`).
//! 10. `Lscala/runtime/BoxedUnit;` in a **Java** class file means `BoxedUnit`,
//!     not `Unit` (`scala/Unit.scala:41`) -- and such a field read leaves its
//!     reference on the stack, which `gen_select` decided from the descriptor
//!     rather than from the selection's own type (`VerifyError: Operand stack
//!     underflow` the moment the typer accepted it).
//!
//! Resolution (`lz2_resolve`):
//!
//! 11. A block-local `def` with **no result type** is in scope for the whole
//!     block and its type is inferred on demand (`sys/process/Parser.scala:38`),
//!     and it shadows an outer definition of the same name for the statements
//!     above it, as nsc's namer does.
//! 12. A parent that is an **inner class** is written in its enclosing class's
//!     vocabulary, and only the prefix it was written with instantiates that
//!     (`immutable/HashMap.scala:60,63,65`, `immutable/HashSet.scala`).
//! 13. An overload's parameter whose class the argument reaches only through a
//!     **base** type, while the alternative's own parameters are still open
//!     (`collection/SeqView.scala:37`).
//! 14. A **value** used as an extractor has its `unapply` read as seen from the
//!     receiver (`PartialFunction.scala:253`).
//! 15. Type parameters in a **structural refinement**: the declaration carries
//!     its own, and `conforms_to_refinement` alpha-renames them to a candidate
//!     member's before comparing the whole signature
//!     (`collection/convert/StreamExtensions.scala:44,86,87,88`).
//!
//! Two roots have no fixture here because they are only reachable when the
//! library's own sources supply the class in question, and
//! `tests/scalalib_measure.sh` is their test: `Module(args)` sugar reading an
//! inherited `apply` as seen from the module (`immutable/HashMap.scala:1777`,
//! `util/Either.scala:419` -- in jar mode `List` and `Vector` come from the
//! prelude, so the root is invisible), and the eleven singletons
//! `docs/scala-library.md` lists.
//!
//! `lz2_fwdref_bad` is a file of its own: nsc reports SLS 4.1's forward
//! reference from **RefChecks**, which does not run once the typer has reported
//! anything, so a fixture carrying a type error would hide scalac's verdict.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-lz2-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn compile(name: &str, out: &Path, jar: &Path, scalac_path: Option<&Path>) -> std::process::Output {
    let src = fixtures_dir().join(format!("{name}.scala"));
    match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    }
}

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

/// Compile `name` (with scala-rs when `scalac_path` is `None`), run it under
/// `-Xverify:all`, and compare with `tests/fixtures/expected/<name>.txt`.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, &out, &jar, scalac_path);
    assert!(
        output.status.success(),
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &jar), expected, "{name}");
    let _ = fs::remove_dir_all(&dir);
}

/// `name` must be refused, and scala-rs's diagnostics must mention each
/// `needles` fragment. `scalac_path` checks that real scalac refuses it too.
fn check_rejects(name: &str, needles: &[&str], scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, &out, &jar, scalac_path);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "{name} was accepted; it must be refused:\n{text}"
    );
    // The wording differs between the two compilers, so only scala-rs's own
    // messages are matched.
    if scalac_path.is_none() {
        for n in needles {
            assert!(text.contains(n), "{name}: expected {n:?} in:\n{text}");
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Roots 1-10: inference and conformance.

#[test]
fn inference_roots_run() {
    check_runs("lz2_infer", None);
}

#[test]
fn scalac_agrees_inference_roots() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lz2_infer", Some(&sc));
}

#[test]
fn inference_roots_still_reject() {
    check_rejects(
        "lz2_infer_bad",
        &[
            // An unrelated function type is still not what the base-type read
            // makes conform.
            "found: Eq[Int, Int]  required: (Int) => String",
            // A self-call's formals are an expected type, not a licence.
            "found: Default[String]  required: Hashing[K]",
            // The two `++=` alternatives really are two, and neither applies.
            "no matching overload for <overload (String)Chars | (IterableOnce[Char])Chars> \
             with arguments (List[Int])",
            // A compound on the right needs every component.
            "required: BF[Any, Int, CC[K, V] with Other[K]]",
            // A non-`Unit` SAM result does not discard.
            "found: String  required: Int",
        ],
        None,
    );
}

#[test]
fn scalac_agrees_inference_roots_reject() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lz2_infer_bad", &[], Some(&sc));
}

// ---------------------------------------------------------------------------
// Roots 11-15: name and member resolution.

#[test]
fn resolution_roots_run() {
    check_runs("lz2_resolve", None);
}

#[test]
fn scalac_agrees_resolution_roots() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lz2_resolve", Some(&sc));
}

#[test]
fn resolution_roots_still_reject() {
    check_rejects(
        "lz2_resolve_bad",
        &[
            // A genuinely recursive local `def` is still a cycle, at the same
            // reference and in the same words as scalac.
            "recursive method f needs result type",
            // A polymorphic structural declaration is not met by a member whose
            // signature differs.
            "could not find implicit value of type <:<[Plain[Int], Coll[Int] \
             { def stepper[S](Shape[Int, S]): S with Eff }]",
            // An inner class read through its prefix is still checked.
            "found: HM.this.KeySet  required: MySet[String]",
        ],
        None,
    );
}

#[test]
fn scalac_agrees_resolution_roots_reject() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lz2_resolve_bad", &[], Some(&sc));
}

// ---------------------------------------------------------------------------
// SLS 4.1: hoisting a block-local `def` must not make a forward reference over
// a value definition legal.

#[test]
fn forward_reference_over_a_value_is_refused() {
    check_rejects(
        "lz2_fwdref_bad",
        &[
            "forward reference to method g extends over definition of value s",
            "forward reference to method h extends over definition of value r",
            "forward reference to method k extends over definition of value f",
        ],
        None,
    );
}

#[test]
fn scalac_agrees_forward_reference_is_refused() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lz2_fwdref_bad", &[], Some(&sc));
}
