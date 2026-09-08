//! `agent/varargsrecv`: a repeated parameter used as a value.
//!
//! `xs: T*` is a `scala.collection.immutable.Seq[T]` inside the body; `T*` is
//! only the declaration form. `Check::seq_of` was finding that `Seq` with a
//! *scope* lookup for the name, and nsc does not: `definitions.SeqClass` is a
//! fixed symbol, and `scala.Seq` is an alias of it. The scope lookup was wrong
//! in both directions.
//!
//! **It found the wrong `Seq`.** A program may bind the name to something of
//! its own. `object Main { class Seq[A] { def tag = "MINE" }; def f(xs: Int*)
//! = xs.tag }` compiled on an unmodified build of the branch point, because
//! `gen_desc` writes `Lscala/collection/immutable/Seq;` for a repeated
//! parameter whatever the typer decided: the emitted `invokevirtual
//! Main$Seq.tag` met an `ArraySeq$ofInt` and threw `ClassCastException` at run
//! time with no diagnostic anywhere. That is why the positive fixture here
//! *runs*.
//!
//! **It found no `Seq` where there was one.** A run that compiles the standard
//! library from source (`tests/scalalib_measure.sh`, which is
//! `--no-scala-library`) binds the name only as `scala/package.scala`'s
//! `type Seq[+A] = scala.collection.immutable.Seq[A]` — an alias, which a
//! class filter rejects — and in a file that writes a single qualified package
//! clause (`package scala.jdk`) not even that. Every repeated parameter in the
//! library was left as the bare `T*`.
//!
//! Fixture prefix: `varargsrecv_`, plus `varargsrecv.scala`.

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

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-varargsrecv-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

/// Real scalac 2.13.16, when this machine has the checkout the measurement
/// scripts install. Every test that needs it skips loudly without it.
fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

struct Run {
    ok: bool,
    text: String,
}

