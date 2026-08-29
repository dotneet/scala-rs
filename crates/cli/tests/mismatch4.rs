//! Fourth slice of the `type mismatch` family on slick: a type alias completed
//! before the signature pass ever reached it, a compound type conforming to an
//! applied abstract type member, `Map` as a `K => V`, `map`'s result on a
//! collection that does not redeclare it, and a stable identifier matched
//! against a scrutinee whose type arguments are still unknown.
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
        "scala-rs-mismatch4-{tag}-{}-{nanos}",
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
fn mismatch4_fixture_dual_run() {
    dual_run("mism4");
}

/// The relaxations must not swallow the errors nsc still reports.
#[test]
fn mism4_bad_is_still_rejected() {
    compile_fails(
        "mism4_bad",
        &[
            "type mismatch; found: $anon$1  required: Fixed5[String, Eff5]",
            "incompatible type in overriding type N5",
            "type mismatch; found: Map[String, Int]  required: (Int) => Int",
            "type mismatch; found: Nd5  required: Nd5.Self",
            "type mismatch; found: Seq[String]  required: IndexedSeq[String]",
        ],
    );
}

// -------------------------------------------------------------- unit-ish cases

/// A type alias is completed on demand the moment a name has to be dealiased,
/// and a *nested* template's parent clause does that during the header pass --
/// before the signature pass reaches the alias. Only the namer had recorded it
/// by then, and the namer records no scopes, so the stack rebuilt from the
/// owner chain had the enclosing templates' members but none of the unit's
/// imports: slick's `type ProfileAction[R, S, E] = FixedSqlAction[R, S, E]`
/// was stuck at `<error>` for the whole run, and every
/// `new SimpleJdbcProfileAction[Unit](…) { … }` reported a mismatch against it.
#[test]
fn an_alias_completed_early_still_sees_the_units_imports() {
    accepts(
        "mism4_alias_import",
        "package hidden {\n\
         \x20 trait Fixed[+R]\n\
         }\n\
         package app {\n\
         \x20 import hidden.Fixed\n\
         \x20 trait Comp {\n\
         \x20   type PA[+R] = Fixed[R]\n\
         \x20   abstract class Simple[+R] extends PA[R]\n\
         \x20   def create: PA[Unit] = new Simple[Unit] {}\n\
         \x20 }\n\
         }\n",
    );
}

/// `B[R] with M[R] <: A[R] with M[R]` where `M` is an abstract type member
/// applied to arguments. Nothing on the right can settle that -- an abstract
/// member has no right-hand side to expand -- so nsc falls through to the
/// rules for the left side, and the compound conforms through its own parent.
#[test]
fn a_compound_conforms_to_an_applied_abstract_member() {
    accepts(
        "mism4_compound_abstract",
        "trait A1[+R]\n\
         trait B1[+R] extends A1[R]\n\
         trait P1 {\n\
         \x20 type M[+R] <: A1[R]\n\
         \x20 type N[+R] <: A1[R] with M[R]\n\
         }\n\
         trait P2 extends P1 {\n\
         \x20 type M[+R] <: B1[R]\n\
         \x20 type N[+R] <: B1[R] with M[R]\n\
         }\n",
    );
}

/// `scala.collection.Map[K, V]` declares `PartialFunction[K, V]` among its
/// parents, so a `Map` *is* a `K => V`. The prelude's hierarchy only had the
/// `Iterable` edge, and `scala.FunctionN` as a class was not recognised as the
/// structural function type either.
#[test]
fn a_map_is_a_function_and_so_is_a_function_class() {
    accepts(
        "mism4_map_is_fn",
        "object M {\n\
         \x20 val m: Map[String, Int] = Map(\"a\" -> 1)\n\
         \x20 val f: String => Int = m\n\
         \x20 val pf: PartialFunction[String, Int] = { case \"a\" => 1 }\n\
         \x20 val g: String => Int = pf\n\
         \x20 val h: Int = f(\"a\") + g(\"a\")\n\
         }\n",
    );
}

/// `IndexedSeq` does not redeclare `map`, so the declaration it inherits says
/// `Seq[B]` -- but the real signature returns the receiver's own type
/// constructor. `Range` keeps the declared `IndexedSeq`: it has no type
/// parameter of its own.
#[test]
fn map_keeps_the_receivers_own_collection() {
    accepts(
        "mism4_map_result",
        "import scala.collection.mutable.ArrayBuffer\n\
         object M {\n\
         \x20 val a: IndexedSeq[String] = IndexedSeq(1, 2).map(_.toString)\n\
         \x20 val b: Seq[String] = Seq(1, 2).map(_.toString)\n\
         \x20 val c: Vector[String] = Vector(1, 2).map(_.toString)\n\
         \x20 val d: List[String] = List(1, 2).map(_.toString)\n\
         \x20 val e: ArrayBuffer[String] = ArrayBuffer(1, 2).map(_.toString)\n\
         \x20 val f: IndexedSeq[Int] = (1 to 3).map(_ * 2)\n\
         }\n",
    );
}

/// `type Self >: this.type <: Node` is written in `Node`'s vocabulary, so seen
/// from `trait NullaryNode extends Node` the lower bound is
/// `NullaryNode.this.type`: `def mapChildren(…): Self = this` is right, and so
/// is the `if (…) this else rebuild(…)` whose branches meet at `Self`. Only a
/// `this` tree gets this -- `def wrong(a: Node, b: Node): a.Self = b` still
/// fails, which is what kept the naive lower-bound rule out until now.
#[test]
fn this_conforms_to_a_self_member_declared_by_a_parent() {
    accepts(
        "mism4_self_member",
        "trait Nd {\n\
         \x20 type Self >: this.type <: Nd\n\
         \x20 def mapCh(f: Nd => Nd): Self\n\
         }\n\
         trait Tagged { def tag: Int }\n\
         trait Nullary extends Nd with Tagged {\n\
         \x20 final def mapCh(f: Nd => Nd): Self = this\n\
         \x20 def keep: Self with Tagged = this\n\
         }\n\
         trait Unary extends Nd {\n\
         \x20 def child: Nd\n\
         \x20 def rebuild(c: Nd): Self\n\
         \x20 final def mapCh(f: Nd => Nd): Self = {\n\
         \x20   val c2 = f(child)\n\
         \x20   val n: Self = if (c2 eq child) this else rebuild(c2)\n\
         \x20   n\n\
         \x20 }\n\
         }\n",
    );
}

/// `case ScalaBaseType.byteType =>` inside `def f[T](t: ScalaType[T])` compares
/// a `ScalaNumericType[Byte]` with a `ScalaType[T]`. `T` could be `Byte`, and
/// the pattern is only an `==` at run time, so a scrutinee that still names a
/// type parameter rules nothing out.
#[test]
fn a_stable_id_pattern_may_meet_an_abstract_scrutinee() {
    accepts(
        "mism4_stable_id_pattern",
        "trait ST[T]\n\
         class Num[T](val n: String) extends ST[T]\n\
         object Num {\n\
         \x20 val byteType: Num[Byte] = new Num[Byte](\"byte\")\n\
         \x20 val intType: Num[Int] = new Num[Int](\"int\")\n\
         }\n\
         object M {\n\
         \x20 def width[T](t: ST[T]): Int = t match {\n\
         \x20   case Num.byteType => 1\n\
         \x20   case Num.intType  => 4\n\
         \x20   case _            => 0\n\
         \x20 }\n\
         }\n",
    );
}
