//! Taking scala/scala's own `src/library` towards zero errors (`agent/libzero`).
//!
//! Nine independent roots, each reduced to a standalone program and compared
//! with real scalac 2.13.16 in both directions.
//!
//! Inference (`lz_infer`):
//!
//! 1. A *value* of function type is not applicable to a `PartialFunction`
//!    formal. `arg_score` scores one because that is also how an already-typed
//!    function literal reaches such a formal, and specificity then preferred the
//!    `PartialFunction` alternative -- `f1 andThen k` for a `k: B => C` chose
//!    `PartialFunction.andThen[C](k: PartialFunction[B, C])` and reported the
//!    mismatch on the argument it had just chosen against
//!    (`PartialFunction.scala:277`).
//! 2. nsc's `adjustTypeArgs` instantiates a `Nothing` the arguments inferred
//!    wherever the parameter is covariant in the result, expected type or not:
//!    the expected type does not get to improve on a covariant `Nothing`, so it
//!    cannot be the reason to hold the solution back either
//!    (`util/control/Exception.scala:274-276,365`).
//! 3. A lower bound naming another type parameter of the same method
//!    (`[B, B1 >: B]`) is applied after the first solving pass, with the
//!    solutions substituted in (`immutable/TreeSet.scala:163,180`).
//! 4. `Nothing` and `Null` in an *invariant* position of the expected type are
//!    a real answer -- nothing can widen the parameter afterwards
//!    (`mutable/TreeSet.scala:206,213,216`).
//! 5. A type parameter's *contravariant* occurrences bound it from above, so
//!    they lose to any position that pinned it, and are still the answer when
//!    nothing else contributed (`collection/StringOps.scala:282,302`).
//! 6. The implicit-search pass takes the type parameters the implicit clause
//!    mentions instead of demanding that it mention every one, so an `implicit
//!    def` member of the enclosing class can answer it
//!    (`immutable/SortedMap.scala:175`, `mutable/SortedMap.scala:101`).
//!
//! Resolution (`lz_resolve`):
//!
//! 7. `new B(x)` for a parameterized alias `type B[+A] = p.Box[A]` constructs
//!    `p.Box` with its arguments left to inference (`Option.scala:575`).
//! 8. `->` is read off the receiver only for the prelude's deliberately
//!    imprecise `ArrowAssoc`; a `->` with a signature of its own is typed from
//!    it (`Predef.scala:352`).
//! 9. A receiver widened to its bound so its members could be found is still
//!    what an inserted conversion is applied to (`collection/package.scala:78`),
//!    and `super.m` reached through a parent that only *inherits* `m` is read at
//!    this class's own arguments for the class that declares it
//!    (`immutable/BitSet.scala:83`, `mutable/BitSet.scala:188`,
//!    `immutable/TreeMap.scala:164`).
//!
//! Two of the roots are visible only when the library is compiled from source
//! (`--no-scala-library`), so they have no fixture here and
//! `tests/scalalib_measure.sh` is their test: the case class whose companion is
//! hidden by `val :: = scala.collection.immutable.::` in `package scala`
//! (`concurrent/duration/Duration.scala:79,80`) and `Exception.scala`'s
//! `Catcher[Nothing]` chain. `lz_resolve` still exercises the first one's
//! machinery (a case-class extractor whose companion is shadowed at the use
//! site).
//!
//! Fixture prefix: `lz_`.

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
        "scala-rs-lz-{tag}-{}-{nanos}-{seq}",
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

/// Compile `name` (with scala-rs when `scalac_path` is `None`), run it, and
/// compare with `tests/fixtures/expected/<name>.txt`.
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
// Roots 1-6: inference.

#[test]
fn inference_roots_run() {
    check_runs("lz_infer", None);
}

#[test]
fn scalac_agrees_inference_roots() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lz_infer", Some(&sc));
}

#[test]
fn inference_roots_still_reject() {
    check_rejects(
        "lz_infer_bad",
        &[
            // A value of function type is not a PartialFunction.
            "found: (Int) => Int  required: PartialFunction[Int, Int]",
            // `B1 >: B` with explicit arguments.
            "[String,Any,Null] do not conform to method upd's type parameter bounds",
            // An upper bound with explicit arguments.
            "[Int] do not conform to method narrow's type parameter bounds",
            // A non-`implicit` override of an `implicit def` is not an implicit.
            "could not find implicit value of type Ord[K]",
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
    check_rejects("lz_infer_bad", &[], Some(&sc));
}

// ---------------------------------------------------------------------------
// Roots 7-9: name and member resolution.

#[test]
fn resolution_roots_run() {
    check_runs("lz_resolve", None);
}

#[test]
fn scalac_agrees_resolution_roots() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lz_resolve", Some(&sc));
}

#[test]
fn resolution_roots_still_reject() {
    check_rejects(
        "lz_resolve_bad",
        &[
            // `new` on a deferred type member is still not a class type.
            "class type required but HasT.this.T found",
            // A case class's extractor keeps its constructor's arity.
            "extractor Pair expects 2 argument(s), found 3",
            // `super.m` read at the declaring class is still conformance-checked.
            "found: WideBox[(Int, B)]  required: NarrowBox[(Int, B)]",
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
    check_rejects("lz_resolve_bad", &[], Some(&sc));
}
