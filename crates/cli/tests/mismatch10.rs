//! Four causes behind slick's remaining `type mismatch`es, two of which were
//! also silent miscompiles.
//!
//!  * A parent's constructor *arguments* are ordinary expressions, and the
//!    signature pass types them before every unit's members have their types.
//!    slick's `case class ColumnOrdered[T](column: Rep[T], ord: Ordering)
//!    extends Ordered(Vector((column.toNode, ord)))` is compiled with
//!    `Rep.scala` later on the command line, so `toNode` was not a member yet
//!    and the tuple came out `(?T1, Ordering)`. The body pass types the same
//!    tree again and gets it right, so the signature pass's complaints about a
//!    parent's arguments are dropped -- exactly as the header pass's are.
//!  * A primary constructor declares no type parameters of its own, and its
//!    defaulted parameters have no `name$default$n` getter (there is no
//!    receiver to call one on), so their bodies are typed at the call site --
//!    where the class's `A` is bound to nothing. `class C[A](l: List[A] =
//!    List.empty[A])` reported `found: List[A]  required: List[A]`.
//!  * An undetermined variable reaches an argument's position as its declared
//!    bound. A bare one was recognised and left open so a literal's body could
//!    decide it; one inside a tuple was not, so
//!    `TreeMap.collect { case (k, v) => … }` typed its case bodies against
//!    `(Any, Any)` and then asked for `Ordering[Any]`.
//!  * A library member is read from the pickle on demand and installed on the
//!    class that declares it, so which of an overridden pair the typer holds
//!    depended on what earlier code asked for. `TreeMap(…).collect(pf)`
//!    returned a **`List`** when a plain `Map.collect` appeared earlier in the
//!    same file, with no diagnostic anywhere.
//!
//! The fixtures are dual-run: compiled against the real `scala-library` jar
//! and (where the private runtime can back them) on the private runtime, under
//! `-Xverify:all`, and their stdout is compared with what nsc 2.13.16 prints.

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
        "scala-rs-mism10-{tag}-{}-{nanos}-{seq}",
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

