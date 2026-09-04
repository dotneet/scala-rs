//! E2E tests for the `agent/aliaslookup` slice: two ways a jar's members go
//! missing or double up, both found while chasing gitbucket's ~220 slick
//! `Session` diagnostics (`docs/gitbucket.md`).
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. The miniature library is `alib`, modelled on
//! `slick.jdbc.JdbcBackend` and on scalatra's `ScalatraContext` /
//! `DynamicScope`. Neither shape reproduces without a jar: a `type` alias
//! leaves no trace in the bytecode, and a pickled member's owner is what the
//! second one turns on.
//!
//! 1. **`object X extends X` lost the trait's own members.**
//!    `SigCache::lin_of` deduplicated its linearization by class *name*, and a
//!    module class carries the same dotted name as its companion. So for
//!
//!    ```scala
//!    trait JdbcBackend extends BaseBackend { type Database = DatabaseDef }
//!    object JdbcBackend extends JdbcBackend
//!    ```
//!
//!    the walk from `JdbcBackend$` put the trait second and then dropped it as
//!    a duplicate of the head. `import JdbcBackend.{Database => …}` found only
//!    `BaseBackend`'s abstract `type Database`, never the alias -- which is
//!    what real scalac calls "`JdbcBackend.Database` (which expands to)
//!    `DatabaseDef`". Slick's cake traits are written this way throughout.
//!
//! 2. **One inherited implicit was offered twice.** `implicits_in_scope` walks
//!    the parents and collects each base's members separately, so a
//!    declaration in one trait stood beside the definition that implements it
//!    in another. nsc's `findMember` sees a single member; we saw two of the
//!    same name and the same type and reported `ambiguous implicit: ctx, ctx`.
//!    scalatra declares `implicit def request: HttpServletRequest` in
//!    `ScalatraContext` and defines it in `DynamicScope` -- unrelated traits,
//!    both mixed into `ScalatraFilter` -- and 169 gitbucket diagnostics were
//!    that pair, with ~219 more downstream of them.
//!
//! `DefFirst` and `DeclFirst` mix the two traits in both orders, because the
//! rule that keeps one of them is "the linearization reaches it first" and
//! both orders have to work.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-aliaslookup-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    None
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn jar_tool() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(home).join("bin/jar");
        if p.is_file() {
            return Some(p);
        }
    }
    let out = Command::new("which").arg("jar").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    p.is_file().then_some(p)
}

