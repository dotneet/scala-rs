//! `agent/basetypeargs`: the element type and the `CC` of a collection are
//! read off its base type, not guessed from its own first type argument.
//!
//! `Iterator[A].sliding(n)` returns `Iterator.GroupedIterator[B]`, an inner
//! class of `trait Iterator` declared `extends AbstractIterator[immutable.Seq[B]]`.
//! Its element is `Seq[B]` and its `CC` is `Iterator`, so it is the one shape
//! in the library that breaks both of the guesses this compiler was making at
//! once. Three defects met on it:
//!
//! 1. `PickleSupply::ensure_class` split `scala/collection/Iterator$GroupedIterator`
//!    at the last `/` alone, so a *nested* class was entered as a package-level
//!    class whose simple name was `Iterator$GroupedIterator` -- a **second
//!    symbol** for the class the class-file loader enters correctly as
//!    `GroupedIterator` inside `Iterator`. Which one a program got depended on
//!    which path reached it first: write the type and `install_java_class_in`
//!    built the member with its parents; let `it.sliding(n)`'s pickled result
//!    type arrive first and the twin was built with `AnyRef` for a parent list.
//!    On the branch point `def x[A](it: Iterator[A]): Iterator[Seq[A]] =
//!    it.sliding(2)` was rejected, and adding a *val of the named type* one
//!    line above made the same expression compile.
//! 2. That stub also had no parents. `stub_superclass_from_classfile` declines
//!    a nested class with type parameters, because a class file cannot say what
//!    arguments its superclass is applied at -- its pickle can, and one hop is
//!    not a hierarchy either: `AbstractIterator` was standing at `AnyRef` too.
//! 3. With the hierarchy there, `Check::elem_type` still answered `A` for the
//!    element (`args[0]`) and `rebuild_from_receiver` still put
//!    `GroupedIterator` back as the `CC`. Both now read the receiver's base
//!    type at `IterableOnce`.
//!
//! The positive fixture *runs*, because every one of these is a type this
//! compiler can be talked into and the JVM will not check: a
//! `GroupedIterator[Int]` claims elements of type `Seq[Int]` for a value whose
//! elements are `Int`.
//!
//! Fixture prefix: `btargs_`.

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
        "scala-rs-btargs-{tag}-{}-{nanos}-{seq}",
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

fn compile_rs(srcs: &[PathBuf], out: &Path, jar: &Path) -> Run {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    cmd.args(["--scala-library", jar.to_str().unwrap()]);
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

fn ok(run: Run, what: &str) {
    assert!(run.ok, "expected {what} to compile:\n{}", run.text);
}

fn write_src(dir: &Path, name: &str, src: &str) -> PathBuf {
    let p = dir.join(format!("{name}.scala"));
    fs::write(&p, src).unwrap();
    p
}

// ---------------------------------------------------------------------------
// The positive fixture, executed.

/// Compiled by this compiler and run. The expected output is real scalac
/// 2.13.16's for the same file, which the next test asserts rather than
/// assumes.
///
/// On the branch point this file does not compile at all: three errors,
/// `found: Iterator$GroupedIterator[A] required: Iterator[Seq[A]]`,
/// `found: (Seq[A]) => String required: (A) => String`, and
/// `value size is not a member of A`.
#[test]
fn btargs_groupedelem_runs() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip btargs_groupedelem_runs: jar or java not present");
        return;
    };
    let dir = tmp_dir("run");
    ok(
        compile_rs(&[fixture("btargs_groupedelem")], &dir, &jar),
        "btargs_groupedelem",
    );
    assert_eq!(
        run_java(&dir, &jar),
        expected_stdout("btargs_groupedelem"),
        "our run of btargs_groupedelem must print what scalac's does"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The expected file is scalac's, checked by producing it again here. Without
/// this, `expected/btargs_groupedelem.txt` is only what this compiler happened
/// to print the day it was written.
#[test]
fn btargs_expected_output_is_scalacs() {
    let (Some(jar), Some(sc), true) = (scala_library_jar(), scalac(), java_available()) else {
        eprintln!("skip btargs_expected_output_is_scalacs: jar, scalac or java not present");
        return;
    };
    let dir = tmp_dir("scalac");
    ok(
        compile_scalac(&sc, &[fixture("btargs_groupedelem")], &dir, &jar),
        "btargs_groupedelem under scalac",
    );
    assert_eq!(run_java(&dir, &jar), expected_stdout("btargs_groupedelem"));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The restriction.

/// The negative fixture, rejected by both compilers at the same three lines.
///
/// scalac 2.13.16 reports lines 8, 12 and 15 -- `sliding` is not an
/// `Iterator[A]`, its `map` does not take an `A => B`, and `Vector.map` still
/// narrows to a `Vector` and so is not a `List`. The third is the control: the
/// receiver-class narrowing must survive the other two fixes.
#[test]
fn btargs_bad_is_rejected_where_scalac_rejects() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip btargs_bad_is_rejected_where_scalac_rejects: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("bad");
    let src = fixture("btargs_groupedelem_bad");
    let theirs = compile_scalac(&sc, std::slice::from_ref(&src), &dir, &jar);
    assert!(
        !theirs.ok,
        "scalac is expected to reject btargs_groupedelem_bad:\n{}",
        theirs.text
    );
    let ours = compile_rs(std::slice::from_ref(&src), &dir, &jar);
    assert!(
        !ours.ok,
        "we must reject btargs_groupedelem_bad too:\n{}",
        ours.text
    );
    for line in ["_bad.scala:8", "_bad.scala:12", "_bad.scala:15"] {
        assert!(
            theirs.text.contains(line),
            "scalac is expected to report {line}:\n{}",
            theirs.text
        );
        assert!(
            ours.text.contains(line),
            "we should report {line}:\n{}",
            ours.text
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// One symbol for `Iterator.GroupedIterator`, whichever spelling reaches it
/// first.
///
/// This is the defect underneath the other two, and it is only visible as an
/// *order* dependence: on the branch point the second file below compiled and
/// the first did not, for the same expression. The two are checked against
/// scalac, which accepts both.
#[test]
fn btargs_nested_class_is_one_symbol_either_way() {
    let (Some(jar), Some(sc)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip btargs_nested_class_is_one_symbol_either_way: jar or scalac absent");
        return;
    };
    let dir = tmp_dir("order");
    // The pickled result type of `sliding` reaches the class first.
    let from_pickle = write_src(
        &dir,
        "FromPickle",
        "object FromPickle {\n  \
         def x[A](it: Iterator[A]): Iterator[Seq[A]] = it.sliding(2)\n\
         }\n",
    );
    // A written type reaches it first, through the class-file loader. scalac
    // rejects the *spelling* `Iterator.GroupedIterator` (the object has no such
    // member), so the order is forced with a plain member selection instead.
    let from_source = write_src(
        &dir,
        "FromSource",
        "object FromSource {\n  \
         def warm[A](it: Iterator[A]): Seq[A] = it.sliding(2).next()\n  \
         def x[A](it: Iterator[A]): Iterator[Seq[A]] = it.sliding(2)\n\
         }\n",
    );
    for src in [&from_pickle, &from_source] {
        let out = dir.join(format!(
            "out-{}",
            src.file_stem().unwrap().to_string_lossy()
        ));
        fs::create_dir_all(&out).unwrap();
        ok(
            compile_scalac(&sc, std::slice::from_ref(src), &out, &jar),
            &format!("{} under scalac", src.display()),
        );
        ok(
            compile_rs(std::slice::from_ref(src), &out, &jar),
            &format!("{}", src.display()),
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
