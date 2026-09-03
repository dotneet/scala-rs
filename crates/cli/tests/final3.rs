//! E2E tests for the `agent/final3` slice: the last of slick's single
//! diagnostics (`lifted/Shape.scala`, `relational/RelationalProfile.scala`,
//! `memory/DistributedProfile.scala`, `jdbc/SQLiteProfile.scala`,
//! `compiler/FixRowNumberOrdering.scala`).
//!
//! Seven diagnostics, five roots -- and no root was what its diagnostic said:
//!
//! * **`Function does not take type parameters`** (`Shape.scala:397`). Nothing
//!   to do with type lambdas: `Predef` declares
//!   `type Function[-A, +B] = Function1[A, B]`, which the symbol table does not
//!   have, so the bare name resolved to the `scala.Function` *module* class
//!   (arity 0). `RelationalProfile.scala:82`'s
//!   `missing parameter type for expanded function` was this same error one
//!   step downstream -- the pattern-matching anonymous function passed to
//!   `genericFastPath` had no expected type left to take its parameter from.
//!
//! * **`no matching overload for constructor QueryInterpreter with arguments
//!   (<notype>, Any)`** (`DistributedProfile.scala:76`). A signature completed
//!   on demand *during the signature pass* may read a member whose own written
//!   signature that pass has not reached yet -- `HeapBackend.createEmptyDatabase`
//!   lives in a file that sorts after `DistributedProfile.scala`. The result
//!   (`<notype>`) was then cached forever. A nested template's parents are
//!   typed in the enclosing template's signature phase, which is what forced
//!   `val emptyHeapDB` that early.
//!
//! * **`recursive method run needs result type`** (`DistributedProfile.scala:91`).
//!   *Not* a cascade of the one above -- it survived that fix. Same ordering,
//!   different consumer: `overridden_ret_type` deliberately does not force a
//!   candidate's signature, so `override def run(n: Node) = …` found nothing to
//!   borrow from `QueryInterpreter.run(n: Node): Any` and stayed
//!   inference-bound. The search is now run again before the body is typed,
//!   and re-entering a completion whose type is already known is no longer
//!   reported as a cycle.
//!
//! * **`value apply is not a member of AnyRef`** (`SQLiteProfile.scala:138`).
//!   `FunctionN` has its own `Type` variant, so `lub` never reached the
//!   same-class arm that joins type arguments: the join of `String => Timestamp`
//!   and `String => String` walked the base type sequence and answered
//!   `AnyRef`, and the `Seq` of converters lost its element type.
//!
//! * **`no matching overload for (Node, Option[Comprehension[Option[Node]]])Node
//!   with arguments (Node, Some[Comprehension[_]])`**
//!   (`FixRowNumberOrdering.scala:19`). `C[_]` for
//!   `class C[+F <: Option[Node]]` means `C[_$1] forSome { type _$1 <: Option[Node] }`;
//!   the wildcard argument had no bound to answer with, so the covariant check
//!   `_ <: Option[Node]` failed.
//!
//! Left unfixed: `SQLiteProfile.scala:183` (`super.insertAll` against the
//! abstract type member `type RowsPerStatement >: One.type <: RowsPerStatement`
//! that `MultipleRowsPerStatementSupport` refines) -- an as-seen-from problem
//! for refined abstract type members, not the same shape as anything here.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with other
//! agents; see `.agent-brief.md`. All fixtures use the `final3` prefix.

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
        "scala-rs-final3-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Compile the named fixtures **in the given order**: both cross-file roots in
/// this slice are signature-pass ordering bugs, so the order the sources are
/// handed to the compiler is part of what is under test.
fn compile_fixtures_with(tag: &str, names: &[&str], extra: &[&str]) -> PathBuf {
    let out = tmp_dir(tag);
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for n in names {
        cmd.arg(fixtures_dir().join(format!("{n}.scala")));
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {names:?} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn run_java(out: &Path, main: &str, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(e) => format!("{}:{}", out.display(), e),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-bad"));
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
        !output.status.success(),
        "expected compile of {name} (extra={extra:?}) to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// `Predef.Function`, the lub of two function types and a bounded wildcard
/// argument, run against the real scala-library.
#[test]
fn fixtures_final3_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run final3: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixtures_with("final3", &["final3"], &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, "Main", Some(jar_s)),
        expected_stdout("final3"),
        "stdout mismatch for library-ABI final3"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded
/// expectation, scalac's stdout and ours all have to agree.
#[test]
fn real_scalac_dual_run_final3() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff final3: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("final3.scala");
    let ref_out = tmp_dir("final3-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile final3");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, "Main", Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout("final3"),
        "recorded expectation for final3 does not match real scalac"
    );
    let out = compile_fixtures_with("final3", &["final3"], &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, "Main", Some(jar_s)),
        reference,
        "stdout differs from real scalac for final3"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// The two signature-pass ordering roots. `final3_use.scala` **must** come
/// first on the command line; with the definitions first neither bug fires.
#[test]
fn fixtures_final3_forward_file_order() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run final3_use: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixtures_with(
        "final3use",
        &["final3_use", "final3_def"],
        &["--scala-library", jar_s],
    );
    assert_eq!(
        run_java(&out, "final3use.Main", Some(jar_s)),
        expected_stdout("final3_use"),
        "stdout mismatch for final3_use/final3_def"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same pair through real scalac, in the same order.
#[test]
fn real_scalac_dual_run_final3_pair() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff final3 pair: scalac or jar not obtainable");
        return;
    };
    let ref_out = tmp_dir("final3pair-scalac-ref");
    let status = Command::new(&scalac)
        .arg(fixtures_dir().join("final3_use.scala"))
        .arg(fixtures_dir().join("final3_def.scala"))
        .args(["-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile final3 pair");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, "final3use.Main", Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout("final3_use"),
        "recorded expectation for final3_use does not match real scalac"
    );
    let out = compile_fixtures_with(
        "final3use",
        &["final3_use", "final3_def"],
        &["--scala-library", jar_s],
    );
    assert_eq!(
        run_java(&out, "final3use.Main", Some(jar_s)),
        reference,
        "stdout differs from real scalac for the final3 pair"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// The bound on a wildcard argument is the type parameter's, not `Any`, and a
/// method that really has no result type to borrow is still a cycle. scalac
/// 2.13.16 reports both (`type mismatch; found: ComprB[_] required:
/// ComprB[Some[NdB]]` and `recursive method loop needs result type`).
#[test]
fn fixtures_final3_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "final3_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "with arguments (Some[ComprB[_]])",
            "recursive method loop needs result type",
        ],
    );
}
