//! Declaration, member-lookup and default-value fixes found compiling
//! scala/scala's own `src/library` (`tests/scalalib_measure.sh`), each reduced
//! to user code and checked against real scalac 2.13.16 in both directions:
//! the valid program runs with scalac's output, and the nearby invalid one is
//! rejected by both compilers.
//!
//! * `var x: T = _` is a field left at its JVM default -- never stored by the
//!   constructor -- and is refused for a `val`, a `lazy val`, a pattern and a
//!   local variable (`libdecl_defaultinit`).
//! * A single constructor's parameter types are the arguments' expected
//!   types (`libdecl_ctorproto`).
//! * A source `scala.Predef`'s type aliases are open in signatures
//!   (`libdecl_predef`).
//! * A parent's self alias is not a member; a lone `Nothing` solution stands
//!   when nothing is expected; a parameterless getter with a setter takes
//!   `op=`; a Java `Object` parameter matches `AnyRef` and `Any` when
//!   overriding and an `Object[]` parameter takes `Array[AnyRef]`; numeric
//!   branches meet at their weak lub (`libdecl_misc`).
//!
//! Fixture prefix: `libdecl_`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-libdecl-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn run_java(cp: &str) -> String {
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile fixture `name` with scala-rs (or with scalac when `scalac_path` is
/// given), run it, and compare with the expected output (scalac's).
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    };
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    assert_eq!(run_java(&cp), expected, "stdout mismatch for {name}");
    let _ = fs::remove_dir_all(&dir);
}

fn scalac_runs(name: &str) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs(name, Some(&sc));
}

