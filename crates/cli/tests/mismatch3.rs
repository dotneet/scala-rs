//! Third slice of the `type mismatch` family on slick: an abstract type member
//! an alias overrides, a type parameter no argument can pin, `this.type` read
//! back through the receiver, a block that is not an argument list, and
//! protected access from a class written inside the one that declares it.
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
        "scala-rs-mismatch3-{tag}-{}-{nanos}",
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
fn compile(out: &Path, jar: &Path, srcs: &[PathBuf], extra_cp: Option<&Path>) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    if let Some(cp) = extra_cp {
        cmd.args(["-cp", cp.to_str().unwrap()]);
    }
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
    let (ok, msgs) = compile(&out, &jar, &[src], None);
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
    let (ok, msgs) = compile(&out, &jar, &[src], None);
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
    let (_, msgs) = compile(&out, &jar, &[src], None);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Compile `lib` into its own directory, then compile `user` against it with
/// `-cp`: what crosses is the `ScalaSignature` pickle, not the source.
fn accepts_separately(tag: &str, lib: &str, user: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {tag}: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let lib_src = dir.join("lib.scala");
    let user_src = dir.join("user.scala");
    fs::write(&lib_src, lib).unwrap();
    fs::write(&user_src, user).unwrap();
    let lib_out = dir.join("libout");
    let user_out = dir.join("userout");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&user_out).unwrap();
    let (ok, msgs) = compile(&lib_out, &jar, &[lib_src], None);
    assert!(ok, "{tag}: library failed to compile:\n{msgs}");
    let (_, msgs) = compile(&user_out, &jar, &[user_src], Some(&lib_out));
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile against the classpath, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ fixtures

#[test]
fn mismatch3_fixture_dual_run() {
    dual_run("mism3");
}

/// The relaxations must not swallow the errors nsc still reports.
#[test]
fn mism3_bad_is_still_rejected() {
    compile_fails(
        "mism3_bad",
        &[
            "value secret cannot be accessed as a member of P3 from Q3",
            "type mismatch; found: Cell3[Int]  required: Cell3[String]",
        ],
    );
}

// -------------------------------------------------------------- unit-ish cases

/// Inherited members were entered depth-first, so a *grandparent's* deferred
/// declaration could land in the scope before its own subclass's concrete one:
/// `new SimpleFeatureNode[T] with SimpleFunction` saw `Node`'s abstract
/// `type Self` instead of `SimpleFeatureNode`'s alias.
#[test]
fn an_alias_overrides_the_abstract_member_it_inherits_twice() {
    accepts(
        "mism3_alias_wins",
        "trait N {\n\
         \x20 type Self >: this.type <: N\n\
         \x20 def self: Self\n\
         }\n\
         abstract class Base[T] extends N {\n\
         \x20 type Self = Base[T]\n\
         }\n\
         trait Extra extends N\n\
         object M {\n\
         \x20 def make[T](t: T): Base[T] = new Base[T] with Extra {\n\
         \x20   def self: Self = this\n\
         \x20   def again: Self = make(t)\n\
         \x20 }\n\
         }\n",
    );
}

/// The alias's right-hand side is written in its owner's vocabulary; reached
/// through a `Base[String]` it means `Base[String]`, not `Base[T]`.
#[test]
fn an_alias_type_member_is_read_through_its_prefix() {
    accepts(
        "mism3_alias_prefix",
        "trait N2 { type Self; def self: Self }\n\
         abstract class Base2[T](val label: T) extends N2 { type Self = Base2[T] }\n\
         object M {\n\
         \x20 def f(b: Base2[String]): String = b.self.label\n\
         }\n",
    );
}

/// A type parameter that occurs in no parameter type is one no argument could
/// pin. Once the expected type has had its say nsc instantiates it to a bound;
/// leaving it a parameter made slick's `dbAction { … }` report
/// `found: FixedBasicAction[Unit, S, Schema]`.
#[test]
fn a_parameter_no_argument_mentions_is_instantiated_to_its_bound() {
    accepts(
        "mism3_leftover_tparam",
        "trait NoStream\n\
         trait Effect\n\
         class Act[+R, +S <: NoStream, -E <: Effect](val r: R)\n\
         object M {\n\
         \x20 def act[R, S <: NoStream, E <: Effect](f: Int => R): Act[R, S, E] = new Act(f(0))\n\
         \x20 val a: Act[Int, Nothing, Effect] = act(_ + 1)\n\
         }\n",
    );
}

/// nsc's `canApply`: `new C { … }` is not a function, so the block on the next
/// line is a statement of its own. Applying it made slick's `def build(…) = new
/// SimpleFeatureNode[T] with SimpleFunction { … }` swallow the lambda that
/// follows it.
#[test]
fn a_block_after_an_anonymous_class_is_not_an_argument() {
    accepts(
        "mism3_new_then_block",
        "trait TT\n\
         class FN(val p: Int)\n\
         object M {\n\
         \x20 def mk(n: String): Int => FN = {\n\
         \x20   def build(p: Int): FN = new FN(p) with TT {\n\
         \x20     val name = n\n\
         \x20   }\n\
         \x20   { (i: Int) => build(i) }\n\
         \x20 }\n\
         }\n",
    );
}

/// A `this.type` result is the receiver, arguments and all, and a receiver may
/// still carry undetermined variables that this call's arguments fix
/// (slick's `ConstArray.newBuilder() + from + select`).
#[test]
fn this_type_keeps_the_receivers_arguments() {
    accepts(
        "mism3_this_type",
        "class B[T] {\n\
         \x20 def add(v: T): this.type = this\n\
         \x20 def result: List[T] = Nil\n\
         }\n\
         object M {\n\
         \x20 def newB[T](cap: Int = 16): B[T] = new B[T]\n\
         \x20 val a: List[String] = newB().add(\"x\").add(\"y\").result\n\
         \x20 val b: List[Int] = (new B[Int]).add(1).result\n\
         }\n",
    );
}

/// Protected access is weighed against every *enclosing* class, not only the
/// innermost one: `new DDL { … self.phase … }` written inside `DDL` is in
/// `DDL`'s own template, so the prefix only has to be a `DDL`.
#[test]
fn protected_access_counts_the_enclosing_class() {
    accepts(
        "mism3_protected_outer",
        "class DDL(val stmts: List[String]) { self =>\n\
         \x20 protected def phase: List[String] = stmts\n\
         \x20 def merge(other: DDL): DDL = new DDL(Nil) {\n\
         \x20   override protected def phase: List[String] = self.phase ++ other.phase\n\
         \x20 }\n\
         }\n",
    );
}

/// A `ScalaSignature` kept only the head name of every type, so a class read
/// back from the classpath had no type arguments and its parameters no kind:
/// `Monad[F]` failed the kind check and `c.as(1)` came back `Any`.
#[test]
fn a_classpath_pickle_carries_kinds_and_type_arguments() {
    accepts_separately(
        "mism3_pickle_hk",
        "package mlib\n\
         trait Monad[F[_]] {\n\
         \x20 def pure[A](a: A): F[A]\n\
         }\n\
         class Cell[A](val value: A) {\n\
         \x20 def pair: (A, A) = (value, value)\n\
         \x20 def as[B](b: B): Cell[B] = new Cell(b)\n\
         }\n",
        "import mlib._\n\
         object Use {\n\
         \x20 def lift[F[_], A](m: Monad[F], a: A): F[A] = m.pure(a)\n\
         \x20 def grab[A](c: Cell[A]): A = c.pair._1\n\
         \x20 def swap[A](c: Cell[A]): Cell[Int] = c.as(1)\n\
         }\n",
    );
}
