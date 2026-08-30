//! 2.13 `BuildFrom`: a collection transformation's result type is the
//! *receiver's* collection, not the class the inherited declaration named.
//!
//! Four independent causes, all of them "the result type is not narrowed to
//! the receiver":
//!
//!  * a curried call solved every clause's type parameters against the
//!    *first* clause's declared types, so `groupMapReduce(key)(f)(reduce)`
//!    never solved `B` and its reduce function came out `(Any, Any) => Any`;
//!  * `MapOps.map[K2, V2]` / `flatMap` / `collect` (and `filterNot`, `++`,
//!    `take`, `partition`, `groupBy`, `groupMap`) were read off `IterableOps`,
//!    so a `Map` came back as an `Iterable[(K, V)]` and an `IndexedSeq` as a
//!    `Seq`;
//!  * `-` / `+` / `updated` on a `TreeMap` erase to a *named* class
//!    (`(Object)Lscala/collection/immutable/Map;`), so narrowing them needed
//!    codegen to cast the result to what the typer settled on;
//!  * `xs.to(ArrayBuffer)` needs `IterableFactory.toFactory`, a view whose
//!    result type (`Factory[A, CC[A]]`) is the only thing that can say what
//!    the call's own `C1` is.
//!
//! The fixtures run against the real `scala-library` jar *and* (where the
//! private runtime can back them) the private runtime, under `-Xverify:all`,
//! and their output is compared with what nsc 2.13.16 prints for the same
//! source.

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
        "scala-rs-buildfrom-{tag}-{}-{nanos}-{seq}",
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

