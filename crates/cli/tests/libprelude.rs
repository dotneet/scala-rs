//! E2E tests for the `agent/libprelude` slice: **members the prelude does not
//! declare.**
//!
//! The brief handed this slice one symptom -- `value & is not a member of
//! Boolean` -- and asked for an audit rather than a patch. The audit is
//! mechanical and is worth more than the symptom: `javap -p` over
//! `scala-library-2.13.16.jar` lists every instance member of `scala.Boolean`,
//! `Byte`, `Short`, `Char`, `Int`, `Long`, `Float`, `Double` and `Unit` --
//! **727** of them, counting each overload -- and a generated probe puts one
//! call to each through both compilers.
//!
//! Two probes, because a *missing* member and a *wrongly typed* one fail
//! differently:
//!
//! * the positive probe ascribes each call to the result type javap reports,
//!   so a member that is absent, or whose result is too narrow, is an error;
//! * the negative probe ascribes each call to the next *narrower* type, so a
//!   member whose result is too wide is an error. Real scalac rejects all 727
//!   of those and so must this compiler.
//!
//! Before this slice the positive probe drew **10** errors and the negative
//! probe none: `Boolean.&`, `Boolean.|`, `Boolean.^`, and `unary_+` on all
//! seven numeric classes. Every other declared member, and every result type,
//! already matched. After it, both probes agree with scalac exactly.
//!
//! Two of those ten cost the library measure 15 errors (`&` 9, `^` 6);
//! `tests/scalalib_measure.sh -no-specialization` goes `1111 / 155` to
//! `1096 / 154`.
//!
//! Adding `unary_+` then turned up a **second, pre-existing** defect that no
//! compile-only check in this repository could see. `x.unary_-` written out
//! in full -- as opposed to the prefix `-x` -- is a bare `Select`, and
//! `gen_select`'s intrinsic chain had no case for the five unary value-class
//! families. It fell through to `invoke_method`, emitted
//! `invokevirtual java.lang.Byte.unary_$minus()`, compiled, verified, and
//! threw `NoSuchMethodError` at run time. All nine members were affected
//! (`unary_-` and `unary_~` on Byte/Short/Char/Int/Long, `unary_-` on
//! Float/Double, `unary_!` on Boolean). `emit_prim_unary` is now shared by
//! both spellings.
//!
//! That is why the positive fixture **runs**. It also runs because `&` is not
//! a spelling of `&&`: `p & q` evaluates `q` unconditionally, so emitting the
//! short-circuiting form would type-check, pass the verifier, pass every
//! classfile check, and silently drop `q`'s side effects. Each operand in the
//! fixture appends to a trace, and the trace distinguishes them.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. Fixture prefix: `libprelude_`.

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
        "scala-rs-libprelude-{tag}-{}-{nanos}-{seq}",
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

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
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

