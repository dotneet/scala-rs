//! `agent/tuplepat`: `scala/collection/Seq.scala`'s twenty-one
//! `is not a member of T1` / `T2` reports.
//!
//! The brief said the defect was the **tuple pattern definition** -- that
//! `private[this] val (elms, idxs) = init()` gave its accessors `Tuple2`'s own
//! parameter names instead of the components of the right-hand side's type.
//! Measured, that is not what happens. A pattern definition whose right-hand
//! side types correctly has always worked (`Pat` in the fixture is six of
//! them, executed). `T1`/`T2` is what a *failed* right-hand side leaves
//! behind: a tuple literal one of whose components did not type keeps
//! `Tuple2`'s parameter uninstantiated, the pattern definition faithfully
//! hands that to every bound name, and each later use reports it. The pattern
//! definition was the messenger.
//!
//! The three roots underneath, all confirmed against real scalac 2.13.16:
//!
//! 1. **A `private[this]` member reached through the trait's self-alias.**
//!    `trait SeqOps[…] { self => … }` and `self.toGenericSeq` from
//!    `PermutationsItr`. `Checker::prefix_is_this` asked the *tree* for a
//!    `This` node; nsc (`Contexts.isAccessible`) asks the prefix **type**
//!    (`pre =:= sym.owner.thisType`), and a self-alias is an `Ident` whose
//!    type is `SeqOps.this.type`. Worth 5 library errors on its own; it is the
//!    first domino of the twenty-one.
//! 2. **An inherited factory `apply` reached as `Obj[K, V](…)`.**
//!    `object HashMap extends MapFactory[HashMap]` inherits
//!    `def apply[K, V](elems: (K, V)*): CC[K, V]`, and the redirect that turns
//!    `mutable.HashMap[A, Int]()` into that `apply` took the declaration's type
//!    raw, so the call came back `CC[A, Int]`. Worth 30 library errors.
//! 3. **`xs.to(SomeFactory)` with an abstract element type.**
//!    `Implicits::open_conversion_fit` required its solution to mention *no*
//!    type parameter at all; a type parameter of an enclosing class is a fixed
//!    type, so `List[Int]` worked and `List[A]` did not. Worth 15.
//!
//! And a **miscompilation found by executing the fixture**, present at the
//! branch point and invisible to every check that does not run code: root 2's
//! redirect left the receiver as a bare `Ident`, which inside a class body
//! means `this`, so `TM[Int](1)` for `object TM extends Fac` emitted
//! `Fac.apply` on `this` and threw `ClassCastException`. Written out,
//! `TM.apply[Int](1)` was correct all along.
//!
//! Fixture prefix: `tuplepat_`, plus `tuplepat.scala`.

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

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-tuplepat-{tag}-{}-{nanos}-{seq}",
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

fn compile_rs(srcs: &[PathBuf], out: &Path, jar: Option<&Path>) -> Run {
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
    out_of(cmd.output().expect("run scala-rs compile"))
}