fn compile(out: &Path, jar: Option<&Path>, srcs: &[PathBuf], extra: &[&str]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    for a in extra {
        cmd.arg(a);
    }
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

fn accepts_with(tag: &str, source: &str, extra: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (_, msgs) = compile(&out, Some(&jar), &[src], extra);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn accepts(tag: &str, source: &str) {
    accepts_with(tag, source, &[]);
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
    let (ok, msgs) = compile(&out, Some(&jar), &[src], &[]);
    assert!(!ok, "{tag} should not compile, got:\n{msgs}");
    assert!(
        msgs.contains(needle),
        "expected {needle:?} in diagnostics for {tag}, got {msgs:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ fixtures

/// Three parameter lists, each clause inferred against its *own* declared
/// types. Runs on the private runtime and against the jar.
#[test]
fn bf_curried_runs_in_both_modes() {
    let name = "bf_curried";
    let src = fixtures_dir().join(format!("{name}.scala"));
    let exp = expected(name);

    let priv_out = tmp_dir("priv");
    let (ok, msgs) = compile(&priv_out, None, std::slice::from_ref(&src), &[]);
    assert!(ok, "compile {name} (private runtime) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&priv_out, None),
            exp,
            "stdout mismatch for {name} on the private runtime"
        );
    }
    let _ = fs::remove_dir_all(&priv_out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name} (jar): scala-library jar not present");
        return;
    };
    let jar_out = tmp_dir("jar");
    let (ok, msgs) = compile(&jar_out, Some(&jar), &[src], &[]);
    assert!(ok, "compile {name} (jar) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&jar_out, Some(&jar)),
            exp,
            "stdout mismatch for {name} against the jar"
        );
    }
    let _ = fs::remove_dir_all(&jar_out);
}

/// The collection fixture: every result type in it is a real
/// `scala.collection` class, so it is library-ABI only.
#[test]
fn bf_coll_runs_against_the_jar() {
    let name = "bf_coll";
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, Some(&jar), &[src], &[]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    if java_available() {
        assert_eq!(run_main(&out, Some(&jar)), expected(name));
    }
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime has no `MapOps`, `Factory` or `TreeMap`; it must say so
/// rather than compile something it cannot back.
#[test]
fn bf_coll_without_library_is_error() {
    let src = fixtures_dir().join("bf_coll.scala");
    let out = tmp_dir("bf_coll_nolib");
    let (ok, msgs) = compile(&out, None, &[src], &[]);
    assert!(!ok, "bf_coll should not compile without the jar:\n{msgs}");
    assert!(
        msgs.contains("error:"),
        "expected diagnostics, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Each of these is an error nsc 2.13.16 gives for the same source.
#[test]
fn bf_coll_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip bf_coll_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("bf_coll_bad.scala");
    let out = tmp_dir("bf_coll_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &[src], &[]);
    assert!(!ok, "bf_coll_bad should not compile, got:\n{msgs}");
    for needle in [
        "found: Iterable[Int]  required: Map[String, Int]",
        "found: ArrayBuffer[Int]  required: List[Int]",
        "found: Map[String, Int]  required: Map[String, String]",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics, got {msgs:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- unit-ish cases

/// The user's own minimisation of the `Map.map` case.
#[test]
fn bf_map_map_returns_a_map() {
    accepts(
        "bf_mapmap",
        "object Main { def main(a: Array[String]): Unit = {\n\
           val m: Map[String, List[Int]] = Map(\"x\" -> List(1,2))\n\
           println(m.map { case (d, g) => d -> g.sum })\n\
           val n: Map[String, Int] = m.map { case (d, g) => d -> g.sum }\n\
           println(n) } }\n",
    );
}

/// The user's own minimisation of the `groupMapReduce` case. The error used to
/// surface at an unrelated line of the same file.
#[test]
fn bf_group_map_reduce_infers_its_third_clause() {
    accepts(
        "bf_gmr",
        "case class E(d: String, s: Int)\n\
         object Main { def main(a: Array[String]): Unit = {\n\
           val es = List(E(\"x\",10), E(\"y\",20))\n\
           println(es.groupMapReduce(_.d)(_.s)(_ + _).toList.sorted)\n\
           val m: Map[String, Int] = es.groupMapReduce(_.d)(_.s)(_ + _)\n\
           println(m.size) } }\n",
    );
}

/// `IndexedSeq` does not redeclare these; the inherited declaration says
/// `Seq`, and 2.13 returns `CC[B]`.
#[test]
fn bf_indexed_seq_keeps_its_class() {
    accepts(
        "bf_ixseq",
        "object Main { def main(a: Array[String]): Unit = {\n\
           val xs: IndexedSeq[Int] = Vector(1,2,3)\n\
           val b: IndexedSeq[Int] = xs.flatMap(i => List(i))\n\
           val c: IndexedSeq[(Int, String)] = xs.zip(List(\"a\"))\n\
           val d: (IndexedSeq[Int], IndexedSeq[Int]) = xs.partition(_ > 1)\n\
           val e: Map[Boolean, IndexedSeq[Int]] = xs.groupBy(_ > 1)\n\
           println(b.size + c.size + d._1.size + e.size) } }\n",
    );
}

/// `TreeMap - key` is a `TreeMap`: the member erases to
/// `(Object)Lscala/collection/immutable/Map;`, so the call site casts.
#[test]
fn bf_tree_map_keeps_its_class() {
    accepts(
        "bf_treemap",
        "import scala.collection.immutable.TreeMap\n\
         object Main { def main(a: Array[String]): Unit = {\n\
           val t = TreeMap(1 -> \"a\", 2 -> \"b\")\n\
           val u: TreeMap[Int, String] = t - 1\n\
           val v: TreeMap[Int, String] = t + ((3, \"c\"))\n\
           val w: TreeMap[Int, String] = t.updated(4, \"d\")\n\
           val x: TreeMap[Int, String] = t.filter(_._1 > 1)\n\
           println(u.size + v.size + w.size + x.size) } }\n",
    );
}

/// `to(factory)` through `IterableFactory.toFactory` / `MapFactory.toFactory`,
/// and the same evidence found as an implicit *value*.
#[test]
fn bf_to_factory_resolves_from_the_companion() {
    accepts(
        "bf_tofactory",
        "import scala.collection.mutable.ArrayBuffer\n\
         object Main { def main(a: Array[String]): Unit = {\n\
           val b: ArrayBuffer[Int] = List(1,2).to(ArrayBuffer)\n\
           val l: List[Int] = Vector(1).to(List)\n\
           val m: Map[String, Int] = List((\"k\", 1)).to(Map)\n\
           val f = implicitly[scala.collection.Factory[Int, Vector[Int]]]\n\
           println(b.size + l.size + m.size + f.fromSpecific(List(1)).size) } }\n",
    );
}

/// `+` and `-` reach the narrowing for *every* receiver; arithmetic and string
/// concatenation must be untouched.
#[test]
fn bf_plus_minus_on_non_collections_is_untouched() {
    accepts(
        "bf_plusminus",
        "object Main { def main(a: Array[String]): Unit = {\n\
           println(1 + 2)\n\
           println(1 - 2)\n\
           println(\"a\" + 1)\n\
           println(1.5 + 2.5) } }\n",
    );
}

/// `Set ++ IterableOnce` is `SetOps.concat`, not "another `Set`".
#[test]
fn bf_set_concat_takes_any_iterable_once() {
    accepts(
        "bf_setconcat",
        "object Main { def main(a: Array[String]): Unit = {\n\
           val s: Set[Int] = Set(1,2) ++ List(3)\n\
           val t: Set[Int] = Set(1,2) ++ Vector(3)\n\
           println(s.size + t.size) } }\n",
    );
}

/// A lambda that does not return a pair keeps `Iterable[B]` -- and the call
/// has to be `IterableOps.map`, or `MapOps.map` throws
/// `ClassCastException: Integer cannot be cast to Tuple2` at run time.
#[test]
fn bf_map_map_without_a_pair_is_an_iterable() {
    rejects(
        "bf_mapnonpair",
        "object Main { def main(a: Array[String]): Unit = {\n\
           val m: Map[String, Int] = Map(\"a\" -> 1).map { case (_, v) => v }\n\
           println(m) } }\n",
        "found: Iterable[Int]  required: Map[String, Int]",
    );
}

/// A user class whose inherited `map` really does return the *parent*'s type
/// keeps it: the rebuild is for `scala.collection` classes only, or every
/// `def map[R2](f: R => R2): Act[R2, NoStream, E]` in slick would lose its
/// other arguments.
#[test]
fn bf_user_subclass_does_not_rebuild() {
    accepts(
        "bf_usersub",
        "trait Base[A] { def map[B](f: A => B): Base[B] = null }\n\
         class Mine[A] extends Base[A]\n\
         object Main { def main(a: Array[String]): Unit = {\n\
           val b: Base[String] = new Mine[Int]().map(_.toString)\n\
           println(b == null) } }\n",
    );
}
