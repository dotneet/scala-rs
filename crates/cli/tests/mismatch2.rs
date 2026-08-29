//! Second slice of the `type mismatch` family on slick: a type argument that
//! was never solved, a declared result type that was overwritten, and a member
//! whose real signature never reached the symbol table.
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
        "scala-rs-mismatch2-{tag}-{}-{nanos}",
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
    let output = cmd
        .args(["-d", out.to_str().unwrap()])
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

/// Compile against the real jar and check the program's output against the
/// recorded nsc output.
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

fn compile_fails(name: &str, needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, &jar, &[src]);
    assert!(!ok, "expected compile of {name} to fail");
    assert!(
        msgs.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {msgs:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Compile the named snippets, in the order given, and require no error.
/// Order matters: a name declared in a later unit is the point of several of
/// these.
fn accepts_units(tag: &str, units: &[(&str, &str)]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let srcs: Vec<PathBuf> = units
        .iter()
        .map(|(name, source)| {
            let p = dir.join(format!("{name}.scala"));
            fs::write(&p, source).unwrap();
            p
        })
        .collect();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (_, msgs) = compile(&out, &jar, &srcs);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn accepts(tag: &str, source: &str) {
    accepts_units(tag, &[(tag, source)]);
}

// ------------------------------------------------------------------ fixtures

#[test]
fn mismatch2_fixture_dual_run() {
    dual_run("mism2");
}

/// The collection shortcut for `map` used to rewrite *any* receiver's result
/// to `Coll[<element>]`, which both dropped two type arguments and hid a real
/// mismatch. The declared result type has to be checked.
#[test]
fn map_does_not_invent_its_result_type() {
    compile_fails(
        "mism2_bad",
        "type mismatch; found: Act[Int, NoStream, Effect]  required: Act[String, NoStream, Effect]",
    );
}

// -------------------------------------------------------------- unit-ish cases

/// A default argument is a *body*, not a signature. Typing it while signatures
/// were still being built let it see only the units that come earlier on the
/// command line, so `CU.dcl` was `<notype>` (slick's
/// `classLoader: ClassLoader = ClassLoaderUtil.defaultClassLoader`).
#[test]
fn a_default_argument_may_name_a_later_units_member() {
    accepts_units(
        "mism2_default_forward",
        &[
            (
                "b",
                "package q.b\n\
                 import q.u.CU\n\
                 trait T {\n\
                 \x20 def f(cl: String = CU.dcl): Int = 1\n\
                 }\n",
            ),
            (
                "a",
                "package q.u\n\
                 object CU {\n\
                 \x20 val dcl: String = \"x\"\n\
                 }\n",
            ),
        ],
    );
}

/// `def map[R2](f: R => R2): Act[R2, NoStream, E]` is not a collection's
/// `map`: rewriting the result to `Act[R2]` dropped two arguments.
#[test]
fn map_keeps_a_result_type_with_more_than_one_argument() {
    accepts(
        "mism2_map_three_args",
        "trait NoStream\n\
         trait Effect\n\
         trait Act[R, S, E] {\n\
         \x20 def map[R2](f: R => R2): Act[R2, NoStream, E] = sys.error(\"\")\n\
         }\n\
         object M {\n\
         \x20 def g[R, E](a: Act[R, NoStream, E]): Act[Int, NoStream, E] = a.map(_ => 1)\n\
         }\n",
    );
}

/// A type parameter that only shows up in a lambda's *result* was re-inferred
/// once the arguments were typed -- but only when the method's result type was
/// the bare parameter. `Act[R2, NoStream, E]` is not, so `R2` stayed open
/// wherever the expected type could not pin it (a covariant occurrence is only
/// an upper bound).
#[test]
fn a_lambdas_result_solves_a_nested_type_parameter() {
    accepts(
        "mism2_lambda_result",
        "trait NoStream\n\
         trait Q[+R, S, E] {\n\
         \x20 def map[R2](f: R => R2): Q[R2, NoStream, E] = sys.error(\"\")\n\
         }\n\
         object M {\n\
         \x20 def a[R, E](x: Q[Seq[R], NoStream, E]): Q[Option[R], NoStream, E] =\n\
         \x20   x.map(_.headOption)\n\
         \x20 def b[R, E](x: Q[Seq[R], NoStream, E]): Q[Int, NoStream, E] = x.map(_.size)\n\
         }\n",
    );
}

/// `RepShape[L, M, U]` means `RepShape.apply[L, M, U]`, and that `apply` is
/// parameterless: the reference is the value. Keeping the nullary method type
/// made the result conform to nothing -- and print identically to what was
/// required (slick's `Shape.scala`).
#[test]
fn a_parameterless_module_apply_is_a_value() {
    accepts(
        "mism2_module_apply",
        "trait Shape[L, M, U, P]\n\
         class Rep[T]\n\
         object RepShape {\n\
         \x20 def apply[L, M, U]: Shape[L, M, U, M] = sys.error(\"\")\n\
         }\n\
         object M {\n\
         \x20 def s[L, T]: Shape[L, Rep[T], T, Rep[T]] = RepShape[L, Rep[T], T]\n\
         }\n",
    );
}

/// `Coll.empty` is `def empty[A]: Coll[A]`; the prelude declared most of them
/// as a monomorphic `Coll[Any]`. And a parameterless polymorphic method used
/// in value position has nothing but the expected type to solve from.
#[test]
fn empty_takes_its_type_arguments_from_the_expected_type() {
    accepts(
        "mism2_empty",
        "import scala.collection.mutable\n\
         object M {\n\
         \x20 val a: Vector[Int] = Vector.empty\n\
         \x20 val b: Seq[String] = Seq.empty\n\
         \x20 val c: Iterable[(String, Int)] = Vector.empty\n\
         \x20 val d: mutable.HashMap[String, Int] = mutable.HashMap.empty\n\
         \x20 val e: mutable.ArrayBuffer[Int] = mutable.ArrayBuffer.empty\n\
         }\n",
    );
}

/// The same, one level down: an argument is typed with no expected type, so
/// the reference still carries its own open parameters when the constructor
/// parameter it fills is finally known.
#[test]
fn a_constructor_argument_is_solved_against_the_parameter_it_fills() {
    accepts(
        "mism2_ctor_arg",
        "class Box(val m: Map[String, Int], val v: Vector[Int])\n\
         object M {\n\
         \x20 val a = new Box(Map.empty, Vector.empty)\n\
         }\n",
    );
}

/// A local `def` is in scope for the whole block: two of them may call each
/// other whichever way round they are written. Only its signature is built up
/// front, so the body still sees the block in source order.
#[test]
fn a_local_def_may_be_called_before_it_is_written() {
    accepts(
        "mism2_local_def",
        "object M {\n\
         \x20 def outer(n: Int): Int = {\n\
         \x20   def first(x: Int): (String, Int) = second(x)\n\
         \x20   def second(x: Int): (String, Int) = (\"ab\", x)\n\
         \x20   val (s, i) = first(n)\n\
         \x20   s.length + i\n\
         \x20 }\n\
         }\n",
    );
}

/// A function literal argument is only a placeholder while the other arguments
/// are typed; taking it for a solution hid the expected type from the very
/// parameter it fills.
#[test]
fn a_function_literal_gets_its_parameter_type_from_the_expected_type() {
    accepts(
        "mism2_lambda_arg",
        "object M {\n\
         \x20 def mk[A, B](x: A, y: B): (A, B) = (x, y)\n\
         \x20 val a: (Int, Int => Int) = (1, n => n + 1)\n\
         \x20 val b: (Int, Int => Int) = mk(1, n => n + 1)\n\
         \x20 val c: List[Int => Int] = List(n => n * 2)\n\
         }\n",
    );
}

/// `scala.reflect.classTag[T](implicit ct: ClassTag[T])` reached the symbol
/// table through the package object's *classfile*, and a JVM descriptor cannot
/// say that a clause is implicit.
#[test]
fn a_package_objects_implicit_clause_survives() {
    accepts(
        "mism2_class_tag",
        "import scala.reflect.{classTag, ClassTag}\n\
         object M {\n\
         \x20 val a: ClassTag[Short] = classTag[Short]\n\
         \x20 def b(i: Int): ClassTag[_] = i match {\n\
         \x20   case 1 => classTag[Short]\n\
         \x20   case _ => classTag[String]\n\
         \x20 }\n\
         }\n",
    );
}

/// Eta-expansion is for methods that take parameters. A parameterless `def f:
/// T` is applied even where a function type is expected, so `val g: Int => Int
/// = xs.head` is the head, not `() => (Int => Int)`.
#[test]
fn a_parameterless_method_is_applied_where_a_function_is_expected() {
    accepts(
        "mism2_nullary_function",
        "object M {\n\
         \x20 val fs: List[Int => Int] = List((n: Int) => n * 2)\n\
         \x20 val g: Int => Int = fs.head\n\
         \x20 def h: Int => Int = fs.head\n\
         }\n",
    );
}
