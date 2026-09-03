//! E2E tests for the `agent/lastone` slice: slick's **last** type error,
//! `jdbc/SQLiteProfile.scala:183`, and the two codegen bugs that only became
//! reachable once it type-checked.
//!
//! * **`no matching overload for (Iterable[U], JdbcActionComponent.RowsPerStatement)…`**
//!   `super.m`'s member types were read off the parent named on its own. They
//!   are seen from `this.type`: `SQLiteProfile` mixes in
//!   `MultipleRowsPerStatementSupport`, whose
//!   `override type RowsPerStatement = slick.jdbc.RowsPerStatement` is what the
//!   inherited `insertAll(values, rowsPerStatement: RowsPerStatement)` takes.
//!   Read off `InsertActionComposerImpl` alone the parameter stayed the
//!   *abstract* member `>: One.type <: RowsPerStatement`, which nothing but
//!   its own lower bound conforms to. Nothing to do with named arguments or
//!   with the sole candidate being an "overload".
//!
//! * **An abstract type member erased to `Object`.** SLS 3.7 erases it like a
//!   type parameter, to its upper bound -- scalac writes
//!   `insertAll(Iterable, Rps)`. With `Object` the inherited method and the
//!   profile's override became *different* JVM methods and the trait's
//!   `$super$` accessor was a `NoSuchMethodError`.
//!
//! * **The `T$$super$m` accessor was called and forwarded at the wrong
//!   descriptor.** The accessor is a member of the trait, so it carries the
//!   *overriding* method's erasure, while the method it forwards to keeps its
//!   own: `type Rows = One.type` narrowing `>: One.type <: Rps` makes the two
//!   differ, and both the call and the forward named a method that did not
//!   exist.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts with other
//! agents; see `.agent-brief.md`. All fixtures use the `lastone` prefix.

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
        "scala-rs-lastone-{tag}-{}-{nanos}-{seq}",
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

fn compile_fixture(tag: &str, name: &str, extra: &[&str]) -> PathBuf {
    let out = tmp_dir(tag);
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    cmd.arg(fixtures_dir().join(format!("{name}.scala")));
    cmd.args(["-d", out.to_str().unwrap()]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
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

/// `super.m` against an abstract type member the current class refines, in
/// both directions (widened to the bound, and narrowed below it), run against
/// the real scala-library. Covers the erasure and the `$super$` accessor
/// descriptors as well as the typing: `-Xverify:all` loads every class.
#[test]
fn fixtures_lastone_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run lastone: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture("lastone", "lastone", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, "Main", Some(jar_s)),
        expected_stdout("lastone"),
        "stdout mismatch for library-ABI lastone"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded
/// expectation, scalac's stdout and ours all have to agree.
#[test]
fn real_scalac_dual_run_lastone() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff lastone: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("lastone.scala");
    let ref_out = tmp_dir("lastone-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile lastone");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, "Main", Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout("lastone"),
        "recorded expectation for lastone does not match real scalac"
    );
    let out = compile_fixture("lastone", "lastone", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, "Main", Some(jar_s)),
        reference,
        "stdout differs from real scalac for lastone"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// Seeing the member through `this` must not make it *anything*. scalac
/// 2.13.16 reports both of these (`found: BadRps.All.type required:
/// BadNarrowProfile.this.Rows (which expands to) BadRps.One.type`, and the
/// same against the unrefined `BadOpenProfile.this.Rows`).
#[test]
fn fixtures_lastone_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "lastone_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            // the mixin's narrower refinement is what the super call sees
            "(Iterable[U], One.type)String with arguments (Iterable[U], All$)",
            // and with no refinement in sight the abstract member stands
            "(Iterable[U], BadComp.Rows)String with arguments (Iterable[U], All$)",
        ],
    );
}

/// The private-runtime mode has to agree with the library-ABI one: neither
/// the erasure of an abstract type member nor the `$super$` accessor
/// descriptors depend on the jar.
#[test]
fn fixtures_lastone_private_runtime() {
    if !java_available() {
        return;
    }
    let out = compile_fixture("lastone-priv", "lastone", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, "Main", None),
        expected_stdout("lastone"),
        "stdout mismatch for private-runtime lastone"
    );
    let _ = fs::remove_dir_all(&out);
}
