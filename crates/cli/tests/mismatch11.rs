//! Three causes behind slick's remaining `type mismatch`es. Two of them were
//! also silent miscompiles -- code that type-checked and then destructured the
//! wrong thing at run time.
//!
//!  * A pickled member found through the linearization is expressed in the
//!    queried class's type parameters by substituting each hop's arguments,
//!    and that substitution captured. `Iterator.GroupedIterator[B] extends
//!    AbstractIterator[Seq[B]]` makes one hop `A := Seq[B]`, and the
//!    `Iterator.map[B](f: A => B)` it lands on binds a `B` of its own -- so the
//!    class's `B` fell under the method's binder and `map` took a `B` where it
//!    takes a `Seq[B]`.
//!  * The rule that a collection's element type is its receiver's first type
//!    argument was overruling a parameter type the signature states outright,
//!    and replacing a two-parameter function with a one-parameter one
//!    (`LazyZip2.map(f: (El1, El2) => B)`).
//!  * `find_or_stub_java_class` enters a bare placeholder for every name a
//!    class file's parent list mentions, and those were refused their pickled
//!    type parameters merely for being in `scala.`. `ReusableBuilder` came in
//!    that way from `ArrayBuilder`'s parent list, so
//!    `ReusableBuilder[T, Array[T]]` was "applied to 2 arguments but the symbol
//!    has 0" and `ArrayBuilder` never became a `Builder[E, Array[E]]`. The
//!    class file's own generic signature can only write
//!    `ReusableBuilder<T, Object>`, and `To` is invariant, so the pickled
//!    parent refines the erased one.
//!  * An undetermined *type constructor* reached an argument's expected type
//!    as its bound. `Any` is not an inhabitant of a constructor's kind, so
//!    slick's `flatMap[F, T, D[_]](f: E => Query[F, T, D])` typed its lambda
//!    against `Query[F, T, Any]`.
//!
//! The fixtures are dual-run where the private runtime can back them:
//! compiled against the real `scala-library` jar and on the private runtime,
//! under `-Xverify:all`, with their stdout compared against nsc 2.13.16's.

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
        "scala-rs-mism11-{tag}-{}-{nanos}-{seq}",
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

/// Runs an emitted `Main` in both modes and compares its stdout.
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

/// The undetermined type constructor. No library type is involved, so it runs
/// in both modes.
#[test]
fn mism11_hkopen_runs_in_both_modes() {
    dual_run("mism11_hkopen");
}

/// `grouped` and `ArrayBuilder`: real `scala.collection` classes, so
/// library-ABI only. Both cases returned the wrong thing (or refused a correct
/// call) before, and the `case Seq(i, t)` one was a `VerifyError`.
#[test]
fn mism11_coll_runs_against_the_jar() {
    let name = "mism11_coll";
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

/// The private runtime has neither `ArrayBuilder` nor `ClassTag`; it must say
/// so rather than compile something it cannot back.
#[test]
fn mism11_coll_without_library_is_error() {
    let src = fixtures_dir().join("mism11_coll.scala");
    let out = tmp_dir("mism11_coll_nolib");
    let (ok, msgs) = compile(&out, None, &[src]);
    assert!(
        !ok,
        "mism11_coll should not compile without the jar:\n{msgs}"
    );
    assert!(
        msgs.contains("error:"),
        "expected diagnostics, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// None of the three makes the typer more permissive: a lambda that takes the
/// source's element type, a `Builder` whose invariant `To` does not match, and
/// a constructor position that *is* determined all stay errors. nsc 2.13.16
/// reports the same three.
#[test]
fn mism11_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip mism11_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("mism11_bad.scala");
    let out = tmp_dir("mism11_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "mism11_bad should not compile, got:\n{msgs}");
    for needle in [
        "required: (Seq[Int]) => Int",
        "required: Builder[Int, Array[String]]",
        "required: Qry2[String, Box2]",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- unit-ish cases

/// The captured binder, on its own: the element type of a grouped iterator is
/// a `Seq`, and every member inherited from `Iterator` sees it that way.
#[test]
fn mism11_grouped_element_is_a_seq() {
    accepts(
        "mism11_grouped",
        "object Main { def main(a: Array[String]): Unit = {\n\
         \x20 val g = Seq(1, 2, 3, 4).iterator.grouped(2)\n\
         \x20 val n: Seq[Int] = g.next()\n\
         \x20 println(n)\n\
         \x20 val m: Iterator[Int] = Seq(1, 2, 3, 4).iterator.grouped(2).map(s => s.sum)\n\
         \x20 println(m.toList)\n\
         \x20 val f = Seq(1, 2, 3, 4).iterator.grouped(2).flatMap(s => s)\n\
         \x20 println(f.toList) } }\n",
    );
}

/// A two-argument function parameter keeps both of its parameters: the
/// element-type rule is for the one-argument shape it was written for.
#[test]
fn mism11_two_argument_lambda_keeps_its_parameters() {
    accepts(
        "mism11_zip2",
        "object Main { def main(a: Array[String]): Unit = {\n\
         \x20 val xs = List(1, 2, 3)\n\
         \x20 println(xs.zip(List(\"a\", \"b\", \"c\")).map { case (i, s) => s * i })\n\
         \x20 println(xs.foldLeft(0)((acc, x) => acc + x)) } }\n",
    );
}

/// `ArrayBuilder` is a `Builder` whose `To` is the array, and a
/// `ReusableBuilder`'s own members are reachable through it.
#[test]
fn mism11_array_builder_is_a_builder() {
    accepts(
        "mism11_ab",
        "import scala.collection.mutable\n\
         import scala.reflect.ClassTag\n\
         object Main {\n\
         \x20 def mk[E: ClassTag]: mutable.Builder[E, Array[E]] = mutable.ArrayBuilder.make[E]\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val b = mk[Long]\n\
         \x20   b += 1L\n\
         \x20   b.clear()\n\
         \x20   b += 2L\n\
         \x20   println(b.result().toList) } }\n",
    );
}

/// The ordinary collections are unmoved: the element type of a `Map`'s
/// iteration is still the pair, and a `Range`'s is still `Int`.
#[test]
fn mism11_ordinary_element_types_are_unchanged() {
    accepts(
        "mism11_plain",
        "object Main { def main(a: Array[String]): Unit = {\n\
         \x20 println(Map(1 -> \"a\", 2 -> \"bb\").map { case (k, v) => (k, v.length) })\n\
         \x20 println((1 to 3).map(i => i * 2))\n\
         \x20 println(List(1, 2, 3).flatMap(i => List(i, i)))\n\
         \x20 println(Set(1, 2).map(_ + 1))\n\
         \x20 println(Option(3).map(_ + 1)) } }\n",
    );
}