fn compile_scalac(sc: &Path, srcs: &[PathBuf], out: &Path, jar: &Path) -> Run {
    let mut cmd = Command::new(sc);
    cmd.args([
        "-classpath",
        jar.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    for s in srcs {
        cmd.arg(s);
    }
    out_of(cmd.output().expect("run scalac"))
}

fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-cp", &cp, "Main"])
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

fn write_src(dir: &Path, name: &str, src: &str) -> PathBuf {
    let p = dir.join(format!("{name}.scala"));
    fs::write(&p, src).unwrap();
    p
}

/// Compile `src` with both compilers and run `Main` under each; returns
/// `(ours, scalacs)`. Both are required to compile.
fn both_outputs(sc: &Path, jar: &Path, dir: &Path, src: &Path) -> (String, String) {
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    let srcs = [src.to_path_buf()];
    ok(
        compile_scalac(sc, &srcs, &theirs, jar),
        "the source, scalac",
    );
    ok(compile_rs(&srcs, &ours, Some(jar)), "the source, scala-rs");
    (run_java(&ours, jar), run_java(&theirs, jar))
}

// ---------------------------------------------------------------------------
// The positive fixture: all three roots, plus the pattern definition in every
// shape SLS 4.1 allows, executed.

/// The library's own shape (`Perms`), an inherited factory `apply` reached as
/// `Obj[K, V](…)` with a higher-kinded and a plain parent parameter, and six
/// pattern definitions -- tuple, extractor, nested, `var`, `private[this]`,
/// and one that proves the right-hand side is evaluated **once** (`effects`
/// prints `1`, not `2`).
///
/// On an unmodified build of the branch point this file does not compile:
/// `method generic in trait Perms cannot be accessed as a member of
/// Perms.this.type from class Itr in trait Perms`, `value size is not a member
/// of CC[K, V]`, and `no matching overload for (Factory[A, C1])C1 with
/// arguments (ArrayBuffer$)`.
#[test]
fn tuplepat_runs() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip tuplepat_runs: jar or java not present");
        return;
    };
    let dir = tmp_dir("run");
    ok(
        compile_rs(&[fixture("tuplepat")], &dir, Some(&jar)),
        "tuplepat",
    );
    assert_eq!(run_java(&dir, &jar), expected_stdout("tuplepat"));
    let _ = fs::remove_dir_all(&dir);
}

/// The expected file is scalac's, checked by producing it again here. Without
/// this, `expected/tuplepat.txt` is only what this compiler happened to print
/// the day it was written.
#[test]
fn tuplepat_expected_output_is_scalacs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip tuplepat_expected_output_is_scalacs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("scalac");
    ok(
        compile_scalac(&sc, &[fixture("tuplepat")], &dir, &jar),
        "tuplepat under scalac",
    );
    assert_eq!(run_java(&dir, &jar), expected_stdout("tuplepat"));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Root 2's miscompilation: the receiver, not the type.

/// `object TM extends Fac` where `Fac` declares `apply`: `TM[Int](1)` written
/// **inside a class or object body**. `peel_fun` reads the receiver off the
/// tree shape and a bare `Ident` there means `this`, so the branch point
/// emitted `Fac.apply` on the enclosing instance -- legal bytecode, consistent
/// types, `ClassCastException: class Uses cannot be cast to class Fac` at run
/// time. Nothing but execution sees it, which is why this test runs the
/// program rather than reading the signature.
///
/// Four call shapes, because only one of them was broken: the other three
/// (`TM.apply[Int](1)`, `TM(1)`, and an `apply` the object declares itself)
/// were already right and are here so a fix cannot trade one for another.
#[test]
fn tuplepat_inherited_apply_calls_the_module_not_this() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!(
            "skip tuplepat_inherited_apply_calls_the_module_not_this: jar/scalac/java absent"
        );
        return;
    };
    let dir = tmp_dir("recv");
    let src = write_src(
        &dir,
        "Main",
        "trait Fac { def apply[K](x: K): String = \"F\" + x.toString }\n\
         object TM extends Fac\n\
         object Own { def apply[K](x: K): String = \"O\" + x.toString }\n\
         class InClass { def a: String = TM[Int](1); def b: String = TM.apply[Int](2)\n  \
           def c: String = TM(3); def d: String = Own[Int](4) }\n\
         object InObject { def a: String = TM[Int](5) }\n\
         object Main {\n  \
           def main(args: Array[String]): Unit = {\n    \
             val u = new InClass\n    \
             println(u.a + u.b + u.c + u.d + InObject.a)\n  }\n}\n",
    );
    let (ours, theirs) = both_outputs(&sc, &jar, &dir, &src);
    assert_eq!(theirs, "F1F2F3O4F5\n", "scalac's own answer");
    assert_eq!(ours, theirs, "receiver of an inherited `apply`");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Root 1's other half: `private[this]` is still object-private.

