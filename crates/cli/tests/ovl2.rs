//! E2E tests for the `agent/ovl2` slice: how an overload's *candidate set* is
//! built.
//!
//! * inheriting is not overriding, so a subclass alternative does not hide the
//!   parent's (`Base.f(Int)` next to `Derived.f(String)`) -- neither in the
//!   typer nor in the erasure bridges the backend emits,
//! * a `private[this]` member (a bare constructor parameter is one) is not
//!   inherited, so a subclass's own parameter of that name is the only one in
//!   scope,
//! * a `val` implementing an abstract `def` is one member, not an overload,
//! * `java.lang.String` implements `CharSequence` / `Comparable` /
//!   `Serializable`, and has the `indexOf(Int)` alternative a `Char` argument
//!   widens into,
//! * an overloaded method named where a function type is expected settles on
//!   the alternative that eta-expands to it (`constOp[Long]("min")(math.min)`),
//! * `ArrayBuffer`'s `def this(initialSize: Int)`.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All new fixtures
//! use the `ovl2` prefix.

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
        "scala-rs-ovl2-{tag}-{}-{nanos}-{seq}",
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
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
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

/// `-Xverify:all`: a bridge emitted for an *overload* rather than an override
/// is a `VerifyError`, not a silent difference in the output.
fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(e) => format!("{}:{}", out.display(), e),
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

fn compile_fails(name: &str, extra: &[&str], needle: &str) {
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
    assert!(
        err.contains(needle),
        "expected {name} error to contain {needle:?}, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded expectation,
/// scalac's stdout and ours all have to agree.
fn real_scalac_dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff {name}: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );

    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        reference,
        "stdout differs from real scalac for {name}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// Nothing in `ovl2.scala` needs the scala-library: the candidate-set rules
/// are the language's, and `java.lang.String`'s interfaces come from the JDK,
/// which both modes link against.
#[test]
fn fixtures_ovl2_private_runtime() {
    let out = compile_fixture_with("ovl2", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout("ovl2"),
            "stdout mismatch for private-runtime ovl2"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_ovl2_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run ovl2: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ovl2", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("ovl2"),
        "stdout mismatch for library-ABI ovl2"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_ovl2() {
    real_scalac_dual_run("ovl2");
}

#[test]
fn fixtures_ovl2_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run ovl2_lib: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ovl2_lib", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("ovl2_lib"),
        "stdout mismatch for library dual-run ovl2_lib"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_ovl2_lib() {
    real_scalac_dual_run("ovl2_lib");
}

/// `math.min` and `ArrayBuffer`'s constructors are the library's, not ours:
/// `--no-scala-library` has to keep diagnosing them rather than inventing a
/// call the private runtime cannot back.
#[test]
fn fixtures_ovl2_lib_without_library_is_error() {
    compile_fails(
        "ovl2_lib",
        &["--no-scala-library"],
        "value ArrayBuffer is not a member of package scala.collection.mutable",
    );
}

/// A parent's bare constructor parameter is `private[this]`, so it is not in
/// scope in the subclass -- exactly what scalac 2.13.16 reports.
#[test]
fn fixtures_ovl2_bad_private_this_ctor_param() {
    compile_fails("ovl2_bad", &["--no-scala-library"], "not found: value tag");
}
