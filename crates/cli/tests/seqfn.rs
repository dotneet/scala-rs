//! `Seq[A] <: PartialFunction[Int, A] <: Int => A` (agent/seqfn).
//!
//! 2.13's `scala.collection.Seq[A]` declares `PartialFunction[Int, A]` among
//! its parents (`javap scala.collection.Seq`); the prelude's hierarchy only
//! had the `Iterable` edge, so `List`, `Vector`, `mutable.ArrayBuffer`, and
//! friends could not be passed where an `Int => A` was wanted, and
//! `isDefinedAt` / `applyOrElse` / `lift` / `orElse` (all `PartialFunction`
//! members) were not members of any `Seq` either. `Array` reaches the same
//! place one step removed, through `Predef.wrapBooleanArray`.
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
        "scala-rs-seqfn-{tag}-{}-{nanos}",
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

/// Compile the sources and return whether it succeeded plus the diagnostics.
/// `jar: None` compiles under `--no-scala-library` instead.
fn compile(out: &Path, jar: Option<&Path>, srcs: &[PathBuf]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    match jar {
        Some(jar) => {
            cmd.args(["--scala-library", jar.to_str().unwrap()]);
        }
        None => {
            cmd.arg("--no-scala-library");
        }
    }
    let output = cmd.output().expect("run scala-rs compile");
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
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
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
    let (ok, msgs) = compile(&out, Some(&jar), &[src]);
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
    let (_, msgs) = compile(&out, Some(&jar), &[src]);
    assert!(
        !msgs.contains("error:"),
        "{tag} should compile, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn rejects_without_library(tag: &str, source: &str, needle: &str) {
    let dir = tmp_dir(tag);
    let src = dir.join(format!("{tag}.scala"));
    fs::write(&src, source).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, None, &[src]);
    assert!(!ok, "{tag} should fail to compile under --no-scala-library");
    assert!(
        msgs.contains(needle),
        "expected {needle:?} in diagnostics for {tag}, got {msgs:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ fixtures

#[test]
fn seqfn_fixture_dual_run() {
    dual_run("sf");
}

/// `Seq <: PartialFunction[Int, A]` must not turn into "any `Seq` is any
/// function": the domain stays `Int`, and the codomain keeps its own
/// variance.
#[test]
fn sf_bad_is_still_rejected() {
    compile_fails(
        "sf_bad",
        &[
            "type mismatch; found: List[Int]  required: (String) => Int",
            "type mismatch; found: List[Animal]  required: (Int) => Dog",
        ],
    );
}

// -------------------------------------------------------------- unit-ish cases

/// `List(0, 2).map(s)` -- a `Seq` passed as an ordinary argument, not just in
/// assignment position -- and a direct `val f: Int => Int = xs` assignment.
#[test]
fn a_list_is_usable_as_int_to_a() {
    accepts(
        "sf_list_as_fn",
        "object M {\n\
         \x20 val s = List(10, 20, 30)\n\
         \x20 val f: Int => Int = s\n\
         \x20 val picked = List(0, 2).map(s)\n\
         \x20 require(f(1) == 20)\n\
         \x20 require(picked == List(10, 30))\n\
         }\n",
    );
}

/// `isDefinedAt` / `applyOrElse` / `lift` / `orElse` all reach `Seq` through
/// the same `PartialFunction` parent; `PartialFunction.apply` does not
/// upstage `Seq`'s own concrete `apply(Int): A` for plain indexing (`s(1)`,
/// `s.apply(2)`), which stays the JVM `SeqOps.apply` -- not the boxed
/// `PartialFunction.apply` -- the way it did before `Seq` inherited a second
/// `apply`.
#[test]
fn partial_function_members_reach_list_without_upstaging_its_own_apply() {
    accepts(
        "sf_partial_fn_members",
        "object M {\n\
         \x20 val s = List(1, 2, 3)\n\
         \x20 require(s(1) == 2)\n\
         \x20 require(s.apply(2) == 3)\n\
         \x20 require(s.isDefinedAt(1))\n\
         \x20 require(!s.isDefinedAt(5))\n\
         \x20 val lifted = s.lift\n\
         \x20 require(lifted(1) == Some(2))\n\
         \x20 require(lifted(5) == None)\n\
         \x20 val fb: PartialFunction[Int, Int] = { case n if n < 0 => -1 }\n\
         \x20 val combined = s.orElse(fb)\n\
         \x20 require(combined(0) == 1)\n\
         \x20 require(combined(-1) == -1)\n\
         }\n",
    );
}

/// `Vector`, `IndexedSeq`, and `mutable.ArrayBuffer` all reach `Function1`
/// through the same one edge on `scala/collection/Seq`.
#[test]
fn vector_indexed_seq_and_array_buffer_are_all_usable_as_functions() {
    accepts(
        "sf_other_seqs_as_fn",
        "import scala.collection.mutable.ArrayBuffer\n\
         object M {\n\
         \x20 val v: Vector[Int] = Vector(1, 2, 3)\n\
         \x20 val fv: Int => Int = v\n\
         \x20 require(fv(0) == 1)\n\
         \x20 val idx: IndexedSeq[Int] = IndexedSeq(4, 5, 6)\n\
         \x20 val fi: Int => Int = idx\n\
         \x20 require(fi(1) == 5)\n\
         \x20 val ab = ArrayBuffer(7, 8, 9)\n\
         \x20 val fa: Int => Int = ab\n\
         \x20 require(fa(2) == 9)\n\
         }\n",
    );
}

/// `Predef.wrapString` is an ordinary implicit `String => WrappedString`,
/// and `WrappedString extends ... Seq[Char]`: no extra wiring beyond the
/// `Seq <: PartialFunction[Int, A]` edge itself is needed for a `String`.
#[test]
fn a_string_is_usable_as_int_to_char_via_wrapped_string() {
    accepts(
        "sf_string_as_fn",
        "object M {\n\
         \x20 val str = \"abcd\"\n\
         \x20 val f: Int => Char = str\n\
         \x20 require(f(2) == 'c')\n\
         \x20 require(str.isDefinedAt(1))\n\
         \x20 require(!str.isDefinedAt(9))\n\
         }\n",
    );
}

/// `Array[Boolean]` is not itself a `Seq`, but `Predef.wrapBooleanArray`
/// turns it into `mutable.ArraySeq[Boolean]`, which is one. Both the
/// assignment case and the argument case (`filter`, which -- unlike a plain
/// `val` -- first has to survive overload scoring) are covered.
#[test]
fn a_boolean_array_is_usable_as_int_to_boolean() {
    accepts(
        "sf_bool_array_as_fn",
        "object M {\n\
         \x20 val arr = Array(true, false, true)\n\
         \x20 val f: Int => Boolean = arr\n\
         \x20 require(f(0))\n\
         \x20 require(!f(1))\n\
         \x20 val kept = List(0, 1, 2).filter(arr)\n\
         \x20 require(kept == List(0, 2))\n\
         }\n",
    );
}

/// `List[Dog] <: Int => Animal`: `Seq` is covariant, so the codomain widens
/// the same way an ordinary covariant type parameter would.
#[test]
fn a_list_of_a_subtype_is_usable_as_int_to_the_supertype() {
    accepts(
        "sf_variance",
        "object M {\n\
         \x20 class Animal(val name: String)\n\
         \x20 class Dog(name: String) extends Animal(name)\n\
         \x20 val dogs: List[Dog] = List(new Dog(\"Rex\"), new Dog(\"Fido\"))\n\
         \x20 val f: Int => Animal = dogs\n\
         \x20 require(f(0).name == \"Rex\")\n\
         }\n",
    );
}

/// `--no-scala-library`: the private runtime's `List`/`PartialFunction`
/// classfiles do not implement the members this slice adds (no `lift`
/// default method, no `List implements Function1`), so the type-level facts
/// stay `library_abi`-only and the existing "not a member" / "type mismatch"
/// diagnostics keep firing instead of type-checking a call that would not
/// link.
#[test]
fn without_the_library_the_old_diagnostics_still_fire() {
    rejects_without_library(
        "sf_no_lib_assign",
        "object M {\n\
         \x20 val s = 1 :: 2 :: 3 :: Nil\n\
         \x20 val f: Int => Int = s\n\
         }\n",
        "type mismatch; found: List[Int]  required: (Int) => Int",
    );
    rejects_without_library(
        "sf_no_lib_isdefinedat",
        "object M {\n\
         \x20 val s = 1 :: 2 :: 3 :: Nil\n\
         \x20 s.isDefinedAt(1)\n\
         }\n",
        "value isDefinedAt is not a member of List[Int]",
    );
}
