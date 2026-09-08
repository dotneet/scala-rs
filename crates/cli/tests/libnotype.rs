//! E2E tests for the `agent/libnotype` slice: receivers whose type was never
//! computed, printed as `<notype>`.
//!
//! `tests/scalalib_measure.sh` carried 56 errors naming `<notype>` (48 of them
//! in the two clusters the brief called out, `value + is not a member of
//! <notype>` and `value min is not a member of <notype>`). They were **three**
//! roots, and only one of them was a missing diagnostic:
//!
//! 1. **A directory classpath was matched case-insensitively.** macOS's
//!    default APFS volume is case-insensitive, so `<cp>/scala/Math` answered
//!    `is_dir()` for the real directory `scala/math`.
//!    `Typer::complete_binary_member` takes a directory under a package as
//!    proof a *package* of that name exists, so `package scala.Math` and
//!    `package scala.Runtime` were invented, entered in scope ahead of the
//!    implicit `import java.lang._`, and every `Math.min` / `Math.max` /
//!    `Runtime.getRuntime` in the library selected on a package. Nothing was
//!    reported where the name was lost -- the typer thought it had resolved
//!    it. 26 errors. `BinaryIndex` now verifies the case of every component it
//!    found on a directory entry.
//!
//! 2. **A blank line before a bare block was read as an argument list.** nsc's
//!    scanner distinguishes `NEWLINE` from `NEWLINES` and
//!    `newLineOptWhenFollowedBy(LBRACE)` skips only the former, so `f\n{ … }`
//!    is an application and `f\n\n{ … }` is two statements. Our lexer
//!    collapsed every run of line breaks into one token, so
//!    `HashMap.concat`'s
//!
//!    ```scala
//!    var newCachedHashCode = 0
//!
//!    {
//!      …
//!    }
//!    ```
//!
//!    parsed as `0 { … }`; the `var` took that failed application's type and
//!    all 18 `newCachedHashCode += …` beneath it reported on `<notype>`. Here
//!    a diagnostic *was* emitted -- "value apply is not a member of 0", at the
//!    `var`, which is the wrong site and the wrong complaint. 26 errors.
//!
//! 3. **`new X` was resolved in the term namespace.** `TreeKind::New`'s
//!    unqualified-`Ident` arm used `SymbolTable::lookup`, which stops at the
//!    innermost scope binding the name at all. `val accum = new accum` (again
//!    `HashMap.concat`, with `class accum` in the enclosing block) therefore
//!    found only the `val` being defined, fell through to typing `accum` as an
//!    *expression*, and got the half-built value's own `NoType` back -- with
//!    no diagnostic at all, which is the one place here an error really was
//!    owed. `lookup_type` skips a term-only scope. 35 errors, most of them
//!    `List.scala`'s `new ::(x, xs)` reaching past `List`'s own `def ::`.
//!
//! `files=538 errors=1111 files_with_errors=155` before,
//! `files=538 errors=1024 files_with_errors=150` after. No error kind in that
//! log went *up* and no new kind appeared.
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

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-libnotype-{tag}-{}-{nanos}-{seq}",
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

fn run_java(out: &Path, main: &str, cp_extra: Option<&str>) -> String {
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

/// The private runtime. Output is real scalac 2.13.16's, from compiling the
/// same fixture against `scala-library-2.13.16.jar`.
#[test]
fn fixtures_libnotype_runs() {
    let out = compile_fixture_with("libnotype", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, "libnt.Main", None),
            expected_stdout("libnotype")
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture against the real library ABI.
#[test]
fn fixtures_libnotype_runs_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("libnotype", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, "libnt.Main", Some(jar_s)),
        expected_stdout("libnotype")
    );
    let _ = fs::remove_dir_all(&out);
}

/// Real scalac reports four errors here, on lines 10, 12, 16 and 19. Three of
/// them are pinned; line 10 (`missing argument list for method one`) is a
/// separate, pre-existing laxity about eta-expansion in this compiler and is
/// deliberately not asserted.
///
/// Line **12** is what the parser change buys: if the blank line did not end
/// the expression, `one { (x: Int) => x }` is a well-typed application and the
/// file compiles clean.
#[test]
fn fixtures_libnotype_bad_is_error() {
    let err = compile_errors("libnotype_bad", &["--no-scala-library"]);
    assert!(
        err.contains("libnotype_bad.scala:12"),
        "the blank line must end the expression, leaving the block a statement \
         that does not conform to Int; got: {err}"
    );
    assert!(
        err.contains("type mismatch; found: (Int) => Int  required: Int"),
        "expected scalac's line-12 mismatch, got: {err}"
    );
    assert!(
        err.contains("not found: type NotAType") && err.contains("libnotype_bad.scala:16"),
        "expected `new NotAType` to be reported as a missing *type*, got: {err}"
    );
    assert!(
        err.contains("class type required but T found") && err.contains("libnotype_bad.scala:19"),
        "expected `new T` to be rejected, got: {err}"
    );
}

/// A directory on the classpath is matched by exact spelling.
///
/// `<cp>/scala/math` is a real directory in `tests/scalalib_measure.sh`'s Java
/// classpath (`scala/math/ScalaNumber.class`), and on a case-insensitive file
/// system `<cp>/scala/Math` used to answer `is_dir()` for it -- which invented
/// `package scala.Math` and shadowed `java.lang.Math` for the whole run.
///
/// On a case-sensitive file system this test passes either way; it is the
/// regression guard for the platforms where it did not.
#[test]
fn dir_classpath_is_case_sensitive() {
    let root = tmp_dir("cp");
    let cp = root.join("cp");
    fs::create_dir_all(cp.join("scala/math")).unwrap();
    fs::create_dir_all(cp.join("scala/runtime")).unwrap();
    let src = root.join("CaseCp.scala");
    fs::write(
        &src,
        "object CaseCp {\n  \
         def main(args: Array[String]): Unit = {\n    \
         println(Math.max(3, 4))\n    \
         println(Math.min(3, 4))\n  \
         }\n}\n",
    )
    .unwrap();
    let out = root.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            cp.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "a `scala/math` directory on the classpath must not invent `scala.Math`: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    if java_available() {
        assert_eq!(run_java(&out, "CaseCp", None), "4\n3\n");
    }
    let _ = fs::remove_dir_all(&root);
}
