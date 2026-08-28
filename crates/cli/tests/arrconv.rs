//! E2E tests for the ArrayOps conversion/aggregation methods and
//! `scala.collection.MapView` slice (fixture prefixes `arrconv` / `mapview`).
//!
//! Self-contained (own copies of the small helpers `e2e.rs` also has) per
//! `.agent-brief.md`: e2e.rs / lang.rs / anoncap.rs are owned by other
//! in-flight agents, so this lives in its own test binary rather than
//! appending to a shared file.
//!
//! All of the members covered here (`ArrayOps.toList` / `toSet` /
//! `toVector` / `toBuffer` / `sum` / `product` / `min` / `max` / `minBy` /
//! `maxBy` / `mkString` / `reduce` / `reduceLeft`, plus `MapView` itself)
//! are absent from `scala.collection.ArrayOps` in nsc 2.13.16 (confirmed via
//! `javap -s scala.collection.ArrayOps`) and only exist through the real
//! jar's `Predef.wrapXArray` / `IterableOnceOps` machinery, so they are
//! `library_abi`-gated and only verified via the `--scala-library` dual-run
//! path (`compile_fixture_with(name, &["--scala-library", jar])`), matching
//! `scala_library_dual_run_*` in `e2e.rs`.

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
        "scala-rs-arrconv-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

/// Library-ABI dual-run must not fall back to emitting the private runtime
/// classfiles (would collide with the real scala-library.jar on the
/// classpath). Only the small subset actually touched by these fixtures.
const LIBRARY_COLLIDERS: &[&str] = &[
    "scala/Option.class",
    "scala/Predef$.class",
    "scala/collection/ArrayOps.class",
    "scala/collection/ArrayOps$.class",
    "scala/collection/immutable/List.class",
    "scala/collection/immutable/Map.class",
    "scala/collection/immutable/Map$.class",
    "scala/collection/immutable/Vector.class",
    "scala/collection/immutable/Set.class",
    "scala/collection/immutable/IndexedSeq.class",
    "scala/collection/mutable/ArrayBuffer.class",
    "scala/collection/View.class",
    "scala/collection/SeqView.class",
];

fn assert_no_private_stdlib(out: &Path) {
    for rel in LIBRARY_COLLIDERS {
        let p = out.join(rel);
        assert!(
            !p.is_file(),
            "library ABI must not emit {} (would collide with scala-library.jar)",
            p.display()
        );
    }
}

fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_no_private_stdlib(&out);
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp out:scala-library failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails_lib(name: &str, needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip compile_fails_lib {name}: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
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
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `--no-scala-library` (private runtime) has no `ArrayOps`/`MapView` ABI to
/// back these members against, so they must be gated off with a clean
/// diagnostic rather than silently miscompiling.
fn compile_fails_no_lib(name: &str, needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-nolib"));
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile --no-scala-library");
    assert!(
        !output.status.success(),
        "expected --no-scala-library compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in --no-scala-library diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn scala_library_dual_run_arrconv1() {
    dual_run_fixture("arrconv1");
}

#[test]
fn scala_library_dual_run_arrconv2() {
    dual_run_fixture("arrconv2");
}

#[test]
fn scala_library_dual_run_mapview1() {
    dual_run_fixture("mapview1");
}

#[test]
fn fixtures_arrconv1_bad_is_error() {
    compile_fails_lib("arrconv1_bad", "noSuchToList is not a member");
}

#[test]
fn fixtures_arrconv2_bad_is_error() {
    compile_fails_lib("arrconv2_bad", "noSuchSum is not a member");
}

#[test]
fn fixtures_mapview1_bad_is_error() {
    compile_fails_lib("mapview1_bad", "noSuchMapValues is not a member");
}

/// `Array(...).toList` (an `ArrayOps`-only, library-ABI-only member) must
/// not be silently accepted by the private runtime.
#[test]
fn fixtures_arrconv_gate_no_scala_library_is_error() {
    compile_fails_no_lib("arrconv_gate", "toList is not a member");
}

/// `Map(...).view` (`MapView`, library-ABI-only) must not be silently
/// accepted by the private runtime either.
#[test]
fn fixtures_mapview_gate_no_scala_library_is_error() {
    // Without the library `Map` itself is unavailable, so the gate holds at
    // the constructor. `.view` is not reported: nsc does not follow a
    // selection out of a receiver that already failed.
    compile_fails_no_lib("mapview_gate", "not found: value Map");
}
