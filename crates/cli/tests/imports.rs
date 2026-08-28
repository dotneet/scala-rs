//! Import resolution: every selector shape (`.X`, `._`, `.*`, `{A, B => C}`,
//! `A as B`) against every kind of prefix — a package defined in the same run,
//! a package object, and a package that exists only inside a classpath jar.
//!
//! Behaviour is pinned to scalac 2.13.16 with `-Xsource:3`; the fixtures are
//! dual-run against the real scala-library.

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

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-imports-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java").arg("-version").output().is_ok()
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn diagnostics(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    )
}

fn compile(names: &[&str], jar: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for n in names {
        cmd.arg(fixture(n));
    }
    cmd.args([
        "-d",
        out.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    cmd.args(extra);
    cmd.output().expect("run scala-rs compile")
}

fn run_main(out: &Path, jar: &Path) -> String {
    let output = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "Main",
        ])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile `names` together and check `Main`'s output against `expected/<main>.txt`.
fn check_runs(tag: &str, names: &[&str], extra: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip imports {tag}: scala-library not available");
        return;
    };
    let out = tmp_dir(tag);
    let output = compile(names, &jar, &out, extra);
    assert!(
        output.status.success(),
        "compile {names:?} failed:\n{}",
        diagnostics(&output)
    );
    if java_available() {
        assert_eq!(run_main(&out, &jar), expected_stdout(tag), "stdout {tag}");
    }
    let _ = fs::remove_dir_all(&out);
}

const SAME_RUN: &[&str] = &[
    "imports_pkgs",
    "imports_pkgs2",
    "imports_pkgs3",
    "imports_pkgobj",
    "imports_main",
];

// ------------------------------------------- packages from the same run

/// One-, two- and three-level packages compiled in the same run, imported by
/// name, by `{A, B}` list, by rename and by the `-Xsource:3` `.*` wildcard —
/// including a package object's members and a nested object inside it.
#[test]
fn same_run_packages_every_selector_shape() {
    check_runs("imports_main", SAME_RUN, &["-Xsource:3"]);
}

#[test]
fn same_run_packages_with_xsource3_cross() {
    check_runs("imports_main", SAME_RUN, &["-Xsource:3-cross"]);
}

// ------------------------------------------------- packages from a jar

/// `scala.collection.mutable`, `scala.math` (a package object) and
/// `scala.collection.immutable` are only readable from the classpath jar:
/// wildcard, rename and plain selectors must all reach them.
#[test]
fn jar_packages_every_selector_shape() {
    check_runs("imports_jar", &["imports_jar"], &["-Xsource:3"]);
}

/// Every `scala.language` feature is an importable name, in every shape,
/// including the nested `scala.language.experimental.macros`.
#[test]
fn language_feature_imports_resolve() {
    check_runs("imports_lang", &["imports_lang"], &[]);
}

// ------------------------------------------------------- error handling

/// `.*` is a Scala 3 spelling: without `-Xsource:3` nothing is imported, and
/// scalac 2.13.16 reports `object * is not a member of package p1` plus the
/// unresolved use. We must reject it too.
#[test]
fn star_wildcard_needs_xsource3() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip imports star: scala-library not available");
        return;
    };
    let out = tmp_dir("star_bad");
    let output = compile(&["imports_pkgs", "imports_star_bad"], &jar, &out, &[]);
    assert!(
        !output.status.success(),
        "expected `import p1.*` without -Xsource:3 to fail"
    );
    let err = diagnostics(&output);
    assert!(
        err.contains("not found: value A"),
        "expected the unresolved use to be reported, got {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `import p.{X => _, _}` hides `X` from the wildcard — including from the
/// lazy classfile fallback, which must not smuggle it back in.
#[test]
fn hidden_selector_stays_hidden() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip imports hide: scala-library not available");
        return;
    };
    let out = tmp_dir("hide");
    let output = compile(
        &["imports_pkgs", "imports_pkgs2", "imports_hide_bad"],
        &jar,
        &out,
        &[],
    );
    assert!(!output.status.success(), "expected `B => _` to hide `B`");
    let err = diagnostics(&output);
    assert!(
        err.contains("not found: value B"),
        "expected the hidden name to stay unresolved, got {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// An import that names nothing is an error, not a silent no-op.
#[test]
fn unknown_selector_is_reported() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip imports unknown: scala-library not available");
        return;
    };
    let out = tmp_dir("unknown");
    let output = compile(&["imports_pkgs", "imports_unknown_bad"], &jar, &out, &[]);
    assert!(
        !output.status.success(),
        "expected an unresolvable import to fail"
    );
    let err = diagnostics(&output);
    assert!(
        err.contains("Nope is not a member"),
        "expected the bad selector to be reported, got {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
