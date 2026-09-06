//! Sixth slice of the `type mismatch` family on slick, and the three codegen
//! bugs recorded next to it: the frame that merges a `match`'s branches, the
//! sort a type-test sub-pattern's value is kept in, and a lambda literal whose
//! parameter types are written out.
//!
//! Also here: the collection hierarchy's `IndexedSeq` and mutable spine, a
//! `Map` used as the function it is, `WithFilter`'s type constructor,
//! `Success`/`Failure`'s type parameter, the type a `try` really has, and a
//! for-comprehension's trailing value definition.
//!
//! The fixture runs against the real `scala-library` jar *and* the private
//! runtime, and its output is compared with what nsc 2.13.16 prints for the
//! same source.

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
    let p = std::env::temp_dir().join(format!(
        "scala-rs-mismatch6-{tag}-{}-{nanos}",
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

/// `-Xverify:all`: every one of the codegen fixes here is a StackMapTable or a
/// local-slot question, and only the verifier notices those.
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

fn compile_fails(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "expected compile of {name} to fail, got:\n{msgs}");
    for needle in needles {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {msgs:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
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

// ------------------------------------------------------------------ fixtures

/// Both modes, both verified, both compared with real scalac's own stdout.
#[test]
fn mism6_fixture_runs_in_both_modes() {
    let name = "mism6";
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();

    let priv_out = tmp_dir("priv");
    let (ok, msgs) = compile(&priv_out, None, std::slice::from_ref(&src));
    assert!(ok, "compile {name} (private runtime) failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&priv_out, None),
            expected,
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
            expected,
            "stdout mismatch for {name} against the jar"
        );
    }
    let _ = fs::remove_dir_all(&jar_out);
}

/// The relaxations must not swallow what nsc still reports. Every one of these
/// is an error real scalac 2.13.16 gives for the same source.
#[test]
fn mism6_bad_is_still_rejected() {
    compile_fails(
        "mism6_bad",
        &[
            "type mismatch; found: (Int) => Int  required: (Int) => Unit",
            "type mismatch; found: Map[String, Int]  required: (Int) => Int",
            "type mismatch; found: ArrayBuffer[Int]  required: Vector[Int]",
            "type mismatch; found: Success[String]  required: Try[Int]",
        ],
    );
}

/// A value definition cannot precede the first generator. Reject it instead
/// of inventing a stream to which the definition could belong.
#[test]
fn a_value_definition_before_a_for_generator_is_diagnosed() {
    compile_fails(
        "mism6_forval_bad",
        &["value definition must follow a generator in a for-comprehension"],
    );
}

// -------------------------------------------------------------- unit-ish cases

/// nsc types a function *literal*'s body against the expected result type. A
/// literal whose parameter types are written out is typed before the expected
/// type is known -- overload resolution needs its result -- so its body never
/// saw one: `xs.foreach((x: Int) => x + 1)` reported
/// `found: (Int) => Int  required: (Int) => Unit`.
#[test]
fn an_annotated_lambdas_body_is_adapted_to_the_expected_result() {
    accepts(
        "mism6_annotated_lambda",
        "object M {\n\
         \x20 def fu(g: Int => Unit): Unit = g(1)\n\
         \x20 def fl(g: Int => Long): Long = g(1)\n\
         \x20 def a(): Unit = fu((x: Int) => x + 1)\n\
         \x20 def b(): Long = fl((x: Int) => x + 1)\n\
         \x20 def c(): Unit = List(1, 2).foreach((x: Int) => x + 1)\n\
         \x20 def d(): Unit = List(1, 2).foreach(x => x + 1)\n\
         }\n",
    );
}

/// 2.13's `MapOps` extends `PartialFunction[K, V]`, so a `Map` *is* the
/// function that looks a key up. The element type of the result is what it
/// returns, not what the receiver held.
#[test]
fn a_map_is_the_function_it_declares() {
    accepts(
        "mism6_map_as_function",
        "object M {\n\
         \x20 val ix: Map[String, Int] = Map(\"a\" -> 1)\n\
         \x20 val on: IndexedSeq[String] = IndexedSeq(\"a\")\n\
         \x20 val a: IndexedSeq[Int] = on.map(ix)\n\
         \x20 val b: Seq[Int] = Seq(\"a\").map(ix)\n\
         \x20 val c: String => Int = ix\n\
         \x20 def poly[B](f: String => B): B = f(\"a\")\n\
         \x20 val d: Int = poly(ix)\n\
         \x20 val e: Int = poly[Int]({ case \"a\" => 1 })\n\
         }\n",
    );
}

/// `class WithFilter[+A, +CC[_]]`: `map[B](f: A => B)` is a `CC[B]`. Holding
/// the filtered collection whole made every guarded comprehension keep the
/// element type it started with.
#[test]
fn with_filter_carries_a_type_constructor() {
    accepts(
        "mism6_with_filter",
        "object M {\n\
         \x20 val a: List[String] = List(1, 2).withFilter(_ > 0).map(_.toString)\n\
         \x20 val b: List[String] = for { m <- List(1, 2) if m > 0 } yield m.toString\n\
         \x20 val c: List[String] = List(1).withFilter(_ > 0).flatMap(m => List(m.toString))\n\
         \x20 val d: IndexedSeq[String] = for { i <- 1 to 3 if i > 1 } yield i.toString\n\
         \x20 val e: Option[String] = Some(1).withFilter(_ > 0).map(_.toString)\n\
         }\n",
    );
}

/// A trailing value definition is not a generator: the generator before it is
/// still the innermost one and takes `map`, not `flatMap`.
#[test]
fn a_for_comprehensions_value_definition_is_not_a_generator() {
    accepts(
        "mism6_for_value",
        "object M {\n\
         \x20 val a: List[String] = for { m <- List(1, 2); q = m.toString } yield q\n\
         \x20 val b: List[String] = for { m <- List(1, 2) if m > 0; q = m.toString } yield q\n\
         \x20 val c: List[String] =\n\
         \x20   for { m <- List(1); q = m.toString; n <- List(q) } yield n\n\
         \x20 def d(): Unit = for { m <- List(1); q = m.toString } println(q)\n\
         }\n",
    );
}

/// `Success[T]` / `Failure[T]` are what their `apply` returns: `T` sits in no
/// parameter of `Failure.apply`, so only the expected type can pin it -- and a
/// *raw* `Failure` could not be pinned at all. The `try` is the lub of its
/// body and its handlers, not the body alone.
#[test]
fn a_try_is_the_lub_of_its_body_and_its_handlers() {
    accepts(
        "mism6_try_lub",
        "import scala.util.{Try, Success, Failure}\n\
         object M {\n\
         \x20 def a[R](f: => R): Try[R] =\n\
         \x20   try Success(f) catch { case e: Exception => Failure(e) }\n\
         \x20 def b(): Try[Int] = Failure(new Exception(\"x\"))\n\
         \x20 def c(): Try[Int] = Success(1)\n\
         \x20 def d(): Option[Int] =\n\
         \x20   try Some(1) catch { case _: Exception => None }\n\
         \x20 def e(): Int = try 1 catch { case _: Exception => 2 }\n\
         \x20 def f(n: Int): Any = try n catch { case _: Exception => \"x\" }\n\
         \x20 def g(n: Int): Unit = try println(n) catch { case _: Exception => () }\n\
         }\n",
    );
}

/// The collection hierarchy had no `scala.collection.IndexedSeq` and no
/// mutable spine at all, so slick's `def and(ns: scala.collection.IndexedSeq
/// [Node])` rejected the `ArrayBuffer` it is given.
#[test]
fn the_mutable_collections_reach_indexed_seq() {
    accepts(
        "mism6_indexed_seq",
        "import scala.collection.mutable.{ArrayBuffer, ListBuffer}\n\
         object M {\n\
         \x20 val b = new ArrayBuffer[Int]\n\
         \x20 val a: scala.collection.IndexedSeq[Int] = b\n\
         \x20 val c: scala.collection.Seq[Int] = b\n\
         \x20 val d: Iterable[Int] = b\n\
         \x20 val e: scala.collection.mutable.Buffer[Int] = b\n\
         \x20 val f: scala.collection.Seq[Int] = new ListBuffer[Int]\n\
         \x20 val g: scala.collection.IndexedSeq[Int] = Vector(1)\n\
         }\n",
    );
}

/// `patch` is declared as returning `C` -- the receiver's own collection --
/// which the prelude cannot spell, so it came back as the inherited `Seq`.
///
/// In its own fixture: naming `scala.collection.Seq` anywhere in a unit loads
/// the real class from the jar, and the raw `Seq` these members are declared
/// to return can no longer be narrowed. That gap predates this slice and is in
/// the README's Remaining.
#[test]
fn patch_keeps_the_receivers_own_collection() {
    accepts(
        "mism6_patch",
        "object M {\n\
         \x20 val a: Vector[String] = Vector(\"a\").patch(0, Seq(\"b\"), 0)\n\
         \x20 def b(v: Vector[String], s: String): Vector[String] = v.patch(1, Seq(s), 0)\n\
         }\n",
    );
}

/// Codegen: a `match` or an `if` whose branches push different classes merges
/// to `java/lang/Object` unless the frame is told what the expression's static
/// type is -- and the `putfield` / `areturn` / argument that follows then
/// fails verification. Same for the local a `try` parks its result in.
#[test]
fn a_branchs_merge_type_is_the_expressions_own() {
    accepts(
        "mism6_join_type",
        "object M {\n\
         \x20 class H { var cur: Option[Int] = None }\n\
         \x20 def take(o: Option[Int]): String = o.toString\n\
         \x20 def a(h: H, n: Int): Unit = h.cur = n match { case 0 => None; case k => Some(k) }\n\
         \x20 def b(h: H, n: Int): Unit = h.cur = if (n == 0) None else Some(n)\n\
         \x20 def c(n: Int): Option[Int] = n match { case 0 => None; case k => Some(k) }\n\
         \x20 def d(n: Int): String = take(n match { case 0 => None; case k => Some(k) })\n\
         \x20 def e(n: Int): List[Int] = n match { case 0 => Nil; case k => k :: Nil }\n\
         \x20 // The merge is the *expression's* type at every depth; it used to\n\
         \x20 // be the enclosing method's return type, which is a different\n\
         \x20 // thing entirely for an inner branch.\n\
         \x20 def f(n: Int): Option[Any] =\n\
         \x20   if (n == 0) None else Some(n match { case 1 => \"one\"; case _ => n })\n\
         }\n",
    );
}

/// Codegen: a `_: T` sub-pattern **tests**, so it needs the erased reference
/// the `instanceof` reads. Unboxing before the test left an `int` in the local
/// the test then `aload`ed (`VerifyError: Bad local variable type`).
#[test]
fn a_type_test_sub_pattern_keeps_the_erased_reference() {
    accepts(
        "mism6_typed_subpattern",
        "object M {\n\
         \x20 def a(b: Boolean): Int = { val (n: Int, s: String) = if (b) (1, \"x\") else (0, \"y\"); n + s.length }\n\
         \x20 def b(o: Option[Any]): Int = o match { case Some(n: Int) => n; case _ => 0 }\n\
         \x20 def c(o: Option[Any]): String = o match { case Some(s: String) => s; case _ => \"\" }\n\
         \x20 def d(t: (Any, Any)): Int = t match { case (n: Int, _: String) => n; case _ => 0 }\n\
         \x20 def e(xs: List[Any]): Int = xs match { case (n: Int) :: _ => n; case _ => 0 }\n\
         }\n",
    );
}
