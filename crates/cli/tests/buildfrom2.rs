//! `BuildFrom` matched at a *higher kind*, so `LazyZip2.map`'s result type is
//! decided by the implicit search.
//!
//! 2.13 declares
//!
//! ```text
//! class LazyZip2[+El1, +El2, C1] {
//!   def map[B, C](f: (El1, El2) => B)(implicit bf: BuildFrom[C1, B, C]): C
//! }
//! ```
//!
//! `C` appears nowhere but the implicit clause, so only the witness can say
//! what it is -- and the only general witness is
//!
//! ```text
//! implicit def buildFromIterableOps[CC[X] <: Iterable[X] with IterableOps[X, CC, _], A0, A]
//!   : BuildFrom[CC[A0], A, CC[A]]
//! ```
//!
//! Five things stood between the two, and each of them hid the next:
//!
//!  * **`BuildFrom`'s companion object was in no symbol table.** Companions of
//!    `scala.*` classes were skipped on the grounds that the prelude describes
//!    the standard library -- but the prelude describes what programs *name*,
//!    and nothing ever names an implicit. Nothing named `BuildFrom` either, so
//!    unless the program happened to `import scala.collection.BuildFrom`, its
//!    witnesses were in no scope at all. A companion the prelude *did* declare
//!    is still left alone, and so is one already entered under that JVM name.
//!  * **Its low-priority half was still missing.** `object BuildFrom extends
//!    BuildFromLowPriority1 extends BuildFromLowPriority2`, and
//!    `buildFromIterableOps` is declared in the last of those.
//!  * **Supplying an implicit deleted it.** `supply_implicit_members` drops the
//!    crude classfile member a pickled signature replaces, but completion
//!    caches what it has served: when the answer *was* the pickled member, it
//!    was both the "stale" one and the new one, and the class ended up with no
//!    member of that name.
//!  * **The two-sided unifier could not match an unknown type constructor.**
//!    `CC[A0]` is an `Applied` whose head is a type parameter and `List[String]`
//!    is a `Class`; no case related the two, so the pair fell through to
//!    `a == b`. A fully applied summon still worked, because it falls back to
//!    the *one-sided* `unify_one` (which does read a constructor) -- and that
//!    fallback is skipped exactly when the call site has an undetermined
//!    parameter of its own, which is the `LazyZip2.map` case.
//!    `xs.lazyZip(ys).map(f)` therefore reported
//!    `could not find implicit value of type BuildFrom[…, C]` and then
//!    `value mkString is not a member of C`.
//!  * **Nothing told the witnesses apart.** They are the same type but for
//!    their bounds. A higher-kinded bound reaches the typer folded into the
//!    type (`buildFromSortedSetOps` is
//!    `BuildFrom[CC[A0] with SortedSet[A0], A, CC[A] with SortedSet[A]]`), so
//!    unifying refinements is what enforces it -- and a `TreeSet` is only a
//!    `collection.SortedSet` once the prelude's hierarchy says so. A
//!    *first-order* F-bound stays in `bound_hi`, and unchecked,
//!    `buildFromBitSet[C <: BitSet with BitSetOps[C]]: BuildFrom[C, Int, C]`
//!    answered for a `List` (it is declared in the companion itself, so it wins
//!    on origin) and `List(1, 2).lazyZip(…).map(_ + _)` type-checked and then
//!    died with `class ::$ cannot be cast to class scala.collection.BitSet`.
//!
//! Three bugs on the way out were found by running the fixtures rather than by
//! reading them. A witness that takes its own implicits was emitted as a bare
//! name, so codegen loaded `this` and cast it. An unknown *call-site*
//! constructor must not be solvable by a conversion: letting it be made
//! `firstLength[A, M[+X] <: Iterable[X]](in: M[A])` reach `M[A]` from a
//! `List[Int]` through `IterableOnce.iterableOnceExtensionMethods`, which
//! `tests/fixtures/mism12_lib.scala` caught as a `ClassCastException`. And a
//! standard-library companion read from its *classfile* puts erased
//! signatures next to the pickled ones -- `object Option` gained a second
//! `apply` and `Option(2)` became `ambiguous overload`
//! (`tests/fixtures/jarpk.scala`), so for `scala.*` only the implicits are
//! installed and the rest keeps coming from the pickle on demand.
//!
//! The fixtures run against the real `scala-library` jar under `-Xverify:all`,
//! with their stdout compared against nsc 2.13.16's.

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
        "scala-rs-buildfrom2-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

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
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile(out: &Path, jar: Option<&Path>, srcs: &[PathBuf]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    match jar {
        Some(j) => cmd.args(["--scala-library", j.to_str().unwrap()]),
        None => cmd.arg("--no-scala-library"),
    };
    let output = cmd.output().expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn accepts(tag: &str, source: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (_, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn runs(tag: &str, source: &str, want: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(ok, "{tag} should compile, got:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&out, Some(&jar)),
            want,
            "stdout mismatch for {tag}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

fn rejects(tag: &str, source: &str, needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "{tag} should not compile, got:\n{msgs}");
    assert!(
        msgs.contains(needle),
        "expected {needle:?} in diagnostics for {tag}, got {msgs:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ fixtures

/// The whole slice, run under the verifier; every line of its output is what
/// nsc 2.13.16 prints for the same source.
#[test]
fn bf2_lazyzip_runs_against_the_jar() {
    let name = "bf2_lazyzip";
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    if java_available() {
        assert_eq!(run_main(&out, Some(&jar)), expected(name));
    }
    let _ = fs::remove_dir_all(&out);
}

/// `BuildFrom`, `LazyZip2` and `IterableOps` are jar-only; the private runtime
/// has to say so rather than compile something it cannot back.
#[test]
fn bf2_lazyzip_without_library_is_error() {
    let src = fixtures_dir().join("bf2_lazyzip.scala");
    let out = tmp_dir("bf2_nolib");
    let (ok, msgs) = compile(&out, None, &[src]);
    assert!(
        !ok,
        "bf2_lazyzip should not compile without the jar:\n{msgs}"
    );
    assert!(
        msgs.contains("error:"),
        "expected diagnostics, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Each of these is an error nsc 2.13.16 gives for the same source
/// ("Cannot construct a collection of type …"): the higher-kinded match must
/// not turn into "any `C` will do".
#[test]
fn bf2_lazyzip_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip bf2_lazyzip_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("bf2_lazyzip_bad.scala");
    let out = tmp_dir("bf2_lazyzip_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "bf2_lazyzip_bad should not compile, got:\n{msgs}");
    for needle in [
        "BuildFrom[List[Int], String, Vector[String]]",
        "BuildFrom[Int, String, List[String]]",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics, got {msgs:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- unit-ish cases

/// slick's `JdbcStatementBuilderComponent`: the receiver is an `Iterable[U]`
/// at an abstract element type, so `CC := Iterable` and `A0 := U`.
#[test]
fn bf2_lazyzip_at_an_abstract_element_type() {
    accepts(
        "bf2_abstract",
        "object Main {\n\
           def mux[U, RU](vs: Iterable[U], ks: Iterable[RU]): Seq[(U, RU)] =\n\
             vs.lazyZip(ks).map((a, b) => (a, b)).toSeq\n\
           def main(a: Array[String]): Unit = println(mux(List(1), List(\"x\"))) }\n",
    );
}

/// A `Map` receiver goes through `buildFromMapOps`, whose element type is a
/// pair; the result is a `Map`, not an `Iterable`.
#[test]
fn bf2_lazyzip_on_a_map_rebuilds_a_map() {
    runs(
        "bf2_map",
        "object Main { def main(a: Array[String]): Unit = {\n\
           val m = Map(1 -> \"a\")\n\
           val n: Map[Int, String] = m.lazyZip(List(9)).map((kv, i) => (kv._1 + i, kv._2))\n\
           println(n) } }\n",
        "Map(10 -> a)\n",
    );
}

/// The bound is what tells the `BuildFrom` witnesses apart. A `List` is no
/// `SortedSet`, so `buildFromSortedSetOps` must not be the one that answers —
/// if it were, the result would be built through `sortedIterableFactory` and
/// blow up at run time even though the types checked.
#[test]
fn bf2_sorted_witness_does_not_answer_for_a_list() {
    runs(
        "bf2_notsorted",
        "object Main { def main(a: Array[String]): Unit = {\n\
           val xs: List[Int] = List(3, 1).lazyZip(List(1, 1)).map((x, y) => x + y)\n\
           println(xs) } }\n",
        "List(4, 2)\n",
    );
}

/// A `TreeSet` receiver *does* take `buildFromSortedSetOps`, which needs an
/// `Ordering` for the new element type.
#[test]
fn bf2_sorted_witness_answers_for_a_treeset() {
    runs(
        "bf2_treeset",
        "import scala.collection.immutable.TreeSet\n\
         object Main { def main(a: Array[String]): Unit = {\n\
           val t = TreeSet(1, 2, 3)\n\
           val u: TreeSet[Int] = t.lazyZip(List(10, 20, 30)).map((x, y) => x + y)\n\
           println(u) } }\n",
        "TreeSet(11, 22, 33)\n",
    );
}

/// A `String` receiver has its own witness (`buildFromString`), and the
/// element type decides between it and the `IndexedSeq` fallback.
#[test]
fn bf2_string_receiver_builds_a_string() {
    runs(
        "bf2_string",
        "object Main { def main(a: Array[String]): Unit = {\n\
           val s: String = \"abc\".lazyZip(List(1, 2, 3)).map((c, i) => (c + i).toChar)\n\
           println(s) } }\n",
        "bdf\n",
    );
}

/// The unknown constructor is solved from the *first* argument only; a wanted
/// result the witness cannot build is still a hard error.
#[test]
fn bf2_wrong_result_collection_is_rejected() {
    rejects(
        "bf2_wrongcc",
        "object Main { def main(a: Array[String]): Unit = {\n\
           val v: Vector[Int] = List(1).lazyZip(List(2)).map((x, y) => x + y)\n\
           println(v) } }\n",
        "error:",
    );
}
