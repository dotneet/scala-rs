//! The five known failures the cats and gitbucket *execution* harnesses carried.
//!
//! `tests/cats_run.sh` reported `progs=8 ok=4` and `tests/gitbucket_run.sh`
//! `progs=6 ok=5` with a ledger of four plus one expected failures
//! (`docs/notes/rh-cats-gitbucket-run.md`). Every one of them typechecks, emits,
//! passes `tests/classfile_lint.py` and loads; what they fail is *running* the
//! code, or letting real scalac compile against it.
//!
//! Two kinds of test, the same two `crates/cli/tests/rh.rs` uses:
//!
//!  * `rhf_*` fixtures are dual-run -- compiled with scala-rs, run under
//!    `java -Xverify:all`, compared with the recorded stdout, and a second test
//!    compiles the same source with real *scalac* and asserts the same stdout;
//!  * `rhf_pickle_*` is a **round trip**: the library is compiled by scala-rs and
//!    the client by real scalac *against our class files*, so it tests the
//!    `ScalaSignature` we write rather than the code we emit. Its reference is the
//!    same client compiled against a scalac-built library.
//!
//! Fixture prefix: `rhf_`.

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
        "scala-rs-rhf-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn run_java(cp: &str, main: &str) -> String {
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, main])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java {main} failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `name` with scala-rs (or with `scalac_path`), run it, and compare with
/// the recorded stdout.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    };
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    assert_eq!(
        run_java(&cp, "Main"),
        expected,
        "stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// cats `Chains`: a module must not re-emit a `final` accessor its superclass
/// already carries for a mixed-in trait's `val`.
#[test]
fn a_superclass_keeps_its_own_mixin_members() {
    check_runs("rhf_finalmixin", None);
}

#[test]
fn scalac_agrees_a_superclass_keeps_its_own_mixin_members() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("rhf_finalmixin", Some(&sc));
}

/// cats `NaturalTransforms`: a pattern's extractor reads the enclosing instance,
/// so the lambda the `for` desugars to captures it.
#[test]
fn a_pattern_extractor_captures_the_enclosing_instance() {
    check_runs("rhf_patternouter", None);
}

#[test]
fn scalac_agrees_a_pattern_extractor_captures_the_enclosing_instance() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("rhf_patternouter", Some(&sc));
}

/// gitbucket `Utils`: an implicit conversion whose parameter is a cake's inner
/// class solves its type parameters from the receiver.
#[test]
fn a_view_solves_its_parameters_through_a_prefix() {
    check_runs("rhf_viewtargs", None);
}

#[test]
fn scalac_agrees_a_view_solves_its_parameters_through_a_prefix() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("rhf_viewtargs", Some(&sc));
}

/// Build the pickle library with `use_scalac` or with scala-rs, then compile
/// `rhf_pickle_client` with **real scalac** against it and run it.
fn pickle_round_trip(use_scalac: bool) -> Option<String> {
    let jar = scala_library_jar()?;
    let sc = scalac()?;
    let dir = tmp_dir(if use_scalac { "pickle-sc" } else { "pickle-rs" });
    let lib = dir.join("lib");
    let cl = dir.join("client");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&cl).unwrap();
    let lib_src = fixtures_dir().join("rhf_pickle_lib.scala");
    let alias_src = fixtures_dir().join("rhf_pickle_alias.scala");
    let cl_src = fixtures_dir().join("rhf_pickle_client.scala");

    let built = if use_scalac {
        Command::new(&sc)
            .args(["-classpath", jar.to_str().unwrap()])
            .args(["-d", lib.to_str().unwrap()])
            .arg(&lib_src)
            .arg(&alias_src)
            .output()
            .expect("run scalac")
    } else {
        Command::new(bin())
            .arg("compile")
            .arg(&lib_src)
            .arg(&alias_src)
            .args(["-d", lib.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile")
    };
    assert!(
        built.status.success(),
        "library compile failed (scalac={use_scalac}):\n{}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );

    let cp = format!("{}:{}", lib.display(), jar.display());
    let client = Command::new(&sc)
        .args(["-classpath", &cp])
        .args(["-d", cl.to_str().unwrap()])
        .arg(&cl_src)
        .output()
        .expect("run scalac");
    assert!(
        client.status.success(),
        "real scalac could not compile the client against the {} library:\n{}{}",
        if use_scalac {
            "scalac-built"
        } else {
            "scala-rs-built"
        },
        String::from_utf8_lossy(&client.stdout),
        String::from_utf8_lossy(&client.stderr)
    );

    let run_cp = format!("{}:{}:{}", cl.display(), lib.display(), jar.display());
    let out = run_java(&run_cp, "Main");
    let _ = fs::remove_dir_all(&dir);
    Some(out)
}

/// cats `Chains` / `Monoids` / `Transformers` and gitbucket `Utils` on the pickle
/// axis: real scalac must be able to compile *and* run the same client against
/// our class files and against its own, with the same output.
#[test]
fn scalac_can_use_our_newtype_and_refinement_pickles() {
    let Some(ours) = pickle_round_trip(false) else {
        eprintln!("skip: scalac or the scala-library jar is not present");
        return;
    };
    let theirs = pickle_round_trip(true).expect("scalac side available");
    assert_eq!(ours, theirs, "pickle round trip differs");
    assert!(ours.contains("List(1, 2, 3)"), "unexpected output: {ours}");
    assert!(ours.contains("round"), "unexpected output: {ours}");
}
