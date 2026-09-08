//! E2E tests for the `agent/intrinsicqual` slice. Two defects, both left
//! behind by slices that fixed their neighbours.
//!
//! **1. `identity` / `locally` / `implicitly` lost their qualifier.**
//! `agent/sysout` fixed the print intrinsic, which `gen_apply` dispatched on
//! the *name* alone, and named the one immediately below it as the same root:
//!
//! ```text
//! if ctx.library_abi
//!     && (fun.name() == Some("identity")
//!         || fun.name() == Some("locally")
//!         || fun.name() == Some("implicitly")
//!         || …)
//! ```
//!
//! `gen_predef_poly` discards the qualifier and every argument but the first,
//! emitting `scala/Predef$.<name>:(Ljava/lang/Object;)Ljava/lang/Object;` --
//! the identity function. So a user-defined `identity` gave back its own
//! argument, a receiver expression was never evaluated, and a second argument
//! was never even constructed. It compiles, it verifies, and only running it
//! says otherwise. The fix (`gen_expr::predef_poly_name`) restricts the
//! name-only fallback to a call with no symbol at all, exactly as
//! `unresolved_print` does.
//!
//! **2. A private or protected constructor was not access-checked.**
//! `agent/accessmsg` rebuilt the access diagnostic the way nsc's `AccessError`
//! builds it, including the `isClassConstructor` branch (`in <owner>` rather
//! than `as a member of <prefix>`), and found that branch unreachable:
//! `new C(…)` is not a member selection, so `type_select`'s access check never
//! saw an `<init>`, and the primary constructor's symbol did not even carry
//! the modifier the class wrote. Four corpus `neg` tests name it:
//! `neg/sensitive`, `neg/t4987`, `neg/t6601`, `neg/protected-constructors`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.
//! Fixture prefix: `intrinsicqual_`.

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
        "scala-rs-intrinsicqual-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

/// Compile a source string as a file of its own; panic with the diagnostics.
fn compile(tag: &str, src: &str, extra: &[&str]) -> PathBuf {
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        file.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {tag} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// Compile a source string that is expected to be *rejected*, and return the
/// diagnostics.
fn compile_expecting_errors(tag: &str, src: &str, extra: &[&str]) -> String {
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        file.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "{tag} was accepted; scalac 2.13.16 rejects it:\n{text}"
    );
    text
}

/// Compile a fixture file by name and return the output directory.
fn compile_fixture(name: &str, extra: &[&str]) -> PathBuf {
    let out = tmp_dir(name);
    let src = fixtures_dir().join(format!("{name}.scala"));
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "java -Xverify:all {main} failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(stderr, "", "nothing belongs on stderr:\n{stderr}");
    stdout
}

/// Both linking modes, as `(tag, compiler flags, runtime classpath extra)`.
/// The private-runtime mode is not decoration here: the defect was inside
/// `if ctx.library_abi`, so `--no-scala-library` is the half that was already
/// right and has to stay right.
fn both_modes() -> Vec<(&'static str, Vec<String>, Option<String>)> {
    let mut modes: Vec<(&'static str, Vec<String>, Option<String>)> =
        vec![("private", vec!["--no-scala-library".into()], None)];
    if let Some(jar) = scala_library_jar() {
        let jar_s = jar.to_str().unwrap().to_string();
        modes.push((
            "jar",
            vec!["--scala-library".into(), jar_s.clone()],
            Some(jar_s),
        ));
    }
    modes
}

// ------------------------------------------------------- defect 1: the poly
// -------------------------------------------------------- intrinsic's qualifier

/// The fixture, in both modes, against real scalac 2.13.16's own output.
///
/// It holds both halves in one program on purpose: the user's `identity` /
/// `locally` / `implicitly` must run, and `Predef`'s must stay the intrinsic.
/// The `C-` / `O-` prefixes are what tell them apart -- before the fix every
/// line of the first group printed the bare argument instead.
#[test]
fn the_poly_fixture_matches_scalac_in_both_modes() {
    if !java_available() {
        return;
    }
    let name = "intrinsicqual_predef";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile_fixture(name, &flags);
        let stdout = run_java(&out, cp.as_deref(), "Main");
        assert_eq!(stdout, expected_stdout(name), "[{tag}] stdout for {name}");
        let _ = fs::remove_dir_all(&out);
    }
}

