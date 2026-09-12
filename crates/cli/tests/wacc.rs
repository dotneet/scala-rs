//! Programs scalac 2.13.16 rejects that scala-rs used to accept
//! (agent/wrongacc), and their valid neighbours.
//!
//! * Modifiers (`crate::modifier_rules`): nsc `Namers.validate` and the
//!   parser's by-name parameter checks.
//! * Names entered twice into one scope, across files too, and companions
//!   written in different files (`crate::double_names`).
//! * Overloaded references in value position, a function-typed `val` tied
//!   with a `def`, `new C` without arguments, defaults in two overloaded
//!   alternatives, unresolved annotation classes, a value class redefining
//!   `getClass`, a dotted package clause's scope, a header-pass completion
//!   whose errors were dropped, and interpolation / escape scanning.
//!
//! Every `_bad` fixture is written so that scalac reports all of its errors
//! in one phase; the agreement tests check that scalac rejects it with the
//! same number of errors and the same messages.
//!
//! Fixture prefix: `wacc_`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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
        "scala-rs-wacc-{tag}-{}-{nanos}-{seq}",
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

/// Compile `srcs` with scala-rs (`scalac_path` = None) or scalac into `out`;
/// returns (success, combined output).
fn compile(srcs: &[PathBuf], out: &Path, jar: &Path, scalac_path: Option<&Path>) -> (bool, String) {
    let o = match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .args(srcs)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .args(srcs)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    };
    (
        o.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
    )
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

fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, log) = compile(&[src], &out, &jar, scalac_path);
    assert!(ok, "compile failed:\n{log}");
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// The number of errors a compiler's summary line reports.
fn error_count(log: &str, scalac: bool) -> usize {
    let found = log.lines().rev().find_map(|l| {
        let l = l.trim();
        if scalac {
            l.strip_suffix(" errors")
                .or_else(|| l.strip_suffix(" error"))
                .and_then(|n| n.parse().ok())
        } else {
            l.split_once(" error(s)").and_then(|(n, _)| n.parse().ok())
        }
    });
    found.unwrap_or(0)
}

/// `srcs` (fixture-relative) are rejected, with every message in `want`,
/// by scala-rs -- or, given `scalac_path`, by scalac with the same count.
fn check_rejects(srcs: &[&str], want: &[&str], count: usize, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let paths: Vec<PathBuf> = srcs.iter().map(|s| fixtures_dir().join(s)).collect();
    let dir = tmp_dir("bad");
    let (ok, log) = compile(&paths, &dir, &jar, scalac_path);
    assert!(!ok, "{srcs:?} compiled:\n{log}");
    for w in want {
        assert!(log.contains(w), "{srcs:?}: missing `{w}` in:\n{log}");
    }
    assert_eq!(
        error_count(&log, scalac_path.is_some()),
        count,
        "{srcs:?}: error count in:\n{log}"
    );
    let _ = fs::remove_dir_all(&dir);
}

const MODIFIERS: &[&str] = &[
    "illegal combination of modifiers: implicit and case for: class IntOps",
    "`sealed` modifier can be used only for classes",
    "illegal combination of modifiers: private and protected for: method g",
    "illegal combination of modifiers: abstract and final for: trait T",
    "`override` modifier not allowed for classes",
    "`implicit` modifier not allowed for constructors",
    "abstract member may not have private modifier",
    "abstract member may not have final modifier",
    "only traits and abstract classes can have declared but undefined members",
    "`abstract override` modifier not allowed for type members",
];

const NAMES: &[&str] = &[
    "x is already defined as value x",
    "y is already defined as value y",
    "C is already defined as class C",
    "C is already defined as object C",
    "D is already defined as (compiler-generated) case class companion object D",
    "value z is defined twice;\n  the conflicting method z was defined at line 9:17",
    "T is already defined as type T",
    "a is already defined as value a",
    "b is already defined as value b",
    "method m is defined twice;\n  the conflicting method m was defined at line 11:43",
    "u is already defined as value u",
];

const REFS: &[&str] = &[
    "ambiguous reference to overloaded definition,\nboth method over in object Test of type (a: String): Int\nand  method over in object Test of type (a: Int): Int\nmatch expected type ?",
    "missing argument list for method any in object Test",
    "both method over in class Holder of type (a: String): Int",
    "not enough arguments for constructor X: (x: Int): X.\nUnspecified value parameter x.",
    "not enough arguments for constructor Two: (a: Int)(b: Int): Two.",
    "not found: type NoSuchAnnot",
    "type xxxxx is not a member of package",
    "not found: type Missing",
];

