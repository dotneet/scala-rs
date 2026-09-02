//! E2E tests for the `agent/proj` slice: type-projection member re-reading
//! (`A#B`) and what a `package` clause actually opens.
//!
//! 1. **`A#B` lost its prefix.** `project_from_prefix` answered a projection
//!    with a bare `Type::Class`, which has no room for one, so every later
//!    selection read `B`'s members at *`B`'s owner's* declarations. slick
//!    writes `def run(ctx: HeapBackend#BasicActionContext) = f(ctx.session)`,
//!    and `session: Session` is declared on `BasicBackend`, where `Session`
//!    is abstract -- only `HeapBackend` says `type Session = HeapSessionDef`.
//!    Result: "value database is not a member of BasicBackend.Session".
//!
//!    The projection now carries what the prefix settles as a type-only
//!    refinement (`Checker::projected_class_type`), marked with
//!    `symbol::AS_SEEN_FROM_MARK` so that subtyping and display read it as the
//!    bare parent: `JdbcBackend#JdbcSessionDef` and the same class reached
//!    through `type Session = JdbcSessionDef` have to stay one type, and a
//!    first cut that left the refinement constraining invented eight new
//!    errors saying so.
//!
//! 2. **A `package` clause opens the package it names, not the ones on the
//!    way there** (SLS 9.2). `package p.q` opens only `p.q`; the nested
//!    spelling `package p { package q { … } }` opens both. Walking the owner
//!    chain instead let slick's own `slick.cats` package shadow the real
//!    `cats` for every file under `package slick.*`, so `cats.effect.IO` in
//!    `package slick.dbio` came out as
//!    "value effect is not a member of <notype>".
//!
//!    `Checker::open_packages` now answers with the file's own clauses, the
//!    root last. A last-resort walk over the packages in between survives as
//!    `expose_from_unopened_packages`, and its doc comment names the hole it
//!    covers: a default argument's right-hand side is typed at the *call
//!    site*, in the caller's scope, whenever no `f$default$n` getter is there
//!    to call.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `pj` prefix.

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
        "scala-rs-proj-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    None
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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
        "compile {name} failed extra={extra:?}:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// `-Xverify:all`, so a bad frame from the projected receiver is a
/// verification failure here rather than a silent difference in the output.
fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
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
        "java -Xverify:all {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `--no-scala-library`: the private runtime.
fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None, "Main"),
            expected_stdout(name),
            "stdout mismatch for {name} (private runtime)"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// `--scala-library`: linked against the real 2.13.16 ABI, then run.
fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s), "Main"),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
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
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------- 1. `A#B` member re-reading

#[test]
fn fixtures_pj_projmember() {
    dual_run_fixture("pj_projmember");
}

#[test]
fn fixtures_pj_projmember_private() {
    check_private("pj_projmember");
}

/// Reading `A#B`'s members through `A` is not a licence to accept anything:
/// an unsettled prefix, a prefix that settles the member to a class without
/// that member, and a name that is simply not there are all still errors --
/// the same three nsc 2.13.16 reports.
#[test]
fn fixtures_pj_projmember_bad_is_rejected() {
    compile_fails(
        "pj_projmember_bad",
        &["--no-scala-library"],
        &[
            "value database is not a member of Base.S",
            "value database is not a member of Other",
            "value nosuch is not a member of Sess",
        ],
    );
}

/// The same three, straight from scalac, so the fixture cannot drift into
/// asserting behaviour nsc does not have.
#[test]
fn scalac_agrees_pj_projmember_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-projmember-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("pj_projmember_bad.scala"))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted the bad fixture");
    for needle in [
        "value database is not a member of Base#S",
        "value database is not a member of Other",
        "value nosuch is not a member of Sess",
    ] {
        assert!(
            err.contains(needle),
            "scalac output missing {needle:?}: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// -------------------------------------------- 2. what a `package` opens

#[test]
fn fixtures_pj_pkgscope() {
    dual_run_fixture("pj_pkgscope");
}

#[test]
fn fixtures_pj_pkgscope_private() {
    check_private("pj_pkgscope");
}

/// The qualified spelling. `package pjq.sub` opens `pjq.sub` and nothing
/// else, so the `inner` in `inner.Deep` is the *top-level* one even though
/// `pjq.inner` exists -- which is exactly slick's `slick.cats` versus the
/// real `cats`. Needs several files (a leading `package p.q` clause is one
/// per file), so it builds its sources rather than using a fixture.
const Q_CFG: &str = "package pjq\nobject Cfg { val n = 41 }\n";
const Q_PKG_INNER: &str = "package pjq.inner\nobject Deep { val d = 1 }\n";
const Q_TOP_INNER: &str = "package inner\nobject Deep { val d = 2 }\n";
const Q_USE: &str = "package pjq.sub\nobject Use { val b = inner.Deep.d }\n";
const Q_MAIN: &str = "object Main { def main(a: Array[String]): Unit = println(pjq.sub.Use.b) }\n";

fn write_qualified_sources(dir: &Path) -> Vec<PathBuf> {
    let files = [
        ("cfg.scala", Q_CFG),
        ("pkginner.scala", Q_PKG_INNER),
        ("topinner.scala", Q_TOP_INNER),
        ("use.scala", Q_USE),
        ("main.scala", Q_MAIN),
    ];
    files
        .iter()
        .map(|(n, s)| {
            let p = dir.join(n);
            fs::write(&p, s).unwrap();
            p
        })
        .collect()
}

#[test]
fn a_qualified_package_clause_does_not_open_its_parent() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let dir = tmp_dir("qualified");
    let srcs = write_qualified_sources(&dir);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in &srcs {
        cmd.arg(s);
    }
    cmd.args([
        "-d",
        out.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        "2\n",
        "`inner` under `package pjq.sub` must be the top-level package, \
         not `pjq.inner`"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The same program through scalac: 2, not 1. Without this the test above
/// only says what this compiler does.
#[test]
fn scalac_agrees_a_qualified_package_clause_does_not_open_its_parent() {
    let (Some(sc), true) = (scalac(), java_available()) else {
        eprintln!("skip: scalac or java not available");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let dir = tmp_dir("scalac-qualified");
    let srcs = write_qualified_sources(&dir);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut cmd = Command::new(sc);
    cmd.args(["-d", out.to_str().unwrap()]);
    for s in &srcs {
        cmd.arg(s);
    }
    let output = cmd.output().expect("run scalac");
    assert!(
        output.status.success(),
        "scalac failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(run_java(&out, Some(jar.to_str().unwrap()), "Main"), "2\n");
    let _ = fs::remove_dir_all(&dir);
}
