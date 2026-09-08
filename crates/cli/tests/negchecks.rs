//! E2E tests for the `agent/negchecks` slice: restrictions the compiler did
//! not implement, and that had been hidden by bogus errors elsewhere.
//!
//! `agent/libnotype` removed three sets of wrong diagnostics, and four corpus
//! `neg` tests stopped "passing" with them — each had been rejected for a
//! reason that appears nowhere in its `.check` file. This file pins the ones
//! this slice implements, at scalac 2.13.16's own lines and with its own text.
//!
//! **Every rule here makes the compiler reject more**, which is the direction
//! that costs working programs when a rule is drawn too broadly. So each
//! negative fixture is paired with the *legal neighbour* of the illegal
//! shape, and that positive fixture is a **running** one: it compiles and
//! prints, and its expected output is real scalac 2.13.16's for the same
//! source. It also compiles and prints the same thing on the pre-slice
//! binary, which is what makes it a guard rather than a restatement.
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
        "scala-rs-negchecks-{tag}-{}-{nanos}-{seq}",
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

/// Compile one fixture against the real library ABI. Returns the output
/// directory; panics with the compiler's own diagnostics when it fails.
fn compile_ok(tag: &str, fixture: &str, jar: &Path) -> PathBuf {
    let out = tmp_dir(tag);
    let output = Command::new(bin())
        .arg("compile")
        .arg(fixtures_dir().join(fixture))
        .arg("-d")
        .arg(&out)
        .arg("--scala-library")
        .arg(jar)
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "{fixture} must compile, and does under scalac 2.13.16: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn run_main(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
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

/// Compile a fixture that must be rejected; returns everything it printed.
fn compile_bad(tag: &str, fixture: &str) -> String {
    let out = tmp_dir(tag);
    let mut cmd = Command::new(bin());
    cmd.arg("compile")
        .arg(fixtures_dir().join(fixture))
        .arg("-d")
        .arg(&out);
    match scala_library_jar() {
        Some(jar) => {
            cmd.arg("--scala-library").arg(jar);
        }
        None => {
            cmd.arg("--no-scala-library");
        }
    }
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "{fixture} compiled; scalac 2.13.16 rejects it"
    );
    let err = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    let _ = fs::remove_dir_all(&out);
    err
}

/// nsc's `Typers.checkEphemeral`, on both of its callers.
///
/// The expected list is real scalac 2.13.16's output for
/// `tests/fixtures/negchecks_ephemeral_bad.scala`, line for line and word for
/// word — nine errors and no others. Both halves of every `implementation
/// restriction` message are pinned, including the second line ("This
/// restriction is planned to be removed in subsequent releases."), because nsc
/// issues them as one `context.error` and the `.check` files in
/// `test/files/neg` carry both lines under one `error:` header.
///
/// The universal-trait wording ("... is not allowed in universal trait
/// extending from class Any") is the same function's other `where`, which is
/// why one fixture pins both: writing the two message sets apart is how they
/// drift.
#[test]
fn ephemeral_restrictions_match_scalac_line_and_text() {
    let err = compile_bad("ephemeral", "negchecks_ephemeral_bad.scala");
    let expected: &[(&str, &str)] = &[
        (
            "negchecks_ephemeral_bad.scala:3",
            "field definition is not allowed in universal trait extending from class Any",
        ),
        (
            "negchecks_ephemeral_bad.scala:5",
            "this statement is not allowed in universal trait extending from class Any",
        ),
        (
            "negchecks_ephemeral_bad.scala:7",
            "field definition is not allowed in universal trait extending from class Any",
        ),
        (
            "negchecks_ephemeral_bad.scala:9",
            "implementation restriction: nested object is not allowed in universal trait extending from class Any",
        ),
        (
            "negchecks_ephemeral_bad.scala:19",
            "implementation restriction: nested object is not allowed in value class",
        ),
        (
            "negchecks_ephemeral_bad.scala:20",
            "implementation restriction: nested trait is not allowed in value class",
        ),
        (
            "negchecks_ephemeral_bad.scala:21",
            "implementation restriction: nested class is not allowed in value class",
        ),
        // The deep half: nsc's `checkEphemeralDeep` searches a `def`'s
        // right-hand side at any nesting depth, and only for a value class.
        (
            "negchecks_ephemeral_bad.scala:23",
            "implementation restriction: nested object is not allowed in value class",
        ),
        (
            "negchecks_ephemeral_bad.scala:24",
            "implementation restriction: nested class is not allowed in value class",
        ),
    ];
    for (line, msg) in expected {
        assert!(
            err.contains(msg),
            "missing scalac's wording {msg:?}:\n{err}"
        );
        assert!(err.contains(line), "missing a diagnostic at {line}:\n{err}");
    }
    assert_eq!(
        err.matches("This restriction is planned to be removed in subsequent releases.")
            .count(),
        6,
        "the second line belongs to each `implementation restriction`:\n{err}"
    );
    // Nothing extra: scalac reports exactly nine. `def ok`, `type T`,
    // `class AlsoOk` in the universal trait and `def fine` / `type Alias` in
    // the value class are legal and must stay silent.
    assert_eq!(
        err.matches("error:").count(),
        expected.len(),
        "expected exactly the {} errors scalac reports:\n{err}",
        expected.len()
    );
    for silent in [
        "bad.scala:11",
        "bad.scala:13",
        "bad.scala:15",
        "bad.scala:26",
    ] {
        assert!(
            !err.contains(silent),
            "reported a legal member at {silent}:\n{err}"
        );
    }
}