fn out_of(output: std::process::Output) -> Run {
    Run {
        ok: output.status.success(),
        text: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// scala-rs `compile`, against the jar.
fn compile_rs(srcs: &[PathBuf], out: &Path, jar: Option<&Path>) -> Run {
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
    out_of(cmd.output().expect("run scala-rs compile"))
}

fn compile_scalac(sc: &Path, srcs: &[PathBuf], out: &Path, jar: &Path) -> Run {
    let mut cmd = Command::new(sc);
    cmd.args([
        "-classpath",
        jar.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    for s in srcs {
        cmd.arg(s);
    }
    out_of(cmd.output().expect("run scalac"))
}

fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-cp", &cp, "Main"])
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

fn javap(out: &Path, class: &str) -> String {
    let o = Command::new("javap")
        .args(["-c", "-p", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("run javap");
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn ok(run: Run, what: &str) {
    assert!(run.ok, "expected {what} to compile:\n{}", run.text);
}

fn rejected(run: Run, what: &str, needle: &str) {
    assert!(
        !run.ok && run.text.contains(needle),
        "expected {what} to be rejected with {needle:?}; got ok={} :\n{}",
        run.ok,
        run.text
    );
}

fn write_src(dir: &Path, name: &str, src: &str) -> PathBuf {
    let p = dir.join(format!("{name}.scala"));
    fs::write(&p, src).unwrap();
    p
}

// ---------------------------------------------------------------------------
// The positive fixture: every shape the standard library's repeated parameters
// take, executed.

/// A primitive element (`Int`, `Short`), `Unit`, a bare type parameter, an
/// `Array` element, an empty call, and a `xs: _*` forward to another varargs
/// method -- with the name `Seq` bound, in the same object, to a class of the
/// program's own and to an alias.
///
/// The expected output is real scalac 2.13.16's for the same file, which the
/// next test asserts rather than assumes.
#[test]
fn varargsrecv_runs() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip varargsrecv_runs: jar or java not present");
        return;
    };
    let dir = tmp_dir("run");
    ok(
        compile_rs(&[fixture("varargsrecv")], &dir, Some(&jar)),
        "varargsrecv",
    );
    assert_eq!(run_java(&dir, &jar), expected_stdout("varargsrecv"));
    let _ = fs::remove_dir_all(&dir);
}

/// The expected file is scalac's, checked by producing it again here. Without
/// this, `expected/varargsrecv.txt` is only what this compiler happened to
/// print the day it was written.
#[test]
fn varargsrecv_expected_output_is_scalacs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip varargsrecv_expected_output_is_scalacs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("scalac");
    ok(
        compile_scalac(&sc, &[fixture("varargsrecv")], &dir, &jar),
        "varargsrecv under scalac",
    );
    assert_eq!(run_java(&dir, &jar), expected_stdout("varargsrecv"));
    let _ = fs::remove_dir_all(&dir);
}

/// The bytecode, not only the output: a repeated parameter is a
/// `scala/collection/immutable/Seq` in the descriptor, and the call site is
/// where the wrapping happens. Both halves are read off `javap -c` for this
/// compiler and for scalac, and compared.
#[test]
fn varargsrecv_descriptors_and_wrapping_match_scalac() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip varargsrecv_descriptors_and_wrapping_match_scalac: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("javap");
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    ok(
        compile_rs(&[fixture("varargsrecv")], &ours, Some(&jar)),
        "varargsrecv",
    );
    ok(
        compile_scalac(&sc, &[fixture("varargsrecv")], &theirs, &jar),
        "varargsrecv under scalac",
    );
    let a = javap(&ours, "Main$");
    let b = javap(&theirs, "Main$");

    // The declaration side. `units` is the one that says the element type is
    // carried too: a `Unit*` erases to `BoxedUnit`, not to `Object`.
    for decl in [
        "public java.lang.String ints(scala.collection.immutable.Seq<java.lang.Object>);",
        "public int shorts(scala.collection.immutable.Seq<java.lang.Object>);",
        "public java.lang.String units(scala.collection.immutable.Seq<scala.runtime.BoxedUnit>);",
        "public <T> java.lang.String gen(scala.collection.immutable.Seq<T>);",
        "public java.lang.String arrs(scala.collection.immutable.Seq<int[]>);",
        "public java.lang.String forward(scala.collection.immutable.Seq<java.lang.Object>);",
    ] {
        assert!(
            b.contains(decl),
            "scalac is expected to declare {decl}:\n{b}"
        );
        assert!(a.contains(decl), "we should declare {decl}:\n{a}");
    }

    // The use side, in `main`. Every one of these is nsc's, at nsc's place:
    // an `int` varargs is `wrapIntArray`, a `Unit` one `wrapUnitArray`, a
    // reference one `wrapRefArray`, and an *empty* one is `Nil`, not an empty
    // `ArraySeq`.
    for insn in [
        "scala/runtime/ScalaRunTime$.wrapIntArray:([I)Lscala/collection/immutable/ArraySeq;",
        "scala/runtime/ScalaRunTime$.wrapUnitArray:([Lscala/runtime/BoxedUnit;)\
         Lscala/collection/immutable/ArraySeq;",
        "scala/runtime/ScalaRunTime$.wrapRefArray:([Ljava/lang/Object;)\
         Lscala/collection/immutable/ArraySeq;",
        "Field scala/collection/immutable/Nil$.MODULE$:Lscala/collection/immutable/Nil$;",
    ] {
        let needle = insn.replace("         ", "");
        assert!(
            b.contains(&needle),
            "scalac is expected to emit {needle}:\n{b}"
        );
        assert!(a.contains(&needle), "we should emit {needle}:\n{a}");
    }

    // Found here, not fixed here: `gen_wrap_varargs` has `wrapIntArray`,
    // `wrapUnitArray` and `wrapRefArray` and no other primitive, so a
    // `Short*` call boxes and goes through `wrapRefArray` where nsc uses
    // `wrapShortArray`. The two build the same sequence -- the fixture's
    // output is identical either way -- but the `ArraySeq` is an `ofRef`
    // rather than an `ofShort`. This is the call site, not the receiver, and
    // it is asserted on the *current* behaviour so that a later slice closing
    // it is told by a failing test.
    assert!(
        b.contains("wrapShortArray"),
        "scalac is expected to use wrapShortArray for a Short* call:\n{b}"
    );
    assert!(
        !a.contains("wrapShortArray"),
        "we are known not to emit wrapShortArray yet; if this fails the gap is closed \
         and this assertion should become the opposite one:\n{a}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Negatives.

/// `T*` does not follow the program's own `Seq`. Rejected here in scalac's
/// words; the accompanying comment in the fixture records what accepting it
/// cost.
#[test]
fn varargsrecv_shadowed_seq_is_not_the_parameters_type() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip varargsrecv_shadowed_seq_is_not_the_parameters_type: jar not present");
        return;
    };
    let dir = tmp_dir("shadow");
    rejected(
        compile_rs(&[fixture("varargsrecv_shadow_bad")], &dir, Some(&jar)),
        "a member of the program's own `Seq` on a repeated parameter",
        "value tag is not a member of Seq[Int]",
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The same file under real scalac, so the sentence above is nsc's and not
/// ours.
#[test]
fn varargsrecv_shadowed_seq_is_rejected_by_scalac_too() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip varargsrecv_shadowed_seq_is_rejected_by_scalac_too: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("shadowsc");
    rejected(
        compile_scalac(&sc, &[fixture("varargsrecv_shadow_bad")], &dir, &jar),
        "the shadowed-Seq fixture under scalac",
        "value tag is not a member of Seq[Int]",
    );
    let _ = fs::remove_dir_all(&dir);
}

/// A repeated parameter covers every argument from its position on, so one
/// that is not last in its clause has no meaning. nsc's sentence, on a method,
/// a `case class` and a plain class.
#[test]
fn varargsrecv_star_parameter_must_come_last() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip varargsrecv_star_parameter_must_come_last: jar not present");
        return;
    };
    let dir = tmp_dir("last");
    let run = compile_rs(&[fixture("varargsrecv_last_bad")], &dir, Some(&jar));
    assert!(!run.ok, "expected rejection:\n{}", run.text);
    assert_eq!(
        run.text.matches("*-parameter must come last").count(),
        3,
        "one per declaration -- method, case class, class:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}

/// scalac's own answer for the same file, so the count and the wording above
/// are nsc's.
#[test]
fn varargsrecv_star_parameter_must_come_last_matches_scalac() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!(
            "skip varargsrecv_star_parameter_must_come_last_matches_scalac: jar/scalac absent"
        );
        return;
    };
    let dir = tmp_dir("lastsc");
    let run = compile_scalac(&sc, &[fixture("varargsrecv_last_bad")], &dir, &jar);
    assert!(!run.ok, "expected scalac to reject:\n{}", run.text);
    assert_eq!(
        run.text.matches("*-parameter must come last").count(),
        3,
        "scalac reports one per declaration:\n{}",
        run.text
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The clause is the unit: scalac 2.13.16 accepts a repeated parameter in a
/// clause that is not the last one, as long as it is last in its own.
#[test]
fn varargsrecv_star_may_be_last_of_an_earlier_clause() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip varargsrecv_star_may_be_last_of_an_earlier_clause: jar or java absent");
        return;
    };
    let dir = tmp_dir("clause");
    let src = write_src(
        &dir,
        "Main",
        "object Main {\n  \
           def g(xs: Int*)(y: Int): Int = xs.length + y\n  \
           def main(args: Array[String]): Unit = println(g(1, 2)(3))\n}\n",
    );
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    ok(compile_rs(&[src], &out, Some(&jar)), "g(xs: Int*)(y: Int)");
    assert_eq!(run_java(&out, &jar), "5\n");
    let _ = fs::remove_dir_all(&dir);
}

/// `T*` is a parameter's declaration form, not a type: a `val` may not have
/// one. Both compilers refuse; the wording differs because nsc refuses it in
/// the parser and this compiler parses the `*`. Asserted on the rejection
/// alone, which is the part that matters.
#[test]
fn varargsrecv_val_of_repeated_type_is_rejected() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip varargsrecv_val_of_repeated_type_is_rejected: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("val");
    let ours = compile_rs(&[fixture("varargsrecv_val_bad")], &dir, Some(&jar));
    assert!(
        !ours.ok,
        "expected `val v: Int*` to be rejected:\n{}",
        ours.text
    );
    let theirs = compile_scalac(&sc, &[fixture("varargsrecv_val_bad")], &dir, &jar);
    assert!(
        !theirs.ok,
        "scalac is expected to reject `val v: Int*` too:\n{}",
        theirs.text
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Found here, not fixed here: `T*` in a *type argument*. nsc refuses
/// `List[Int*]` in the parser (`identifier expected but ']' found.`); this
/// compiler's `parse_infix_type` accepts a postfix `*` in any type position
/// and builds a `Repeated`, so `List[Int*]` is a `List` of a repeated type.
/// Nothing reads that element as a `Seq` -- `seq_of` is applied to parameter
/// symbols and to nothing else -- so it is an over-acceptance and not a wrong
/// answer, and closing it is a parser change with its own blast radius.
/// Asserted on the current behaviour so a later slice is told.
#[test]
fn varargsrecv_repeated_as_a_type_argument_is_still_accepted() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!(
            "skip varargsrecv_repeated_as_a_type_argument_is_still_accepted: jar/scalac absent"
        );
        return;
    };
    let dir = tmp_dir("targ");
    let src = write_src(
        &dir,
        "Main",
        "object Main {\n  def f: List[Int*] = null\n}\n",
    );
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let srcs = [src];
    let theirs = compile_scalac(&sc, &srcs, &dir.join("sc"), &jar);
    assert!(
        !theirs.ok,
        "scalac is expected to reject `List[Int*]`:\n{}",
        theirs.text
    );
    let ours = compile_rs(&srcs, &out, Some(&jar));
    assert!(
        ours.ok,
        "this is a known gap: `List[Int*]` is still accepted. If this now fails the \
         gap is closed and the assertion should become the opposite one:\n{}",
        ours.text
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The library shape, in miniature.

/// `tests/scalalib_measure.sh` in four files: a source
/// `scala.collection.immutable.Seq`, `scala/package.scala`'s alias for it, a
/// file that writes `package scala` (where the alias is the only binding of
/// the name) and one that writes a single qualified clause (where the name is
/// not bound at all, because a source `type` alias is not entered into the
/// `scala._` auto-import scope the way a source class is).
///
/// `--no-scala-library`, because that is the mode the measurement runs in, and
/// **compile-only**: the private runtime ships no `scala.collection.immutable.
/// Seq` and no `scala.runtime.ScalaRunTime`, so a varargs *call* cannot
/// execute there. `crates/cli/tests/override.rs` says the same about its own
/// fixture, and `crates/cli/tests/kindproj.rs` is library-ABI-only for exactly
/// this reason.
///
/// The member each body reaches for is one only *this* `Seq` has, so the test
/// says which class the parameter became and not merely that something
/// resolved. An unmodified build of the branch point reports four errors here:
/// `value onlyOnSourceSeq is not a member of Int*`, `value length is not a
/// member of Int*`, and the same two for `T*`.
const MINI_SEQ: &str = "package scala.collection.immutable\n\n\
     trait Seq[+A] {\n  def length: Int\n  def onlyOnSourceSeq: String\n}\n";
const MINI_PKG: &str = "package object scala {\n  \
     type Seq[+A] = scala.collection.immutable.Seq[A]\n}\n";
const MINI_IN_SCALA: &str = "package scala\n\nobject InScala {\n  \
     def f(xs: Int*): String = xs.onlyOnSourceSeq + xs.length\n}\n";
const MINI_QUALIFIED: &str = "package scala.jdk\n\nobject Qualified {\n  \
     def g[T](xs: T*): String = xs.onlyOnSourceSeq + xs.length\n}\n";

fn mini_library(dir: &Path, last: &str) -> Vec<PathBuf> {
    vec![
        write_src(dir, "Seq", MINI_SEQ),
        write_src(dir, "package", MINI_PKG),
        write_src(dir, "InScala", MINI_IN_SCALA),
        write_src(dir, "Qualified", last),
    ]
}

#[test]
fn varargsrecv_source_library_seq_is_the_parameters_type() {
    let dir = tmp_dir("mini");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let srcs = mini_library(&dir, MINI_QUALIFIED);
    ok(
        compile_rs(&srcs, &out, None),
        "a source scala.collection.immutable.Seq under --no-scala-library",
    );
    // Not only "it compiled": the emitted descriptor names that same class.
    let text = javap(&out, "scala.InScala$");
    assert!(
        text.contains("java.lang.String f(scala.collection.immutable.Seq<java.lang.Object>)"),
        "the repeated parameter should erase to the source Seq:\n{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The other half: a member that source `Seq` does *not* have is still
/// refused, so the test above is not passing because the receiver became
/// something permissive.
#[test]
fn varargsrecv_source_library_seq_still_refuses_a_stranger() {
    let dir = tmp_dir("minineg");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let srcs = mini_library(
        &dir,
        "package scala.jdk\n\nobject Qualified {\n  \
         def g[T](xs: T*): String = xs.notOnSourceSeq\n}\n",
    );
    rejected(
        compile_rs(&srcs, &out, None),
        "a member no `Seq` has",
        "notOnSourceSeq",
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The mode's own limit, pinned rather than papered over: with no
/// `scala.collection.immutable.Seq` anywhere -- the private runtime ships none
/// -- a repeated parameter has no members, and this compiler says so instead
/// of inventing one. Closing this needs a `Seq` (and a `ScalaRunTime`) in
/// `backend::runtime`, not a change here.
#[test]
fn varargsrecv_private_runtime_has_no_seq() {
    let dir = tmp_dir("nolib");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let src = write_src(
        &dir,
        "Main",
        "object Main {\n  def f(xs: Int*): Int = xs.length\n}\n",
    );
    rejected(
        compile_rs(&[src], &out, None),
        "a repeated parameter used as a value with no Seq in the run",
        "value length is not a member of Int*",
    );
    let _ = fs::remove_dir_all(&dir);
}
