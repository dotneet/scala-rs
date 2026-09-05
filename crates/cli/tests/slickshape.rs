//! E2E tests for the `agent/slickshape` slice: slick's `Shape` implicits, and
//! an operator two conversions offer.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. The jar-backed halves use the published
//! `slick_2.13-3.4.1.jar` from the local Coursier cache and skip when it is
//! not there, the way `crates/cli/tests/slickimpl.rs` does.
//!
//! Two roots, measured separately on `tests/gitbucket_measure.sh`
//! (1736 → 1694 → 1588, and 1736 → 1630 for the second alone):
//!
//! 1. **A pickled existential lost its bound, and the bound's class had no
//!    parents.** `Query.map` is
//!    `def map[F, G, T](f: E => F)(implicit shape: Shape[_ <: FlatShapeLevel, F, T, G])`,
//!    so `T` and `G` are undetermined and only the witness can settle them.
//!    `PickleSupply::conv_at` turned every quantified variable into a bare
//!    `Type::Wildcard`, which left `repColumnShape`'s own `Level <: ShapeLevel`
//!    with nothing opposite it — and a candidate with an unsolved type
//!    parameter is dropped, so `q.map(_.title)` was "could not find implicit
//!    value of type Shape[_, Rep[String], T, G]" while `q.sortBy(_.id)` (no
//!    implicit clause at all) worked. Keeping the bound is half of it: the
//!    other half is that `candidate_bounds_hold` then asks whether
//!    `_ <: FlatShapeLevel` is a `ShapeLevel`, and `FlatShapeLevel` is a jar
//!    class nothing in the program names, so its parent list was empty and the
//!    answer was no. Naming `FlatShapeLevel` anywhere in the same file made it
//!    compile, which is the shape of a missing completion. Both halves are
//!    needed; neither alone moves anything.
//!
//! 2. **A conversion whose member cannot take the arguments written was still
//!    a candidate.** gitbucket's `implicit class RichColumn(c1: Rep[Boolean])
//!    { def &&(c2: => Rep[Boolean], guard: => Boolean) }` sits in scope beside
//!    slick's one-argument `&&`; the two tied in `search_extension`, which
//!    compares conversions and not members, and every comparison in the
//!    project was `value && is not a member of Rep[Boolean]`. nsc's
//!    `adaptToArguments` asks for a view whose result has a member *applicable
//!    to these arguments*, so this is not an ambiguity.
//!
//! `tests/fixtures/sh_extarity.scala` is root 2 without slick or any jar, and
//! runs in both modes; `sh_shape_jar.scala` is both roots against the real
//! jar. Both `_bad` fixtures pin that the relaxations did not turn into
//! accepting something nsc rejects, and each is cross-checked against real
//! scalac 2.13.16 on the same lines.

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
        "scala-rs-slickshape-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn real_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// slick 3.4.1 and the jars it needs, from the local Coursier cache if they
/// happen to be there. Nothing is downloaded. Same list as
/// `crates/cli/tests/slickimpl.rs`.
fn slick_jars() -> Option<Vec<PathBuf>> {
    let home = std::env::var("HOME").ok()?;
    let roots = [
        PathBuf::from(&home).join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        PathBuf::from(&home).join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ];
    let wanted = [
        ("com/typesafe/slick/slick_2.13", "slick_2.13", Some("3.4.1")),
        ("com/typesafe/config", "config", None),
        ("org/slf4j/slf4j-api", "slf4j-api", None),
        (
            "org/reactivestreams/reactive-streams",
            "reactive-streams",
            None,
        ),
    ];
    let mut out = Vec::new();
    for (rel, prefix, pin) in wanted {
        let mut found = None;
        for root in &roots {
            let Ok(rd) = fs::read_dir(root.join(rel)) else {
                continue;
            };
            for ent in rd.flatten() {
                let version = ent.file_name().to_string_lossy().into_owned();
                if pin.is_some_and(|p| p != version) {
                    continue;
                }
                let candidate = ent.path().join(format!("{prefix}-{version}.jar"));
                if candidate.is_file() {
                    found = Some(candidate);
                }
            }
        }
        out.push(found?);
    }
    Some(out)
}

fn classpath(jars: &[PathBuf]) -> String {
    jars.iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// Compile one fixture. Answers (success, diagnostics, output directory).
fn compile(name: &str, extra: &[&str]) -> (bool, String, PathBuf) {
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
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), msgs, out)
}