/// The legal neighbour of every shape the check rejects, executed.
///
/// A universal trait holding only `def`s, an import, a type member and a
/// nested *class* (which nsc allows in a universal trait and forbids in a
/// value class); a value class with a nested type alias, an anonymous class
/// and a `PartialFunction` literal in a method body (scala/bug#7571, the two
/// lines `neg/valueclasses-impl-restrictions.scala` marks "allowed"). The
/// expected output is real scalac 2.13.16's, and the pre-slice binary prints
/// it too.
#[test]
fn legal_universal_trait_and_value_class_still_run() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip negchecks library run: scala-library jar not present");
        return;
    };
    let out = compile_ok("ephemeral-pos", "negchecks_ephemeral.scala", &jar);
    let expected =
        fs::read_to_string(fixtures_dir().join("expected/negchecks_ephemeral.txt")).unwrap();
    assert_eq!(run_main(&out, &jar), expected);
    let _ = fs::remove_dir_all(&out);
}

/// A companion pair has to be *co-defined*, and for two local definitions nsc
/// spells that as the same `Scope` rather than the same owner.
///
/// `Contexts.lookupSibling`'s comment is this program:
///
/// ```text
/// // Must be owned by the same Scope, to ensure that in
/// // `{ class C; { ...; object C } }`, the class is not seen as a companion
/// // of the object.
/// ```
///
/// Two blocks of one method share an owner, so before this slice the inner
/// `object C` counted as the companion of the outer `class C` and read its
/// `private def x`. scalac 2.13.16 rejects both accesses in this fixture, at
/// lines 8 and 16.
///
/// **The wording is ours, not scalac's, and deliberately so.** scalac says
/// `method x in class C cannot be accessed as a member of C from object C`;
/// this compiler says `value x cannot be accessed as a member of C from C$`
/// for *every* access error it reports, has done since long before this
/// slice, and four other test files pin that spelling. `fullLocationString`
/// ("method x in class C") and `directObjectString` ("object C" for a module
/// class) are one cross-cutting change to every access diagnostic in the
/// compiler, not part of this rule. The line and the cause are what this test
/// pins; see `docs/comparison-with-scalac.md`.
#[test]
fn a_local_object_in_a_nested_block_is_not_a_companion() {
    let err = compile_bad("companion-scope", "negchecks_companion_scope_bad.scala");
    for line in [
        "negchecks_companion_scope_bad.scala:8",
        "negchecks_companion_scope_bad.scala:16",
    ] {
        assert!(
            err.contains(line),
            "expected an inaccessible-member error at {line}:\n{err}"
        );
    }
    assert_eq!(
        err.matches("cannot be accessed as a member of").count(),
        2,
        "expected exactly the two accesses scalac rejects:\n{err}"
    );
}

/// The legal neighbour: a companion that reads `private` state the way Scala
/// does allow, executed.
///
/// The top-level `object Counter` reads `private val start` and `private def
/// step` on an instance handed to it, and a local `class L` / `object L` in
/// **one** block do the same. A `class M` and an `object M` in different
/// blocks are not companions, but `M` reading `M`'s *public* member is still
/// fine — the rule must cost that nothing. The expected output is real
/// scalac 2.13.16's, and the pre-slice binary prints it too.
#[test]
fn legal_companion_private_access_still_runs() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip negchecks library run: scala-library jar not present");
        return;
    };
    let out = compile_ok("companion-pos", "negchecks_companion_scope.scala", &jar);
    let expected =
        fs::read_to_string(fixtures_dir().join("expected/negchecks_companion_scope.txt")).unwrap();
    assert_eq!(run_main(&out, &jar), expected);
    let _ = fs::remove_dir_all(&out);
}
