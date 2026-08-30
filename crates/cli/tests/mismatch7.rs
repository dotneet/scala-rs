//! Seventh slice of the `type mismatch` family on slick.
//!
//! Eight independent causes: a captured parameter read as-seen-from an
//! anonymous class of the same generic class, a compound *type* rejected by a
//! rule that belongs to templates, eta-expansion that solved a polymorphic
//! method from the expected *result* instead of its parameters, an abstract
//! type's lower bound never used on the right of `<:`, 2.13's two
//! `indexWhere` overloads collapsed into one, a module -> `apply` redirect
//! that read a signature nothing had completed, an argument whose residual
//! implicit clause was filled only after it had already constrained the call,
//! and the lub of an invariant type argument.
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
        "scala-rs-mismatch7-{tag}-{}-{nanos}-{seq}",
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
fn mism7_fixture_runs_in_both_modes() {
    let name = "mism7";
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

/// What the relaxations must not swallow. Every one of these is an error real
/// scalac 2.13.16 gives for the same source.
#[test]
fn mism7_mixin_bad_is_still_rejected() {
    compile_fails(
        "mism7_mixin_bad",
        &["class B needs to be a trait to be mixed in"],
    );
}

#[test]
fn mism7_lobound_bad_is_still_rejected() {
    compile_fails("mism7_lobound_bad", &["type mismatch; found: O"]);
}

#[test]
fn mism7_capture_bad_is_still_rejected() {
    compile_fails(
        "mism7_capture_bad",
        &["type mismatch; found: B  required: T"],
    );
}

// -------------------------------------------------------------- unit-ish cases

/// A method's parameter is owned by the *method*, not by a class, so no prefix
/// is read into its type. Substituting as-seen-from the anonymous class turned
/// the `f: T => B` of `trait It[T] { def map[B](f: T => B) = new It[B] { … } }`
/// into a `B => B`, because the anonymous class's parent `It[B]` binds `T`.
#[test]
fn a_captured_parameter_is_not_seen_through_the_anonymous_class() {
    accepts(
        "mism7_capture",
        "trait It[T] { self =>\n\
         \x20 def next(): T\n\
         \x20 def map[B](f: T => B): It[B] = new It[B] { def next(): B = f(self.next()) }\n\
         }\n\
         trait Box[T]\n\
         trait Jt[T] { self =>\n\
         \x20 def m1[B](f: T => B): Box[B] = new Box[B] { val g: T => B = f }\n\
         \x20 def m2[B](f: T => B): Jt[B] = new Jt[B] { val g: T => B = f }\n\
         }\n",
    );
}

/// `A with B` is a type, not a template: scalac accepts the signature even
/// when nothing can inhabit it, and slick writes
/// `Query[B, BU, C] & TableQuery[B]` where one *is* a subclass of the other.
#[test]
fn a_compound_type_may_name_two_classes() {
    accepts(
        "mism7_compound",
        "class Ca\n\
         class Cb\n\
         class AT\n\
         sealed abstract class Query[+E, U, C[_]]\n\
         class TableQuery[E <: AT] extends Query[E, Int, Seq]\n\
         object M {\n\
         \x20 def a(x: Ca with Cb): Int = 1\n\
         \x20 def b[B <: AT, BU, C[_]](x: Query[B, BU, C] with TableQuery[B]): Int = 2\n\
         }\n",
    );
}

/// A function's parameters are contravariant and its result covariant, so
/// `A => A <: T => ?U` solves `A` from the parameter. Taking both at once made
/// `xs.map(identity)` a `List[Any]`, because a `map` still being inferred
/// expects `T => Any`.
#[test]
fn eta_expansion_solves_its_type_parameters_from_the_parameters() {
    accepts(
        "mism7_eta",
        "object M {\n\
         \x20 class CA[+T] { def map[U](f: T => U): CA[U] = new CA[U] }\n\
         \x20 def a[T](r: CA[T]): CA[T] = r.map(identity)\n\
         \x20 def b[T](xs: List[T]): List[T] = xs.map(identity)\n\
         \x20 def c(xs: List[Int]): List[Int] = xs.map(identity)\n\
         \x20 val d: Int => Int = identity\n\
         \x20 val e: String => String = identity _\n\
         \x20 def g[A](x: A): A = x\n\
         \x20 val h: String => String = g _\n\
         }\n",
    );
}

/// An abstract type on the right of `<:` is at least its lower bound. Without
/// it `def f[E, O >: E](x: E): O = x` was a mismatch, and so was every
/// `ShapedValue[_ <: E, U]` where a `ShapedValue[_ <: O, U]` is wanted.
#[test]
fn an_abstract_types_lower_bound_is_a_subtype_of_it() {
    accepts(
        "mism7_lobound",
        "object M {\n\
         \x20 class SV[T, U]\n\
         \x20 def a[E, O >: E](x: E): O = x\n\
         \x20 def b[E, O >: E, U](x: SV[E, U]): SV[? <: O, U] = x\n\
         \x20 def c[E, O >: E, U](x: SV[? <: E, U]): SV[? <: O, U] = x\n\
         }\n",
    );
}

/// 2.13's `SeqOps` declares both `indexWhere(p, from)` and `indexWhere(p)`.
/// At most one function-taking overload per name *and arity* may be supplied;
/// dropping the shorter one outright made `xs.indexWhere(p)` an arity error.
#[test]
fn both_arities_of_index_where_are_supplied() {
    accepts(
        "mism7_index_where",
        "object M {\n\
         \x20 val xs: Vector[String] = Vector(\"a\")\n\
         \x20 val a: Int = xs.indexWhere(_ == \"a\")\n\
         \x20 val b: Int = xs.indexWhere(_ == \"a\", 0)\n\
         \x20 val c: Int = xs.lastIndexWhere(_ == \"a\")\n\
         \x20 val d: Int = xs.lastIndexWhere(_ == \"a\", 0)\n\
         \x20 val e: Int = xs.segmentLength(_ == \"a\")\n\
         \x20 def f(ps: Vector[String], p: String): Vector[String] =\n\
         \x20   ps.patch(ps.lastIndexWhere(_ == p) + 1, Seq(p), 0)\n\
         }\n",
    );
}

/// `Module[T1, T2]` with no `.apply` written redirects to the module's
/// factory. The redirect reaches a symbol nothing selected, so nothing had run
/// its signature: named before its own definition, an `apply` with an inferred
/// result type came back `<notype>`.
#[test]
fn the_module_apply_redirect_completes_the_signature() {
    accepts(
        "mism7_forward_apply",
        "object M {\n\
         \x20 trait Rep[T]\n\
         \x20 object A {\n\
         \x20   val u: S1[Int, Int] = S1[Int, Int]\n\
         \x20   val v: S2[Rep[Int], Int] = S2[Rep[Int], Int]\n\
         \x20 }\n\
         \x20 trait S1[T, U]\n\
         \x20 object S1 { def apply[T, U] = (new Object).asInstanceOf[S1[T, U]] }\n\
         \x20 trait S2[T, U]\n\
         \x20 object S2 extends S2[Rep[Any], Any] {\n\
         \x20   def apply[T <: Rep[?], U] = this.asInstanceOf[S2[T, U]]\n\
         \x20 }\n\
         }\n",
    );
}

/// An argument that still carries an all-implicit clause is not a value yet.
/// It fixed this call's own parameters from the residual
/// `(A <:< (K, V))Map[K, V]` and only then found the witness, so what it had
/// to conform to stayed `Map[K, V]` while the argument had become
/// `Map[String, Int]`.
#[test]
fn an_implicit_only_argument_is_filled_before_it_constrains_the_call() {
    accepts(
        "mism7_implicit_arg",
        "object M {\n\
         \x20 def one[A2](a2: A2): Int = 0\n\
         \x20 def a(kvs: Seq[(String, Int)]): Int = one(kvs.toMap)\n\
         \x20 def b(kvs: Seq[(String, Int)]): (Int, Map[String, Int]) = (1, kvs.toMap)\n\
         \x20 def c[K, V](kvs: Seq[(K, V)]): (Int, Map[K, V]) = (1, kvs.toMap)\n\
         \x20 def d(kvs: Seq[(String, Int)]): Map[String, Int] = kvs.toMap\n\
         }\n",
    );
}

/// An *invariant* parameter admits neither argument in place of the other, so
/// joining them is not a type either side conforms to: nsc's lub of
/// `Inv[Boolean]` and `Inv[Int]` is an existential. And a varargs call is
/// never retried packed into a tuple -- `Seq(a, b)` had quietly become a
/// `Seq[(A, B)]`.
#[test]
fn the_lub_of_an_invariant_argument_is_an_existential() {
    accepts(
        "mism7_inv_lub",
        "import scala.reflect.ClassTag\n\
         object M {\n\
         \x20 class Inv[T] { def tag(implicit c: ClassTag[T]): ClassTag[T] = c }\n\
         \x20 val a = Seq(new Inv[Boolean], new Inv[Int])\n\
         \x20 val b: Seq[Inv[?]] = a\n\
         \x20 val c: Int = a.length\n\
         \x20 val d: List[Inv[?]] = List(new Inv[Boolean], new Inv[Int])\n\
         }\n",
    );
}