/// The smallest statement of the defect: a method that merely *has* the name.
///
/// Before the fix this printed `a` / `b` / `c` -- `gen_predef_poly` had
/// replaced all three with `scala.Predef.identity`, which returns its
/// argument.
#[test]
fn a_user_defined_identity_is_not_predefs() {
    if !java_available() {
        return;
    }
    let src = r#"
object Helper {
  def identity(x: String): String = "O-identity:" + x
  def locally(x: String): String = "O-locally:" + x
  def implicitly(x: String): String = "O-implicitly:" + x
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Helper.identity("a"))
    println(Helper.locally("b"))
    println(Helper.implicitly("c"))
  }
}
"#;
    let expect = "O-identity:a\nO-locally:b\nO-implicitly:c\n";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("userpoly-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), expect, "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// The half a return value cannot show: the *receiver* and the arguments past
/// the first were not evaluated at all.
///
/// `gen_predef_poly` reads `args(0)` and drops the rest, and loads
/// `scala/Predef$.MODULE$` in place of whatever the program wrote -- the same
/// shape as `slick/util/TreePrinter`'s `print(n, out)` under the `println`
/// defect, where the `PrintWriter` was never constructed. Before the fix the
/// trace read `<arg:i>` alone.
#[test]
fn the_qualifier_and_the_later_arguments_are_evaluated() {
    if !java_available() {
        return;
    }
    let src = r#"
class Poly(tag: String) {
  def identity(a: String, b: String): String = "C:" + tag + ":" + a + "|" + b
}

object Main {
  var trace: String = ""
  def mk(tag: String): Poly = { trace = trace + "<mk:" + tag + ">"; new Poly(tag) }
  def side(x: String): String = { trace = trace + "<arg:" + x + ">"; x }

  def main(args: Array[String]): Unit = {
    println(mk("m").identity(side("i"), side("j")))
    println(trace)
  }
}
"#;
    let expect = "C:m:i|j\n<mk:m><arg:i><arg:j>\n";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("polyside-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), expect, "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// The exact analogue of the `agent/sysout` reproduction, one intrinsic over.
///
/// A program whose own sources define `scala.Predef` emits its own
/// `scala/Predef$.class`, which precedes the jar's on the classpath. With a
/// *monomorphic* `identity` the hijacked call names a descriptor that is not
/// there, and before the fix this compiled, verified, and then threw
/// `NoSuchMethodError: 'java.lang.Object scala.Predef$.identity(java.lang.Object)'`.
#[test]
fn a_source_predef_identity_is_the_ordinary_call_it_is() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile(
        "srcpredef",
        r#"
package scala {
  object Predef {
    type String = java.lang.String
    def identity(a: java.lang.String): java.lang.String = "P:" + a
  }
}

object Main {
  def main(args: Array[String]): Unit = java.lang.System.out.println(identity("x"))
}
"#,
        &["--scala-library", jar_s],
    );
    assert_eq!(run_java(&out, Some(jar_s), "Main"), "P:x\n");
    let _ = fs::remove_dir_all(out.parent().unwrap());
}

// ------------------------------------------------- what must not have moved

/// `Predef`'s own three are still the intrinsic, spelled every way, in both
/// modes. This is the guard: a fix that keyed on the tree shape would lose
/// `scala.Predef.identity(x)`, and one that keyed on the owner's internal
/// name would claim a source `Predef`'s members back.
#[test]
fn predef_identity_locally_implicitly_still_work() {
    if !java_available() {
        return;
    }
    let src = r#"
object Main {
  implicit val ev: String = "ev"
  def main(args: Array[String]): Unit = {
    println(identity("k"))
    println(Predef.identity("l"))
    println(scala.Predef.identity("m"))
    println(locally { "n" })
    println(locally("o"))
    println(implicitly[String])
    println(identity(7))
    println(locally(8))
    // `println(identity(()))` belongs here and is left out: it is a
    // pre-existing `VerifyError: Operand stack underflow` on an unmodified
    // build of the branch point too. `gen_predef_poly` pops its own result
    // when the result type is `Unit`, which is right in statement position
    // and wrong when the value is an argument. Reported, not fixed here.
  }
}
"#;
    let expect = "k\nl\nm\nn\no\nev\n7\n8\n";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("predefpoly-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), expect, "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// `implicitly[T]` is how the library summons a witness, so it has to keep
/// working against the real jar's type classes as well as against a plain
/// `implicit val`.
#[test]
fn implicitly_still_summons_a_library_witness() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile(
        "implicitly-jar",
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    println(implicitly[Ordering[Int]].compare(1, 2))
    println(identity(List(1, 2, 3)).sum)
  }
}
"#,
        &["--scala-library", jar_s],
    );
    assert_eq!(run_java(&out, Some(jar_s), "Main"), "-1\n6\n");
    let _ = fs::remove_dir_all(out.parent().unwrap());
}

// ------------------------------------ defect 2: the constructor access check

/// The legal neighbours, in both modes, against real scalac's own output.
///
/// This is the fixture that matters for a rejection rule: the check must not
/// refuse a companion building its own class, a subclass writing `extends
/// Prot(…)`, a `private[p]` constructor used inside `p`, or a class whose
/// *other* constructor is the accessible one. An unmodified build of the
/// branch point compiles and runs it to the same output.
#[test]
fn the_ctor_fixture_runs_and_matches_scalac() {
    if !java_available() {
        return;
    }
    let name = "intrinsicqual_ctor";
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile_fixture(name, &flags);
        let stdout = run_java(&out, cp.as_deref(), "iqpkg.Main");
        assert_eq!(stdout, expected_stdout(name), "[{tag}] stdout for {name}");
        let _ = fs::remove_dir_all(&out);
    }
}

