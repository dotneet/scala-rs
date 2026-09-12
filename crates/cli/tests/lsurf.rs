//! The extractor protocol and gaps in the library surface the typer supplies.
//!
//! * Name-based extractors (SLS 8.1.8): an `unapply` result with
//!   `isEmpty` / `get` that is not an `Option`, one sub-pattern for the whole
//!   `get` value, product selectors `_1` … `_N` for several, value-class and
//!   inherited `isEmpty` / `get`, a `Unit` scrutinee, implicit clauses after
//!   the scrutinee, an instance extractor reached through a trait.
//! * Extractors scalac rejects: a non-`Boolean` `isEmpty`, no `get`, an
//!   `unapply` that does not take exactly one argument.
//! * What a collection transformation returns: the receiver's pickled
//!   `IterableOps[A, CC, C]`, not its own class (`NumericRange.map`,
//!   `IndexedSeqView.filter`, `MapView.drop`); `SortedSet.map` and
//!   `BitSet.map` through the receiver's own pickled declarations.
//! * `map` on `Option` / `Try` / `Either` and `Try.apply` with real type
//!   parameters; `TupleN <: ProductN`.
//! * `emptyIterator[T]()` on a Java static; `StringContext(..).s(..)` as a
//!   call.
//!
//! Every expected file is scalac 2.13.16's output. Fixture prefix: `lsurf_`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-lsurf-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `name` with scalac (when `scalac_path` is given) or scala-rs.
fn compile(name: &str, scalac_path: Option<&Path>, jar: &Path, out: &Path) -> Output {
    let src = fixtures_dir().join(format!("{name}.scala"));
    match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    }
}

/// Compile `name`, run it under `-Xverify:all`, and compare with the
/// expected output.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, scalac_path, &jar, &out);
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// Compile `name`, which must be rejected with every message in `want`.
fn check_rejects(name: &str, scalac_path: Option<&Path>, want: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, scalac_path, &jar, &out);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "expected a rejection:\n{text}");
    for w in want {
        assert!(text.contains(w), "missing `{w}` in:\n{text}");
    }
    let _ = fs::remove_dir_all(&dir);
}

const BAD_MESSAGES: &[&str] = &[
    "an unapply result must have a member `def isEmpty: Boolean` (found: `def isEmpty: Casey`)",
    "an unapply result must have a member `def isEmpty: Boolean`",
    "The result type of an unapply method must contain a member `get` to be used as an \
     extractor pattern, no such member exists in NoGet",
    "object Foo is not a case class, nor does it have a valid unapply/unapplySeq member",
    "def unapply: Option[Int] exists in object Foo, but it cannot be used as an extractor: \
     an unapply method must accept a single argument",
    "def unapply(): Option[Int] exists in object Foo2, but it cannot be used as an extractor: \
     an unapply method must accept a single argument",
    "def unapply(a: Int, b: Int): Option[Int] exists in object Foo3, but it cannot be used as \
     an extractor as it has more than one (non-implicit) parameter",
];

#[test]
fn lsurf_extract_runs() {
    check_runs("lsurf_extract", None);
}

#[test]
fn scalac_agrees_lsurf_extract() {
    if let Some(sc) = scalac() {
        check_runs("lsurf_extract", Some(&sc));
    }
}

#[test]
fn lsurf_colls_runs() {
    check_runs("lsurf_colls", None);
}

#[test]
fn scalac_agrees_lsurf_colls() {
    if let Some(sc) = scalac() {
        check_runs("lsurf_colls", Some(&sc));
    }
}

#[test]
fn lsurf_misc_runs() {
    check_runs("lsurf_misc", None);
}

#[test]
fn scalac_agrees_lsurf_misc() {
    if let Some(sc) = scalac() {
        check_runs("lsurf_misc", Some(&sc));
    }
}

#[test]
fn lsurf_extract_bad_rejected() {
    check_rejects("lsurf_extract_bad", None, BAD_MESSAGES);
}

#[test]
fn scalac_agrees_lsurf_extract_bad() {
    if let Some(sc) = scalac() {
        check_rejects("lsurf_extract_bad", Some(&sc), BAD_MESSAGES);
    }
}
