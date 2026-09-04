//! E2E tests for the `agent/genrep` slice, driven by the sources slick's
//! build generates from its FreeMarker templates:
//!
//! * a class type parameter whose bound names an imported type
//!   (`class Boxed[T <: Rep[_]]` under `import genrep.lifted._`),
//! * `implicit class` with type parameters, whose synthetic conversion has to
//!   carry them,
//! * `TupleN extends Product with Serializable`,
//! * one `apply` on `scala.collection.Seq`, not an ambiguous overload,
//! * an argument list adapted into a tuple (`Some(a, b)`),
//! * a class merely *named* like a tuple (`TupleOps2`, slick's `TupleShape`)
//!   staying a class of its own,
//! * `package p { … }` followed by top-level definitions.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All new
//! fixtures use the `genrep` prefix.

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
        "scala-rs-genrep-{tag}-{}-{nanos}-{seq}",
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

fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
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

/// Our build of `genrep.scala` runs against the real scala-library and
/// prints what the recorded expectation says.
#[test]
fn scala_library_dual_run_genrep() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run genrep: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("genrep", &["--scala-library", jar_s]);
    // `package genrep { … }` followed by `object Main`: the object is a
    // top-level sibling of the package, so its classfile is `Main.class`, not
    // `genrep/Main.class`.
    assert!(
        out.join("Main.class").is_file(),
        "top-level Main after a braced package clause landed elsewhere"
    );
    assert_eq!(
        run_java(&out, jar_s),
        expected_stdout("genrep"),
        "stdout mismatch for library dual-run genrep"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded
/// expectation, scalac's stdout and ours all have to agree. The point of the
/// fixture is *which* overload and *which* adaptation get chosen, so a shared
/// misreading would go unnoticed without this.
#[test]
fn real_scalac_dual_run_genrep() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff genrep: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("genrep.scala");
    let ref_out = tmp_dir("genrep-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile genrep");
    let ref_cp = format!("{}:{}", ref_out.display(), jar.display());
    let reference = Command::new("java")
        .args(["-cp", &ref_cp, "Main"])
        .output()
        .expect("java (real scalac build)");
    assert!(
        reference.status.success(),
        "java Main (real-scalac build) failed: {}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let reference = String::from_utf8_lossy(&reference.stdout).to_string();
    assert_eq!(
        reference,
        expected_stdout("genrep"),
        "recorded expectation for genrep does not match real scalac"
    );

    let out = compile_fixture_with("genrep", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, jar.to_str().unwrap()),
        reference,
        "stdout differs from real scalac for genrep"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// The namer resolves a class type parameter's bound before the unit's
/// imports exist, so it stays silent about what it cannot find there. A bound
/// that names nothing at all must still be reported, by the signature pass.
#[test]
fn fixtures_genrep_bound_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip genrep_bound_bad: jar not obtainable");
        return;
    };
    compile_fails(
        "genrep_bound_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "not found: type Nope",
    );
}

/// Packing an argument list into a tuple is a last resort, not a way to make
/// a wrong call compile.
#[test]
fn fixtures_genrep_tuple_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip genrep_tuple_bad: jar not obtainable");
        return;
    };
    compile_fails(
        "genrep_tuple_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "no matching overload for (Int)Int with arguments (1, 2)",
    );
}

/// `TupleN extends Product with Serializable` is linked from the jar. The
/// private runtime has neither interface, so `--no-scala-library` must keep
/// diagnosing a tuple used as a `Product` instead of accepting it.
///
/// The wording changed with `agent/accepttoomuch`, which made a written type
/// annotation resolve strictly. `Product` is not a symbol at all without the
/// jar, so the annotation itself is now what is reported; before, the name
/// stayed an unresolved `Type::Named` placeholder and the tuple was found not
/// to conform to it. Both are the same fact -- the private runtime has no
/// `Product` -- and naming the missing type is the more direct of the two.
#[test]
fn fixtures_genrep_product_bad_without_library_is_error() {
    compile_fails(
        "genrep_product_bad",
        &["--no-scala-library"],
        "not found: type Product",
    );
}
