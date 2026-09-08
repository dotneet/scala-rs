//! E2E tests for the `agent/impprio` slice: SLS 2's four-level precedence for
//! a simple name.
//!
//! A binding of higher precedence **hides** one of lower precedence in the
//! same scope; it does not merely join it. scala-rs entered every route into
//! a scope the same way, so the answer fell out of insertion order and symbol
//! id, and a wildcard import could outrank both an explicit import and a
//! definition. gitbucket says so in a comment of its own:
//!
//! ```text
//! import gitbucket.core.model.Profile.profile.blockingApi._
//! // Imported names have higher precedence than names, defined in other files.
//! // If Database is not bound by explicit import, then "Database" refers to
//! // the Database introduced by the wildcard import above.
//! import gitbucket.core.servlet.Database
//! ```
//!
//! **Compiling is not the same as being right**: every arrangement in
//! `Main_1.scala` compiles whichever definition wins, and prints which one
//! did. The expected output is real scalac 2.13.16's for the same three
//! files. On the pre-fix binary the first four lines read `wildcard`, and so
//! do the `same-unit` and `local` ones.
//!
//! The negative half is `Bad_1.scala`. Two bindings of the *same* precedence
//! in one scope are an ambiguous reference, which scalac rejects at three
//! lines; without pinning them, "a wildcard ranks below an explicit import"
//! could drift into "prefer whichever candidate we like", which is not a
//! ranking. `p5` in the positive fixture is the mirror image and matters just
//! as much: a package member from *another* unit is precedence 4 and the
//! wildcard import outranks it.
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/impprio")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-impprio-{tag}-{}-{nanos}-{seq}",
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
        "impprio positive fixture failed to compile: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn expected() -> String {
    fs::read_to_string(multi_dir().join("expected.txt")).unwrap()
}

/// Which definition each arrangement selects, under the private runtime --
/// so this also proves the rule is in the typer and not in anything the
/// library ABI supplies.
#[test]
fn precedence_selects_the_same_definitions_as_scalac() {
    if !java_available() {
        return;
    }
    let out = compile_positive("private", &["--no-scala-library"]);
    assert_eq!(
        run_main(&out, None),
        expected(),
        "a name bound to the wrong definition under the private runtime"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same three files against the real scala-library 2.13.16 ABI.
#[test]
fn precedence_selects_the_same_definitions_against_the_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip impprio library run: scala-library jar not present");
        return;
    };
    let out = compile_positive("library", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_main(&out, Some(&jar)),
        expected(),
        "a name bound to the wrong definition against the library ABI"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Two bindings of one precedence in one scope stay ambiguous, at the three
/// lines real scalac 2.13.16 reports and at no others.
#[test]
fn same_precedence_bindings_stay_ambiguous() {
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
        "impprio Bad_1.scala compiled; it must not"
    );
    let err = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    // scalac: "reference to x is ambiguous; it is imported twice in the same
    // scope by ...", at 25 (two wildcards), 32 (two explicit imports) and 40
    // (two wildcards whose members would otherwise overload).
    for (line, name) in [
        ("Bad_1.scala:25", "x"),
        ("Bad_1.scala:32", "x"),
        ("Bad_1.scala:40", "f"),
    ] {
        assert!(
            err.contains(line),
            "expected an ambiguity at {line} for {name}: {err}"
        );
    }
    assert_eq!(
        err.matches("error: reference to").count(),
        3,
        "expected exactly the three ambiguities scalac reports: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
