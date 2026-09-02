//! Eight causes behind slick's remaining `type mismatch`es (the thirteenth
//! slice). Three of them were also silent wrong answers -- a member read at
//! the wrong type argument, a `copy` rewritten to a class the file cannot
//! name, and a `<:<` witness applied as if it were a method.
//!
//!  * A substitution applied three times. `pick_ctor_at` substituted once for
//!    the applicability check and once more on the way out, and the `new` path
//!    a third time; idempotent until the type arguments *mention* the
//!    parameter they replace, which `new Box[(T, T2), (U, U2)]` written inside
//!    `Box[T, U]` does -- `T` came back `(((T, T2), T2), T2)`.
//!  * An implicit whose class merely *inherits* `Function1` is a view.
//!    `sealed abstract class <:<[-From, +To] extends (From => To)`, so
//!    `implicit ev: P <:< Rp[Option[QO]]` converts a `P`; nothing but the
//!    structural `Type::Function` was recognised. Applying one is a
//!    `Function1.apply`, not a call of the value's own symbol -- codegen
//!    emitted a call to a member of the enclosing *method*.
//!  * A variable that only a lambda's *result* can fix. Opened to its bound,
//!    `def h[B](f: Int => Bx[B])` checked the body against `Bx[Any]`, and an
//!    invariant `Bx[Int]` is not that. A wildcard is what "not decided yet"
//!    means in a position `is_sub_type` understands.
//!  * `lub` skipped the *head* of a base type sequence, so `lub(Some[X],
//!    Option[Y])` never saw `Option` on the second side, walked past
//!    `Option[X]` and answered `Product`.
//!  * An inherited member is declared in its owner's type parameters.
//!    `subst_as_seen_from` walks the parent chain properly; reading the result
//!    a *second* time at the receiver's own arguments assumed the positions
//!    line up, and slick's `BaseJoinQuery[E1, E2, U1, U2, C, B1, B2] <:
//!    Query[+E, U, C[_]]` gave `Query.map`'s `Query[G, T, C]` the join's third
//!    argument.
//!  * Explicit type arguments settle a parameter, and that settled type is the
//!    expected type of the argument. `Ref.of[F, State[F]](State(…))` has to
//!    read the higher-kinded `F` of `case class State[F[_]]` off `State[F]`,
//!    since no argument mentions it.
//!  * `copy` is rewritten to `new C(…)` spelled by *name*, which re-resolves
//!    the class in whatever file the rewrite runs in. `override def
//!    getDumpInfo = super.getDumpInfo.copy(mainInfo = …)` is written in files
//!    that never import `DumpInfo`.
//!  * The branches of an `if`/`match` are joined after a variable nothing
//!    pinned is read at its bound: `if (c) Vector.empty else v` joined
//!    `Vector[A]` with `Vector[(String, Int)]` and walked up to
//!    `Vector[AnyRef]`.
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
        "scala-rs-mism13-{tag}-{}-{nanos}-{seq}",
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

fn run_main(out: &Path, jar: Option<&Path>, main: &str) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}",
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

/// Compiles a snippet against the jar and asserts it is accepted.
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
            run_main(&priv_out, None, "Main"),
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
            run_main(&jar_out, Some(&jar), "Main"),
            exp,
            "stdout mismatch for {name} against the jar"
        );
    }
    let _ = fs::remove_dir_all(&jar_out);
}

// ------------------------------------------------------------------ fixtures

/// The self-referential `new`, the lambda-result variable, the inherited
/// member at the owner's parameters, the higher-kinded companion `apply` and
/// the branch join: all plain language rules, so both modes run them.
#[test]
fn mism13_lang_runs_in_both_modes() {
    dual_run("mism13_lang");
}

/// `scala.<:<` as an implicit view: library-ABI only.
#[test]
fn mism13_lib_runs_against_the_jar() {
    let name = "mism13_lib";
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    if java_available() {
        assert_eq!(run_main(&out, Some(&jar), "Main"), expected(name));
    }
    let _ = fs::remove_dir_all(&out);
}