const REFCHECKS: &[&str] = &[
    "in object Test, multiple overloaded alternatives of method apply define default arguments.",
    "in class B, multiple overloaded alternatives of method f define default arguments.\nThe members with defaults are defined in trait T and class A.",
    "`override` modifier required to override concrete member:",
];

#[test]
fn wacc_accepts_runs() {
    check_runs("wacc_accepts", None);
}

#[test]
fn scalac_agrees_wacc_accepts() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("wacc_accepts", Some(&sc));
}

#[test]
fn wacc_modifiers_rejected() {
    check_rejects(&["wacc_modifiers_bad.scala"], MODIFIERS, 10, None);
}

#[test]
fn scalac_agrees_wacc_modifiers() {
    if let Some(sc) = scalac() {
        check_rejects(&["wacc_modifiers_bad.scala"], MODIFIERS, 10, Some(&sc));
    }
}

#[test]
fn wacc_byname_rejected() {
    // An implicit by-name parameter of a method is fine (the fixture's
    // `imp`); only the two field parameters are errors.
    let want = ["`val` parameters may not be call-by-name"];
    check_rejects(&["wacc_byname_bad.scala"], &want, 2, None);
    if let Some(sc) = scalac() {
        check_rejects(&["wacc_byname_bad.scala"], &want, 2, Some(&sc));
    }
}

#[test]
fn wacc_names_rejected() {
    check_rejects(&["wacc_names_bad.scala"], NAMES, 11, None);
}

#[test]
fn scalac_agrees_wacc_names() {
    if let Some(sc) = scalac() {
        check_rejects(&["wacc_names_bad.scala"], NAMES, 11, Some(&sc));
    }
}

#[test]
fn wacc_companions_in_two_files_rejected() {
    let srcs = [
        "wacc_companion/A.scala",
        "wacc_companion/B.scala",
        "wacc_companion/C.scala",
    ];
    let want = [
        "Companions 'class Split' and 'object Split' must be defined in same file:",
        "Twice is already defined as class Twice",
    ];
    check_rejects(&srcs, &want, 2, None);
    if let Some(sc) = scalac() {
        check_rejects(&srcs, &want, 2, Some(&sc));
    }
}

#[test]
fn wacc_refs_rejected() {
    let mut want = REFS.to_vec();
    want.push("ambiguous overload for f with arguments (\"\")");
    check_rejects(&["wacc_refs_bad.scala"], &want, 9, None);
}

#[test]
fn scalac_agrees_wacc_refs() {
    if let Some(sc) = scalac() {
        check_rejects(&["wacc_refs_bad.scala"], REFS, 9, Some(&sc));
    }
}

#[test]
fn wacc_refchecks_rejected() {
    check_rejects(&["wacc_refchecks_bad.scala"], REFCHECKS, 3, None);
    if let Some(sc) = scalac() {
        check_rejects(&["wacc_refchecks_bad.scala"], REFCHECKS, 3, Some(&sc));
    }
}

#[test]
fn wacc_header_pass_completion_reports() {
    let want = [
        "type mismatch",
        "not enough arguments for constructor X: (x: Int): X.",
    ];
    check_rejects(&["wacc_import_bad.scala"], &want, 2, None);
    if let Some(sc) = scalac() {
        check_rejects(&["wacc_import_bad.scala"], &want, 2, Some(&sc));
    }
}

#[test]
fn wacc_dotted_package_clause_scope() {
    check_rejects(&["wacc_pkg_bad.scala"], &["not found: type Top"], 1, None);
    if let Some(sc) = scalac() {
        check_rejects(
            &["wacc_pkg_bad.scala"],
            &["not found: type Top"],
            1,
            Some(&sc),
        );
    }
}

#[test]
fn wacc_interpolation_and_escape_scanning() {
    let want = [
        "invalid string interpolation $ , expected: $$, $\", $identifier or ${expression}",
        "invalid escape",
    ];
    check_rejects(&["wacc_syntax_bad.scala"], &want, 2, None);
    if let Some(sc) = scalac() {
        check_rejects(&["wacc_syntax_bad.scala"], &want, 2, Some(&sc));
    }
}