/// The rejection fixture: three programs scalac 2.13.16 refuses on this very
/// file, at these very lines, with these very sentences. All three compiled
/// before this slice.
#[test]
fn the_ctor_bad_fixture_is_rejected_with_nscs_wording() {
    let name = "intrinsicqual_ctor_bad";
    let src = fs::read_to_string(fixtures_dir().join(format!("{name}.scala"))).unwrap();
    for (tag, extra, _) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let text = compile_expecting_errors(&format!("ctorbad-{tag}"), &src, &flags);
        for want in [
            "constructor Priv in class Priv cannot be accessed in object Outsider from object Outsider in package iqbad",
            "constructor Prot in class Prot cannot be accessed in object Outsider from object Outsider in package iqbad",
            "constructor Prot in class Prot cannot be accessed in class Sub from class Sub in package iqbad",
        ] {
            assert!(text.contains(want), "[{tag}] missing {want:?} in:\n{text}");
        }
    }
}

/// The three corpus `neg` tests this closes, verbatim, each checked against
/// the line and the sentence in its own `.check` file.
///
/// `neg/t6601` is the fourth and is **not** here: it is a separate
/// compilation, and the privacy of a constructor does not survive the round
/// trip through a class file yet. See `docs/comparison-with-scalac.md`.
#[test]
fn the_corpus_negatives_are_rejected_at_nscs_line_and_text() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let cases: [(&str, &str, &str); 3] = [
        (
            // neg/sensitive
            "sensitive",
            r#"class Certificate{}

object Admin extends Certificate;

class SecurityViolationException extends Exception

object Sensitive {
  def makeSensitive(credentials: Certificate): Sensitive =
    if (credentials == Admin) new Sensitive()
    else throw new SecurityViolationException
}
class Sensitive private () {
}

object Attacker {
  val x = Sensitive.makeSensitive(null)
  val y = new Sensitive()
}
"#,
            "constructor Sensitive in class Sensitive cannot be accessed in object Attacker from object Attacker",
        ),
        (
            // neg/t4987
            "t4987",
            "class Foo2 private (a: Int, b: Int)\nobject Bar2 { new Foo2(0, 0) }\n",
            "constructor Foo2 in class Foo2 cannot be accessed in object Bar2 from object Bar2",
        ),
        (
            // neg/protected-constructors, the `.check`'s line 18
            "protected-constructors",
            r#"package dingus {
  class Foo1() { protected def this(name: String) = this() }
  class Foo2 protected (name: String) { }
  object Ding {
    protected class Foo3(name: String) { }
  }
}

package hungus {
  import dingus._

  object P {
    class Bar1 extends Foo1("abc")
    class Bar2 extends Foo2("abc")

    val foo2 = new Foo2("abc")
  }
}
"#,
            "constructor Foo2 in class Foo2 cannot be accessed in object P from object P in package hungus",
        ),
    ];
    for (name, src, want) in cases {
        let text =
            compile_expecting_errors(&format!("corpus-{name}"), src, &["--scala-library", jar_s]);
        assert!(text.contains(want), "{name}: missing {want:?} in:\n{text}");
    }
}

