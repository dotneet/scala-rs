//! E2E tests for the `agent/ovl3` slice: the `no matching overload` cluster in
//! slick.
//!
//! `no matching overload` is also what the typer prints when a *single*
//! candidate rejects its arguments, so a signature the prelude modelled
//! monomorphically reads like a missing alternative. Three roots, all of that
//! shape:
//!
//! * `Option.getOrElse` / `Option.orElse` / `Map.getOrElse` were declared
//!   without their `[B >: A]` (`[V1 >: V]`) type parameter, so
//!   `(o: Option[Sub]).getOrElse(aBase)` was an argument of the wrong type
//!   rather than a call whose result widens to `Base`,
//! * `mutable.HashSet` / `mutable.HashMap` extended `AnyRef` and nothing else,
//!   so a `scala.collection.Set` / `Map` parameter rejected them,
//! * the view that makes an `Option` an `IterableOnce`
//!   (`Option.option2Iterable`) lives only in the library pickle, and the
//!   applicability test cannot read a class file, so `Seq("a") ++ anOption`
//!   found no conversion unless some earlier line in the same file had already
//!   warmed `Option`'s implicit scope.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All new fixtures
//! use the `o3` prefix.

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
        "scala-rs-ovl3-{tag}-{}-{nanos}-{seq}",
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
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

/// `-Xverify:all`: a widened signature that changed an erased descriptor would
/// be a `VerifyError` here, not a silent difference in the output.
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

/// `Option.getOrElse` / `orElse` are backed by the private runtime too, so the
/// widened signatures have to work in both modes.
#[test]
fn fixtures_o3_private_runtime() {
    let out = compile_fixture_with("o3", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout("o3"),
            "stdout mismatch for private-runtime o3"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_o3_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run o3: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("o3", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("o3"),
        "stdout mismatch for library-ABI o3"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_o3() {
    real_scalac_dual_run("o3");
}

#[test]
fn fixtures_o3_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run o3_lib: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("o3_lib", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("o3_lib"),
        "stdout mismatch for library dual-run o3_lib"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_o3_lib() {
    real_scalac_dual_run("o3_lib");
}

/// `mutable.HashSet` and `scala.collection.Set`'s members are the library's:
/// `--no-scala-library` still has to diagnose them rather than invent a call
/// the private runtime cannot back.
#[test]
fn fixtures_o3_lib_without_library_is_error() {
    compile_fails(
        "o3_lib",
        &["--no-scala-library"],
        "value size is not a member of Set[String]",
    );
}

/// Widening is to the *lub*, not to anything the argument asks for:
/// `Option[Int].getOrElse("no")` is an `Any`, and scalac rejects it too.
#[test]
fn fixtures_o3_bad_get_or_else_widens_to_lub() {
    compile_fails(
        "o3_bad",
        &["--no-scala-library"],
        "type mismatch; found: Any  required: Int",
    );
    if let Some(jar) = scala_library_jar() {
        compile_fails(
            "o3_bad",
            &["--scala-library", jar.to_str().unwrap()],
            "type mismatch; found: Any  required: Int",
        );
    }
}
