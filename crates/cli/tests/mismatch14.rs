//! E2E tests for the `agent/mismatch14` slice: slick's remaining
//! `no matching overload` / `type mismatch` reports.
//!
//! Four roots, none of them an overload set that could not be narrowed:
//!
//! * a *monomorphic* callee handed out no expected type for its arguments, so
//!   a function literal nested inside one (`f(if (c) { s => … } else { s => …;
//!   … })`, slick's `JdbcBackend`) had nothing to read its parameter types
//!   from -- the one-expression branch only ever worked because
//!   `section_param_types` recovers the parameter from the call inside it,
//! * an implicit conversion whose parameter is a generic *supertype* of the
//!   receiver (`mapAsScalaMapConverter[K, V](m: java.util.Map[K, V])` applied
//!   to a `ConfigObject`) solved its type parameters by zipping the receiver's
//!   own arguments, of which there are none, and fell back to `AnyRef`,
//! * `Any` written for a *Java* method's type parameter is the `Object` the
//!   parameter is really bounded by (nsc's `ObjectTpeJava`), which is what
//!   makes `java.util.Arrays.copyOf[Any](a: Array[AnyRef], n)` -- slick's
//!   `ConstArray` -- a legal call even though `Array` is invariant,
//! * an inherited result type mentioning an abstract type member was not read
//!   through the class that inherits it, so `StructNode.rebuild` returning a
//!   `StructNode` was `found: StructNode required: Node.Self`.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All new fixtures
//! use the `mism14` prefix.

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
        "scala-rs-mism14-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: a signature read differently would be a `VerifyError` here,
/// not a silent difference in the output.
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

/// Nothing here needs the library ABI: the private runtime has to accept the
/// same program and print the same thing.
#[test]
fn fixtures_mism14_private_runtime() {
    let out = compile_fixture_with("mism14", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout("mism14"),
            "stdout mismatch for private-runtime mism14"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_mism14_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run mism14: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("mism14", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("mism14"),
        "stdout mismatch for library-ABI mism14"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_mism14() {
    real_scalac_dual_run("mism14");
}

/// The `asScala` shape from slick's `GlobalConfig`: the receiver only
/// *extends* the `java.util.Map[K, V]` the conversion takes. Its converters
/// live in the library pickle, so this fixture is library-ABI only.
#[test]
fn fixtures_mism14_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run mism14_lib: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("mism14_lib", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("mism14_lib"),
        "stdout mismatch for library dual-run mism14_lib"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_mism14_lib() {
    real_scalac_dual_run("mism14_lib");
}

/// `scala.jdk.CollectionConverters` has no private-runtime backing: the
/// fixture has to be diagnosed there, not quietly accepted.
#[test]
fn fixtures_mism14_lib_without_library_is_error() {
    compile_fails(
        "mism14_lib",
        &["--no-scala-library"],
        "value asScala is not a member of Names",
    );
}

/// Reading `Any` as `Object` for a Java type parameter is not a licence to
/// ignore `Array`'s invariance: `copyOf[Any]` still takes an `Array[Object]`
/// only, and real scalac rejects the `Array[String]` too.
#[test]
fn fixtures_mism14_bad_array_is_still_invariant() {
    compile_fails(
        "mism14_bad",
        &["--no-scala-library"],
        "no matching overload for (Array[AnyRef], Int)Array[AnyRef] with arguments (Array[String], 3)",
    );
    if let Some(jar) = scala_library_jar() {
        compile_fails(
            "mism14_bad",
            &["--scala-library", jar.to_str().unwrap()],
            "no matching overload for (Array[AnyRef], Int)Array[AnyRef] with arguments (Array[String], 3)",
        );
    }
}