fn pack_jar(classes: &Path, dest: &Path) {
    let tool = jar_tool().expect("jar tool");
    let out = Command::new(tool)
        .args([
            "cf",
            dest.to_str().unwrap(),
            "-C",
            classes.to_str().unwrap(),
        ])
        .arg(".")
        .output()
        .expect("run jar");
    assert!(
        out.status.success(),
        "jar failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_against(out: &Path, jar: &Path, src: &Path, cp: &Path) -> (bool, String) {
    let output = Command::new(bin())
        .arg("compile")
        .arg(src)
        .args(["-d", out.to_str().unwrap()])
        .args(["-cp", cp.to_str().unwrap()])
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

/// The two library shapes, in one file so the fixture costs one scalac run.
const A_LIB: &str = r#"
package alib

class Ctx(val tag: String)

// `org.scalatra.ScalatraContext`: declares the implicit and nothing else.
trait CtxScope {
  implicit def ctx: Ctx
}

// `org.scalatra.DynamicScope`: defines the same member, and is *not* a
// subclass of the trait that declares it.
trait DynScope {
  implicit def ctx: Ctx = new Ctx("dyn")
}

trait DefFirst extends CtxScope with DynScope
trait DeclFirst extends DynScope with CtxScope

// Two implicits of one type that really are ambiguous, so the rule that
// merges a declaration with its definition cannot merge these as well.
trait OneCtx { implicit def one: Ctx = new Ctx("one") }
trait TwoCtx { implicit def two: Ctx = new Ctx("two") }

object U {
  def useCtx(implicit c: Ctx): String = c.tag
}

// `slick.jdbc.JdbcBackend`: an object extending the trait of its own name,
// where the trait turns an inherited abstract type into an alias.
class DatabaseDef { def label: String = "db" }

trait BaseBackend {
  type Database
  def open(): Database
}

trait JdbcBackend extends BaseBackend {
  type Database = DatabaseDef
  def open(): Database = new DatabaseDef
}

object JdbcBackend extends JdbcBackend
"#;

/// gitbucket's `TransactionFilter` renames the alias exactly like this.
const A_USER: &str = r#"
import alib.JdbcBackend.{Database => MyDatabase}

class AliasUser {
  val db: MyDatabase = alib.JdbcBackend.open()
  def dbLabel: String = db.label
}

class DefFirstUser extends alib.DefFirst { def tag: String = alib.U.useCtx }
class DeclFirstUser extends alib.DeclFirst { def tag: String = alib.U.useCtx }

object Main {
  def main(args: Array[String]): Unit = {
    println(new AliasUser().dbLabel)
    println(new DefFirstUser().tag)
    println(new DeclFirstUser().tag)
  }
}
"#;

/// Reading the alias is not the same as accepting anything under its name,
/// and merging a declaration with its definition is not the same as merging
/// two implicits. Real scalac reports both of these too.
const A_USER_BAD: &str = r#"
import alib.JdbcBackend.{Database => MyDatabase}

class BadAlias {
  val db: MyDatabase = "not a database"
}

class BadAmbiguous extends alib.OneCtx with alib.TwoCtx {
  def tag: String = alib.U.useCtx
}
"#;

const A_EXPECTED: &str = "db\ndyn\ndyn\n";

fn build_lib_jar(dir: &Path) -> PathBuf {
    let src = dir.join("alib.scala");
    fs::write(&src, A_LIB).unwrap();
    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();
    let scalac = self::scalac().expect("checked by caller");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&lib_out)
        .arg(&src)
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac failed on the miniature library:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lib_jar = dir.join("alib.jar");
    pack_jar(&lib_out, &lib_jar);
    lib_jar
}

/// A pickled alias declared by a trait its own companion extends, and an
/// inherited implicit declared by one trait and defined by another.
#[test]
fn a_companions_alias_and_a_doubled_inherited_implicit() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    if scalac().is_none() {
        eprintln!("skip: no scalac to write the pickle with");
        return;
    }
    if jar_tool().is_none() {
        eprintln!("skip: no `jar` tool");
        return;
    }
    let dir = tmp_dir("alias");
    let lib_jar = build_lib_jar(&dir);

    let user = dir.join("user.scala");
    fs::write(&user, A_USER).unwrap();
    let user_out = dir.join("userout");
    fs::create_dir_all(&user_out).unwrap();
    let (ok, msgs) = compile_against(&user_out, &jar, &user, &lib_jar);
    assert!(ok, "the fixture failed to compile against the jar:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let bad = dir.join("bad.scala");
    fs::write(&bad, A_USER_BAD).unwrap();
    let bad_out = dir.join("badout");
    fs::create_dir_all(&bad_out).unwrap();
    let (ok, msgs) = compile_against(&bad_out, &jar, &bad, &lib_jar);
    assert!(!ok, "expected both bad cases to be rejected:\n{msgs}");
    assert!(
        msgs.contains("required: DatabaseDef"),
        "expected the alias to be reported as the `DatabaseDef` it expands to, got:\n{msgs}"
    );
    assert!(
        msgs.contains("ambiguous implicit: one, two"),
        "expected two genuinely ambiguous implicits still to be ambiguous, got:\n{msgs}"
    );

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_jar.display());
        assert_eq!(
            run_java(&user_out, &cp),
            A_EXPECTED,
            "stdout mismatch for a_companions_alias_and_a_doubled_inherited_implicit"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The fixture is ordinary Scala, not a quirk of ours: real scalac compiles
/// the same two files, prints the same thing, and rejects the same two
/// programs.
#[test]
fn real_scalac_agrees_on_both_fixtures() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some(scalac) = scalac() else {
        eprintln!("skip: no scalac");
        return;
    };
    if !java_available() {
        eprintln!("skip: no java");
        return;
    }
    let dir = tmp_dir("alias-scalac");
    let lib = dir.join("alib.scala");
    let user = dir.join("user.scala");
    fs::write(&lib, A_LIB).unwrap();
    fs::write(&user, A_USER).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&out)
        .args([&lib, &user])
        .output()
        .expect("run scalac");
    assert!(
        result.status.success(),
        "scalac rejected the fixture:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(run_java(&out, jar.to_str().unwrap()), A_EXPECTED);

    let bad = dir.join("bad.scala");
    fs::write(&bad, A_USER_BAD).unwrap();
    let bad_out = dir.join("badout");
    fs::create_dir_all(&bad_out).unwrap();
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&bad_out)
        .arg("-cp")
        .arg(&out)
        .arg(&bad)
        .output()
        .expect("run scalac");
    let msgs = String::from_utf8_lossy(&result.stderr).into_owned();
    assert!(!result.status.success(), "scalac accepted the bad file");
    assert!(
        msgs.contains("(which expands to)") && msgs.contains("ambiguous implicit values"),
        "scalac reported something else:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}
