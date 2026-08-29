//! Regressions for the `type mismatch` family: a type that carries type
//! arguments losing them somewhere along the way, and `Any` being invented in
//! their place.
//!
//! The fixture runs against the real `scala-library` jar and its output is
//! compared with what nsc 2.13.16 prints for the same source.

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
        "scala-rs-mismatch-{tag}-{}-{nanos}",
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

fn run_main(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
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

/// Compile against the real jar and check the program's output against the
/// recorded nsc output.
fn dual_run(name: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed");
    if java_available() {
        let expected =
            fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
                .unwrap();
        assert_eq!(run_main(&out, &jar), expected, "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails(name: &str, needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Compile a snippet against the jar and return its diagnostics.
fn diagnostics_of(tag: &str, source: &str) -> String {
    let Some(jar) = scala_library_jar() else {
        return String::new();
    };
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            dir.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&dir);
    err
}

fn accepts(tag: &str, source: &str) {
    if scala_library_jar().is_none() {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    }
    let err = diagnostics_of(tag, source);
    assert!(!err.contains("error:"), "{tag} should compile, got:\n{err}");
}

// ------------------------------------------------------------------ fixtures

#[test]
fn mismatch_fixture_dual_run() {
    dual_run("mism");
}

#[test]
fn mismatch_invariant_argument_is_still_an_error() {
    compile_fails(
        "mism_bad",
        "type mismatch; found: Inv[Int]  required: Inv[Any]",
    );
}

// -------------------------------------------------------------- unit-ish cases

/// A callee's type parameter is solved from an argument whose type mentions
/// the *caller's* type parameter. Relaxing the caller's parameter to `Any`
/// made this `Inv[T]` vs `Inv[Any]`.
#[test]
fn callers_type_parameter_survives_inference() {
    accepts(
        "mism_caller_tparam",
        "class Inv[T]\n\
         object M {\n\
         \x20 def one[A](a: A): Int = 0\n\
         \x20 def two[A, B](a: A, b: B): Int = 0\n\
         \x20 def f[T](a: Inv[T]) = one(a)\n\
         \x20 def g[T](a: Inv[T], b: Inv[T]) = two(a, b)\n\
         \x20 def h[T](a: Inv[T], b: Inv[T]) = (a, b)\n\
         \x20 def i[T](a: Inv[T]) = identity(a)\n\
         }\n",
    );
}

/// `this` in a generic class is `C[A]`, not the raw `C`.
#[test]
fn this_carries_the_classes_type_arguments() {
    accepts(
        "mism_this",
        "trait Box[A] {\n\
         \x20 def self: Box[A] = this\n\
         }\n\
         class Sub[A] extends Box[A]\n",
    );
}

/// `super.m` reads the parent's members as seen from the subclass.
#[test]
fn super_is_seen_from_the_subclass() {
    accepts(
        "mism_super",
        "trait Act[R] {\n\
         \x20 def id: Act[R] = this\n\
         \x20 def wrap[R2](x: R2): Act[R2] = null\n\
         }\n\
         class Sub[A] extends Act[A] {\n\
         \x20 override def id: Act[A] = super.id\n\
         \x20 def other[B](y: B): Act[B] = super.wrap[B](y)\n\
         }\n",
    );
}

/// The collection hierarchy, with its type arguments threaded through.
#[test]
fn collections_conform_to_their_supertypes() {
    accepts(
        "mism_coll",
        "object M {\n\
         \x20 def a(x: List[Int]): Seq[Int] = x\n\
         \x20 def b(x: Vector[Int]): Seq[Int] = x\n\
         \x20 def c(x: Vector[Int]): IndexedSeq[Int] = x\n\
         \x20 def d(x: Seq[Int]): Iterable[Int] = x\n\
         \x20 def e(x: Vector[Int]): Iterable[Int] = x\n\
         \x20 def f(x: Set[Int]): Iterable[Int] = x\n\
         \x20 def g(x: Map[Int, Int]): Iterable[(Int, Int)] = x\n\
         \x20 def h(x: LazyList[Int]): Iterable[Int] = x\n\
         \x20 def i(x: Vector[Int]): collection.Seq[Int] = x\n\
         \x20 def j: Seq[String] = Nil\n\
         \x20 def k: Iterable[String] = Nil\n\
         }\n",
    );
}

/// Joining two instances of a class with a contravariant parameter goes the
/// other way on that parameter instead of giving up and landing on `AnyRef`.
#[test]
fn varargs_join_respects_contravariance() {
    accepts(
        "mism_contra",
        "trait Eff\n\
         trait NoStream\n\
         trait Act[+R, +S <: NoStream, -E <: Eff] {\n\
         \x20 def andThen[R2, S2 <: NoStream, E2 <: Eff](a: Act[R2, S2, E2]): Act[R2, S2, E with E2] =\n\
         \x20   AndThen[R2, S2, E with E2](Vector(this, a))\n\
         }\n\
         case class AndThen[R, +S <: NoStream, -E <: Eff](as: IndexedSeq[Act[Any, NoStream, E]])\n\
         \x20 extends Act[R, S, E]\n",
    );
}

/// An annotated type conforms exactly as its underlying type does; a module's
/// singleton type is the module.
#[test]
fn annotations_and_module_singletons() {
    accepts(
        "mism_annot",
        "import scala.annotation.unchecked.uncheckedVariance\n\
         class Node\n\
         object M {\n\
         \x20 def t1(x: Node @uncheckedVariance): Int = 0\n\
         \x20 def a1(x: Node) = t1(x)\n\
         \x20 def t2(f: Node => (Node @uncheckedVariance)): Int = 0\n\
         \x20 def a2(f: Node => Node) = t2(f)\n\
         \x20 def nil: Some[Nil.type] = Some(Nil)\n\
         }\n",
    );
}

/// A polymorphic method eta-expanded against an expected function type has its
/// own type parameters solved from that type first.
#[test]
fn eta_expansion_solves_the_methods_type_parameters() {
    accepts(
        "mism_eta",
        "class Node\n\
         object M {\n\
         \x20 def use(f: Node => Node): Int = 0\n\
         \x20 val f: Node => Node = identity\n\
         \x20 def g = use(identity)\n\
         }\n",
    );
}