/// The private runtime has no `scala.<:<` to back the view with; it must say
/// so rather than compile something it cannot emit.
#[test]
fn mism13_lib_without_library_is_error() {
    let src = fixtures_dir().join("mism13_lib.scala");
    let out = tmp_dir("mism13_lib_nolib");
    let (ok, msgs) = compile(&out, None, &[src]);
    assert!(
        !ok,
        "mism13_lib should not compile without the jar:\n{msgs}"
    );
    assert!(
        msgs.contains("not found: type <:<"),
        "expected the gated diagnostic, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// None of the eight makes the typer more permissive. nsc 2.13.16 reports the
/// same six errors on this fixture.
#[test]
fn mism13_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip mism13_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("mism13_bad.scala");
    let out = tmp_dir("mism13_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "mism13_bad should not compile, got:\n{msgs}");
    for needle in [
        "found: T  required: (T, T2)",
        "found: Qy[G, T, C]  required: Qy[G, T, D]",
        "found: By[Int]  required: Bx[Int]",
        "found: St[By]  required: St[Bx]",
        "could not find implicit value of type <:<[String, Rp[Option[Int]]]",
        "found: \"s\"  required: Int",
    ] {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics, got:\n{msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- unit-ish cases

/// A class instantiating itself at a type that mentions its own parameter.
/// The substitution has to run exactly once, whichever path picks the
/// constructor -- `new`, `extends`, and an overloaded constructor all end up
/// in `pick_ctor_at`.
#[test]
fn mism13_self_new_substitutes_once() {
    accepts(
        "mism13_selfnew",
        "class Sh[T, U]\n\
         case class Box[T, U](value: T, shape: Sh[T, U]) {\n\
         \x20 def zip[T2, U2](v2: T2) =\n\
         \x20   new Box[(T, T2), (U, U2)]((value, v2), new Sh[(T, T2), (U, U2)]) }\n\
         class Ovl[T](val t: T) { def this(t: T, n: Int) = this(t) }\n\
         class Sub[T](t: T) extends Ovl[(T, Int)]((t, 1), 2)\n\
         object Main { def main(a: Array[String]): Unit = println(\"ok\") }\n",
    );
}

/// `implicit ev: P <:< Q` is a view, and applying it is `Function1.apply`.
#[test]
fn mism13_conformance_witness_is_a_view() {
    accepts(
        "mism13_view",
        "class Rp[T]\n\
         class Ext[P](r: P) {\n\
         \x20 def flatMap[QO](f: P => Rp[Option[QO]]): Rp[Option[QO]] = ???\n\
         \x20 def flatten[QO](implicit ev: P <:< Rp[Option[QO]]): Rp[Option[QO]] =\n\
         \x20   flatMap[QO](identity(_))\n\
         \x20 def direct[QO](implicit ev: P <:< Rp[Option[QO]]): Rp[Option[QO]] = r }\n\
         object Main { def main(a: Array[String]): Unit = println(\"ok\") }\n",
    );
}

/// A variable that occurs only *inside* the result of a function-typed
/// parameter is the lambda body's to fix, not its bound's.
#[test]
fn mism13_nested_lambda_result_variable() {
    accepts(
        "mism13_nested",
        "class Bx[R]\n\
         trait NoStream\n\
         trait Effect\n\
         trait Act[+R, +S <: NoStream, -E <: Effect] {\n\
         \x20 def flatMap[R2, S2 <: NoStream, E2 <: Effect](f: R => Act[R2, S2, E2])\n\
         \x20   : Act[R2, S2, E with E2] = ???\n\
         \x20 def andFinally[E2 <: Effect](a: Act[?, NoStream, E2]): Act[R, S, E with E2] = ??? }\n\
         class SetIso extends Act[Unit, NoStream, Effect]\n\
         object Top { def h[B](f: Int => Bx[B]): Bx[B] = ??? }\n\
         class C[R, S <: NoStream, E <: Effect](a: Act[R, S, E], b: Bx[Int]) {\n\
         \x20 val v0: Bx[Int] = Top.h(x => b)\n\
         \x20 def go: Act[R, S, E] =\n\
         \x20   (new SetIso).flatMap(old => a.andFinally(new SetIso)) }\n\
         object Main { def main(a: Array[String]): Unit = println(\"ok\") }\n",
    );
}

/// `lub` reads the head of each base type sequence: `Some[X]` joined with an
/// `Option[Y]` is an `Option`, never `Product`.
#[test]
fn mism13_lub_sees_the_sequence_head() {
    accepts(
        "mism13_lub",
        "trait Prod\n\
         sealed trait Opt[+A] extends Prod\n\
         case object Non extends Opt[Nothing]\n\
         final case class Som[+A](a: A) extends Opt[A]\n\
         object Main {\n\
         \x20 def pick(o: Opt[Som[Int]], flag: Boolean): Opt[Opt[Any]] = {\n\
         \x20   val joined = if (flag) Som(Non) else o\n\
         \x20   joined }\n\
         \x20 def main(a: Array[String]): Unit = println(pick(Non, true)) }\n",
    );
}

/// An inherited member read at the *owner's* arguments, not the receiver's.
#[test]
fn mism13_inherited_member_at_owner_targs() {
    accepts(
        "mism13_owner",
        "trait Lvl\n\
         trait Flat extends Lvl\n\
         abstract class Shp[Level <: Lvl, -M, U, P]\n\
         class SV[+E, U]\n\
         abstract class Qy[+E, U, C[_]] {\n\
         \x20 def shaped: SV[? <: E, U]\n\
         \x20 def map[F, G, T](f: E => F): Qy[G, T, C] = ???\n\
         \x20 def mapI[F, G, T](f: E => F)(implicit s: Shp[? <: Flat, F, T, G]): Qy[G, T, C] = ???\n\
         \x20 private[this] def join[E2, U2, D[_]](q2: Qy[E2, U2, D]) =\n\
         \x20   new BJQ[E, E2, U, U2, C, E, E2](shaped, q2.shaped)\n\
         \x20 def zw1[E2, U2, F, G, T, D[_]](q2: Qy[E2, U2, D], f: (E, E2) => F): Qy[G, T, C] =\n\
         \x20   join(q2).map[F, G, T](x => f(x._1, x._2))\n\
         \x20 def zw2[E2, U2, F, G, T, D[_]](q2: Qy[E2, U2, D], f: (E, E2) => F)\n\
         \x20                               (implicit s: Shp[? <: Flat, F, T, G]): Qy[G, T, C] =\n\
         \x20   join(q2).mapI[F, G, T](x => f(x._1, x._2)) }\n\
         class Wrap[+E, U, C[_]](val shaped: SV[? <: E, U]) extends Qy[E, U, C]\n\
         final class BJQ[+E1, +E2, U1, U2, C[_], +B1, +B2](b1: SV[? <: E1, U1], b2: SV[? <: E2, U2])\n\
         \x20   extends Wrap[(E1, E2), (U1, U2), C](new SV[(E1, E2), (U1, U2)])\n\
         object Main { def main(a: Array[String]): Unit = println(\"ok\") }\n",
    );
}

/// Explicit type arguments are the argument's expected type: a higher-kinded
/// `F` no argument mentions comes off it.
#[test]
fn mism13_explicit_targs_are_the_argument_pt() {
    accepts(
        "mism13_hk",
        "import scala.collection.immutable.TreeMap\n\
         class Df[F[_], A]\n\
         final case class St[F[_]](available: Long, waiting: TreeMap[Long, Df[F, Unit]])\n\
         object Use {\n\
         \x20 def take[F[_], A](x: A): A = x\n\
         \x20 def mk[F[_]]: St[F] = take[F, St[F]](St(1L, TreeMap.empty)) }\n\
         object Main { def main(a: Array[String]): Unit = println(\"ok\") }\n",
    );
}

/// The branch join reads an unpinned covariant variable at its bound.
#[test]
fn mism13_branch_join_closes_a_free_variable() {
    accepts(
        "mism13_branch",
        "object Main {\n\
         \x20 def names: Iterable[String] = Nil\n\
         \x20 def kids: Seq[Int] = Nil\n\
         \x20 def f(flag: Boolean): Iterable[(String, Int)] = {\n\
         \x20   val ch = flag match {\n\
         \x20     case true  => Vector.empty\n\
         \x20     case false => names.zip(kids).toVector }\n\
         \x20   ch }\n\
         \x20 def g(flag: Boolean): Vector[(String, Int)] =\n\
         \x20   if (flag) Vector.empty else names.zip(kids).toVector\n\
         \x20 def main(a: Array[String]): Unit = println(f(true)) }\n",
    );
}

/// An enclosing `def f[T]`'s own `T` is still open: only a leftover the scope
/// cannot name is read at its bound, and the answer is always one of the two
/// branch types.
#[test]
fn mism13_branch_join_keeps_a_parameter_in_scope() {
    accepts(
        "mism13_branch_scope",
        "object Main {\n\
         \x20 def f[T](x: T, flag: Boolean): List[Any] =\n\
         \x20   if (flag) List.empty[T] else List(1)\n\
         \x20 def main(a: Array[String]): Unit = println(f(\"s\", true)) }\n",
    );
}

// --------------------------------------------------- the multi-file cause

/// `copy` in a file that cannot name the class. The rewrite builds a `new
/// DumpInfo(…)`, and spelling that class by name re-resolved it in the
/// *subclass's* file -- which only inherits `getDumpInfo`, and never imports
/// `slick.util.DumpInfo`. Three files, because the error needs the definition,
/// the inherited member and the override to be in three different scopes; nsc
/// 2.13.16 compiles the same three and prints the same two lines.
#[test]
fn mism13_copy_names_no_class_in_the_using_file() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip mism13_copy: scala-library jar not present");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi");
    let srcs: Vec<PathBuf> = ["mism13_util.scala", "mism13_ast.scala", "mism13_jdbc.scala"]
        .iter()
        .map(|f| dir.join(f))
        .collect();
    let out = tmp_dir("mism13_copy");
    let (ok, msgs) = compile(&out, Some(&jar), &srcs);
    assert!(ok, "compile mism13 copy cake failed:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&out, Some(&jar), "mism13.jdbc.Main"),
            "c idx=3\ntrue\n"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
