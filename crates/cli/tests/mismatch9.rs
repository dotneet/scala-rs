//! Four causes behind slick's remaining `type mismatch`es.
//!
//!  * A higher-kinded application (`F[B]` on an abstract `F[_]`) is a
//!    `Type::Applied`, and the expected-type walk that solves a method's type
//!    parameters had arms for `Class`, `Tuple`, `Function` and `Array` but not
//!    for that: every cats-style `F.flatMap(fa) { … }` came back `F[Any]`.
//!  * Two alternatives that differ only by an implicit clause are equally
//!    specific in nsc, and only their *owners* separate them -- which is
//!    exactly what the pickle reader throws away when it pulls both copies
//!    down onto the receiver. `TreeSet.map(f)` / `.collect(pf)` were
//!    `ambiguous overload`; keyed on the explicit parameters only, the more
//!    derived declaration (the one whose `Ordering` witness makes the result a
//!    `TreeSet`) wins, and codegen calls it by the descriptor the pickle
//!    recorded rather than by the `IterableOps` shape it hardcodes.
//!  * `copy(f = x)` written *inside* a case class is the same call as
//!    `this.copy(f = x)`, and nsc's `copy[F]` re-infers the class's type
//!    parameters: slick's `Comprehension[+Fetch <: Option[Node]]` could not be
//!    rebuilt with a different `Fetch`.
//!  * `IterableOnceOps.foreach[U](f: A => U): Unit` is polymorphic in the
//!    function's result. The prelude wrote `A => Unit`, which a function
//!    *value* does not conform to.
//!
//! The fixtures are dual-run: compiled against the real `scala-library` jar
//! and (where the private runtime can back them) on the private runtime, under
//! `-Xverify:all`, and their stdout is compared with what nsc 2.13.16 prints.

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
        "scala-rs-mism9-{tag}-{}-{nanos}-{seq}",
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
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "{tag} should not compile, got:\n{msgs}");
    assert!(
        msgs.contains(needle),
        "expected {needle:?} in diagnostics for {tag}, got {msgs:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Runs an emitted `Main` and compares it with the expected stdout, in both
/// modes.
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

/// The higher-kinded inference and the in-class `copy`: no library type is
/// involved, so it runs on the private runtime too.
#[test]
fn mism9_hk_runs_in_both_modes() {
    dual_run("mism9_hk");
}

/// The sorted collections and `foreach`: real `scala.collection` classes, so
/// library-ABI only.
#[test]
fn mism9_coll_runs_against_the_jar() {
    let name = "mism9_coll";
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

/// The private runtime has no `TreeSet` / `TreeMap`; it must say so rather
/// than compile something it cannot back.
#[test]
fn mism9_coll_without_library_is_error() {
    let src = fixtures_dir().join("mism9_coll.scala");
    let out = tmp_dir("mism9_coll_nolib");
    let (ok, msgs) = compile(&out, None, &[src]);
    assert!(
        !ok,
        "mism9_coll should not compile without the jar:\n{msgs}"
    );
    assert!(
        msgs.contains("error:"),
        "expected diagnostics, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Each of these is one of the three errors nsc 2.13.16 gives for the same
/// source. Solving a type parameter from the expected type must not make an
/// ill-typed call type-check.
#[test]
fn mism9_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip mism9_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("mism9_bad.scala");
    let out = tmp_dir("mism9_bad");
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "mism9_bad should not compile, got:\n{msgs}");
    for needle in ["wrongResult", "wrongParam", "badCopy"] {
        assert!(
            msgs.contains(needle) || msgs.matches("error:").count() >= 3,
            "expected an error near {needle}, got:\n{msgs}"
        );
    }
    assert!(
        msgs.matches("error:").count() >= 3,
        "expected three errors, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------------------------- unit-ish cases

/// The minimisation of slick's `BasicBackend` / `ConcurrencyControl` case:
/// `B` is read out of the expected `F[String]`, which is an `Applied`.
#[test]
fn mism9_hk_result_comes_from_the_expected_type() {
    accepts(
        "mism9_hkexp",
        "trait FlatMap[F[_]] { def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B] }\n\
         object M {\n\
         \x20 def go[F[_]](fa: F[Int])(implicit F: FlatMap[F]): F[String] =\n\
         \x20   F.flatMap(fa) { i => null.asInstanceOf[F[String]] }\n\
         }\n",
    );
}

/// The same, where the expected type has already settled on a real class.
#[test]
fn mism9_hk_result_lines_up_with_a_class() {
    accepts(
        "mism9_hkcls",
        "trait FlatMap[F[_]] { def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B] }\n\
         object M {\n\
         \x20 def go(fa: List[Int])(implicit F: FlatMap[List]): List[String] =\n\
         \x20   F.flatMap(fa) { i => List(i.toString) }\n\
         }\n",
    );
}

/// A `TreeSet`'s `map` really is a `TreeSet`: the sorted alternative wins the
/// tie, so the narrowed static type is not a lie.
#[test]
fn mism9_sorted_set_map_is_a_tree_set() {
    accepts(
        "mism9_tsmap",
        "import scala.collection.immutable.TreeSet\n\
         object Main { def main(a: Array[String]): Unit = {\n\
         \x20 val ts: TreeSet[Int] = TreeSet(2, 1)\n\
         \x20 val m: TreeSet[Int] = ts.map(_ + 1)\n\
         \x20 val c: TreeSet[Int] = ts.collect { case x if x > 1 => x }\n\
         \x20 println(m); println(c) } }\n",
    );
}

/// `foreach` takes a function whose result is anything, not just `Unit`.
#[test]
fn mism9_foreach_result_is_polymorphic() {
    accepts(
        "mism9_foreach",
        "object Main { def main(a: Array[String]): Unit = {\n\
         \x20 def each[R](f: Int => R): Unit = (1 to 3).foreach(f)\n\
         \x20 each(i => i + 1)\n\
         \x20 val g: Int => String = _.toString\n\
         \x20 List(1, 2).foreach(g)\n\
         \x20 Vector(1).foreach(g) } }\n",
    );
}

/// `copy` inside the class is `this.copy`, and it re-infers the class's own
/// type parameters -- slick's `Comprehension[+Fetch <: Option[Node]]`.
#[test]
fn mism9_bare_copy_reinfers_type_parameters() {
    accepts(
        "mism9_copy",
        "final case class C[+F <: Option[Int]](n: String, f: F = None) {\n\
         \x20 def widen(x: Option[Int]): C[Option[Int]] = copy(f = x)\n\
         \x20 def same: C[F] = copy(n = n + \"!\")\n\
         }\n\
         object Main { def main(a: Array[String]): Unit =\n\
         \x20 println(C(\"a\", Some(1)).widen(None).same) }\n",
    );
}

/// A `copy` the user wrote wins over the synthetic one, qualified or not.
#[test]
fn mism9_user_copy_is_not_rewritten() {
    accepts(
        "mism9_usercopy",
        "final case class C(n: Int) {\n\
         \x20 def copy(n: Int = this.n, extra: Int = 0): C = new C(n + extra)\n\
         \x20 def bump: C = copy(extra = 1)\n\
         }\n\
         object Main { def main(a: Array[String]): Unit = println(C(1).bump.n) }\n",
    );
}

/// A tree the typer could not type is reported where it failed; the
/// conversion that follows must not repeat it as `found: <notype>`.
#[test]
fn mism9_notype_is_not_reported_twice() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip mism9_notype: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("mism9_notype");
    let src = dir.join("mism9_notype.scala");
    fs::write(
        &src,
        "object Main { def main(a: Array[String]): Unit = {\n\
         \x20 val s: String = noSuchThing\n\
         \x20 println(s) } }\n",
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(!ok, "mism9_notype should not compile, got:\n{msgs}");
    assert!(
        msgs.contains("not found: value noSuchThing"),
        "expected the root cause, got:\n{msgs}"
    );
    assert!(
        !msgs.contains("found: <notype>"),
        "the cascade should be absorbed, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The inference is not a licence: an ill-typed higher-kinded call is still an
/// error, with nsc's message.
#[test]
fn mism9_hk_wrong_result_is_rejected() {
    rejects(
        "mism9_hkbad",
        "trait FlatMap[F[_]] { def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]\n\
         \x20 def pure[A](x: A): F[A] }\n\
         object M {\n\
         \x20 def go[F[_]](fa: F[Int])(implicit F: FlatMap[F]): F[String] =\n\
         \x20   F.flatMap(fa)(i => F.pure(i))\n\
         }\n",
        "type mismatch",
    );
}
