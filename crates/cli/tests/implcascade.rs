//! A failed implicit search reports once and stops.
//!
//! Two defects, one consequence. Both were at the head of gitbucket.
//!
//! * **The wanted type was already erroneous.** `implicit_fit_at` starts with
//!   `plausibly_inhabits(candidate, pt)`, and `Type::Error` inhabits anything,
//!   so every implicit in scope became a candidate and the search answered
//!   `Ambiguous`. gitbucket's `def extractFromJsonBody[A](implicit request:
//!   HttpServletRequest, mf: Manifest[A])` has an erroneous parameter type here
//!   -- our `Predef` has no `Manifest` alias -- and each of its 23 call sites
//!   reported `ambiguous implicit: jsonFormats, context, RichRequest,
//!   RichString, context2ApiJsonFormatContext, RichSession, request2Session`.
//!   In `AccountController` the same search named **thirty-two** candidates,
//!   every slick column type among them. nsc's `inferImplicit` never starts a
//!   search against an erroneous type, and `applyImplicitArgs` reports a
//!   missing implicit only `if (!param.tpe.isErroneous)`.
//!
//! * **The application kept its declared result type.** nsc's
//!   `applyImplicitArgs` ends `if (args contains EmptyTree) setError(tree)`.
//!   Ours handed back the result type with the type parameters the missing
//!   witness was the only thing that could have solved still in it. slick's
//!   `map[F, T, G](f)(implicit shape: Shape[_, F, T, G]): Query[G, T, C]` is
//!   exactly that shape, and `IssuesService.scala` alone reported `value _1 is
//!   not a member of T` six times on one line, after the missing `Shape` had
//!   already been reported on the line above.
//!
//! Together they were 496 -> 416 errors on gitbucket, 96 -> 83 files with
//! errors.
//!
//! The negative fixture is the load-bearing half: real scalac 2.13.16 reports
//! exactly two errors in it, at lines 22 and 38, and the pre-fix binary
//! reported five. The positive fixture executes, because suppressing a cascade
//! is only right if a *successful* search is untouched -- and a witness that
//! settles a type parameter appearing nowhere in the value arguments is
//! precisely the case a compile-only test would not catch.

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
        "scala-rs-implcascade-{tag}-{}-{nanos}-{seq}",
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

/// A witness that settles a type parameter appearing in no value argument
/// still settles it, and the result is usable. Expected output is real scalac
/// 2.13.16's for the same source.
#[test]
fn a_found_witness_still_determines_the_result_type() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip implcascade run: scala-library jar not present");
        return;
    };
    let out = compile("implcascade", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout("implcascade"),
        "stdout mismatch against the library ABI"
    );
    let _ = fs::remove_dir_all(&out);
}

/// scalac 2.13.16 on `implcascade_bad.scala`:
///
/// ```text
/// implcascade_bad.scala:22: error: not found: type NoSuchTag
/// implcascade_bad.scala:38: error: could not find implicit value for parameter shape: Cascade.Shape[String,T]
/// 2 errors
/// ```
///
/// Two errors, at those two lines. Before the fix we reported five: those two
/// plus `ambiguous implicit: aString, anInt, augmentString, intWrapper,
/// wrapString` at 25 and 26, and `value _1 is not a member of T` at 38.
#[test]
fn a_failed_implicit_search_reports_once_and_stops() {
    let src = fixtures_dir().join("implcascade_bad.scala");
    let out = tmp_dir("implcascade_bad");
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
        "implcascade_bad compiled; it must not"
    );
    let err = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(
        err.contains("not found: type NoSuchTag"),
        "the cause must still be reported: {err}"
    );
    assert!(
        err.contains("implcascade_bad.scala:22"),
        "expected the missing type at line 22: {err}"
    );
    assert!(
        err.contains("could not find implicit value of type Shape[String, T]"),
        "the missing witness must still be reported: {err}"
    );
    assert!(
        err.contains("implcascade_bad.scala:38"),
        "expected the missing witness at line 38: {err}"
    );
    // An erroneous wanted type is not a search: no candidate list, at either
    // call site of `tagged`.
    assert!(
        !err.contains("ambiguous implicit"),
        "an erroneous wanted type must not produce a candidate list: {err}"
    );
    // The application whose implicit argument is missing is an error tree.
    assert!(
        !err.contains("is not a member of"),
        "nothing may be selected off an application with a missing implicit: {err}"
    );
    assert_eq!(
        err.matches("error:").count(),
        2,
        "expected exactly the two errors scalac reports: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
