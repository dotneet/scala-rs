//! implicit 解決の残件と prelude の穴。
//!
//! slick で残っていた implicit 関連のエラーを追ったときに出てきた経路をまとめて
//! 固定する。
//!
//! - 一度解決した呼び出しをもう一度型付けする経路（`retry_tupled_args` の
//!   タプル化リトライ）が、前のパスが埋めた implicit 引数を argument list に
//!   残したまま再解決していた。`LiteralNode(1)` が
//!   `not found: value intType` になっていたのはこれ。
//! - prelude の `scala.math.Numeric` に `Ordering` の親が張られておらず、
//!   `Numeric[T] <: Ordering[T]` が成り立たなかった。
//! - 値引数がどれも触れない型パラメータ（`def mk[T: TT](s: String)`）は
//!   implicit 探索だけが決められる。
//! - 関数値の `apply` は関数そのもの。
//! - 引数位置に残った implicit 節（`take(Array.empty)` の
//!   `(ClassTag[T])Array[T]`）は、パラメータ型が決まってから埋める。
//! - `implicit object X` は implicit 候補としては 1 つ（module と
//!   module class の 2 つではない）。
//!
//! フィクスチャは実 `scala-library` の jar に対してコンパイルし、実行結果を
//! nsc 2.13.16 が同じソースに対して出すものと比較する。

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
        "scala-rs-itail-{tag}-{}-{nanos}",
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

/// Compile the named snippet and require no error.
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

