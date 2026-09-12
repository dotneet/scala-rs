//! What the cats and gitbucket *execution* harnesses found.
//!
//! `tests/cats_run.sh` and `tests/gitbucket_run.sh` compile each project twice
//! (scala-rs and real scalac 2.13.16) and run ordinary client programs against
//! both builds, comparing stdout byte for byte. Until they existed, nothing had
//! executed one instruction of either project: every defect reduced here
//! typechecks, emits, passes `tests/classfile_lint.py` and loads -- and then
//! fails the verifier, throws, or (for the pickle half) makes real scalac
//! unable to use our classfiles at all.
//!
//! Two kinds of test:
//!
//!  * `rh_*` fixtures run under the usual dual-run pattern -- compile with
//!    scala-rs, run under `java -Xverify:all`, compare with the recorded stdout,
//!    and a second test that the same source compiled by *scalac* prints it too;
//!  * `rh_pickle_*` is a **round trip**: the library is compiled by scala-rs and
//!    the client by real scalac *against our classfiles*, so it tests the
//!    `ScalaSignature` we write rather than the code. Its reference is the same
//!    client compiled against a scalac-built library.
//!
//! Fixture prefix: `rh_`.

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
        "scala-rs-rh-{tag}-{}-{nanos}-{seq}",
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

/// kind-projector, which `rh_pickle_lib` needs on the *scalac* side: `Val2[E, *]`
/// is the plugin's syntax and nsc rejects it without the plugin, exactly as it
/// rejects it here without `-Ykind-projector`.
fn kind_projector() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join(
        "Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/typelevel/\
         kind-projector_2.13.16/0.13.3/kind-projector_2.13.16-0.13.3.jar",
    );
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

#[test]
fn evidence_beside_an_explicit_implicit_clause_runs() {
    check_runs("rh_evidence", None);
}

#[test]
fn scalac_agrees_evidence_beside_an_explicit_implicit_clause() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("rh_evidence", Some(&sc));
}

#[test]
fn primitive_branches_of_a_reference_typed_if_box() {
    check_runs("rh_branchbox", None);
}

#[test]
fn scalac_agrees_primitive_branches_of_a_reference_typed_if() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("rh_branchbox", Some(&sc));
}

#[test]
fn a_tuple_binder_keeps_its_own_arity() {
    check_runs("rh_tuplearity", None);
}

#[test]
fn scalac_agrees_a_tuple_binder_keeps_its_own_arity() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("rh_tuplearity", Some(&sc));
}

#[test]
fn qualified_private_is_not_jvm_private() {
    check_runs("rh_qprivate", None);
}

#[test]
fn scalac_agrees_qualified_private_is_not_jvm_private() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("rh_qprivate", Some(&sc));
}

#[test]
fn a_this_type_result_is_cast_to_its_own_class() {
    check_runs("rh_thistype", None);
}

#[test]
fn scalac_agrees_a_this_type_result_is_cast_to_its_own_class() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("rh_thistype", Some(&sc));
}

/// Build `rh_pickle_lib` with `use_scalac` or with scala-rs, then compile
/// `rh_pickle_client` with **real scalac** against it and run it.
fn pickle_round_trip(use_scalac: bool) -> Option<String> {
    let jar = scala_library_jar()?;
    let sc = scalac()?;
    let kp = kind_projector()?;
    let dir = tmp_dir(if use_scalac { "pickle-sc" } else { "pickle-rs" });
    let lib = dir.join("lib");
    let cl = dir.join("client");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&cl).unwrap();
    let lib_src = fixtures_dir().join("rh_pickle_lib.scala");
    let cl_src = fixtures_dir().join("rh_pickle_client.scala");

    let built = if use_scalac {
        Command::new(&sc)
            .args(["-classpath", jar.to_str().unwrap()])
            .args(["-d", lib.to_str().unwrap()])
            .arg(format!("-Xplugin:{}", kp.display()))
            .arg(&lib_src)
            .output()
            .expect("run scalac")
    } else {
        Command::new(bin())
            .arg("compile")
            .arg(&lib_src)
            .args(["-d", lib.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .arg("-Ykind-projector")
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
        .arg(format!("-Xplugin:{}", kp.display()))
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

/// The round trip itself: real scalac must be able to compile *and* run the same
/// client against our classfiles and against its own, with the same output.
#[test]
fn scalac_can_use_our_pickles() {
    let Some(ours) = pickle_round_trip(false) else {
        eprintln!("skip: scalac, the scala-library jar or kind-projector is not present");
        return;
    };
    let theirs = pickle_round_trip(true).expect("scalac side available");
    assert_eq!(ours, theirs, "pickle round trip differs");
    assert!(ours.contains("Good(3)"), "unexpected output: {ours}");
}
