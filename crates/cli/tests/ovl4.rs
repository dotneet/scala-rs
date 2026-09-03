//! E2E tests for the `agent/ovl4` slice: six independent roots behind slick's
//! `no matching overload` / `ambiguous overload` cluster.
//!
//! * a rigid type parameter *argument* was scored applicable to every
//!   parameter, so every alternative of a Java overload set matched and
//!   `String.valueOf(value)` for a `value: R` was `ambiguous overload`;
//!   the rigid parameter is now weighed through its *upper bound*;
//! * a compound parameter mentioning the callee's own type parameter
//!   (slick's `type BaseColumnType[T] = ScalaType[T] with BaseTypedType[T]`)
//!   was applicable to nothing, and neither `class_ctor_matches_typeparam_args`
//!   nor `unify_one` looked inside a refinement;
//! * a *monomorphic* callee handed its argument no expected type, so an
//!   argument whose own type parameters are inferred settled them from itself:
//!   `takeBox(Box(n))` for a `takeBox(b: Box[Any])` was `no matching overload
//!   … with arguments (Box[String])`;
//! * a rigid type parameter argument now reaches inference through its upper
//!   bound as well, so `mapOrNone(c.fetch)` for a `fetch: F <: Option[String]`
//!   solves `A = String` instead of `Any`;
//! * constructors are not inherited: `resolve_overload` rebuilt the
//!   alternative list with `lookup_member`, which walks the parents, so
//!   `new java.util.Properties(null)` was ambiguous between
//!   `Properties(Properties)` and `Hashtable(Map)`;
//! * an argument whose class is still a `-cp` stub (`parents = [AnyRef]`)
//!   conforms to nothing, and overload scoring runs on `&self` and cannot read
//!   a classfile -- `new OutputStreamWriter(System.out)` asked before anything
//!   had read `java/io/PrintStream`.
//!
//! Kept in its own test binary rather than appended to `e2e.rs`, per
//! `.agent-brief.md`. Every case lives in the one `ovl4` fixture: a scala-rs
//! run costs 0.4 s but a real-scalac one costs 1.8 s, so fixtures are wide,
//! not many.

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
        "scala-rs-ovl4-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: a wrongly picked overload whose erased descriptor differs
/// would be a `VerifyError` here, not a silent difference in the output.
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

/// Nothing here is library-only: `Option`, `String.valueOf`, `java.util` and
/// `java.io` are all backed by the private runtime too.
#[test]
fn fixtures_ovl4_private_runtime() {
    let out = compile_fixture_with("ovl4", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout("ovl4"),
            "stdout mismatch for private-runtime ovl4"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_ovl4_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run ovl4: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ovl4", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("ovl4"),
        "stdout mismatch for library-ABI ovl4"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_ovl4() {
    real_scalac_dual_run("ovl4");
}

/// The other side of the rule: a rigid `T` is only what its bounds say it is,
/// so it does not inhabit a `List[Int]` parameter. Real scalac 2.13.16 rejects
/// this too (`type mismatch; found: T  required: List[Int]`); scoring it
/// applicable is what made `String.valueOf(r)` ambiguous.
#[test]
fn ovl4_bad_type_param_argument_is_rejected() {
    compile_fails(
        "ovl4_bad",
        &[
            "--scala-library",
            "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
        ],
        "no matching overload for (List[Int])Int with arguments (T)",
    );
}

#[test]
fn ovl4_bad_type_param_argument_is_rejected_private_runtime() {
    compile_fails(
        "ovl4_bad",
        &["--no-scala-library"],
        "no matching overload for (List[Int])Int with arguments (T)",
    );
}
