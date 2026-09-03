//! E2E tests for the `Set`/`Map` construction-and-addition overload slice and
//! for `Array` seen as a `Seq`/`Iterable` (fixture prefix `setmap`).
//!
//! Self-contained (own copies of the small helpers `e2e.rs` also has) per
//! `.agent-brief.md`: the shared test files belong to other in-flight agents,
//! so this lives in its own test binary.
//!
//! Everything here is `library_abi`-only — `Predef.genericWrapArray`,
//! `Predef.copyArrayToImmutableIndexedSeq`, `scala.collection.MapOps`'s
//! `contains`/`apply`/`get` and `IterableOps.++` all live in the real jar and
//! have no counterpart in the private runtime — so the positive cases run only
//! through the `--scala-library` dual-run, and `setmap_gate` pins that
//! `--no-scala-library` still reports a plain `type mismatch` there rather
//! than silently accepting the conversion.

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
        "scala-rs-setmap-{tag}-{}-{nanos}-{seq}",
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

/// Library-ABI mode must not emit private-runtime copies of classes the real
/// jar already has: only the ones this fixture touches.
const LIBRARY_COLLIDERS: &[&str] = &[
    "scala/Option.class",
    "scala/Predef$.class",
    "scala/collection/Map.class",
    "scala/collection/immutable/HashMap.class",
    "scala/collection/immutable/HashSet.class",
    "scala/collection/immutable/List.class",
    "scala/collection/immutable/Map.class",
    "scala/collection/immutable/Map$.class",
    "scala/collection/immutable/Set.class",
    "scala/collection/immutable/IndexedSeq.class",
    "scala/collection/mutable/ArraySeq.class",
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

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
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
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail (extra={extra:?})"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {err:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// All eight slick shapes at once: `Array` as `Seq`/`IndexedSeq`/`Iterable`,
/// `Array` reaching an `IndexedSeq` parameter through overload resolution,
/// `Set() ++ Option ++ Option`, `Set[String] ++ Option[Int]`,
/// `Map() ++ Array[(K, V)]`, `collection.Map`'s `contains`/`apply`/`get`,
/// `HashSet#map` over `@uncheckedVariance` tuples, and `HashMap + (k -> v)`.
/// The expected output is byte-for-byte what scalac 2.13.16 prints for the
/// same file.
#[test]
fn scala_library_dual_run_setmap1() {
    dual_run_fixture("setmap1");
}

/// The wrappings must not make everything conform: scalac rejects both of
/// these too (`Array[Int]` is no `Seq[String]`, and `collection.Map` has no
/// `noSuchLookup`).
#[test]
fn fixtures_setmap1_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip fixtures_setmap1_bad_is_error: jar not obtainable");
        return;
    };
    compile_fails(
        "setmap1_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "type mismatch; found: Array[Int]  required: Seq[String]",
            "value noSuchLookup is not a member",
        ],
    );
}

/// `--no-scala-library` has no `Predef.genericWrapArray` to call, so the
/// conversion must stay unavailable and the diagnostic must stand.
#[test]
fn fixtures_setmap_gate_no_scala_library_is_error() {
    compile_fails(
        "setmap_gate",
        &["--no-scala-library"],
        &["type mismatch; found: Array[Any]  required: Iterable[Any]"],
    );
}

/// ...and the same file *does* compile against the real jar, so the gate is a
/// gate and not a missing feature.
#[test]
fn fixtures_setmap_gate_compiles_with_library() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip fixtures_setmap_gate_compiles_with_library: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("setmap_gate", &["--scala-library", jar.to_str().unwrap()]);
    let _ = fs::remove_dir_all(&out);
}
