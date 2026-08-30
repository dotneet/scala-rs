//! Eighth slice of the `type mismatch` family on slick.
//!
//! Seven independent causes: an expected type that is a type alias never
//! dealiased before a call's type parameters were solved, a repeated parameter
//! the call left empty that stayed *unsolved* instead of unconstrained, a
//! `xs: _*` argument unwrapped on one side only, the `-Xsource:3` splat
//! `f(xs*)`, nsc's `protoTypeArgs` (the expected type is the prototype an
//! argument deserves), a dependent method type read off the parameter's class
//! instead of the argument's, and `private[p]` resolved by a global lookup
//! rather than from the definition.
//!
//! The fixture runs against the real `scala-library` jar *and* the private
//! runtime, under `-Xverify:all`, and its output is compared with what nsc
//! 2.13.16 prints for the same source.

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
        "scala-rs-mismatch8-{tag}-{}-{nanos}-{seq}",
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

fn compile_fails(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, Some(&jar), &[src], &[]);
    assert!(!ok, "expected compile of {name} to fail, got:\n{msgs}");
    for needle in needles {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {msgs:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
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

// ------------------------------------------------------------------ fixtures

/// Both modes, both verified, both compared with real scalac's own stdout.
#[test]
fn mism8_fixture_runs_in_both_modes() {
    let name = "mism8";
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();

    let priv_out = tmp_dir("priv");
    let (ok, msgs) = compile(&priv_out, None, std::slice::from_ref(&src), &[]);
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
    let (ok, msgs) = compile(&jar_out, Some(&jar), &[src], &[]);
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

/// What the relaxations must not swallow. Every one of these is an error real
/// scalac 2.13.16 gives for the same source.
#[test]
fn mism8_access_bad_is_still_rejected() {
    compile_fails(
        "mism8_access_bad",
        &["value slot cannot be accessed", "value y"],
    );
}

#[test]
fn mism8_dep_bad_is_still_rejected() {
    compile_fails(
        "mism8_dep_bad",
        &["type mismatch; found: Option[Int]  required: Option[String]"],
    );
}

/// `f(xs*)` is `-Xsource:3` only; at 2.13 it is still the postfix operator.
#[test]
fn mism8_star_needs_xsource3() {
    compile_fails("mism8_star_bad", &["value * is not a member of List[Int]"]);
}

// -------------------------------------------------------------- unit-ish cases

/// An alias declared in an object is a `TypeMember` with a right-hand side.
/// The walk that reads a call's type parameters out of the expected type
/// never dealiased it, so `Map.empty` checked against `Type.Scope` solved
/// nothing and came back a `Map[Nothing, Nothing]`.
#[test]
fn an_expected_type_alias_is_seen_through_before_it_solves() {
    accepts(
        "mism8_alias_pt",
        "object T { type Scope = Map[String, Int]; type L = List[Int] }\n\
         object M {\n\
         \x20 val a: T.Scope = Map.empty\n\
         \x20 val b: T.L = List.empty\n\
         \x20 val c: T.L = List()\n\
         \x20 val d: T.Scope = Map()\n\
         \x20 def e(s: T.Scope): Map[String, Int] = s\n\
         }\n",
    );
}

/// A repeated parameter the call left empty has nothing to solve its element
/// from: it is *unconstrained*, and nsc minimises it to `Nothing`. Keeping it
/// in the signature's parameters made `List()`, `Seq()` and `Map()` all keep
/// the callee's own type parameter.
#[test]
fn an_empty_repeated_parameter_is_unconstrained_not_unsolved() {
    accepts(
        "mism8_empty_varargs",
        "object M {\n\
         \x20 def mk[A](xs: A*): List[A] = xs.toList\n\
         \x20 val a: List[Int] = List()\n\
         \x20 val b: Seq[Int] = Seq()\n\
         \x20 val c: Map[String, Int] = Map()\n\
         \x20 val d: List[Int] = mk()\n\
         \x20 val e: Seq[Seq[Int]] = Seq()\n\
         }\n",
    );
}

/// `param_at` unwraps a repeated *parameter* to its element; a `xs: _*`
/// argument is the matching `Repeated` of its own element. Unwrapping one
/// side only solved `def mk[A](xs: A*)` to `A = Int*`, so `mk(xs: _*)` was a
/// `List[Int*]`.
#[test]
fn a_splatted_argument_is_the_element_type() {
    accepts(
        "mism8_splat",
        "object M {\n\
         \x20 def mk[A](xs: A*): List[A] = xs.toList\n\
         \x20 def a(xs: Seq[Int]): List[Int] = mk(xs: _*)\n\
         \x20 def b(xs: Seq[Int]): Seq[Int] = Seq(xs: _*)\n\
         \x20 def c(xs: Seq[Int]): Vector[Int] = Vector(xs: _*)\n\
         \x20 def d(xs: Seq[Int]): Array[Int] = Array(xs: _*)\n\
         \x20 def e(kvs: Seq[(String, Int)]): Map[String, Int] = Map(kvs: _*)\n\
         \x20 def f(xs: Seq[Int]): Set[Int] = Set(xs: _*)\n\
         }\n",
    );
}

/// The `-Xsource:3` spelling of the same splat. slick writes
/// `Map(elems *)` in three places.
#[test]
fn xsource3_spells_the_splat_without_the_ascription() {
    accepts_with(
        "mism8_star",
        "object M {\n\
         \x20 def Scope(elems: (String, Int)*): Map[String, Int] = Map(elems*)\n\
         \x20 def two(xs: Int*): Seq[Int] = Seq(xs*)\n\
         \x20 def three(xs: Int*): Int = List(1, xs*).length\n\
         }\n",
        &["-Xsource:3"],
    );
}

/// nsc's `protoTypeArgs`: the expected type already says what a callee's type
/// parameters are before an argument is typed, and that is the prototype the
/// argument deserves. Without it the invariant `Map` in
/// `(Select(…), Map(s -> a2)): (Node, Map[TermSymbol, Aggregate])` came back
/// keyed by the argument's own `AnonSymbol`.
#[test]
fn the_expected_type_is_the_prototype_of_a_tuple_component() {
    accepts(
        "mism8_proto",
        "trait TermSymbol\n\
         class AnonSymbol extends TermSymbol\n\
         class Agg\n\
         class Node\n\
         class Sel extends Node\n\
         object M {\n\
         \x20 def p2[A, B](a: A, b: B): (A, B) = (a, b)\n\
         \x20 def q(s: AnonSymbol, a2: Agg): (Node, Map[TermSymbol, Agg]) =\n\
         \x20   p2(new Sel, Map(s -> a2))\n\
         \x20 def r(s: AnonSymbol, a2: Agg): (Node, Map[TermSymbol, Agg]) =\n\
         \x20   (new Sel, Map(s -> a2))\n\
         }\n",
    );
}

/// A prototype is a hint and nothing else: an argument the expected type does
/// not fit -- one whose implicit clause is still open -- is typed again as if
/// there had been none.
#[test]
fn a_prototype_that_does_not_fit_is_dropped() {
    accepts(
        "mism8_proto_retry",
        "object M {\n\
         \x20 def p2[A, B](a: A, b: B): (A, B) = (a, b)\n\
         \x20 def a(kvs: Seq[(String, Int)]): (Int, Map[String, Int]) = p2(1, kvs.toMap)\n\
         \x20 def b(xs: Seq[Int]): (Int, List[Int]) = p2(1, xs.toList)\n\
         }\n",
    );
}

/// nsc's `dependentTypeMap`. `def get[P <: Phase](p: P): Option[p.State]`
/// reads `State` off the *argument*. The type carries no prefix, so the
/// parameter that could have been one is found by its bound.
#[test]
fn a_dependent_method_type_reads_its_member_off_the_argument() {
    accepts(
        "mism8_dependent",
        "case class UsedFeatures(aggregate: Boolean, distinct: Boolean)\n\
         trait Phase { type State; val name: String }\n\
         class AUS extends Phase { val name = \"aus\"; type State = UsedFeatures }\n\
         object Phase { val aus = new AUS }\n\
         class CState {\n\
         \x20 def get[P <: Phase](p: P): Option[p.State] = None\n\
         }\n\
         object M {\n\
         \x20 def f(s: CState): Boolean =\n\
         \x20   s.get(Phase.aus).map(_.aggregate).getOrElse(true)\n\
         \x20 def g(s: CState): Option[UsedFeatures] = s.get(Phase.aus)\n\
         }\n",
    );
}

/// `private[p]` names an enclosing class or package **of the definition**.
/// Resolving the name in the *use site's* scope found `scala.util` for
/// slick's `private[util]`, and every `ConstArray.copySliceTo` was
/// inaccessible. And a constructor parameter that never became a `val` is not
/// a member of any prefix but `this`: `o.name` means the inherited one.
#[test]
fn a_qualified_private_is_resolved_from_the_definition() {
    accepts(
        "mism8_access",
        "package util {\n\
         \x20 class CA[T](a: Array[Any]) {\n\
         \x20   def length: Int = a.length\n\
         \x20   private[util] def copySliceTo(d: Array[Any], s: Int, p: Int, l: Int): Unit =\n\
         \x20     System.arraycopy(a, s, d, p, l)\n\
         \x20   def cat[U >: T](u: CA[U]): CA[U] = {\n\
         \x20     val ar = new Array[Any](length + u.length)\n\
         \x20     u.copySliceTo(ar, 0, length, u.length)\n\
         \x20     new CA[U](ar)\n\
         \x20   }\n\
         \x20 }\n\
         \x20 class Sym(val name: String)\n\
         \x20 class Fun(name: String) extends Sym(name) {\n\
         \x20   override def equals(o: Any): Boolean = o match {\n\
         \x20     case o: Fun => name == o.name\n\
         \x20     case _      => false\n\
         \x20   }\n\
         \x20   override def hashCode: Int = name.hashCode\n\
         \x20 }\n\
         }\n",
    );
}