/// nsc drops inaccessible alternatives *before* overload resolution, so a
/// class that also has a constructor this site may call is resolved among
/// those. Our pick is made first, so this is where the check has to decline
/// to speak -- reporting here would refuse a program scalac accepts.
#[test]
fn an_inaccessible_alternative_does_not_hide_an_accessible_one() {
    if !java_available() {
        return;
    }
    let src = r#"
class Mixed(val v: String) {
  private def this(len: Int) = this("len" + len.toString)
}

object Main {
  def main(args: Array[String]): Unit = println(new Mixed("direct").v)
}
"#;
    for (tag, extra, cp) in both_modes() {
        let flags: Vec<&str> = extra.iter().map(String::as_str).collect();
        let out = compile(&format!("mixedctor-{tag}"), src, &flags);
        assert_eq!(run_java(&out, cp.as_deref(), "Main"), "direct\n", "[{tag}]");
        let _ = fs::remove_dir_all(out.parent().unwrap());
    }
}

/// The one byte of bytecode this slice moves, and why it is scalac agreeing.
///
/// Giving the primary constructor's symbol the modifier the class wrote also
/// puts it in the `ScalaSignature`, which is what nsc pickles too. The proof
/// that it is right is that **real scalac 2.13.16**, reading our class file,
/// now refuses the call it used to accept -- our pickle had been telling it
/// the constructor was public.
#[test]
fn real_scalac_reads_the_constructor_as_private_from_our_classfile() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let scalac = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if !scalac.is_file() {
        eprintln!("skip: scalac 2.13.16 not obtainable");
        return;
    }
    let jar_s = jar.to_str().unwrap();
    let out = compile(
        "pickled-private-ctor",
        "class SepPriv private (val s: String)\n",
        &["--scala-library", jar_s],
    );
    let client = out.join("SepUse.scala");
    fs::write(&client, "class SepUse {\n  new SepPriv(\"x\")\n}\n").unwrap();
    let output = Command::new(&scalac)
        .args([
            "-classpath",
            &format!("{jar_s}:{}", out.display()),
            "-d",
            out.to_str().unwrap(),
            client.to_str().unwrap(),
        ])
        .output()
        .expect("run scalac");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("constructor SepPriv in class SepPriv cannot be accessed"),
        "scalac should refuse the private constructor read from our class file:\n{text}"
    );
    let _ = fs::remove_dir_all(out.parent().unwrap());
}
