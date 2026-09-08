//! E2E tests for the `agent/libcaseeq` slice: the `canEqual(that: Any):
//! Boolean` that SLS 5.3.2 synthesizes for every `case class` and `case
//! object`.
//!
//! The *backend* has always emitted the method. The *typer* did not allocate a
//! symbol for it, so it did not know the class had it, and a case class whose
//! `Equals` declaration came from source — every `TupleN`, `Some`, `Left`,
//! `Right`, `Success`, `Failure` and `None` when `scala/scala`'s own
//! `src/library` is compiled — was reported "needs to be abstract. Missing
//! implementation for member of trait Equals". Against the prebuilt jar the
//! declaration arrives from a pickle, whose modifiers
//! `override_check::modifiers_are_known` withholds, so the gap did not show
//! there.
//!
//! What a synthesized `canEqual` *does* at run time decides what two values
//! compare equal, so the positive fixture executes and is diffed against real
//! scalac 2.13.16 compiling the same source, in both library modes.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` and
//! `crates/cli/tests/product.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `caseeq` prefix.

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
        "scala-rs-caseeq-{tag}-{}-{nanos}-{seq}",
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
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
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

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
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
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

fn javap(out: &Path, class: &str) -> String {
    let text = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    String::from_utf8_lossy(&text.stdout).into_owned()
}

/// Private runtime (`--no-scala-library`): nothing in the fixture names a
/// library type, so the synthesized `canEqual` has to work here too.
#[test]
fn fixtures_caseeq_private_runtime() {
    let out = compile_fixture_with("caseeq", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout("caseeq"),
            "stdout mismatch for private-runtime caseeq"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Library ABI (`--scala-library`), where `Product`/`Equals` come from the
/// pickle instead of from this run's sources.
#[test]
fn fixtures_caseeq_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip caseeq library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("caseeq", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("caseeq"),
        "stdout mismatch for library dual-run caseeq"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The recorded expectation *is* real scalac 2.13.16's stdout, and ours has to
/// match it byte for byte. `canEqual` decides equality at run time, so this is
/// the check that matters: a synthesized member that compiles and answers the
/// wrong thing would pass every other test in this file.
#[test]
fn real_scalac_dual_run_caseeq() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip caseeq real-scalac diff: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("caseeq.scala");
    let ref_out = tmp_dir("caseeq-scalac-ref");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile caseeq");
    let reference = run_java(&ref_out, jar.to_str().unwrap().into());
    assert_eq!(
        reference,
        expected_stdout("caseeq"),
        "recorded expectation for caseeq does not match real scalac"
    );
    let out = compile_fixture_with("caseeq", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, jar.to_str().unwrap().into()),
        reference,
        "stdout differs from real scalac for caseeq"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// A *plain* class that inherits the declaration is still abstract. This is
/// the same diagnostic `TupleN` used to get wrongly, so it has to keep firing
/// where it is right. scalac 2.13.16 prints the same three lines.
#[test]
fn fixtures_caseeq_bad() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip caseeq_bad: jar not obtainable");
        return;
    };
    compile_fails(
        "caseeq_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "class Plain needs to be abstract.",
            "Missing implementation for member of trait MyEquals:",
            "def canEqual(that: Any): Boolean = ???",
        ],
    );
    // Same in the private runtime: the check is on the source declaration, not
    // on anything the jar supplies.
    compile_fails(
        "caseeq_bad",
        &["--no-scala-library"],
        &["class Plain needs to be abstract."],
    );
}

/// And a non-case class does not acquire a `canEqual` member out of nowhere.
#[test]
fn fixtures_caseeq_member_bad() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip caseeq_member_bad: jar not obtainable");
        return;
    };
    compile_fails(
        "caseeq_member_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &["value canEqual is not a member of NoEquals"],
    );
}

/// The emitted shape. nsc gives the class `public boolean canEqual(Object)`
/// whose body is a bare `instanceof`, and the hand-written one in `Tagged`
/// must be the only `canEqual` on that class — the synthesizer must not add a
/// second, colliding method.
#[test]
fn caseeq_classfile_shape() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip caseeq_classfile_shape: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("caseeq", &["--scala-library", jar.to_str().unwrap()]);

    let point = javap(&out, "Point");
    assert!(
        point.contains("public boolean canEqual(java.lang.Object);"),
        "Point should carry canEqual:\n{point}"
    );
    assert!(
        point.contains("instanceof"),
        "Point.canEqual should be an instanceof test:\n{point}"
    );

    // A `case object`'s module class gets one too.
    let origin = javap(&out, "Origin$");
    assert!(
        origin.contains("public boolean canEqual(java.lang.Object);"),
        "Origin$ should carry canEqual:\n{origin}"
    );

    // Exactly one `canEqual` on the class with a hand-written one.
    let tagged = javap(&out, "Tagged");
    assert_eq!(
        tagged
            .matches("public boolean canEqual(java.lang.Object);")
            .count(),
        1,
        "Tagged should carry exactly one canEqual:\n{tagged}"
    );
    // ... and it is the hand-written `false`, not an `instanceof`.
    assert!(
        !tagged.contains("instanceof Tagged"),
        "Tagged.canEqual should be the hand-written one:\n{tagged}"
    );

    // A plain class gets none.
    let plain = javap(&out, "Plain");
    assert!(
        !plain.contains("canEqual"),
        "Plain should have no canEqual:\n{plain}"
    );

    let _ = fs::remove_dir_all(&out);
}