/// Runs an emitted `Main` and compares it with the expected stdout, in both
/// modes.
fn dual_run(name: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let exp = expected(name);

    let priv_out = tmp_dir("priv");
    let (ok, msgs) = compile(&priv_out, None, std::slice::from_ref(&src));
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
    let (ok, msgs) = compile(&jar_out, Some(&jar), &[src]);
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

// ------------------------------------------------------------------ fixtures

/// The class-header cases: a parent constructor argument that needs a member
/// declared further down, and constructor defaults written in the class's own
/// type parameters. No library type is involved, so it runs in both modes.
#[test]
fn mism10_ctor_runs_in_both_modes() {
    dual_run("mism10_ctor");
}

/// `collect` on a sorted map: real `scala.collection` classes, so
/// library-ABI only.
#[test]
fn mism10_coll_runs_against_the_jar() {
    let name = "mism10_coll";
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

/// The private runtime has no `TreeMap` / `TreeSet`; it must say so rather
/// than compile something it cannot back.
#[test]
fn mism10_coll_without_library_is_error() {
    let src = fixtures_dir().join("mism10_coll.scala");
    let out = tmp_dir("mism10_coll_nolib");
    let (ok, msgs) = compile(&out, None, &[src]);
    assert!(
        !ok,
        "mism10_coll should not compile without the jar:\n{msgs}"
    );
    assert!(
        msgs.contains("error:"),
        "expected diagnostics, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Dropping the signature pass's diagnostics for a parent's arguments, and
/// binding the class's type parameters for a default's body, must not let an
/// ill-typed one through.
#[test]
fn mism10_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip mism10_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("mism10_bad.scala");
    let out = tmp_dir("mism10_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "mism10_bad should not compile, got:\n{msgs}");
    assert!(
        msgs.contains("WrongParentArg") || msgs.contains("constructor Base"),
        "expected the parent-argument error, got:\n{msgs}"
    );
    assert!(
        msgs.contains("Chain[String]"),
        "expected the default-argument error, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- unit-ish cases

/// A `TreeMap`'s `collect` really is a `TreeMap`: `K2` comes from the case
/// bodies, so the narrowed static type is not a lie.
#[test]
fn mism10_sorted_map_collect_infers_its_key() {
    accepts(
        "mism10_tmcollect",
        "import scala.collection.immutable.TreeMap\n\
         object Main { def main(a: Array[String]): Unit = {\n\
         \x20 val m: TreeMap[Int, String] = TreeMap(2 -> \"bb\", 1 -> \"a\")\n\
         \x20 println(m.collect { case (k, v) => (k * 10, v.length) })\n\
         \x20 val c: TreeMap[Int, Int] = m.collect { case (k, v) => (k * 10, v.length) }\n\
         \x20 val d: TreeMap[String, Int] = m.collect { case (k, v) if k > 1 => (v, k) }\n\
         \x20 println(c); println(d) } }\n",
    );
}

/// The receiver's own overload wins even when an ancestor's copy of the name
/// was installed first: one file, in this order, so the earlier `Map.collect`
/// is what `TreeMap.collect` used to resolve to.
#[test]
fn mism10_sorted_map_collect_after_a_plain_map() {
    accepts(
        "mism10_order",
        "import scala.collection.immutable.TreeMap\n\
         object Main { def main(a: Array[String]): Unit = {\n\
         \x20 val plain = Map(1 -> \"a\")\n\
         \x20 println(plain.collect { case (k, v) => (k, v) })\n\
         \x20 val pf: PartialFunction[(Int, String), (Int, Int)] = { case (k, v) => (k, v.length) }\n\
         \x20 val m: TreeMap[Int, Int] = TreeMap(1 -> \"a\").collect(pf)\n\
         \x20 println(m) } }\n",
    );
}

/// A constructor default in the class's own type parameters, monomorphic and
/// higher-kinded.
#[test]
fn mism10_ctor_default_names_the_class_type_parameters() {
    accepts(
        "mism10_ctordef",
        "class Box[A](val one: List[A] = List.empty[A], val two: Int = 7)\n\
         trait Cell[F[_]] { def tag: String }\n\
         object Cell { def empty[F[_]]: Cell[F] = new Cell[F] { def tag = \"e\" } }\n\
         class HkBox[F[_]](val cell: Cell[F] = Cell.empty[F])\n\
         object Main { def main(a: Array[String]): Unit = {\n\
         \x20 println(new Box[String]().one)\n\
         \x20 println(new HkBox[List]().cell.tag) } }\n",
    );
}

/// An ordinary method's default is unaffected: its type parameters are its
/// own, and a `name$default$n` getter is what fills it in.
#[test]
fn mism10_method_default_still_works() {
    accepts(
        "mism10_methdef",
        "object M { def m[A](l: List[A] = List.empty[A], n: Int = 3): Int = l.length + n }\n\
         object Main { def main(a: Array[String]): Unit = {\n\
         \x20 println(M.m[Int]())\n\
         \x20 println(M.m(List(1, 2))) } }\n",
    );
}

/// A parent's constructor argument that names a member declared further down
/// the file.
#[test]
fn mism10_parent_argument_sees_a_later_member() {
    accepts(
        "mism10_parentarg",
        "class Base(val cols: List[(String, Int)])\n\
         case class Sub[T](c: Cel[T], n: Int) extends Base(List((c.label, n)))\n\
         trait Cel[T] { def label: String }\n\
         object Main { def main(a: Array[String]): Unit =\n\
         \x20 println(Sub(new Cel[Int] { def label = \"x\" }, 1).cols) }\n",
    );
}

/// The suppression is not a licence: a parent argument that is wrong is still
/// reported, by the pass that runs with every signature in hand.
#[test]
fn mism10_wrong_parent_argument_is_rejected() {
    rejects(
        "mism10_parentbad",
        "class Base(val n: Int)\n\
         class Sub extends Base(\"nope\")\n\
         object Main { def main(a: Array[String]): Unit = println(new Sub().n) }\n",
        "Base",
    );
}

/// So is a constructor default that does not conform once the class's own
/// type parameters are bound.
#[test]
fn mism10_wrong_ctor_default_is_rejected() {
    rejects(
        "mism10_defbad",
        "class Box[A](val one: List[A] = List(\"a\"))\n\
         object Main { def main(a: Array[String]): Unit = println(new Box[Int]().one) }\n",
        "type mismatch",
    );
}
