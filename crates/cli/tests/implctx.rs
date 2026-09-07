//! A `private` member is not inherited (SLS 5.2), so it is not an implicit
//! candidate outside its own class. Three collection routes reached one
//! anyway:
//!
//! * the parent walk in `Typer::implicits_in_scope`,
//! * `bind_self_type`, which enters a self type's members into the template
//!   scope -- a self type is a conformance obligation, not membership, and
//!   does not widen access,
//! * `import_wildcard`, which walks the imported object's ancestors and so
//!   imported members the object does not have.
//!
//! gitbucket is where it showed. `trait RequestCache` has a `private implicit
//! def context2Session`; `object gitbucket.core.view.helpers` mixes
//! `RequestCache` in, three view traits carry `self: RequestCache =>`, and
//! every controller writes `import gitbucket.core.view.helpers._`. The
//! private definition reached the implicit scope by all three routes and
//! competed with the `Implicits.request2Session` nsc actually chooses, so 14
//! calls in `IndexController` were `ambiguous implicit: context2Session,
//! request2Session`.
//!
//! Choosing the other candidate is a wrong answer at run time, not a compile
//! error, so the positive fixture executes and prints which implicit ran. Its
//! expected file is real scalac 2.13.16's output for the same source.
//!
//! The negative fixture is the load-bearing half: two candidates that nsc
//! *does* reach must stay ambiguous, including a class's own private member
//! competing with an import inside that same class.

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
        "scala-rs-implctx-{tag}-{}-{nanos}-{seq}",
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

fn run_java(out: &Path, jar: Option<&Path>) -> String {
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
        "java failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile(name: &str, extra: &[&str]) -> PathBuf {
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
        "compile {name} failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// The private runtime, so this also proves the rule is in the typer and not
/// in anything the library ABI supplies.
#[test]
fn private_members_are_not_inherited_into_the_implicit_scope() {
    if !java_available() {
        return;
    }
    let out = compile("implctx_privinherit", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        expected_stdout("implctx_privinherit"),
        "stdout mismatch under the private runtime"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture against the real scala-library 2.13.16 ABI.
#[test]
fn private_members_are_not_inherited_against_the_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip implctx_privinherit library run: jar not present");
        return;
    };
    let out = compile(
        "implctx_privinherit",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout("implctx_privinherit"),
        "stdout mismatch against the library ABI"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Candidates nsc really does reach stay ambiguous. Without this, the fix
/// above would read as "prefer the nearer candidate", which nsc does not do:
/// `ImplicitComputation` flattens `Context.implicitss` with `flatMap` and
/// separates levels only by *name* shadowing.
#[test]
fn genuinely_ambiguous_arrangements_stay_ambiguous() {
    let src = fixtures_dir().join("implctx_privinherit_bad.scala");
    let out = tmp_dir("implctx_privinherit_bad");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "implctx_privinherit_bad compiled; it must not"
    );
    let err = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    // Two candidates declared side by side.
    assert!(
        err.contains("ambiguous implicit: b, a") || err.contains("ambiguous implicit: a, b"),
        "two same-level candidates stopped being ambiguous: {err}"
    );
    // A class's *own* private member competes with an import in that body.
    assert!(
        err.contains("ambiguous implicit: mine, c") || err.contains("ambiguous implicit: c, mine"),
        "an own private member stopped competing with the import: {err}"
    );
    // A non-private inherited member is inherited, and still competes.
    assert!(
        err.contains("ambiguous implicit: inherited, c")
            || err.contains("ambiguous implicit: c, inherited"),
        "a public inherited member stopped competing with the import: {err}"
    );
    // scalac reports these at 20, 29 and 40, and at no other line.
    for line in [
        "implctx_privinherit_bad.scala:20",
        "implctx_privinherit_bad.scala:29",
        "implctx_privinherit_bad.scala:40",
    ] {
        assert!(err.contains(line), "expected a diagnostic at {line}: {err}");
    }
    assert_eq!(
        err.matches("error: ambiguous implicit").count(),
        3,
        "expected exactly the three ambiguities scalac reports: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
