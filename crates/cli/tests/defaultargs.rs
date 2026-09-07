//! E2E tests for the `agent/defaultargs` slice: default arguments whose
//! `name$default$n` getter is reached through a prefix the call site does not
//! write, and default arguments declared in a separately compiled class file.
//!
//! Two roots, both about *which symbol answers for the getter*:
//!
//! 1. A method declared in a class file had no defaults at all. A class file
//!    carries no per-parameter "has a default" bit, and the two places that
//!    could supply one both declined: the class file reader
//!    (`classpath::fill_java_members`) never looked at the `name$default$n`
//!    methods sitting beside the method, and the pickle reader skipped a case
//!    class's companion `apply` because nsc marks it `SYNTHETIC`. json4s's
//!    `FieldSerializer[String]()` therefore reached overload selection with
//!    no arguments against `(PartialFunction, PartialFunction, Boolean,
//!    ClassTag[String])`.
//!
//! 2. The getter was selected off the *enclosing class* of the call site
//!    rather than off the prefix the method itself resolved through --
//!    "value avatar$default$3 is not a member of IndexControllerBase" for a
//!    name that came in through `import helpers._`, and "value
//!    getAccountByUserNameIgnoreCase$default$2 is not a member of $anon$61"
//!    for one a cake's self type contributes, seen from inside an anonymous
//!    class. The main call was emitted correctly in the first shape and
//!    *miscompiled* in the second (`aload_0; checkcast AccountService` on an
//!    anonymous class that does not implement it -- a `ClassCastException`
//!    from a program that type-checked), so the backend's receiver walk had
//!    to learn about self types too.
//!
//! Every default here is called and printed, so a getter that answers the
//! wrong value cannot pass as a green test. `tests/multi/defaultargs_binary`
//! is compiled by **real scalac** first, which is the setting the first root
//! only appears in.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.

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

fn multi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/defaultargs_binary")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-defaultargs-{tag}-{}-{nanos}-{seq}",
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
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
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

fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_errors(name: &str, extra: &[&str]) -> String {
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
    let _ = fs::remove_dir_all(&out);
    err
}

/// The private-runtime run of the receiver fixture.
#[test]
fn fixtures_da_defaults() {
    let out = compile_fixture_with("da_defaults", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None, "da.Main"),
            expected_stdout("da_defaults")
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture against the real `scala-library` ABI.
#[test]
fn fixtures_da_defaults_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip da_defaults dual-run: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("da_defaults", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s), "da.Main"),
        expected_stdout("da_defaults")
    );
    let _ = fs::remove_dir_all(&out);
}

/// A default does not excuse a missing argument, a mistyped one, or a name the
/// method does not declare -- the three real scalac reports for this file.
#[test]
fn fixtures_da_defaults_bad() {
    let err = compile_errors("da_defaults_bad", &["--no-scala-library"]);
    assert!(
        err.contains("with arguments (\"octocat\")"),
        "omitting a parameter with no default must fail: {err}"
    );
    assert!(
        err.contains("with arguments (\"x\", \"yes\")"),
        "a named argument still has to typecheck: {err}"
    );
    assert!(
        err.contains("unknown parameter name: removed"),
        "an undeclared parameter name must fail: {err}"
    );
}

/// The class-file root: `dalib` is compiled by **real scalac**, so the
/// consumer sees only its class files and pickle -- the situation gitbucket's
/// json4s and scalatra calls are in.
#[test]
fn multi_da_defaults_from_classfiles() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip defaultargs_binary: scala-library jar or scalac not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let dir = multi_dir();
    let out = tmp_dir("binary");
    for lib in ["Lib_1.scala", "Halt_1.scala"] {
        let status = Command::new(&scalac)
            .arg("-d")
            .arg(&out)
            .arg("-cp")
            .arg(&out)
            .arg(dir.join(lib))
            .status()
            .expect("run scalac");
        assert!(status.success(), "scalac {lib} failed");
    }
    let output = Command::new(bin())
        .args(["compile"])
        .arg(dir.join("Main_1.scala"))
        .arg("-cp")
        .arg(&out)
        .arg("-d")
        .arg(&out)
        .args(["--scala-library", jar_s])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compiling Main_1.scala against scalac's class files failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar_s), "Main"),
        fs::read_to_string(dir.join("expected.txt")).unwrap()
    );
    let _ = fs::remove_dir_all(&out);
}
