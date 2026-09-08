//! E2E tests for the `agent/nameamb` slice: SLS 2's *other* ambiguous
//! reference -- a definition and an `import` clause nested more deeply than
//! it.
//!
//! `agent/impprio` ranked the four levels, so a definition hides an import
//! and an explicit import hides a wildcard one. That is only half the rule.
//! nsc lets precedence decide when the two bindings sit at the same nesting
//! level; when the import is *deeper*, `Contexts.lookupSymbol` consults it
//! (`imp1.depth > symbolDepth`) and then refuses to choose between the two,
//! reporting `ambiguousDefnAndImport`.
//!
//! We chose, silently:
//!
//! ```text
//! object ColumnOption { object PrimaryKey }
//! class A {
//!   def PrimaryKey: Any = ???
//!   { import ColumnOption._; PrimaryKey }
//! }
//! ```
//!
//! is `neg/name-lookup-stable` in the scala/scala corpus, scalac rejects it
//! at two lines, and scala-rs compiled it and answered with the import. So
//! the negative half is the load-bearing one, and `Bad_1.scala` pins all
//! three lines of the message as well as the line it lands on -- an owner
//! ("class Member", "trait Base", "method pick") and the import clause's own
//! source text are what nsc puts there.
//!
//! `Main_1.scala` is the over-reach guard, and it *runs*: this slice makes
//! the compiler reject more, which is the direction that breaks working
//! programs. Every arrangement in it must stay unambiguous, and it prints
//! which definition each one selected. The expected file is real scalac
//! 2.13.16's output for the same three files.
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn multi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/nameamb")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-nameamb-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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

fn run_main(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "nameamb.Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java nameamb.Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_positive(tag: &str, extra: &[&str]) -> PathBuf {
    let dir = multi_dir();
    let out = tmp_dir(tag);
    let mut cmd = Command::new(bin());
    cmd.arg("compile")
        .arg(dir.join("Lib_1.scala"))
        .arg(dir.join("Sib_1.scala"))
        .arg(dir.join("Main_1.scala"))
        .arg("-d")
        .arg(&out);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "nameamb positive fixture failed to compile: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn expected() -> String {
    fs::read_to_string(multi_dir().join("expected.txt")).unwrap()
}

/// The arrangements that must stay unambiguous still compile, and still
/// select the definition real scalac selects -- under the private runtime, so
/// the rule is shown to be in the typer and not in anything the library ABI
/// supplies.
#[test]
fn unambiguous_arrangements_still_select_scalacs_definitions() {
    if !java_available() {
        return;
    }
    let out = compile_positive("private", &["--no-scala-library"]);
    assert_eq!(
        run_main(&out, None),
        expected(),
        "an arrangement that must stay unambiguous changed its answer"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same three files against the real scala-library 2.13.16 ABI.
#[test]
fn unambiguous_arrangements_hold_against_the_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip nameamb library run: scala-library jar not present");
        return;
    };
    let out = compile_positive("library", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_main(&out, Some(&jar)),
        expected(),
        "an arrangement that must stay unambiguous changed its answer"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_bad() -> String {
    let out = tmp_dir("bad");
    let output = Command::new(bin())
        .arg("compile")
        .arg(multi_dir().join("Bad_1.scala"))
        .arg("-d")
        .arg(&out)
        .arg("--no-scala-library")
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "nameamb Bad_1.scala compiled; it must not"
    );
    let err = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    let _ = fs::remove_dir_all(&out);
    err
}

/// A definition and a *deeper* import are reported, in the three lines real
/// scalac 2.13.16 writes and at the lines it writes them.
#[test]
fn a_deeper_import_makes_the_reference_ambiguous() {
    let err = compile_bad();
    // Verified against
    //   scalac -classpath scala-library-2.13.16.jar Bad_1.scala
    // which reports exactly these four, in this order.
    for (line, name, owner, clause) in [
        (28, "who", "class Member", "import Imp._"),
        (38, "who", "trait Base", "import Imp.who"),
        (49, "who", "method pick", "import Imp._"),
        (62, "Tag", "class Pattern", "import Imp._"),
    ] {
        let message = format!(
            "reference to {name} is ambiguous;\n\
             it is both defined in {owner} and imported subsequently by\n\
             {clause}"
        );
        assert!(
            err.contains(&message),
            "expected scalac's message for {name} at line {line}:\n{message}\ngot:\n{err}"
        );
        assert!(
            err.contains(&format!("Bad_1.scala:{line}:")),
            "expected an ambiguity at Bad_1.scala:{line}: {err}"
        );
    }
    assert_eq!(
        err.matches("error: reference to").count(),
        4,
        "expected exactly the four ambiguities scalac reports: {err}"
    );
}
