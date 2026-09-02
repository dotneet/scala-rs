//! E2E tests for the `agent/tail6` slice.
//!
//! 1. **A default argument's right-hand side was typed at the call site.**
//!    A default with no `name$default$n` getter to call -- a primary
//!    constructor's above all, since nsc emits those on the companion and this
//!    compiler synthesizes none -- was spliced into the argument list as the
//!    tree the namer stored and typed wherever the call happened to be. Its
//!    names were then resolved in the *caller's* scope, and its span was
//!    reported against the caller's source, so the caret landed on an
//!    unrelated line. slick's `class DriverDataSource(…, classLoader:
//!    ClassLoader = ClassLoaderUtil.defaultClassLoader)` is written under
//!    `import slick.util.ClassLoaderUtil`; the caller in
//!    `slick/jdbc/DatabaseConfig.scala` is not, and reported `not found: value
//!    ClassLoaderUtil` pointing at a `new DriverDataSource(…)` two files away.
//!
//!    `Checker::record_default_scope` now keeps the scope the default was
//!    written in and `type_default_rhs_here` types it there, marking the
//!    result `NodeId::PRETYPED_DEFAULT` so the argument list does not type it
//!    again. That is what `agent/proj` left behind as the reason its
//!    `expose_from_unopened_packages` fallback had to stay; the fallback is
//!    **deleted** in this slice.
//!
//! 2. **An implicit parameter with a default.** nsc falls back to the default
//!    when the search finds nothing. slick's
//!    `ScalaBaseType.apply[T](implicit classTag: ClassTag[T], ordering:
//!    Ordering[T] = null)` is written around that, and `ScalaBaseType[T]` for
//!    an abstract `T` was `could not find implicit value of type Ordering[T]`.
//!
//! 3. **`prelude_regex` was shadowing the library.** It declared `findAllIn` /
//!    `findFirstMatchIn` / `replaceAllIn` / `replaceFirstIn` / `split` as a
//!    fallback, guarded on `lookup_member(...).is_empty()` -- which is always
//!    true at install time, because a jar member is only visible once
//!    something has asked for it. So the fallback was what every call got:
//!    `Any` results (`value map is not a member of Any`) and `String`
//!    parameters where the library takes `CharSequence`, which links to
//!    nothing (`NoSuchMethodError: Regex.replaceAllIn(String, String)`).
//!
//! 4. **A jar candidate's parents were never read.** `implicit F: Async[F]`
//!    answers `Sync[F]` only through `Async extends … Sync[F]`, and for a
//!    class the program has merely *named* that parent list is empty until
//!    something completes it. The implicit search runs under an immutable
//!    borrow and cannot complete anything itself, so it found nothing --
//!    unless an earlier line in the same file happened to mention `Async[F]`
//!    as a type, which warmed it. `warm_implicit_candidates` now runs after a
//!    search comes up empty.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `t6` prefix.

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
        "scala-rs-tail6-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
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

/// `-Xverify:all`, so a spliced default that reads a field off the wrong
/// receiver is a verification failure here rather than a silent difference.
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

// ------------------------------------- 1 + 2. where a default is typed

#[test]
fn fixtures_t6_defaults() {
    dual_run_fixture("t6_defaults");
}

#[test]
fn fixtures_t6_defaults_private() {
    check_private("t6_defaults");
}

/// A default is typed in the scope it was written in, and names that are not
/// in *that* scope are errors -- including the class's own members, which a
/// constructor default cannot reach because `new C(1)` has no instance yet.
///
/// `val a = 99` sits at the call site on purpose: before this slice the
/// spliced tree was typed there and `a` resolved to the caller's local (or,
/// with the class scope still in play, to the *field*, which then read off the
/// caller's `this` and threw `ClassCastException` at run time).
#[test]
fn fixtures_t6_defaults_bad_is_rejected() {
    compile_fails(
        "t6_defaults_bad",
        &["--no-scala-library"],
        &["not found: value a", "not found: value Hidden"],
    );
}

