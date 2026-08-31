//! Five causes behind slick's remaining `type mismatch`es. Two of them were
//! also silent wrong answers: a member read at the wrong element type, and an
//! argument checked against a bound instead of against the type the signature
//! states.
//!
//!  * A type *constructor* parameter stands for its bound **applied to the
//!    arguments the application passes**: `M[A]` where `M[+X] <: IterableOnce[X]`
//!    is an `IterableOnce[A]`. Only the bare `M` case was widened, so
//!    `in.iterator` on slick's `traverse[A, B, M[+X] <: IterableOnce[X]]`
//!    handed back `IterableOnce`'s own `A` and every use of the element was
//!    `found: A  required: A` -- two symbols that print the same.
//!  * A case class's companion `apply` was handed the *class's* type
//!    parameters ("the class's own parameters stand in for the method's").
//!    One symbol then stood both for "fixed here" and "still to be inferred at
//!    this call", so a call from inside the class substituted `U := U`, the
//!    parameter still mentioned a parameter of the callee -- which is how
//!    "undetermined" is read -- and the argument was checked against the
//!    *bound*: `found: Bx[U]  required: Bx[Any]`.
//!  * `scala.math.BigDecimal`'s companion had three of its seventeen `apply`
//!    overloads. A hand-written prelude member declines the pickled copy, so
//!    the missing ones simply did not exist and
//!    `new ScalaNumericType[BigDecimal](BigDecimal.apply)` had no
//!    `(Double)BigDecimal` to eta-expand.
//!  * The implicit scope of a type is its companion **object**, and an
//!    object's members include the ones it inherits. slick declares every
//!    `Shape` in a trait and writes
//!    `object Shape extends ConstColumnShapeImplicits with …`, so none of them
//!    were candidates at all.
//!  * Implicit unification: `_` in the wanted type is a position the search is
//!    not asking about, and a *contravariant* parameter means the wanted type
//!    is the sub type of what the candidate declares
//!    (`constColumnShape: Shape[L, ConstColumn[T], T, ConstColumn[T]]`
//!    answering a wanted `Shape[FlatShapeLevel, LiteralColumn[Boolean], ?, ?BP]`).
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
        "scala-rs-mism12-{tag}-{}-{nanos}-{seq}",
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

/// The constructor bound, the case class's own `apply` and the inherited
/// companion implicits are all plain language rules: no library type is
/// involved, so this runs in both modes.
#[test]
fn mism12_lang_runs_in_both_modes() {
    dual_run("mism12_lang");
}

