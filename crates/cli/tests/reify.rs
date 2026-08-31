//! Calling a member on the class that actually declares it.
//!
//! A Scala trait compiles to a JVM interface, and an interface cannot extend a
//! class: `trait Cap extends Rig` loses `Rig` from its class-file header, and
//! only the pickle still records it. Three things follow, and all three used to
//! be wrong for a class arriving on `-cp`:
//!
//! 1. the trait's own members need `invokeinterface`, not `invokevirtual`
//!    (`IncompatibleClassChangeError` otherwise);
//! 2. a member inherited from a `-cp` parent has to be *found* at all, which
//!    needs the parents the class-file header names;
//! 3. a member the pickle inherits from a class the header cannot reach has to
//!    be called on the class that declares it, after a `checkcast` to it --
//!    `NoSuchMethodError` otherwise.
//!
//! This is exactly the shape `scala.reflect.api.JavaUniverse` has: it is an
//! interface, `Constant()` is declared on `scala.reflect.api.Constants`, and
//! the only path between them runs through the abstract class
//! `scala.reflect.api.Universe`. nsc emits
//! `checkcast scala/reflect/api/Constants` followed by `invokeinterface`; so
//! must we, or no macro implementation can build a tree at run time. See
//! `docs/macros.md`.

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
        "scala-rs-reify-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() {
            return Some(PathBuf::from("scalac"));
        }
    }
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    out
}

