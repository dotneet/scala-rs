//! Type members and aliases that take type parameters, and higher-kinded
//! context bounds.
//!
//! Each accepted fixture is also compiled with scalac 2.13.16 and both binaries
//! are run, so a divergence in what the program prints is ours. The rejected
//! fixtures pin diagnostics that scalac 2.13.16 also produces:
//!
//!   * `def f[F[_] <% V]` → `type F takes type parameters` (a *view* bound on a
//!     higher-kinded parameter is illegal; a *context* bound is not),
//!   * `Missing[Int]` → `not found: type Missing` (an unresolved name applied to
//!     type arguments is a missing type, not a kind error),
//!   * `type C[T] = Int` against `type C[T] <: Bound[T]` → still incompatible.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-tmember-{tag}-{}-{nanos}-{seq}",
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

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    let which = Command::new("which").arg("scalac").output().ok()?;
    which
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&which.stdout).trim().to_string()))
}

fn diagnostics(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    )
}

fn run_java_verified(cp: &str) -> String {
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

fn compile_ours(name: &str, jar: &Path) -> PathBuf {
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
        output.status.success(),
        "compile {name} failed:\n{}",
        diagnostics(&output)
    );
    out
}

/// Compile with scala-rs, run `Main`, and compare against the fixture.
fn check(name: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not obtainable");
        return;
    };
    let out = compile_ours(name, &jar);
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        assert_eq!(
            run_java_verified(&cp),
            expected_stdout(name),
            "stdout {name}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Differential: scalac and scala-rs must make `Main` print the same thing.
fn same_as_scalac(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip differential {name}: scalac or scala-library not available");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac"));
    let status = Command::new(&scalac)
        .args(["-d", ref_out.to_str().unwrap(), src.to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed to compile {name}");
    let expected = run_java_verified(&format!("{}:{}", ref_out.display(), jar.display()));

    let out = compile_ours(name, &jar);
    let actual = run_java_verified(&format!("{}:{}", out.display(), jar.display()));
    assert_eq!(actual, expected, "stdout differs from scalac for {name}");
    // The checked-in fixture is the same text, so a scalac upgrade cannot drift
    // the two tests apart silently.
    assert_eq!(expected, expected_stdout(name), "fixture stale for {name}");
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails_with(name: &str, needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not obtainable");
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
    let err = diagnostics(&output);
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------- parameterized type members

/// `type C[T] <: Bound[T]` declared in one trait, implemented by
/// `type C[T] = Impl[T]` in another, plus `type C[T] = self.C[T]` through a
/// self-type and a context bound whose bound is a parameterized member.
#[test]
fn tmember1_parameterized_members_run() {
    check("tmember1");
}

#[test]
fn tmember1_matches_scalac() {
    same_as_scalac("tmember1");
}

// ------------------------------------------------ higher-kinded context bound

/// `[F[_]: Async]` on a def and on a class, and a term named `F` that must not
/// hide the type parameter `F`.
#[test]
fn tmember2_hk_context_bounds_run() {
    check("tmember2");
}

#[test]
fn tmember2_matches_scalac() {
    same_as_scalac("tmember2");
}

// ------------------------------------- wildcards in bounds and `#` projections

#[test]
fn tmember3_wildcard_bounds_run() {
    check("tmember3");
}

#[test]
fn tmember3_matches_scalac() {
    same_as_scalac("tmember3");
}

// ------------------------------------------------------------- rejected forms

/// A *view* bound on a higher-kinded parameter stays illegal.
#[test]
fn hk_view_bound_is_rejected() {
    compile_fails_with("tmember_bad", "type F takes type parameters");
}

/// An unknown name applied to type arguments is `not found: type X`.
#[test]
fn unknown_applied_type_is_not_found() {
    compile_fails_with("tmember_bad2", "not found: type Missing");
}

/// Aligning the parent's type parameters must not weaken the bound check.
#[test]
fn parameterized_member_bound_is_still_checked() {
    compile_fails_with("tmember_bad3", "incompatible type in overriding type C");
}