fn scalac_run(scalac: &Path, name: &str, cp: Option<&str>) -> (bool, String) {
    let out = tmp_dir(name);
    let mut cmd = Command::new(scalac);
    if let Some(cp) = cp {
        cmd.args(["-cp", cp]);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    cmd.arg(fixtures_dir().join(format!("{name}.scala")));
    let output = cmd.output().expect("run scalac");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&out);
    (output.status.success(), msgs)
}

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Root 2 with no jar at all: the one-argument conversion answers `a &&& b`,
/// the two-argument one answers `a.&&&(b, g)`, and a conversion whose extra
/// parameter has a default stays applicable at the shorter arity.
#[test]
fn two_conversions_offering_one_operator_are_told_apart_by_the_arguments() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip sh_extarity: scala-library jar not present");
        return;
    };
    let (ok, msgs, out) = compile("sh_extarity", &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "sh_extarity failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        assert_eq!(
            run_main(&cp),
            expected_stdout("sh_extarity"),
            "stdout mismatch for sh_extarity"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture against the private runtime: nothing in it needs the real
/// standard library.
#[test]
fn sh_extarity_runs_under_the_private_runtime() {
    let (ok, msgs, out) = compile("sh_extarity", &["--no-scala-library"]);
    assert!(ok, "sh_extarity failed to compile (private):\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    if java_available() {
        assert_eq!(
            run_main(&out.display().to_string()),
            expected_stdout("sh_extarity"),
            "stdout mismatch for sh_extarity under the private runtime"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Narrowing by the argument count only ever *narrows*: a call no conversion
/// can take is still "is not a member", and a genuine ambiguity the argument
/// count cannot break is still rejected.
#[test]
fn sh_extarity_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip sh_extarity_bad: scala-library jar not present");
        return;
    };
    let (ok, msgs, out) = compile(
        "sh_extarity_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected sh_extarity_bad to be rejected:\n{msgs}");
    for want in [
        "value &&& is not a member of Cell",
        "value ~~~ is not a member of Cell",
    ] {
        assert!(msgs.contains(want), "expected {want:?} in:\n{msgs}");
    }
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_run(&scalac, "sh_extarity_bad", None);
        assert!(!ok, "real scalac accepted sh_extarity_bad:\n{msgs}");
        assert!(
            msgs.contains("value &&& is not a member of Cell")
                && msgs.contains("value ~~~ is not a member of Cell"),
            "real scalac rejected sh_extarity_bad for other reasons:\n{msgs}"
        );
    }
}

/// Both roots against the published slick jar: `q.map`, `q.filter`, and an
/// `&&` that ties with gitbucket's `RichColumn`.
#[test]
fn slicks_shape_witnesses_resolve_through_its_published_jar() {
    let Some(lib) = scala_library_jar() else {
        eprintln!("skip sh_shape_jar: scala-library jar not present");
        return;
    };
    let Some(jars) = slick_jars() else {
        eprintln!("skip sh_shape_jar: slick 3.4.1 not in the local Coursier cache");
        return;
    };
    let (ok, msgs, out) = compile(
        "sh_shape_jar",
        &[
            "-cp",
            &classpath(&jars),
            "--scala-library",
            lib.to_str().unwrap(),
        ],
    );
    assert!(ok, "sh_shape_jar failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_run(&scalac, "sh_shape_jar", Some(&classpath(&jars)));
        assert!(ok, "real scalac rejected sh_shape_jar:\n{msgs}");
    }
}

/// A projection slick has no `Shape` for is still a missing implicit, and an
/// operator at an arity no conversion offers is still not a member. Real
/// scalac reports the same two lines.
#[test]
fn sh_shape_jar_bad_is_still_rejected() {
    let Some(lib) = scala_library_jar() else {
        eprintln!("skip sh_shape_jar_bad: scala-library jar not present");
        return;
    };
    let Some(jars) = slick_jars() else {
        eprintln!("skip sh_shape_jar_bad: slick 3.4.1 not in the local Coursier cache");
        return;
    };
    let (ok, msgs, out) = compile(
        "sh_shape_jar_bad",
        &[
            "-cp",
            &classpath(&jars),
            "--scala-library",
            lib.to_str().unwrap(),
        ],
    );
    assert!(!ok, "expected sh_shape_jar_bad to be rejected:\n{msgs}");
    for want in [
        "could not find implicit value of type Shape[_ <: FlatShapeLevel, NoShape, T, G]",
        "value && is not a member of Rep[Boolean]",
    ] {
        assert!(msgs.contains(want), "expected {want:?} in:\n{msgs}");
    }
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_run(&scalac, "sh_shape_jar_bad", Some(&classpath(&jars)));
        assert!(!ok, "real scalac accepted sh_shape_jar_bad:\n{msgs}");
        assert!(
            msgs.contains("No matching Shape found")
                && msgs.contains("value && is not a member of slick.lifted.Rep[Boolean]"),
            "real scalac rejected sh_shape_jar_bad for other reasons:\n{msgs}"
        );
    }
}