fn run_main(out: &Path, cp_extra: &[&Path]) -> String {
    let mut cp = out.display().to_string();
    for e in cp_extra {
        cp.push(':');
        cp.push_str(&e.display().to_string());
    }
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The bytes of a class file, with everything unprintable blanked, so a
/// constant-pool entry can be searched for by name.
fn constant_pool_text(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    bytes
        .iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' })
        .collect()
}

/// `tests/fixtures/reify.scala`: one compilation unit, so the declaring class
/// is known from the source. Runs against the private runtime and against the
/// real jar, and both must print `expected/reify.txt` -- which is what scalac
/// 2.13.16 prints for the same file.
#[test]
fn reify_trait_over_class_dispatches_to_the_declaring_class() {
    if !java_available() {
        return;
    }
    let exp = expected_stdout("reify");

    let out = compile_fixture_with("reify", &["--no-scala-library"]);
    assert_eq!(run_main(&out, &[]), exp, "private runtime");
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip reify library run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("reify", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(run_main(&out, &[&jar]), exp, "scala-library");
    // The interface member is called on the interface, the class member on the
    // class. Both names have to be in `Main$`'s constant pool.
    let pool = constant_pool_text(&out.join("Main$.class"));
    assert!(pool.contains("Cap"), "Main$ never names the trait: {pool}");
    assert!(pool.contains("Rig"), "Main$ never names the class: {pool}");
    let _ = fs::remove_dir_all(&out);
}

/// A name neither the trait nor its class declares is still an error.
#[test]
fn reify_bad_unknown_member_of_a_trait_is_an_error() {
    let src = fixtures_dir().join("reify_bad.scala");
    let out = tmp_dir("reify_bad");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(!output.status.success(), "expected reify_bad to fail");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains("value notDeclaredAnywhere is not a member of Cap"),
        "unexpected diagnostics: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

const LIB_SRC: &str = r#"package rlib

abstract class Rig {
  def tag(): String = "rig"
}
trait Cap extends Rig {
  def both(): String = tag() + "/" + tag()
}
class Gear extends Rig with Cap

object Shop {
  def mk(): Cap = new Gear
  def gear(): Gear = new Gear
}
"#;

const USER_SRC: &str = r#"object Main {
  def main(args: Array[String]): Unit = {
    val c: rlib.Cap = rlib.Shop.mk()
    println(c.both())
    val g: rlib.Gear = rlib.Shop.gear()
    println(g.tag())
    println(g.both())
  }
}
"#;

/// A trait read back from `-cp` is an interface, and a member inherited from a
/// `-cp` parent is visible.
///
/// `c.both()` is the interface case: before the class-file header's
/// `ACC_INTERFACE` was recorded the call went out as `invokevirtual` and the
/// JVM answered `IncompatibleClassChangeError`. `g.tag()` is the inheritance
/// case: `Gear extends Rig` is in `Gear`'s header, and without reading it the
/// member was not found at all.
///
/// The library is built by scala-rs itself, so the test needs no scalac.
#[test]
fn reify_classpath_trait_is_an_interface_and_inherits() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: jar not obtainable");
        return;
    };
    let dir = tmp_dir("cp");
    let lib_src = dir.join("rlib.scala");
    let user_src = dir.join("user.scala");
    fs::write(&lib_src, LIB_SRC).unwrap();
    fs::write(&user_src, USER_SRC).unwrap();
    let lib_out = dir.join("lib");
    let user_out = dir.join("user");

    let status = Command::new(bin())
        .args([
            "compile",
            lib_src.to_str().unwrap(),
            "-d",
            lib_out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("compile rlib");
    assert!(status.success(), "compiling the library failed");

    let status = Command::new(bin())
        .args([
            "compile",
            user_src.to_str().unwrap(),
            "-d",
            user_out.to_str().unwrap(),
            "-cp",
            lib_out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("compile user");
    assert!(status.success(), "compiling against the library failed");

    assert_eq!(
        run_main(&user_out, &[&lib_out, &jar]),
        "rig/rig\nrig\nrig/rig\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

// --- the reflect universe, and quasiquotes on top of it -------------------

fn scala_reflect_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    cached.is_file().then_some(cached)
}

/// Compile a fixture against scala-reflect.jar and run it.
fn run_reflect_fixture(name: &str, jar: &Path, reflect: &Path) -> String {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            reflect.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed");
    let got = run_main(&out, &[reflect, jar]);
    let _ = fs::remove_dir_all(&out);
    got
}

/// Building a reflect tree on the *runtime* universe runs.
///
/// This is what `Symbol::declaring_class` buys: `Constant()` is declared on
/// `scala.reflect.api.Constants`, which `api.JavaUniverse` does not implement
/// in bytecode, so the call has to name `Constants`. Before, it named
/// `JavaUniverse` and the first invocation was a `NoSuchMethodError` -- which
/// no amount of typechecking would have caught.
#[test]
fn reify_runtime_universe_builds_a_tree() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip: scala-library / scala-reflect not obtainable");
        return;
    };
    let dir = tmp_dir("universe");
    let src = dir.join("universe.scala");
    fs::write(
        &src,
        r#"object Main {
  def main(args: Array[String]): Unit = {
    val u = scala.reflect.runtime.universe
    val rs = u.internal.reificationSupport
    val id = rs.SyntacticTermIdent(u.TermName("x"), false)
    println(id)
    println(rs.SyntacticSelectTerm(id, u.TermName("len")))
    println(u.Literal(u.Constant(42)))
  }
}
"#,
    )
    .unwrap();
    let out = dir.join("out");
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            reflect.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compiling against scala-reflect failed");
    assert_eq!(run_main(&out, &[&reflect, &jar]), "x\nx.len\n42\n");
    let _ = fs::remove_dir_all(&dir);
}

/// `tests/fixtures/reify_qq.scala`: quasiquotes, reified.
///
/// The expected output is what real scalac 2.13.16 prints for the same file,
/// and the test re-checks that here whenever scalac is obtainable rather than
/// trusting the recorded copy.
#[test]
fn reify_qq_quasiquotes_build_the_same_trees_as_scalac() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip: scala-library / scala-reflect not obtainable");
        return;
    };
    let want = expected_stdout("reify_qq");
    assert_eq!(run_reflect_fixture("reify_qq", &jar, &reflect), want);

    let Some(scalac) = find_scalac() else {
        eprintln!("skip the scalac half: scalac 2.13 not obtainable");
        return;
    };
    let ref_out = tmp_dir("reify_qq-scalac");
    let out = Command::new(&scalac)
        .args([
            "-cp",
            reflect.to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
            fixtures_dir().join("reify_qq.scala").to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        out.status.success(),
        "scalac rejected reify_qq.scala: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run_main(&ref_out, &[&reflect, &jar]),
        want,
        "scala-rs and scalac build different trees"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// Every form reification does not build is an error naming the form.
///
/// Silently building the wrong tree would be worse than not compiling: the
/// call site would typecheck against a tree nobody wrote.
#[test]
fn reify_qq_bad_names_every_form_it_cannot_build() {
    let (Some(jar), Some(reflect)) = (scala_library_jar(), scala_reflect_jar()) else {
        eprintln!("skip: scala-library / scala-reflect not obtainable");
        return;
    };
    let src = fixtures_dir().join("reify_qq_bad.scala");
    let out = tmp_dir("reify_qq_bad");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            reflect.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(!output.status.success(), "expected reify_qq_bad to fail");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "an `if` without an `else` is not reified yet",
        "a `..$` splice mixed with ordinary arguments is not reified yet",
        "cannot stand for a single tree",
        "docs/macros.md",
    ] {
        assert!(err.contains(needle), "expected {needle:?} in: {err}");
    }
    let _ = fs::remove_dir_all(&out);
}