/// `IterableOnce` / `Factory` and `scala.math.BigDecimal`: library-ABI only.
#[test]
fn mism12_lib_runs_against_the_jar() {
    let name = "mism12_lib";
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

/// The private runtime has neither `IterableOnce`/`Factory` nor
/// `scala.math.BigDecimal`'s companion; it must say so rather than compile
/// something it cannot back.
#[test]
fn mism12_lib_without_library_is_error() {
    let src = fixtures_dir().join("mism12_lib.scala");
    let out = tmp_dir("mism12_lib_nolib");
    let (ok, msgs) = compile(&out, None, &[src]);
    assert!(
        !ok,
        "mism12_lib should not compile without the jar:\n{msgs}"
    );
    assert!(
        msgs.contains("error:"),
        "expected diagnostics, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// None of the five makes the typer more permissive: the element type is still
/// the one the bound gives, the companion `apply` still refuses a wrong
/// argument, an inherited companion implicit is still only found for a type it
/// fits, and the eta-expansion still needs an alternative that matches. nsc
/// 2.13.16 reports the same four.
#[test]
fn mism12_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip mism12_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("mism12_bad.scala");
    let out = tmp_dir("mism12_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "mism12_bad should not compile, got:\n{msgs}");
    for needle in [
        "required: SV[T, U]",
        "value length is not a member of Int",
        "could not find implicit value of type Shp[Flat, String, String, Rp[String]]",
        "required: (Boolean) => BigDecimal",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- unit-ish cases

/// The bound applied to the arguments, on its own: slick's `DBIOAction
/// .traverse` shape, where the element only comes out right if `M[A]`'s bound
/// is read as `IterableOnce[A]`.
#[test]
fn mism12_constructor_bound_is_applied() {
    accepts(
        "mism12_bound",
        "object Main {\n\
         \x20 def sum[M[+X] <: IterableOnce[X]](in: M[Int]): Int =\n\
         \x20   in.iterator.foldLeft(0)((acc, a) => acc + a)\n\
         \x20 def join[A, M[+X] <: Iterable[X]](in: M[A]): String =\n\
         \x20   in.map(a => a.toString).mkString(\",\")\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   println(sum(List(1, 2, 3)))\n\
         \x20   println(join(Vector(\"a\", \"b\"))) } }\n",
    );
}

/// A case class calling its own companion `apply`, inferred and written out.
#[test]
fn mism12_case_apply_from_inside_the_class() {
    accepts(
        "mism12_selfapply",
        "trait Bx[X]\n\
         case class SV[T, U](a: T, b: Bx[U]) {\n\
         \x20 def f: SV[Int, U] = SV(1, b)\n\
         \x20 def g: SV[Int, U] = SV[Int, U](1, b)\n\
         \x20 def h: SV[T, U] = copy(a = a) }\n\
         object Main { def main(a: Array[String]): Unit = println(\"ok\") }\n",
    );
}

/// The overloads the real companion declares.
#[test]
fn mism12_big_decimal_overloads() {
    accepts(
        "mism12_bigdec",
        "object Main { def main(a: Array[String]): Unit = {\n\
         \x20 val f: Double => BigDecimal = BigDecimal.apply\n\
         \x20 println(f(2.5))\n\
         \x20 println(BigDecimal(3L))\n\
         \x20 println(BigDecimal(125L, 2))\n\
         \x20 println(BigDecimal(BigInt(4)))\n\
         \x20 println(BigDecimal(\"1.25\")) } }\n",
    );
}

/// An object's implicit members include the ones it inherits, at every depth.
#[test]
fn mism12_companion_inherits_its_implicits() {
    accepts(
        "mism12_inherited",
        "trait Low { implicit def any2Tag[T]: Tg[T] = new Tg[T] { def s = \"any\" } }\n\
         trait Mid extends Low\n\
         trait Tg[T] { def s: String }\n\
         object Tg extends Mid\n\
         object Main { def main(a: Array[String]): Unit =\n\
         \x20 println(implicitly[Tg[Int]].s) }\n",
    );
}

/// A `_` in the wanted type does not constrain, and a contravariant parameter
/// is matched the other way round.
#[test]
fn mism12_wildcard_and_contravariant_witness() {
    accepts(
        "mism12_shape",
        "trait Rp[T]\n\
         class Const[T] extends Rp[T]\n\
         class Lit[T] extends Const[T]\n\
         abstract class Shp[-M, U, P]\n\
         trait Low { implicit def rep[T]: Shp[Rp[T], T, Rp[T]] = ??? }\n\
         object Shp extends Low { implicit def cst[T]: Shp[Const[T], T, Const[T]] = ??? }\n\
         object Main {\n\
         \x20 def packed[B, BP](b: B)(implicit s: Shp[B, _, BP]): BP = ???\n\
         \x20 def main(a: Array[String]): Unit = {\n\
         \x20   val p: Const[Boolean] = packed(new Lit[Boolean])\n\
         \x20   println(p) } }\n",
    );
}

// --------------------------------------------------- the multi-file cause

/// A cake in four files: the base is in a package the leaf's file never names,
/// and the middle trait comes *after* the leaf on the command line, so the
/// leaf's inherited names only appear in the second round of the header pass.
/// The leaf's `type SchemaDescription = SchemaDescriptionDef` is completed on
/// demand (a nested class's constructor parameter names it), and the scope the
/// completion used was frozen at the first round -- the right-hand side stayed
/// an unresolved name, so `new DDL(…)` was `found: DDL  required:
/// SchemaDescriptionDef` and the override check called the alias incompatible
/// with its own bound. This is slick's `MemoryProfile`, reduced; nsc 2.13.16
/// prints the same three lines.
#[test]
fn mism12_late_parent_type_alias_resolves() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip mism12_cake: scala-library jar not present");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi");
    let srcs: Vec<PathBuf> = [
        "mism12_basic.scala",
        "mism12_memory.scala",
        "mism12_relational.scala",
        "mism12_use.scala",
    ]
    .iter()
    .map(|f| dir.join(f))
    .collect();
    let out = tmp_dir("mism12_cake");
    let (ok, msgs) = compile(&out, Some(&jar), &srcs);
    assert!(ok, "compile mism12 cake failed:\n{msgs}");
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        let output = Command::new("java")
            .args(["-Xverify:all", "-cp", &cp, "mism12.use.Main"])
            .output()
            .expect("java");
        assert!(
            output.status.success(),
            "java mism12.use.Main failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "[people]\ncreate [people]\n[people,orders]\n"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