/// Compile the named snippet and require the given diagnostic.
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
    let (ok, msgs) = compile(&out, &jar, &[src]);
    assert!(!ok, "expected {tag} to be rejected, got:\n{msgs}");
    assert!(
        msgs.contains(needle),
        "expected {needle:?} in diagnostics for {tag}, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn fixture_fails(name: &str, needles: &[&str]) {
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
            "expected {needle:?} in diagnostics for {name}, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------------ fixtures

#[test]
fn itail_fixture_dual_run() {
    dual_run("itail");
}

/// The new paths must not make a missing implicit compile.
#[test]
fn a_missing_implicit_is_still_reported() {
    fixture_fails(
        "itail_bad",
        &[
            "could not find implicit value of type Tagged[String]",
            "could not find implicit value of type Sized[T]",
        ],
    );
}

// ------------------------------------------------------------ unit-ish cases

/// The tupled retry re-types a call whose implicit arguments the first pass
/// already filled in. `LiteralNode(1)` reported `not found: value intType` for
/// a companion implicit the search had already settled.
#[test]
fn an_implicit_filled_call_survives_being_typed_twice() {
    accepts(
        "itail_retypecall",
        "class Tag[T](val n: String)\n\
         object Tag {\n\
         \x20 implicit val i: Tag[Int] = new Tag[Int](\"i\")\n\
         }\n\
         class Lit(val t: Tag[?], val v: Any, val vol: Boolean)\n\
         object Lit {\n\
         \x20 def apply(t: Tag[?], v: Any, vol: Boolean = false): Lit = new Lit(t, v, vol)\n\
         \x20 def apply[T](v: T)(implicit t: Tag[T]): Lit = apply(t, v)\n\
         }\n\
         case class Two(a: Lit, b: Lit)\n\
         object M {\n\
         \x20 val r = Two(Lit(1), Lit(2))\n\
         }\n",
    );
}

/// `trait Numeric[T] extends Ordering[T]`: the prelude synthesized `Numeric`
/// without that parent, so slick's `ScalaNumericType` could not pass its
/// `Numeric[T]` to a `ScalaBaseType[T](implicit …, Ordering[T])`.
#[test]
fn numeric_is_an_ordering() {
    accepts(
        "itail_numord",
        "import scala.reflect.ClassTag\n\
         class B[T](implicit val ct: ClassTag[T], val ord: Ordering[T])\n\
         class N[T](val f: Double => T)(implicit t: ClassTag[T], val num: Numeric[T])\n\
         \x20 extends B[T]()(t, num)\n\
         object M {\n\
         \x20 val o: Ordering[Int] = implicitly[Numeric[Int]]\n\
         \x20 val n = new N[Int](_.toInt)\n\
         }\n",
    );
}

/// The same, spelled as a plain conformance through a type parameter.
#[test]
fn a_numeric_type_parameter_is_an_ordering() {
    accepts(
        "itail_numord2",
        "object M {\n\
         \x20 def takeOrd[T](o: Ordering[T]): Int = 1\n\
         \x20 def f[T](n: Numeric[T]): Int = takeOrd(n)\n\
         \x20 val r = f(implicitly[Numeric[Int]])\n\
         }\n",
    );
}

/// No value argument mentions `T`, so only the implicit search can pin it
/// down (slick's `SimpleFunction.nullary`).
#[test]
fn an_implicit_only_type_parameter_is_solved_by_the_witness() {
    accepts(
        "itail_implicitonly",
        "class TT[T](val name: String)\n\
         object TT {\n\
         \x20 implicit val i: TT[Int] = new TT[Int](\"i\")\n\
         }\n\
         object M {\n\
         \x20 def mk[T](s: String)(implicit t: TT[T]): (Seq[Int] => String) =\n\
         \x20   (xs: Seq[Int]) => s + t.name + xs.size\n\
         \x20 def nullary[R: TT](s: String): String = mk(s).apply(Seq())\n\
         \x20 val f = mk[Int](\"x\")\n\
         }\n",
    );
}

/// `f.apply(xs)` is `f(xs)`: the prelude's `FunctionN.apply` is declared over
/// erased parameters, so the selection used to come out as `Any`.
#[test]
fn apply_on_a_function_value_is_the_function() {
    accepts(
        "itail_funapply",
        "object M {\n\
         \x20 val f: Seq[Int] => String = xs => \"n=\" + xs.size\n\
         \x20 val s: String = f.apply(Seq(1))\n\
         \x20 val g: (Int, Int) => Int = (a, b) => a + b\n\
         \x20 val n: Int = g.apply(1, 2)\n\
         }\n",
    );
}

/// A residual implicit clause in argument position: `Array.empty` is
/// `(ClassTag[T])Array[T]` until the parameter says what `T` is.
#[test]
fn a_residual_implicit_clause_is_applied_in_argument_position() {
    accepts(
        "itail_residual",
        "object M {\n\
         \x20 def take(a: Array[String]): Int = a.length\n\
         \x20 def takeInts(a: Array[Int]): Int = a.length\n\
         \x20 val r = take(Array.empty) + takeInts(Array.empty)\n\
         }\n",
    );
}

/// …and the parameter, not the one witness that happens to be in scope, is
/// what the clause is filled from.
#[test]
fn the_parameter_decides_which_witness_a_residual_clause_needs() {
    rejects(
        "itail_residual_bad",
        "class Tag[T](val n: String)\n\
         object Tag {\n\
         \x20 implicit val i: Tag[Int] = new Tag[Int](\"i\")\n\
         }\n\
         class Box[T](val n: Int)\n\
         object M {\n\
         \x20 def take(b: Box[String]): Int = b.n\n\
         \x20 def empty[T](implicit t: Tag[T]): Box[T] = new Box[T](0)\n\
         \x20 val r = take(empty)\n\
         }\n",
        "could not find implicit value of type Tag[String]",
    );
}

/// `implicit object X` is one implicit value. Both the module and its module
/// class carry the flag, and reporting them as ambiguous with each other made
/// every `GetResult[String]` in slick unresolvable.
#[test]
fn an_implicit_object_is_not_ambiguous_with_itself() {
    accepts(
        "itail_implicitobject",
        "trait Get[T] { def get: T }\n\
         object Get {\n\
         \x20 implicit object GetString extends Get[String] { def get = \"s\" }\n\
         }\n\
         object M {\n\
         \x20 def read[T](implicit g: Get[T]): T = g.get\n\
         \x20 val s: String = read[String]\n\
         }\n",
    );
}

/// A `case class` with a repeated parameter: nsc gives it no `copy`, but
/// scala-rs does, and typing its `copy$default$n` (`this.cells`) against the
/// repeated type reported a mismatch on a tree the user never wrote.
#[test]
fn a_repeated_case_class_parameter_has_a_sequence_default() {
    accepts(
        "itail_repeatedcase",
        "final case class Row(name: String, cells: Int*) {\n\
         \x20 def total: Int = cells.sum\n\
         }\n\
         object M {\n\
         \x20 val r = Row(\"r\", 1, 2)\n\
         \x20 val n = r.total + r.cells.size\n\
         }\n",
    );
}
