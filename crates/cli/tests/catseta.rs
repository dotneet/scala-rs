//! E2E tests for the `agent/catseta` slice: eta-expanding a polymorphic
//! method where a function type is expected, in an argument position.
//!
//! cats' monad transformers are built out of one shape:
//!
//! ```scala
//! def apply[F[_], A](fa: F[A])(implicit F: Functor[F]): IorT[F, A, B] =
//!   IorT(F.map(fa)(Ior.left))
//! ```
//!
//! `map`'s own `B` is undetermined while its function argument is typed, so
//! the expected type handed to `Ior.left` is `A => _`. That pins `Ior.left`'s
//! `A` and says nothing about its `B`, and the eta-expansion used to carry
//! that leftover `B` *symbol* onwards as though it were a fixed type. It
//! travelled through `map`'s result into `IorT[F, A, B]`, where it printed
//! under the same name as the class's own `B` while being a different symbol:
//! `type mismatch; found: IorT[F, A, B]  required: IorT[F, A, B]`.
//!
//! nsc eta-expands into an untyped `x$1 => Ior.left(x$1)` and types that, so
//! the application's unsolved parameters join the enclosing context's
//! `undetparams` and the outer expected type is what fixes them.
//! `record_eta_undet` puts them on the same footing here.
//!
//! Two things follow, and both are tested:
//!
//! * A variable is still bounded. The places that turn a variable into a type
//!   (`solve_eta_tparams`, `instantiate_undet_arg`, `solve_undet_result`) now
//!   refuse a solution the parameter's own bounds refuse; `Box(Inv.make(a))`
//!   with `def make[A, B <: Number]` and a declared `Box[Inv[A, String]]` was
//!   accepted before, where the same call without the wrapper was not.
//! * When found and required print the same string, the message names the
//!   owner of each ambiguous type parameter, the way nsc writes
//!   `A(in method make)`.
//!
//! Fixtures use the `ce_` prefix. Kept out of `crates/cli/tests/e2e.rs` to
//! avoid merge conflicts; see `.agent-brief.md`.

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
        "scala-rs-catseta-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn run_java(out: &Path, cp_extra: &str, main: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
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

fn compile_fails(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
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
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

fn scalac_rejects(name: &str, needles: &[&str]) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir(&format!("scalac-{name}"));
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join(format!("{name}.scala")))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted {name}");
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected scalac's {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The values are printed, so an instantiation that merely makes the file
/// compile (`Nothing`, `Any`) cannot pass for a fix.
#[test]
fn fixtures_ce_etainfer() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = tmp_dir("etainfer");
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("ce_etainfer.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile ce_etainfer failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar_s, "Main"),
        expected_stdout("ce_etainfer")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The expected output is nsc's: scalac 2.13.16 accepts every shape in the
/// fixture, so each one is a gap of ours and not a property of the code.
#[test]
fn scalac_agrees_ce_etainfer_output() {
    let (Some(sc), true) = (scalac(), java_available()) else {
        eprintln!("skip: scalac or java not available");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let out = tmp_dir("scalac-etainfer");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("ce_etainfer.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected ce_etainfer:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str().unwrap(), "Main"),
        expected_stdout("ce_etainfer")
    );
    let _ = fs::remove_dir_all(&out);
}

/// A leftover that becomes a variable is still solved from what encloses it,
/// and the solution still has to conform.
#[test]
fn fixtures_ce_etainfer_bad_is_rejected() {
    compile_fails(
        "ce_etainfer_bad",
        &[
            "found: IorT[F, A, B]  required: IorT[F, B, A]",
            "found: (Int) => Ior[Int, Int]  required: (Int) => Ior[String, Int]",
        ],
    );
}

#[test]
fn scalac_agrees_ce_etainfer_bad_is_rejected() {
    scalac_rejects(
        "ce_etainfer_bad",
        &["required: A => Ior[B,A]", "required: Ior[String,Int]"],
    );
}

/// A variable is still bounded, and a message that would print the same
/// string twice says which type parameter is whose.
///
/// In its own file because nsc stops reporting the bounds violation as soon
/// as the same object carries an unrelated type mismatch.
#[test]
fn fixtures_ce_etabound_bad_is_rejected() {
    compile_fails(
        "ce_etabound_bad",
        &[
            "found: (A) => Inv[A, B]  required: (A) => Inv[A, String]",
            "found: (A) => Inv[A, B (defined in method make)]  \
             required: (A) => Inv[A, B (defined in method shadowed)]",
        ],
    );
}

#[test]
fn scalac_agrees_ce_etabound_bad_is_rejected() {
    scalac_rejects(
        "ce_etabound_bad",
        &[
            "type arguments [A,String] do not conform to method make's type parameter bounds",
            "type arguments [A,B] do not conform to method make's type parameter bounds",
        ],
    );
}