/// A different instance is not `this`. Both compilers reject both selections
/// -- one in the trait itself, one in a class nested inside it -- so the
/// widening in `prefix_is_this` did not turn the rule off. nsc words it
/// differently (it drops the member from a non-`this` prefix's member set
/// altogether), so the count and the rejection are what is asserted.
#[test]
fn tuplepat_object_private_is_still_object_private() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip tuplepat_object_private_is_still_object_private: jar not present");
        return;
    };
    let dir = tmp_dir("acc");
    let run = compile_rs(&[fixture("tuplepat_access_bad")], &dir, Some(&jar));
    assert!(!run.ok, "expected rejection:\n{}", run.text);
    assert_eq!(
        run.text
            .lines()
            .filter(|l| l.starts_with("error:") && l.contains("method gen"))
            .count(),
        2,
        "one per selection:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}

/// scalac's own answer for the same file, so "two errors" is nsc's and not
/// ours.
#[test]
fn tuplepat_object_private_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip tuplepat_object_private_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("accsc");
    let run = compile_scalac(&sc, &[fixture("tuplepat_access_bad")], &dir, &jar);
    assert!(!run.ok, "expected scalac to reject:\n{}", run.text);
    assert_eq!(
        run.text.matches("error:").count(),
        2,
        "scalac reports one per selection:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The pattern definition itself.

/// A **refutable** pattern definition compiles and throws `MatchError` at run
/// time -- it is not a compile error. The message is the JVM's own
/// (`None (of class scala.None$)`), and both compilers produce it.
#[test]
fn tuplepat_refutable_pattern_throws_match_error() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip tuplepat_refutable_pattern_throws_match_error: jar/scalac/java absent");
        return;
    };
    let dir = tmp_dir("refut");
    let src = write_src(
        &dir,
        "Main",
        "object Main {\n  \
           def main(args: Array[String]): Unit = {\n    \
             val o: Option[Int] = None\n    \
             try { val Some(x) = o; println(x) }\n    \
             catch { case e: MatchError => println(\"MatchError:\" + e.getMessage) }\n  }\n}\n",
    );
    let (ours, theirs) = both_outputs(&sc, &jar, &dir, &src);
    assert_eq!(theirs, "MatchError:None (of class scala.None$)\n");
    assert_eq!(ours, theirs);
    let _ = fs::remove_dir_all(&dir);
}

