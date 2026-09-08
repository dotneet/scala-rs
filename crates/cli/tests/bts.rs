//! A base class reached through two parents at two different instantiations is
//! read at the *most derived* of them (SLS 5.1.2), and real scalac 2.13.16 is
//! the oracle for what that means.
//!
//! This is not a question a compile/no-compile check can answer on its own.
//! Which base type a class is seen at decides which member is found and at what
//! signature, and the wrong answer routinely still compiles -- it just runs
//! something else. So `tests/fixtures/bts_basetypeseq.scala` is written so that
//! the wrong instantiation cannot compile *and* the right one produces output;
//! this file then recompiles the same source with scalac and compares the two
//! programs, so the expectation is scalac's and not a transcription of ours.
//!
//! Before `SymbolTable::base_type_args`, `Ops` in that fixture was read at the
//! instantiation the *first written* parent supplies, and the file was rejected
//! with `value onlyOnDerived is not a member of Base[K, V]`. The same defect
//! cost the scala library 131 errors across 26 files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("{name}.scala"))
}

fn expected(name: &str) -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/expected")
            .join(format!("{name}.txt")),
    )
    .unwrap()
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-bts-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_ours(src: &Path, tag: &str) -> PathBuf {
    let out = tmp_dir(tag);
    let run = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        run.status.success(),
        "compile {} failed:\n{}{}",
        src.display(),
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
    out
}

#[test]
fn most_derived_base_type_runs() {
    if !java_available() {
        eprintln!("skip most_derived_base_type_runs: no java");
        return;
    }
    let out = compile_ours(&fixture("bts_basetypeseq"), "run");
    assert_eq!(run_main(out.to_str().unwrap()), expected("bts_basetypeseq"));
    let _ = fs::remove_dir_all(&out);
}

/// The same source through real scalac 2.13.16. The fixture's expected output
/// is scalac's answer, so this is the check that keeps it one.
#[test]
fn most_derived_base_type_matches_scalac() {
    if !java_available() {
        eprintln!("skip most_derived_base_type_matches_scalac: no java");
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip most_derived_base_type_matches_scalac: scalac or jar not obtainable");
        return;
    };
    let src = fixture("bts_basetypeseq");
    let theirs_out = tmp_dir("scalac");
    let run = Command::new(&scalac)
        .args([
            "-classpath",
            jar.to_str().unwrap(),
            "-d",
            theirs_out.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output()
        .expect("run scalac");
    assert!(
        run.status.success(),
        "scalac 2.13.16 rejected bts_basetypeseq: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let theirs = run_main(&format!("{}:{}", theirs_out.display(), jar.display()));
    let ours_out = compile_ours(&src, "ours");
    let ours = run_main(ours_out.to_str().unwrap());
    assert_eq!(
        theirs, ours,
        "base type instantiation differs from scalac 2.13.16"
    );
    assert_eq!(theirs, expected("bts_basetypeseq"));
    let _ = fs::remove_dir_all(&theirs_out);
    let _ = fs::remove_dir_all(&ours_out);
}

/// The same defect read through the real `scala-library` pickle rather than
/// from source, so that a fix which only works on source-built symbols does not
/// pass. `VectorMap[K, +V] extends AbstractMap[K, V] with SeqMap[K, V] with
/// StrictOptimizedMapOps[K, V, VectorMap, VectorMap[K, V]]` reaches `MapOps`
/// as `MapOps[K, V, Map, Map[K, V]]` through `AbstractMap` and at `VectorMap`
/// through the last mixin, and `updatedWith` returns `MapOps`' own `CC[K, V1]`.
/// Before the fix this was `type mismatch; found: SeqMap[Int, String]`; scalac
/// 2.13.16 accepts it, which [`most_derived_base_type_matches_scalac`] pins for
/// the source-built form of the same hierarchy.
///
/// `TreeMap` looks like this shape and is *not* covered here: it still fails,
/// for a different reason (`immutable.SortedMapOps` overrides `updatedWith`
/// with the sorted `CC` and that override is not the one selected). See
/// `docs/not-implemented.md`.
#[test]
fn updated_with_keeps_the_receivers_own_type() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip updated_with_keeps_the_receivers_own_type: no scala-library jar");
        return;
    };
    let dir = tmp_dir("updatedwith");
    let src = dir.join("Bts.scala");
    fs::write(
        &src,
        "import scala.collection.immutable.VectorMap\n\
         object Bts {\n  \
           def v(m: VectorMap[Int, String]): VectorMap[Int, String] =\n    \
             m.updatedWith(1)(_ => Some(\"x\"))\n\
         }\n",
    )
    .unwrap();
    let out = tmp_dir("updatedwith-out");
    let run = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        run.status.success(),
        "updatedWith was not read at the receiver's own type:\n{}{}",
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&out);
}
