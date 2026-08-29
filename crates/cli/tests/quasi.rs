//! Quasiquotes (`q"..."` / `tq"..."` / `pq"..."` / `cq"..."`) and the
//! reflection-API groundwork they sit on. See `docs/macros.md` §6.2 and
//! `crates/typer/src/quasiquote.rs`.
//!
//! Quasiquotes are **not** ordinary library macros: `scala-reflect.jar` holds
//! no implementation for them, so nsc short-circuits to a compiler-internal
//! one and scala-rs has to reify them itself. That is not implemented, and
//! these tests pin down the two things that are:
//!
//! 1. every quasiquote is diagnosed at its own span, distinguishing "this body
//!    uses syntax scala-rs cannot parse" from "the body is fine, the
//!    reification is missing" -- and a *user-defined* `q` interpolator is left
//!    alone;
//! 2. the pieces of the reflection API reached on the way there work, verified
//!    by a dual run against the real scalac: package-object members, `import
//!    <a value>._`, and applying a parameterless `def` whose result has an
//!    `apply` (`def Literal: LiteralExtractor`, then `Literal(...)`).

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
        "scala-rs-quasi-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    None
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

fn compile_lib(name: &str, out: &Path, jar: &Path) -> std::process::Output {
    Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile")
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
        .unwrap_or_else(|e| panic!("read expected/{name}.txt: {e}"))
}

/// Compiling `name` must fail, with every needle in the diagnostics.
fn compile_fails_lib_all(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not obtainable");
        return;
    };
    let out = tmp_dir(name);
    let output = compile_lib(name, &out, &jar);
    let err = diagnostics(&output);
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail, got: {err}"
    );
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {err:?}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// --- the groundwork, dual-run ---------------------------------------------

/// `quasi.scala` under our compiler: the recorded stdout has to match exactly.
///
/// It exercises the three gaps that stood between scala-rs and
/// `scala.reflect`'s universe: a package object's members read from a jar
/// (`scala.math.Pi` is a `val` on `scala/math/package$`, and the package it is
/// folded into has no runtime value -- this used to emit an `invokevirtual`
/// with nothing on the stack), `import <a value>._` (which is how `import
/// c.universe._` works at all), and applying a parameterless `def` whose
/// result has an `apply`.
#[test]
fn scala_library_dual_run_quasi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip quasi dual-run: scala-library jar not obtainable");
        return;
    };
    let out = tmp_dir("quasi");
    let output = compile_lib("quasi", &out, &jar);
    assert!(
        output.status.success(),
        "compile quasi failed: {}",
        diagnostics(&output)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java -Xverify:all Main failed for quasi: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        expected_stdout("quasi"),
        "stdout mismatch for quasi"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded expectation
/// and both compilers' output have to agree, or the fixture is only testing
/// what we happen to do.
#[test]
fn real_scalac_dual_run_quasi() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip quasi real-scalac diff: scalac or jar not obtainable");
        return;
    };
    let ref_out = tmp_dir("quasi-scalac-ref");
    let status = Command::new(&scalac)
        .args([
            fixtures_dir().join("quasi.scala").to_str().unwrap(),
            "-d",
            ref_out.to_str().unwrap(),
        ])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile quasi");
    let cp = format!("{}:{}", ref_out.display(), jar.display());
    let reference = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java (real scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed for quasi: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&reference.stdout),
        expected_stdout("quasi"),
        "recorded expectation for quasi does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

// --- the quasiquotes themselves, all diagnosed ----------------------------

/// Every quasiquote in `quasi_bad.scala` is reported, and reported for the
/// right reason. Silently accepting one would be worse than not having them:
/// the call site would type against a tree we never built.
#[test]
fn fixtures_quasi_bad_is_error() {
    compile_fails_lib_all(
        "quasi_bad",
        &[
            // A body with nothing to reify: the *syntax* is what is wrong.
            "unimplemented syntax: quasiquote q\"...\" (empty quasiquote)",
            // A body that parses: what is missing is reification.
            "cannot expand quasiquote q\"...\"",
            "cannot expand quasiquote tq\"...\"",
            "cannot expand quasiquote pq\"...\"",
            "cannot expand quasiquote cq\"...\"",
            "docs/macros.md",
        ],
    );
}

/// The old diagnostic was `value q is not a member of StringContext`, which is
/// simply untrue -- `q` is a member of `Quasiquotes.Quasiquote` -- and points
/// at the wrong thing to fix. It must not come back.
#[test]
fn quasiquote_is_not_reported_as_a_stringcontext_member() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let out = tmp_dir("quasi_bad_msg");
    let err = diagnostics(&compile_lib("quasi_bad", &out, &jar));
    for prefix in ["q", "tq", "pq", "cq"] {
        assert!(
            !err.contains(&format!("value {prefix} is not a member of StringContext")),
            "quasiquote {prefix} still reported as a StringContext member: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