/// The same two, straight from scalac: both are errors nsc 2.13.16 reports,
/// so the fixture cannot drift into asserting behaviour nsc does not have.
/// (nsc rejects `class Pair(val a: Int, val b: Int = a)` too -- a constructor
/// default's getter is emitted on the companion, where the parameters of the
/// constructor are not in scope.)
#[test]
fn scalac_agrees_t6_defaults_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-defaults-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("t6_defaults_bad.scala"))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted the bad fixture");
    for needle in ["not found: value a", "not found: value Hidden"] {
        assert!(
            err.contains(needle),
            "scalac output missing {needle:?}: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// scalac's own answer for the good fixture, so the expected output is nsc's
/// and not this compiler's opinion.
#[test]
fn scalac_agrees_t6_defaults() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or the scala-library jar is not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-defaults");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("t6_defaults.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected t6_defaults: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str(), "Main"),
        expected_stdout("t6_defaults")
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------- 3. Regex's real ABI

#[test]
fn fixtures_t6_regex() {
    dual_run_fixture("t6_regex");
}

/// The private runtime has no `Regex`, and says so rather than accepting the
/// program with a hand-written signature that links to nothing.
#[test]
fn t6_regex_is_diagnosed_without_the_library() {
    compile_fails(
        "t6_regex",
        &["--no-scala-library"],
        &["value r is not a member of"],
    );
}

#[test]
fn scalac_agrees_t6_regex() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or the scala-library jar is not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-regex");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("t6_regex.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected t6_regex: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str(), "Main"),
        expected_stdout("t6_regex")
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------ 4. a jar candidate answers for its supertypes

/// cats-effect-kernel (with cats-core / cats-kernel) from the local Coursier
/// cache, if they happen to be there. Nothing is downloaded. Same shape as
/// `crates/cli/tests/jarpickle.rs`.
fn cats_effect_jars() -> Option<Vec<PathBuf>> {
    let home = std::env::var("HOME").ok()?;
    let roots = [
        PathBuf::from(&home).join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        PathBuf::from(&home).join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ];
    let wanted = [
        ("org/typelevel/cats-core_2.13", "cats-core_2.13"),
        ("org/typelevel/cats-kernel_2.13", "cats-kernel_2.13"),
        (
            "org/typelevel/cats-effect-kernel_2.13",
            "cats-effect-kernel_2.13",
        ),
        ("org/typelevel/cats-effect_2.13", "cats-effect_2.13"),
        ("org/typelevel/cats-effect-std_2.13", "cats-effect-std_2.13"),
    ];
    let mut out = Vec::new();
    for (rel, prefix) in wanted {
        let mut found = None;
        for root in &roots {
            let Ok(rd) = fs::read_dir(root.join(rel)) else {
                continue;
            };
            for ent in rd.flatten() {
                let version = ent.file_name().to_string_lossy().into_owned();
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

/// `implicit F: Async[F]` is a `Sync[F]` and a `GenTemporal[F, Throwable]`,
/// and the implicit search has to see that without being told: `Async`'s
/// parents live in a jar and nothing had read them. The `probe` lines below
/// each failed on their own before this slice, and passed as soon as any
/// earlier line in the same file mentioned `Async[F]` as a type -- the shape
/// of a missing completion, not of a scoping rule.
const ASYNC_USER: &str = r#"
import cats.effect.Async
import cats.effect.kernel.{GenTemporal, Sync}

// `cats.effect.Async` on purpose: the alias in the `cats.effect` package
// object, which is how slick spells it. Reached that way, `Async`'s parent
// list was still the stub's when the search ran.
class T6Async[F[_]](implicit private val F: Async[F]) {
  def probeSync: Sync[F] = implicitly[Sync[F]]
  def probeTemporal: GenTemporal[F, Throwable] = implicitly[GenTemporal[F, Throwable]]
}
"#;

#[test]
fn an_implicit_from_a_jar_answers_for_its_supertypes() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some(cats) = cats_effect_jars() else {
        eprintln!("skip: no cats-effect jars in the local Coursier cache");
        return;
    };
    let dir = tmp_dir("async");
    let src = dir.join("user.scala");
    fs::write(&src, ASYNC_USER).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let cp = cats
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":");
    let output = Command::new(bin())
        .args(["compile", src.to_str().unwrap()])
        .args(["-d", out.to_str().unwrap()])
        .args(["-cp", &cp])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.status.success(), "compile failed:\n{msgs}");
    let _ = fs::remove_dir_all(&dir);
}
