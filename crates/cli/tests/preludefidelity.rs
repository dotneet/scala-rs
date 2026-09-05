//! E2E tests for the prelude-fidelity slice: attributes the hand-written
//! prelude dropped that the library's own pickle carries.
//!
//! The gaps were found by comparing every class `install_prelude` builds
//! against its `ScalaSignature` in scala-library 2.13.16, and each one here is
//! kept because it made the compiler reject a program scalac accepts:
//!
//! 1. `Some`, `Left`, `Right`, `Success` and `Failure` are `case class`es and
//!    the prelude did not say so, so `Some(1).copy(value = 2)` was "value copy
//!    is not a member". (`javap -p` shows `copy` / `copy$default$1` on each.)
//! 2. `::` was a *second*, empty class symbol beside `$colon$colon`, entered
//!    under that name ahead of the real one: `val c: ::[Int]` was ":: does not
//!    take type parameters" and `new ::(1, Nil)` had no constructor.
//! 3. A qualified constructor pattern took its class from a lexical lookup of
//!    the last segment, so `Ior.Left(a)` found `scala.util.Left`. Latent while
//!    `scala.util.Left` had no `CASE` flag; giving it one made the wrong class
//!    win the constructor arm.
//! 4. A prelude method has parameter *types* and no parameter *symbols* (or,
//!    for the 150 built by `prelude_seq::poly_in`, symbols named `x$1`), so a
//!    named argument on any library method the prelude declares was
//!    "unimplemented syntax: named arguments (method parameters not
//!    resolved)". The names are read back from the pickle on demand.
//!
//! Every fixture is plain Scala 2.13, so each positive one is dual-run against
//! real scalac 2.13.16 and compared by output, and the negative one by the
//! lines both compilers reject.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `pf` prefix.

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
        "scala-rs-pf-{tag}-{}-{nanos}-{seq}",
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

/// Compile a fixture with scala-rs against the real scala-library and run it.
fn scala_rs_runs(name: &str, tag: &str) {
    let jar = scala_library_jar().expect("checked by the caller");
    let jar_s = jar.to_str().unwrap().to_string();
    let out = tmp_dir(tag);
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            &jar_s,
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(run_java(&out, &jar_s, "Main"), expected_stdout(name));
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac: the expected output is nsc's, not
/// this compiler's idea of it.
fn scalac_runs(name: &str, tag: &str) {
    let sc = scalac().expect("checked by the caller");
    let jar = scala_library_jar().expect("checked by the caller");
    let out = tmp_dir(tag);
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join(format!("{name}.scala")))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected {name}:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str().unwrap(), "Main"),
        expected_stdout(name)
    );
    let _ = fs::remove_dir_all(&out);
}

/// The compiler's whole output for a fixture it must reject.
fn rejected(name: &str, tag: &str, jar_mode: bool) -> String {
    let out = tmp_dir(tag);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        fixtures_dir()
            .join(format!("{name}.scala"))
            .to_str()
            .unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    if jar_mode {
        let jar = scala_library_jar().expect("checked by the caller");
        cmd.arg("--scala-library").arg(jar);
    } else {
        cmd.arg("--no-scala-library");
    }
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "{name} was accepted:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    err
}

/// The source lines real scalac 2.13.16 reports an error on, in order.
fn scalac_error_lines(name: &str, tag: &str) -> Vec<u32> {
    let sc = scalac().expect("checked by the caller");
    let out = tmp_dir(tag);
    let path = fixtures_dir().join(format!("{name}.scala"));
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(&path)
        .output()
        .expect("run scalac");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    let mut lines: Vec<u32> = text
        .lines()
        .filter(|l| l.contains(": error:"))
        .filter_map(|l| l.rsplit_once(".scala:").map(|(_, r)| r.to_string()))
        .filter_map(|r| r.split(':').next().and_then(|n| n.parse().ok()))
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// The lines scala-rs reports an error on, the same way.
fn scala_rs_error_lines(err: &str, stem: &str) -> Vec<u32> {
    let needle = format!("{stem}.scala:");
    let mut lines: Vec<u32> = err
        .lines()
        .filter_map(|l| l.rsplit_once(needle.as_str()).map(|(_, r)| r.to_string()))
        .filter_map(|r| r.split(':').next().and_then(|n| n.parse().ok()))
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

// ---------------------------------------------------------------------------
// Case-class flags, `::`, and qualified constructor patterns.

#[test]
fn fixtures_pf_case() {
    if !java_available() || scala_library_jar().is_none() {
        eprintln!("skip: java or the scala-library jar is not present");
        return;
    }
    scala_rs_runs("pf_case", "case");
}

#[test]
fn scalac_agrees_pf_case_output() {
    if !java_available() || scalac().is_none() || scala_library_jar().is_none() {
        eprintln!("skip: java, scalac or the scala-library jar is not present");
        return;
    }
    scalac_runs("pf_case", "scalac-case");
}

// ---------------------------------------------------------------------------
// Named arguments on the library's own methods.

#[test]
fn fixtures_pf_named() {
    if !java_available() || scala_library_jar().is_none() {
        eprintln!("skip: java or the scala-library jar is not present");
        return;
    }
    scala_rs_runs("pf_named", "named");
}

#[test]
fn scalac_agrees_pf_named_output() {
    if !java_available() || scalac().is_none() || scala_library_jar().is_none() {
        eprintln!("skip: java, scalac or the scala-library jar is not present");
        return;
    }
    scalac_runs("pf_named", "scalac-named");
}

/// Without a jar there is no pickle to read the names out of, so the compiler
/// reports what it cannot do instead of accepting the call.
#[test]
fn pf_named_nolib_is_rejected() {
    let err = rejected("pf_named_nolib", "named-nolib", false);
    assert!(
        err.contains("named arguments (method parameters not resolved)"),
        "the private runtime carries no parameter names:\n{err}"
    );
}

// ---------------------------------------------------------------------------
// What stays rejected.

#[test]
fn pf_case_bad_is_rejected() {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    let err = rejected("pf_case_bad", "case-bad", true);
    assert!(
        err.contains("unknown parameter name: x"),
        "`Some`'s field is `value`, and `copy` has exactly the class's fields:\n{err}"
    );
    assert!(
        err.contains("unknown parameter name: extra"),
        "`copy` takes no parameter the class does not declare:\n{err}"
    );
    assert!(
        err.contains("unknown parameter name: g"),
        "`map`'s parameter is `f`, which is the name the pickle gives it:\n{err}"
    );
    assert!(
        err.contains("too many type arguments for $colon$colon: expected 1, found 2"),
        "`::` takes one type parameter:\n{err}"
    );
    assert_eq!(
        scala_rs_error_lines(&err, "pf_case_bad"),
        vec![8, 10, 12, 14, 16]
    );
}

/// nsc rejects exactly the same five lines.
#[test]
fn scalac_agrees_pf_case_bad_is_rejected() {
    if scalac().is_none() {
        eprintln!("skip: scalac not available");
        return;
    }
    assert_eq!(
        scalac_error_lines("pf_case_bad", "scalac-case-bad"),
        vec![8, 10, 12, 14, 16]
    );
}
