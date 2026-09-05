//! E2E tests for the `bt` slice: two roots from `docs/gitbucket.md`'s
//! "what would remove the most next" list.
//!
//! 1. **An argument that already conforms was wrapped in a view first.**
//!    `def ===[P2, R](e: Rep[P2])(implicit om: OM[B1, P2, R])` given a
//!    `Lit[Long]` (which *is* a `Rep[Long]`) solved `P2 = Lit[Long]`, because
//!    the applicability test read the unsolved `P2` as a rigid type, decided
//!    the argument did not fit, and reached for the `T => Rep[T]` view that is
//!    in scope for the literals. The implicit clause then asked for
//!    `OM[Long, Lit[Long], R]` -- a wrong answer, not a missing one.
//! 2. **A companion object read from a class file was shadowed by its class in
//!    term position.** `import p._` over a `-cp` package enters the classes
//!    alone (the companion is a second class file that nothing has asked for),
//!    and the class in scope then stopped the name from ever being looked up
//!    again. `Holder[Int](3)` bound the class and the `Module[T]` →
//!    `Module.apply[T]` redirect never ran.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.
//! All fixtures use the `bt_` prefix.

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
        "scala-rs-bt-{tag}-{}-{nanos}-{seq}",
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

fn run_java(out: &Path, cp_extra: &[&str]) -> String {
    let mut cp = out.display().to_string();
    for e in cp_extra {
        cp.push(':');
        cp.push_str(e);
    }
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

fn compile_errors(name: &str, extra: &[&str], needles: &[&str]) -> String {
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
    for n in needles {
        assert!(
            err.contains(n),
            "expected {name} error to contain {n:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
    err
}

// --- (1) an argument that already conforms ---------------------------------

/// Private-runtime run: nothing here needs the jar.
#[test]
fn fixtures_bt_base_private_runtime() {
    let out = compile_fixture_with("bt_base", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(run_java(&out, &[]), expected_stdout("bt_base"));
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same file against the real `scala-library` ABI. Byte-for-byte what
/// real scalac 2.13.16 prints.
#[test]
fn fixtures_bt_base_library() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("bt_base", &["--scala-library", jar_s]);
    if java_available() {
        assert_eq!(run_java(&out, &[jar_s]), expected_stdout("bt_base"));
    }
    let _ = fs::remove_dir_all(&out);
}

/// Reading the argument through its base type must not make anything *fit*.
/// Real scalac reports the same two `could not find implicit value` errors,
/// for the same two types.
#[test]
fn fixtures_bt_base_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let err = compile_errors(
        "bt_base_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &["OM[Long, String, R]", "OM[String, Long, R]"],
    );
    assert!(
        err.contains("2 error(s)"),
        "expected exactly 2 errors, got: {err}"
    );
}

#[test]
fn fixtures_bt_base_bad_is_error_without_library() {
    compile_errors(
        "bt_base_bad",
        &["--no-scala-library"],
        &["OM[Long, String, R]", "OM[String, Long, R]"],
    );
}

// --- (2) a companion read from a class file --------------------------------

/// Compile `bt_companion_lib.scala` to class files and hand them to the run
/// that compiles `bt_companion.scala`. Returns (use-output, lib-output).
fn compile_against_lib(name: &str, jar: &str) -> (PathBuf, PathBuf) {
    let lib_out = compile_fixture_with("bt_companion_lib", &["--scala-library", jar]);
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-cp",
            lib_out.to_str().unwrap(),
            "--scala-library",
            jar,
            "-d",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile {name} against the lib failed");
    (out, lib_out)
}

#[test]
fn fixtures_bt_companion() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let (out, lib_out) = compile_against_lib("bt_companion", jar_s);
    if java_available() {
        assert_eq!(
            run_java(&out, &[lib_out.to_str().unwrap(), jar_s]),
            expected_stdout("bt_companion")
        );
    }
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&lib_out);
}

/// Finding the companion must not accept what it does not offer. Real scalac
/// reports three errors here too.
#[test]
fn fixtures_bt_companion_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let lib_out = compile_fixture_with("bt_companion_lib", &["--scala-library", jar_s]);
    let err = compile_errors(
        "bt_companion_bad",
        &["-cp", lib_out.to_str().unwrap(), "--scala-library", jar_s],
        &[
            "(Int)Holder[Int] with arguments (\"s\")",
            "Empty with arguments (3)",
            "value missing is not a member of Holder$",
        ],
    );
    assert!(
        err.contains("3 error(s)"),
        "expected exactly 3 errors, got: {err}"
    );
    let _ = fs::remove_dir_all(&lib_out);
}

/// The class namespace is untouched: a *type* of the same name still names the
/// class, and `new` still reaches its constructor. Both are in
/// `bt_companion.scala`; this pins the negative half -- a package whose class
/// has **no** companion still resolves to the class in term position, and
/// says so.
#[test]
fn a_class_without_a_companion_is_still_reported_as_a_class() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let dir = tmp_dir("nocomp");
    let lib_src = dir.join("lib.scala");
    fs::write(&lib_src, "package btn\nclass Lonely[T](val v: T)\n").unwrap();
    let lib_out = tmp_dir("nocomp-lib");
    let status = Command::new(bin())
        .args([
            "compile",
            lib_src.to_str().unwrap(),
            "--scala-library",
            jar_s,
            "-d",
            lib_out.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compiling the companion-less lib failed");

    let use_src = dir.join("use.scala");
    fs::write(
        &use_src,
        "import btn._\nobject Main { def main(a: Array[String]): Unit = println(Lonely[Int](3)) }\n",
    )
    .unwrap();
    let out = tmp_dir("nocomp-use");
    let output = Command::new(bin())
        .args([
            "compile",
            use_src.to_str().unwrap(),
            "-cp",
            lib_out.to_str().unwrap(),
            "--scala-library",
            jar_s,
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "a class with no companion must not gain an `apply`"
    );
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&lib_out);
    let _ = fs::remove_dir_all(&out);
}
