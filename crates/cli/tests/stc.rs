//! Regression for named/default constructor arguments supplied by a classfile.
//!
//! `SlickTreeException` is the shape that stopped slick-testkit: the omitted
//! `parent` parameter has a default, while later named arguments supply a
//! function-valued `mark` and `removeUnmarked`. The constructor's default RHSs
//! are not in the parameter symbols when the class was read from a classfile;
//! scala-rs must call the emitted default getters before typing the lambda.

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
    let dir = std::env::temp_dir().join(format!("scala-rs-stc-{tag}-{nanos}-{seq}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{cp_extra}", out.display());
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

/// Real scalac establishes the reference, then scala-rs compiles the same
/// user source against those external classfiles. This exercises both default
/// getter lookup and expected-type propagation into the named lambda.
#[test]
fn external_ctor_named_defaults_type_lambda() {
    if !java_available() {
        return;
    }
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip stc dual-run: scalac, scala-library, or Java unavailable");
        return;
    };
    let lib = tmp_dir("lib");
    let reference = tmp_dir("reference");
    let ours = tmp_dir("ours");
    let lib_src = fixtures_dir().join("stc_lib.scala");
    let use_src = fixtures_dir().join("stc_use.scala");
    let jar_s = jar.to_str().unwrap();

    let status = Command::new(&sc)
        .args([lib_src.to_str().unwrap(), "-d", lib.to_str().unwrap()])
        .status()
        .expect("scalac stc_lib");
    assert!(status.success(), "scalac failed to compile stc_lib");

    let status = Command::new(&sc)
        .args([
            use_src.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            reference.to_str().unwrap(),
        ])
        .status()
        .expect("scalac stc_use");
    assert!(status.success(), "scalac failed to compile stc_use");
    let expected = run_java(&reference, &format!("{}:{jar_s}", lib.display()));

    let output = Command::new(bin())
        .args([
            "compile",
            use_src.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            ours.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("scala-rs stc_use");
    assert!(
        output.status.success(),
        "scala-rs failed to compile stc_use: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        run_java(&ours, &format!("{}:{jar_s}", lib.display())),
        expected,
        "scala-rs output differs from real scalac"
    );

    let _ = fs::remove_dir_all(lib);
    let _ = fs::remove_dir_all(reference);
    let _ = fs::remove_dir_all(ours);
}

/// A selected stable accessor remains a term prefix in type position. Real
/// scalac first proves that the scala-rs producer exported the path correctly;
/// scala-rs must then accept the same consumer and construct the inner class
/// with the selected value as its enclosing instance.
#[test]
fn external_stable_accessor_is_type_prefix() {
    if !java_available() {
        return;
    }
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip stc stable-prefix dual-run: scalac, scala-library, or Java unavailable");
        return;
    };
    let lib = tmp_dir("stable-lib");
    let reference = tmp_dir("stable-reference");
    let ours = tmp_dir("stable-ours");
    let lib_src = fixtures_dir().join("stc_stable_lib.scala");
    let use_src = fixtures_dir().join("stc_stable_use.scala");
    let jar_s = jar.to_str().unwrap();

    let output = Command::new(bin())
        .args([
            "compile",
            lib_src.to_str().unwrap(),
            "-d",
            lib.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("scala-rs stc_stable_lib");
    assert!(
        output.status.success(),
        "scala-rs failed to compile stc_stable_lib: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let status = Command::new(&sc)
        .args([
            use_src.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            reference.to_str().unwrap(),
        ])
        .status()
        .expect("scalac stc_stable_use");
    assert!(
        status.success(),
        "scalac failed to read scala-rs stable path"
    );
    let expected = run_java(&reference, &format!("{}:{jar_s}", lib.display()));

    let output = Command::new(bin())
        .args([
            "compile",
            use_src.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            ours.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("scala-rs stc_stable_use");
    assert!(
        output.status.success(),
        "scala-rs failed to compile stc_stable_use: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        run_java(&ours, &format!("{}:{jar_s}", lib.display())),
        expected,
        "scala-rs stable-prefix output differs from real scalac"
    );

    let _ = fs::remove_dir_all(lib);
    let _ = fs::remove_dir_all(reference);
    let _ = fs::remove_dir_all(ours);
}

/// A zero-argument Java method has an explicit empty parameter clause even
/// though the classfile representation shares scala-rs's internal shape for a
/// Scala parameterless method. The `JAVA` symbol flag must preserve that
/// distinction when a `Function0` is expected, without accepting a different
/// function arity.
#[test]
fn java_zero_arg_method_eta_expands_with_function0_expected() {
    if !java_available() {
        return;
    }
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip Java nullary eta dual-run: scalac, scala-library, or Java unavailable");
        return;
    };
    let reference = tmp_dir("java-nullary-reference");
    let ours = tmp_dir("java-nullary-ours");
    let bad_reference = tmp_dir("java-nullary-bad-reference");
    let bad_ours = tmp_dir("java-nullary-bad-ours");
    let src = fixtures_dir().join("stc_java_nullary_eta.scala");
    let bad_src = fixtures_dir().join("stc_java_nullary_eta_bad.scala");
    let jar_s = jar.to_str().unwrap();

    let status = Command::new(&sc)
        .args([src.to_str().unwrap(), "-d", reference.to_str().unwrap()])
        .status()
        .expect("scalac stc_java_nullary_eta");
    assert!(status.success(), "scalac rejected Java nullary eta");
    let expected = run_java(&reference, jar_s);

    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            ours.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("scala-rs stc_java_nullary_eta");
    assert!(
        output.status.success(),
        "scala-rs rejected Java nullary eta: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        run_java(&ours, jar_s),
        expected,
        "scala-rs Java nullary eta output differs from real scalac"
    );

    let status = Command::new(&sc)
        .args([
            bad_src.to_str().unwrap(),
            "-d",
            bad_reference.to_str().unwrap(),
        ])
        .status()
        .expect("scalac stc_java_nullary_eta_bad");
    assert!(
        !status.success(),
        "scalac unexpectedly accepted wrong eta arity"
    );
    let output = Command::new(bin())
        .args([
            "compile",
            bad_src.to_str().unwrap(),
            "-d",
            bad_ours.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("scala-rs stc_java_nullary_eta_bad");
    assert!(
        !output.status.success(),
        "scala-rs unexpectedly accepted wrong eta arity"
    );

    let _ = fs::remove_dir_all(reference);
    let _ = fs::remove_dir_all(ours);
    let _ = fs::remove_dir_all(bad_reference);
    let _ = fs::remove_dir_all(bad_ours);
}
