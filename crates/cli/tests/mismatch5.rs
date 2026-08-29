//! Fifth slice of the `type mismatch` family on slick: a trait that extends a
//! function type as a SAM, a type parameter solved to the caller's own, the
//! type arguments an `extends` clause and a `new` leave out, a method whose
//! only clause is implicit, `.apply` through an annotated type, and the
//! collection a same-element-type transformation really returns.
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
        "scala-rs-mismatch5-{tag}-{}-{nanos}",
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

/// Compile the sources against the real jar and return the diagnostics.
fn compile(out: &Path, jar: &Path, srcs: &[PathBuf]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    let output = cmd
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

fn dual_run(name: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, &jar, &[src]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    if java_available() {
        let expected =
            fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
                .unwrap();
        assert_eq!(run_main(&out, &jar), expected, "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, &jar, &[src]);
    assert!(!ok, "expected compile of {name} to fail");
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
    let (_, msgs) = compile(&out, &jar, &[src]);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ fixtures

#[test]
fn mismatch5_fixture_dual_run() {
    dual_run("mism5");
}

/// The relaxations must not swallow the errors nsc still reports.
#[test]
fn mism5_bad_is_still_rejected() {
    compile_fails(
        "mism5_bad",
        &[
            "required: TwoAbs6",
            "no matching overload for constructor Base6",
            "type mismatch; found: UnitRC6[String]  required: RC6[String, Int]",
            "type mismatch; found: Seq[Int]  required: Vector[Int]",
            "type mismatch; found: Set[Int]  required: Set[String]",
            "value apply is not a member of NoApply6 @unchecked",
        ],
    );
}

// -------------------------------------------------------------- unit-ish cases

/// slick writes `trait CanBeQueryCondition[-T] extends (T => Rep[?])` and then
/// assigns a function literal to it. The single abstract method is
/// `Function1.apply`, inherited through a parent written *structurally*: the
/// SAM search, member lookup, the as-seen-from walk, the JVM interface list
/// and the linearization all have to read that parent back as the class. And
/// `apply`'s parameter is the receiver's own `T`, which needs `FunctionN` to
/// carry type parameters at all -- the prelude declared it as `(Any)Any`.
#[test]
fn a_trait_that_extends_a_function_type_is_a_sam() {
    accepts(
        "mism5_sam_function_parent",
        "trait Rep[T] { def value: T }\n\
         class Lit[T](val value: T) extends Rep[T]\n\
         trait CBQ[-T] extends (T => Rep[?])\n\
         trait Mono extends (Int => String)\n\
         object O {\n\
         \x20 implicit val b1: CBQ[Rep[Boolean]] = v => v\n\
         \x20 val m1: Mono = i => i.toString\n\
         \x20 val f1: Function1[Rep[Boolean], Rep[?]] = b1\n\
         \x20 val s: String = m1(3)\n\
         \x20 val t: Any = b1(new Lit(true)).value\n\
         }\n",
    );
}

/// The second inference pass -- the one that re-solves a parameter that shows
/// up only in a lambda's *result* -- threw away every solution that was a type
/// parameter. `mk`'s `T` here is `c1`'s `T`, which is a perfectly good
/// solution; rejecting it printed `GR[T] required GR[T]`.
#[test]
fn a_callees_parameter_may_be_solved_to_the_callers() {
    accepts(
        "mism5_caller_tparam",
        "trait PR\n\
         trait GR[+T]\n\
         object GR {\n\
         \x20 def mk1[T](f: PR => T): GR[T] = new GR[T] {}\n\
         \x20 def mk2[T](implicit f: PR => T): GR[T] = new GR[T] {}\n\
         \x20 def c1[T](value: T): GR[T] = mk1(_ => value)\n\
         \x20 def c2[T](value: T): GR[T] = mk2(_ => value)\n\
         \x20 def c3[T](value: T): GR[T] = mk1(x => value)\n\
         }\n",
    );
}

/// `class DerbySequenceDDLBuilder[T](seq: Sequence[T]) extends
/// OverrideActualStart(seq)` writes no type arguments for the parent, so nsc
/// infers them from the constructor arguments. Both sides of the check printed
/// `Sequence[T]` before, and neither was the other.
#[test]
fn a_parent_gets_its_type_arguments_from_the_ctor_args() {
    accepts(
        "mism5_parent_targs",
        "class Seqn[T](val v: T)\n\
         class Base[T](val s: Seqn[T]) { def g: T = s.v }\n\
         class Derived[T](s2: Seqn[T]) extends Base(s2)\n\
         object M { val n: Int = new Derived[Int](new Seqn(3)).g }\n",
    );
}

/// `new UnitRC` with no type arguments and an expected type that names a
/// *base* class: `UnitRC[R] <: RC[R, Unit]` reads `R` off it. `TmRC` gets two
/// of its three parameters that way and the third from the argument, so both
/// sources have to be merged. The head of `new C(args)` is no longer adapted
/// to the whole application's expected type either.
#[test]
fn a_new_gets_its_type_arguments_from_a_base_expected_type() {
    accepts(
        "mism5_new_targs",
        "trait RC[R, U]\n\
         class UnitRC[R] extends RC[R, Unit]\n\
         class ProdRC[R, U](xs: RC[R, U]*) extends RC[R, U]\n\
         class TmRC[R, U, V](child: RC[R, U], f: U => V) extends RC[R, V]\n\
         object M {\n\
         \x20 def a[R]: RC[R, Unit] = new UnitRC\n\
         \x20 def b[R, U](c: RC[R, U]): RC[R, U] = new ProdRC(c)\n\
         \x20 def c[R, U](x: RC[R, U]): RC[R, String] = new TmRC(x, (u: U) => u.toString)\n\
         }\n",
    );
}

/// A parameter type that names a class and an argument that is a *subclass* of
/// it: `unify_one` zips type arguments positionally and has no symbol table to
/// ask, so the argument has to be lined up with the parameter's class first.
#[test]
fn an_argument_is_lined_up_with_the_parameters_class() {
    accepts(
        "mism5_align_subclass",
        "trait RC[R, U]\n\
         class UnitRC[R] extends RC[R, Unit]\n\
         object M {\n\
         \x20 def id[R, U](c: RC[R, U]): RC[R, U] = c\n\
         \x20 val x: RC[String, Unit] = id(new UnitRC[String])\n\
         }\n",
    );
}

/// `TreeMap.empty` is `[K: Ordering, V]: TreeMap[K, V]`: `V` sits in no
/// implicit parameter, so the search alone cannot pin the parameters -- but
/// the expected type can, and nsc runs `inferExprInstance` before the search.
/// The whole method type used to stand as the value's own type.
#[test]
fn an_implicit_only_clause_is_filled_from_the_expected_type() {
    accepts(
        "mism5_implicit_only",
        "import scala.collection.immutable.TreeMap\n\
         object M {\n\
         \x20 val empty: TreeMap[Long, String] = TreeMap.empty\n\
         \x20 def take(m: TreeMap[Long, String]): Int = m.size\n\
         \x20 val n: Int = take(TreeMap.empty)\n\
         }\n",
    );
}

/// slick binds `val (b, m: Map[…] @unchecked) = …` and then calls `m(f)`. An
/// annotation says nothing about a type's members, so the `.apply` insertion
/// has to look through it -- and a `Select` that resolved to a *value* is a
/// receiver like any other.
#[test]
fn apply_is_inserted_through_an_annotated_type() {
    accepts(
        "mism5_annotated_apply",
        "object M {\n\
         \x20 val m: Map[String, Int] @unchecked = Map(\"a\" -> 1)\n\
         \x20 val n: Int = m(\"a\")\n\
         }\n",
    );
}

/// 2.13 declares `filter` / `++` / `take` / `reverse` / … as returning `C` --
/// the receiver's own collection. The prelude cannot spell `C`, so
/// `Vector.filterNot(p)` came back as the inherited `Seq`. Only members whose
/// erased descriptor returns `Object` are narrowed: `TreeMap.filter` returns
/// `Map` on the JVM, and narrowing it would leave codegen storing a `Map`
/// where a `TreeMap` is wanted.
#[test]
fn a_transformation_keeps_the_receivers_own_collection() {
    accepts(
        "mism5_receiver_collection",
        "object M {\n\
         \x20 val v: Vector[Int] = Vector(1, 2, 3)\n\
         \x20 val a: Vector[Int] = v.filterNot(_ == 2)\n\
         \x20 val b: Vector[Int] = v ++ Seq(4)\n\
         \x20 val c: Vector[Int] = v.take(2)\n\
         \x20 val d: Vector[Int] = v.reverse\n\
         \x20 val e: Vector[Int] = v :+ 5\n\
         \x20 val f: Vector[Int] = v.updated(0, 9)\n\
         \x20 val g: Vector[Int] = v.sortWith(_ > _)\n\
         \x20 val h: Set[Int] = Set(1, 2).filter(_ > 1)\n\
         \x20 val i: Seq[Int] = v.toSeq\n\
         }\n",
    );
}

/// `Set` and `Map` are invariant, so `Set(s): Set[Sym]` is not a subtype
/// question: the factory shortcut -- which computes the element type from the
/// arguments alone -- has to ask the expected type too.
#[test]
fn a_factorys_element_type_is_widened_by_the_expected_type() {
    accepts(
        "mism5_factory_widen",
        "trait Sym\n\
         class AnonSym extends Sym\n\
         object M {\n\
         \x20 def s(x: AnonSym): Set[Sym] = Set(x)\n\
         \x20 def m(x: AnonSym): Map[Sym, Int] = Map(x -> 1)\n\
         }\n",
    );
}