/// The library's `PermutationsItr` line -- `private[this] val (a, b) = init()`
/// inside a class nested in a generic trait, where `init()`'s type is inferred
/// -- reaches the bound names' *component* types and not `Tuple2`'s own
/// parameters. Asserted on a member only the real component has.
#[test]
fn tuplepat_nested_private_this_pattern_definition_has_component_types() {
    let Some(jar) = scala_library_jar() else {
        eprintln!(
            "skip tuplepat_nested_private_this_pattern_definition_has_component_types: jar absent"
        );
        return;
    };
    let dir = tmp_dir("comp");
    let src = write_src(
        &dir,
        "Main",
        "import scala.collection.mutable\n\
         trait Ops[A] { self =>\n  \
           def items: List[A]\n  \
           private[this] def gen: List[A] = items\n  \
           private class Itr {\n    \
             private[this] val (elms, idxs) = init()\n    \
             def n: Int = elms.size + idxs.length\n    \
             private[this] def init() = (self.gen.to(mutable.ArrayBuffer), Array(1, 2))\n  }\n  \
           def n: Int = new Itr().n\n}\n",
    );
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    ok(
        compile_rs(&[src], &out, Some(&jar)),
        "a nested private[this] tuple pattern definition",
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The other half of the one above: a member neither component has is still
/// refused, so the test passes because the types are right and not because the
/// receiver became permissive. `T1` must not appear in the diagnostic -- that
/// was the symptom this slice removed.
#[test]
fn tuplepat_component_types_still_refuse_a_stranger() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip tuplepat_component_types_still_refuse_a_stranger: jar absent");
        return;
    };
    let dir = tmp_dir("compneg");
    let src = write_src(
        &dir,
        "Main",
        "import scala.collection.mutable\n\
         trait Ops[A] { self =>\n  \
           def items: List[A]\n  \
           private[this] def gen: List[A] = items\n  \
           private class Itr {\n    \
             private[this] val (elms, idxs) = init()\n    \
             def n: Int = elms.notAMemberOfArrayBuffer\n    \
             private[this] def init() = (self.gen.to(mutable.ArrayBuffer), Array(1, 2))\n  }\n}\n",
    );
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let run = compile_rs(&[src], &out, Some(&jar));
    assert!(!run.ok, "expected rejection:\n{}", run.text);
    assert!(
        run.text.contains("notAMemberOfArrayBuffer"),
        "the diagnostic should name the member:\n{}",
        run.text
    );
    assert!(
        !run.text.contains("not a member of T1"),
        "the receiver must be the component type, not Tuple2's parameter:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Found here, not fixed here.

/// `Obj.apply[K, V]()` where `apply` takes an **empty parameter list**.
/// scalac calls it; this compiler reads `Obj.apply[K, V]` as the completed
/// application and then treats the `()` as a second one -- "value apply is not
/// a member of QC[Int, Int]". Independent of this slice's roots: it happens
/// whether the `apply` is inherited or declared in the object, and it needs
/// the empty clause (`apply[K, V](x: Int)` is fine). Asserted on the *current*
/// behaviour so that a later slice closing it is told by a failing test.
#[test]
fn tuplepat_explicit_apply_with_empty_param_list_is_still_rejected() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip tuplepat_explicit_apply_with_empty_param_list_is_still_rejected: absent");
        return;
    };
    let dir = tmp_dir("emptyclause");
    let src = write_src(
        &dir,
        "Main",
        "class QC[K, V] { def size: Int = 0 }\n\
         object QCO { def apply[K, V](): QC[K, V] = new QC[K, V] }\n\
         object UC { def f = QCO.apply[Int, Int]().size }\n",
    );
    let srcs = [src];
    let scdir = dir.join("sc");
    fs::create_dir_all(&scdir).unwrap();
    let theirs = compile_scalac(&sc, &srcs, &scdir, &jar);
    assert!(
        theirs.ok,
        "scalac is expected to accept `QCO.apply[Int, Int]()`:\n{}",
        theirs.text
    );
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let ours = compile_rs(&srcs, &out, Some(&jar));
    assert!(
        !ours.ok && ours.text.contains("value apply is not a member of"),
        "this is a known gap. If this now passes the gap is closed and the assertion \
         should become the opposite one:\n{}",
        ours.text
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The synthetic holder a pattern definition creates is **public** here and
/// `private` in nsc, and it has a public accessor nsc does not emit at all
/// (`x$pat2()` beside nsc's bare `private static final scala.Tuple2 x$2`).
/// The programs behave identically -- the whole rest of this file is that
/// claim, executed -- but the ABI carries members it should not. Closing it is
/// a change in how pattern-definition accessors are entered, with its own
/// blast radius; pinned here so a later slice is told.
#[test]
fn tuplepat_synthetic_holder_is_still_public() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip tuplepat_synthetic_holder_is_still_public: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("holder");
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    let src = write_src(&dir, "Main", "object Main { val (a, b) = (\"x\", 1) }\n");
    let srcs = [src];
    ok(
        compile_scalac(&sc, &srcs, &theirs, &jar),
        "the holder, scalac",
    );
    ok(compile_rs(&srcs, &ours, Some(&jar)), "the holder, scala-rs");
    let javap = |d: &Path| -> String {
        let o = Command::new("javap")
            .args(["-p", "-cp", d.to_str().unwrap(), "Main$"])
            .output()
            .expect("run javap");
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    let a = javap(&ours);
    let b = javap(&theirs);
    assert!(
        b.contains("private static final scala.Tuple2 x$"),
        "nsc keeps the holder private and gives it no accessor:\n{b}"
    );
    assert!(
        !b.contains("x$1()") && !b.contains("x$2()"),
        "nsc emits no accessor for the holder:\n{b}"
    );
    assert!(
        a.contains("public scala.Tuple2<java.lang.String, java.lang.Object> x$pat"),
        "we are known to emit a public accessor for the holder; if this fails the gap \
         is closed and this assertion should become the opposite one:\n{a}"
    );
    let _ = fs::remove_dir_all(&dir);
}
