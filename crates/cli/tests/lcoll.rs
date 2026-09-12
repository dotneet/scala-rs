//! The collection hierarchy's own shape arguments (agent/libcoll).
//!
//! Five roots behind the collection-shape clusters in scala/scala's own
//! `src/library`:
//!
//! * a view that is a `Function1` *by inheritance* (`<:<`, `=:=`, a user class
//!   extending `A => B`), and a view result that applies a higher-kinded type
//!   parameter -- `scala/runtime/Tuple2Zipped.scala`'s `invert`;
//! * a higher-kinded parameter's bounds resolved without its own scope, so a
//!   second parameter spelling its argument the same way took the first one's
//!   symbol;
//! * `apply` inserted for a receiver whose type is an abstract type whose
//!   bound declares it -- `scala/collection/convert/impl/IndexedSeqStepper.scala`;
//! * a *compound* upper bound's base-type arguments, merged as nsc's
//!   `GlbBaseTypeSeq` merges them -- `scala/collection/LinearSeq.scala`'s
//!   `these = these.tail`, and `this.type` on an applied abstract receiver;
//! * object-private members are not variance-checked, and a view's type
//!   parameter bounds decide both its applicability and its specificity
//!   (`Predef`'s `wrapRefArray` / `wrapCharArray` / `genericWrapArray`).
//!
//! Fixture prefix: `lcoll_`. Every accepting fixture has a scalac-agreement
//! test that compiles and runs it with real scalac 2.13.16 and compares the
//! same expected output; every rejecting one asserts that scalac rejects it
//! too.

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
        "scala-rs-lcoll-{tag}-{}-{nanos}-{seq}",
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

fn compile(name: &str, jar: &Path, out: &Path, scalac_path: Option<&Path>) -> std::process::Output {
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

/// Compile `name`, run it, and compare with `expected/<name>.txt`.
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
    let output = compile(name, &jar, &out, scalac_path);
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// Compile `name` and require a rejection; every `needle` must appear in the
/// diagnostics, and `count` of them are expected.
fn check_rejects(name: &str, needles: &[&str], count: usize, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, &jar, &out, scalac_path);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "expected a rejection:\n{text}");
    for n in needles {
        assert!(text.contains(n), "missing {n:?} in:\n{text}");
    }
    assert_eq!(
        text.matches("error").count().min(count),
        count,
        "expected {count} errors in:\n{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn view_that_inherits_function1_supplies_members() {
    check_runs("lcoll_viewbound", None);
}

#[test]
fn scalac_agrees_view_that_inherits_function1() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lcoll_viewbound", Some(&sc));
}

#[test]
fn no_view_keeps_the_member_error() {
    check_rejects(
        "lcoll_viewbound_bad",
        &["value iterator is not a member of"],
        3,
        None,
    );
}

#[test]
fn scalac_agrees_no_view_keeps_the_member_error() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("lcoll_viewbound_bad", &["iterator"], 3, Some(&sc));
}

#[test]
fn apply_through_an_abstract_receivers_bound() {
    check_runs("lcoll_absapply", None);
}

#[test]
fn scalac_agrees_apply_through_an_abstract_receivers_bound() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lcoll_absapply", Some(&sc));
}

#[test]
fn a_bound_without_apply_is_still_rejected() {
    check_rejects(
        "lcoll_absapply_bad",
        &["value apply is not a member of T"],
        3,
        None,
    );
}

#[test]
fn scalac_agrees_a_bound_without_apply_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects(
        "lcoll_absapply_bad",
        &["does not take parameters"],
        3,
        Some(&sc),
    );
}

#[test]
fn compound_bound_members_are_read_at_the_glb() {
    check_runs("lcoll_compound", None);
}

#[test]
fn scalac_agrees_compound_bound_members_are_read_at_the_glb() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lcoll_compound", Some(&sc));
}

#[test]
fn object_private_members_are_not_variance_checked() {
    check_runs("lcoll_privatethis", None);
}

#[test]
fn scalac_agrees_object_private_members_are_not_variance_checked() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lcoll_privatethis", Some(&sc));
}

#[test]
fn every_other_access_is_still_variance_checked() {
    check_rejects(
        "lcoll_privatethis_bad",
        &[
            "covariant type T occurs in contravariant position",
            "contravariant type T occurs in covariant position",
        ],
        7,
        None,
    );
}

#[test]
fn scalac_agrees_every_other_access_is_variance_checked() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects(
        "lcoll_privatethis_bad",
        &[
            "covariant type T occurs in contravariant position",
            "contravariant type T occurs in covariant position",
        ],
        7,
        Some(&sc),
    );
}

#[test]
fn views_are_ordered_by_their_parameter_bounds() {
    check_runs("lcoll_viewspec", None);
}

#[test]
fn scalac_agrees_views_are_ordered_by_their_parameter_bounds() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("lcoll_viewspec", Some(&sc));
}