/// `src` must be rejected by scala-rs with a diagnostic containing `want`,
/// and -- when scalac is present -- by scalac with the same text.
fn both_reject(tag: &str, src: &str, want: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let o = Command::new(bin())
        .arg("compile")
        .arg(&file)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!o.status.success(), "{tag}: scala-rs accepted it");
    assert!(err.contains(want), "{tag}: expected `{want}` in:\n{err}");
    if let Some(sc) = scalac() {
        let o = Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(&file)
            .output()
            .expect("run scalac");
        let err = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(!o.status.success(), "{tag}: scalac accepted it");
        assert!(
            err.contains(want),
            "{tag}: scalac says something else:\n{err}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------ var x: T = _

#[test]
fn libdecl_defaultinit_runs() {
    check_runs("libdecl_defaultinit", None);
}

#[test]
fn scalac_agrees_libdecl_defaultinit() {
    scalac_runs("libdecl_defaultinit");
}

/// The private runtime (`--no-scala-library`) emits the same constructor:
/// no store for a `var x: T = _`, so a value written through an overridden
/// method from the superclass constructor survives.
#[test]
fn libdecl_defaultinit_keeps_superclass_writes_without_the_library() {
    let dir = tmp_dir("nolib");
    let file = dir.join("nolib.scala");
    fs::write(
        &file,
        r#"
abstract class Base { init(); def init(): Unit }
class Sub extends Base {
  var x: Int = _
  var s: String = _
  var y: Int = 0
  def init(): Unit = { x = 5; s = "set"; y = 7 }
}
object Main {
  def main(args: Array[String]): Unit = {
    val b = new Sub
    println(b.x)
    println(b.s)
    println(b.y)
  }
}
"#,
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let o = Command::new(bin())
        .arg("compile")
        .arg(&file)
        .args(["-d", out.to_str().unwrap(), "--no-scala-library"])
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(run_java(out.to_str().unwrap()), "5\nset\n0\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn libdecl_default_init_is_refused_where_scalac_refuses_it() {
    both_reject(
        "valwild",
        "class A { val x: Int = _ }\n",
        "unbound placeholder parameter",
    );
    both_reject(
        "tparamval",
        "class A[T] { val x: T = _ }\n",
        "unbound placeholder parameter",
    );
    both_reject(
        "lazywild",
        "class A { lazy val x: Int = _ }\n",
        "unbound placeholder parameter",
    );
    both_reject(
        "patwild",
        "class A { var (a, b): (Int, Int) = _ }\n",
        "unbound placeholder parameter",
    );
    both_reject(
        "localwild",
        "object M { def f(): Int = { var x: Int = _; x } }\n",
        "local variables must be initialized",
    );
    both_reject(
        "localwildref",
        "object M { def f(): Unit = { var s: String = _; println(s) } }\n",
        "local variables must be initialized",
    );
    // SIP-23 (`neg/sip23-uninitialized-2`): a literal type has no default.
    both_reject(
        "literalwild",
        "class C { var f2: 1 = _ }\n",
        "default initialization prohibited for literal-typed vars",
    );
}

// ------------------------------------------- constructor prototypes

#[test]
fn libdecl_ctorproto_runs() {
    check_runs("libdecl_ctorproto", None);
}

#[test]
fn scalac_agrees_libdecl_ctorproto() {
    scalac_runs("libdecl_ctorproto");
}

/// An *overloaded* constructor gives its arguments no expected type, in nsc
/// as here: `Array(x)` still searches `ClassTag[A]`.
#[test]
fn libdecl_overloaded_ctor_gives_no_prototype() {
    both_reject(
        "twoctors",
        r#"
class Two(val a: Array[Any]) { def this(n: Int) = this(Array.empty[Any]) }
object M { def mk[A](x: A): Array[Any] = new Two(Array(x)).a }
"#,
        "No ClassTag available for A",
    );
}

// ------------------------------------------------ source Predef types

#[test]
fn libdecl_predef_runs() {
    check_runs("libdecl_predef", None);
}

#[test]
fn scalac_agrees_libdecl_predef() {
    scalac_runs("libdecl_predef");
}

// ------------------------------------------------------------- misc

#[test]
fn libdecl_misc_runs() {
    check_runs("libdecl_misc", None);
}

#[test]
fn scalac_agrees_libdecl_misc() {
    scalac_runs("libdecl_misc");
}

/// A self alias names `this` inside its own template and nothing else: it is
/// not a member of the class, and not inherited by a subclass. Selecting it
/// compiled and then threw `NoSuchMethodError: F.self()`.
#[test]
fn libdecl_self_alias_is_not_a_member() {
    both_reject(
        "selfsel",
        r#"
trait F { self => def g: Int = 1 }
class C extends F
object M { def f(x: F): Any = x.self }
"#,
        "value self is not a member of F",
    );
    both_reject(
        "selfinh",
        "trait F { self => def g: Int = 1 }\nclass C extends F { def h: Int = self.g }\n",
        "not found: value self",
    );
}

/// nsc's `isVariableOrGetter`: only a *parameterless*, non-stable getter with
/// a `name_=` beside it takes `op=`.
#[test]
fn libdecl_op_assign_needs_a_var_getter() {
    both_reject(
        "parens",
        r#"
class C {
  private[this] var _s = 0
  def size() = _s
  def size_=(s: Int): Unit = _s = s
  def inc(): Unit = { size += 1 }
}
"#,
        "value += is not a member of Int",
    );
    both_reject(
        "nosetter",
        "class C { private[this] var _s = 0; def size = _s; def inc(): Unit = { size += 1 } }\n",
        "value += is not a member of Int",
    );
    both_reject(
        "stableval",
        "class C { val size = 1; def size_=(s: Int): Unit = (); def inc(): Unit = { size += 1 } }\n",
        "value += is not a member of Int",
    );
}

/// Folding `Object` to `Any` is for a *Java* member only: between two Scala
/// methods `Any` and `AnyRef` are two overloads, and a Java `Object`
/// parameter is still not a `String` one. Now that the Java override is seen,
/// leaving out `override` is refused as scalac refuses it.
#[test]
fn libdecl_java_object_override_is_not_a_wildcard() {
    both_reject(
        "strparam",
        r#"
class MyList extends java.util.AbstractList[String] {
  def get(i: Int): String = "x"
  def size: Int = 1
  override def contains(o: String): Boolean = true
}
"#,
        "method contains overrides nothing",
    );
    both_reject(
        "scalaany",
        "class A { def f(x: Any): Int = 1 }\nclass B extends A { override def f(x: AnyRef): Int = 2 }\n",
        "method f overrides nothing",
    );
    both_reject(
        "needsoverride",
        r#"
class MyList extends java.util.AbstractList[String] {
  def get(i: Int): String = "x"
  def size: Int = 1
  def contains(o: AnyRef): Boolean = true
}
"#,
        "`override` modifier required",
    );
}

/// `Array` is invariant: an `Object[]` parameter takes `Array[AnyRef]` and
/// `Array[Any]`, never `Array[String]`.
#[test]
fn libdecl_object_array_param_is_still_invariant() {
    both_reject(
        "strarr",
        "object M { def f(): Unit = java.util.Arrays.fill(Array(\"a\"), \"b\") }\n",
        "fill",
    );
}