/// Compile a fixture in one of the two modes and return its output directory.
fn compile_fixture(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// Compile a source string on its own. Returns whether it succeeded and the
/// combined diagnostics either way.
fn compile_src(tag: &str, src: &str, extra: &[&str]) -> (bool, String) {
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
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let ok = output.status.success();
    let _ = fs::remove_dir_all(&dir);
    (ok, text)
}

// ------------------------------------------------------ the members, running

/// `tests/fixtures/expected/libprelude_boolbit.txt` is real scalac 2.13.16's
/// own stdout for the same source, compiled against the same jar.
///
/// The two lines that matter most are
///
/// ```text
/// false &  true = false trace=LR
/// false && true = false trace=L
/// ```
///
/// Both operands run for `&`; only the left one runs for `&&`. A `&` that
/// emitted `gen_bool_and` would print `trace=L` on the first line and pass
/// every other check in this repository.
///
/// The private runtime backs all of it -- `iand`/`ior`/`ixor` and the unary
/// identity are plain instructions -- so nothing here is gated on
/// `library_abi` and this runs in `--no-scala-library` too.
#[test]
fn bitwise_and_unary_plus_run_on_the_private_runtime() {
    if !java_available() {
        return;
    }
    let name = "libprelude_boolbit";
    let out = compile_fixture(name, &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        expected_stdout(name),
        "stdout mismatch for {name} (--no-scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture, the same expected output, linked against the real jar.
#[test]
fn bitwise_and_unary_plus_run_against_the_jar() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let name = "libprelude_boolbit";
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout(name),
        "stdout mismatch for {name} (--scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The pre-existing defect the `unary_+` work uncovered, on its own.
///
/// `-x` and `x.unary_-` must produce the same bytes. Before this slice the
/// second spelling compiled to a virtual call on the *box* and every one of
/// these lines threw `NoSuchMethodError`.
#[test]
fn the_selected_spelling_of_a_unary_operator_runs() {
    if !java_available() {
        return;
    }
    let dir = tmp_dir("selunary");
    let file = dir.join("Main.scala");
    fs::write(
        &file,
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    val b: Byte = 3
    val s: Short = 4
    val c: Char = 'A'
    val i: Int = -5
    val l: Long = -6L
    val f: Float = -7.5f
    val d: Double = -8.25
    val p: Boolean = true
    println(b.unary_- + " " + b.unary_~ + " " + s.unary_- + " " + c.unary_~)
    println(i.unary_- + " " + i.unary_~ + " " + l.unary_- + " " + l.unary_~)
    println(f.unary_- + " " + d.unary_- + " " + p.unary_!)
  }
}
"#,
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let status = Command::new(bin())
        .args([
            "compile",
            file.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile failed");
    // Real scalac 2.13.16's output for the same source.
    assert_eq!(
        run_java(&out, None),
        "-3 -4 -4 -66\n5 4 6 5\n7.5 8.25 false\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

// --------------------------------------------------------------- the limits

/// Everything the audit says these classes do **not** declare. Real scalac
/// 2.13.16 gives eight errors for `libprelude_boolbit_bad.scala`; so must
/// this compiler, in both modes. Supplying `&` is only right if it stops
/// where nsc stops.
#[test]
fn members_that_do_not_exist_are_still_refused() {
    let name = "libprelude_boolbit_bad";
    let src = fixtures_dir().join(format!("{name}.scala"));
    let mut modes = vec![vec!["--no-scala-library".to_string()]];
    if let Some(j) = scala_library_jar() {
        modes.push(vec![
            "--scala-library".to_string(),
            j.to_str().unwrap().to_string(),
        ]);
    }
    for extra in modes {
        let out = tmp_dir(name);
        let output = Command::new(bin())
            .args([
                "compile",
                src.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .args(&extra)
            .output()
            .expect("run scala-rs compile");
        assert!(
            !output.status.success(),
            "expected {name} to be rejected with {extra:?}, but it compiled"
        );
        let err = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        for want in [
            "value unary_~ is not a member of Boolean",
            "value unary_- is not a member of Boolean",
            "value unary_+ is not a member of Boolean",
            "value & is not a member of Double",
        ] {
            assert!(
                err.contains(want),
                "expected `{want}` with {extra:?}:\n{err}"
            );
        }
        // `p & 1`, `p | 1`, `p ^ 1` and `1 & p`: the member exists, the
        // argument does not fit. Four sites, all diagnosed as overloads.
        assert_eq!(
            err.matches("no matching overload").count(),
            4,
            "expected four argument-type rejections with {extra:?}:\n{err}"
        );
        let _ = fs::remove_dir_all(&out);
    }
}

/// The bitwise three are `Boolean`-only. `1.0 & 2` is not a Scala expression
/// and neither is `1 & 2.0`; adding `Boolean.&` must not have widened the
/// numeric ones.
#[test]
fn bitwise_operators_still_need_integral_operands() {
    for (tag, src) in [
        (
            "dblbit",
            "object M { def f(a: Double, b: Int): Double = a & b }",
        ),
        (
            "fltbit",
            "object M { def f(a: Int, b: Float): Int = a ^ b }",
        ),
    ] {
        let (ok, err) = compile_src(tag, src, &["--no-scala-library"]);
        assert!(!ok, "expected {tag} to be rejected:\n{err}");
    }
}

// ------------------------------------- `eq`/`ne` under a universal trait

/// `lookup_member` finds `AnyRef`'s members by walking a *declared* parent,
/// and `rough_parents` supplies `AnyRef` only when the parent list is empty.
/// A class whose ancestry bottoms out in a **universal trait** therefore had
/// a chain ending at `Any`, and `t eq null` was `value eq is not a member` --
/// while the same program's `def conforms(t: T2[Int, Int]): AnyRef = t` was
/// accepted, so the compiler already agreed the receiver was a reference.
///
/// That is `src/library`'s whole `Tuple`/`Product`/`Iterator` family:
/// `scala.Equals` is `trait Equals extends scala.Any`. Real scalac 2.13.16
/// accepts `tests/fixtures/libprelude_anyref.scala` and its stdout is the
/// expected file.
///
/// Run, because `eq` is reference identity: `p eq q` is false where `p == q`
/// is true, so a fallback that resolved it to `==` would type-check, verify,
/// and print the wrong answer.
#[test]
fn eq_and_ne_on_a_universal_trait_run() {
    if !java_available() {
        return;
    }
    let name = "libprelude_anyref";
    let out = compile_fixture(name, &["--no-scala-library"]);
    assert_eq!(run_java(&out, None), expected_stdout(name));
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture(name, &["--scala-library", jar_s]);
    assert_eq!(run_java(&out, Some(jar_s)), expected_stdout(name));
    let _ = fs::remove_dir_all(&out);
}

/// The fallback is asked last and only supplies what `AnyRef` declares, so it
/// must not have made `eq` legal on a value class or on `Any`. Real scalac
/// 2.13.16 rejects both of these.
#[test]
fn eq_is_still_refused_where_there_is_no_reference() {
    for (tag, src) in [
        (
            "inteq",
            "object M { def f(a: Int, b: Int): Boolean = a eq b }",
        ),
        (
            "anyeq",
            "object M { def f(a: Any, b: AnyRef): Boolean = a eq b }",
        ),
    ] {
        let (ok, err) = compile_src(tag, src, &["--no-scala-library"]);
        assert!(!ok, "expected {tag} to be rejected:\n{err}");
        assert!(
            err.contains("is not a member"),
            "expected a member diagnostic for {tag}:\n{err}"
        );
    }
}

// ------------------------------------------------- `->` and a source Predef

/// `docs/scala-library.md`'s item 0, at last with the right root.
///
/// The doc's twelve-line reproduction is **not** a case this compiler gets
/// wrong: real scalac 2.13.16 rejects it too, with "implicit conversions are
/// not applicable because they are ambiguous", because a user conversion
/// offering `->` genuinely does tie with `Predef.ArrowAssoc`. That shape is
/// pinned below in `two_real_conversions_are_still_an_ambiguity`.
///
/// What `src/library` hits is different: there is only *one* `->` conversion
/// in the program, `Predef`'s own, and this compiler had a second -- its own
/// prelude stand-in for it, under 2.10's name `any2ArrowAssoc`, which
/// `predef_reimport`'s replace-by-name could never displace.
///
/// Run in `--no-scala-library` only. In `--scala-library`, a source
/// `scala.Predef` makes `java.lang.System.out.println(x)` compile to
/// `scala.Predef$.println` and die with `NoSuchMethodError`; that is a
/// separate, pre-existing defect of the source-`Predef` path (it reproduces
/// with no `->` anywhere) and is recorded in `docs/scala-library.md`.
#[test]
fn arrow_resolves_when_the_source_supplies_predef() {
    if !java_available() {
        return;
    }
    let name = "libprelude_arrow";
    let out = compile_fixture(name, &["--no-scala-library"]);
    assert_eq!(run_java(&out, None), expected_stdout(name));
    let _ = fs::remove_dir_all(&out);
}

/// The tie-break fires **only** for a run whose own sources define
/// `scala.Predef`. Two conversions that are both really in scope stay an
/// ambiguity, exactly as real scalac 2.13.16 reports it:
///
/// ```text
/// Note that implicit conversions are not applicable because they are ambiguous:
///  both method ArrowAssoc in object Predef ...
///  and method ArrowAssocQ in object PredefY ...
/// ```
///
/// A blanket "a source conversion beats a prelude one" would accept this.
#[test]
fn two_real_conversions_are_still_an_ambiguity() {
    let (ok, err) = compile_src(
        "twoconv",
        r#"
package scala
object PredefY {
  implicit final class ArrowAssocQ[A](private val self: A) extends AnyVal {
    def -> [B](y: B): (A, B) = (self, y)
  }
}
package other {
  import scala.PredefY._
  object U { val a = 1 -> 2 }
}
"#,
        &["--no-scala-library"],
    );
    assert!(
        !ok,
        "two conversions offering `->` for the same type is an ambiguity in \
         scalac 2.13.16 too; it must not be accepted:\n{err}"
    );
}

/// And an ordinary program -- one that does not define `scala.Predef` -- is
/// untouched: the prelude's own conversion still supplies `->`.
#[test]
fn an_ordinary_arrow_is_unaffected() {
    let mut modes: Vec<Vec<String>> = vec![vec!["--no-scala-library".to_string()]];
    if let Some(j) = scala_library_jar() {
        modes.push(vec![
            "--scala-library".to_string(),
            j.to_str().unwrap().to_string(),
        ]);
    }
    for extra in modes {
        let args: Vec<&str> = extra.iter().map(|s| s.as_str()).collect();
        let (ok, err) = compile_src(
            "ordinaryarrow",
            r#"
object M {
  def f: (Int, String) = 1 -> "a"
  def g[A, B](a: A, b: B): (A, B) = a -> b
}
"#,
            &args,
        );
        assert!(
            ok,
            "the prelude conversion still supplies `->` with {extra:?}:\n{err}"
        );
    }
}
