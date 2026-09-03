//! E2E tests for the `agent/cats3` slice: the two reasons cats' `>>` and
//! cats-effect's `timeoutTo` did not compile.
//!
//! 1. **A by-name formal was not a prototype.** `Infer.protoTypeArgs` solves
//!    the callee's type parameters from the expected type *before* the
//!    arguments are typed, and substitutes them into the formals. scala-rs
//!    only did that for a formal that *is* a bare type parameter, so cats'
//!    `def >>[B](fb: => F[B])(implicit F: FlatMap[F]): F[B]` checked against
//!    `F[Unit]` handed its argument no expected type at all.
//!    `commitResult.fold(asyncF.raiseError, _ => asyncF.unit)` was then the
//!    lub of `F[A]` and `F[Unit]` -- `AnyRef` -- and the call reported
//!    `no matching overload for (=> F[B])(FlatMap[F])F[B] with arguments
//!    (AnyRef)` (slick's `BasicBackend.scala`, three times).
//!
//! 2. **An overloaded member's later clauses were re-read from the
//!    declaration.** Picking an alternative replaced the callee's type with
//!    `st.get(sym).ty` -- the raw declaration, in the *declaring* class's type
//!    parameters -- and `fill_defaults_and_implicits` reads the implicit
//!    clause off that. cats-effect's
//!    `GenTemporalOps_[F[_], A].timeoutTo` is overloaded on `Duration` /
//!    `FiniteDuration`, so its `(implicit F: GenTemporal[F, _])` reached the
//!    search with `GenTemporalOps_`'s own `F` instead of the caller's, and no
//!    candidate could match (slick's `ConcurrencyControl.scala`, twice).
//!    A member that is *not* overloaded kept the substituted type from
//!    `type_select` all along, which is why only overloaded ones were hit.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `c3` prefix.

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
        "scala-rs-cats3-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: a wrongly typed by-name argument is a `Function0` that
/// never gets wrapped, which the verifier catches rather than the type checker.
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

// ------------------------------------------- 1 + 2, with no jar in sight

#[test]
fn fixtures_c3_infer() {
    dual_run_fixture("c3_infer");
}

#[test]
fn fixtures_c3_infer_private() {
    check_private("c3_infer");
}

/// The prototype guides inference; it is not a licence. A value that was
/// already inferred *without* one stays what it was, and an overloaded
/// member's implicit clause read at the receiver's arguments still finds
/// nothing when the witness is for another type constructor.
#[test]
fn fixtures_c3_infer_bad_is_rejected() {
    compile_fails(
        "c3_infer_bad",
        &["--no-scala-library"],
        &[
            "type mismatch",
            "required: Box[Unit]",
            "could not find implicit value of type TC[Box, _]",
        ],
    );
}

/// Straight from scalac 2.13.16, so the negative fixture cannot drift into
/// asserting behaviour nsc does not have. Note the second one in particular:
/// nsc also spells the unfound witness `TC[Box, _]` -- at the *receiver's*
/// type constructor.
#[test]
fn scalac_agrees_c3_infer_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-infer-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("c3_infer_bad.scala"))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted the bad fixture");
    for needle in [
        "type mismatch",
        "required: Box[Unit]",
        "could not find implicit value for parameter t: TC[Box, _]",
    ] {
        assert!(
            err.contains(needle),
            "scalac output missing {needle:?}: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac, run: the expected output is nsc's,
/// not this compiler's idea of it.
#[test]
fn scalac_agrees_c3_infer_output() {
    let (Some(sc), true) = (scalac(), java_available()) else {
        eprintln!("skip: scalac or java not available");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let out = tmp_dir("scalac-infer");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("c3_infer.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected c3_infer:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        expected_stdout("c3_infer")
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------- the real cats jars

/// cats-core / cats-kernel / cats-effect{,-kernel,-std} from the local
/// Coursier cache, if they happen to be there. Nothing is downloaded.
/// Same shape as `crates/cli/tests/tail6.rs`.
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

/// The two shapes `slick/basic/BasicBackend.scala` and
/// `slick/basic/ConcurrencyControl.scala` are written in, reduced to eleven
/// lines. Both reported an error before this slice; real scalac 2.13.16
/// accepts both.
const CATS_USER: &str = r#"
import cats.effect.Async
import cats.syntax.all._
import cats.effect.syntax.all._
import scala.concurrent.duration.FiniteDuration

object C3Cats {
  def andThen[F[_]](implicit F: Async[F]): F[Unit] = {
    val e: Either[Throwable, Unit] = Right(())
    val a: F[Unit] = F.unit
    a >> e.fold(F.raiseError, _ => F.unit)
  }

  def withTimeout[F[_]](wait0: F[Unit], timeout: FiniteDuration)(implicit F: Async[F]): F[Unit] =
    wait0.timeoutTo(timeout, F.raiseError[Unit](new RuntimeException))
}
"#;

fn compile_cats_user() -> Option<String> {
    let jar = scala_library_jar()?;
    let cats = cats_effect_jars()?;
    let dir = tmp_dir("cats");
    let src = dir.join("user.scala");
    fs::write(&src, CATS_USER).unwrap();
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
    let msgs = if output.status.success() {
        String::new()
    } else {
        msgs
    };
    let _ = fs::remove_dir_all(&dir);
    Some(msgs)
}

#[test]
fn cats_flat_map_then_and_timeout_to_compile() {
    let Some(msgs) = compile_cats_user() else {
        eprintln!("skip: cats jars or scala-library jar not present");
        return;
    };
    assert!(msgs.is_empty(), "compile failed:\n{msgs}");
}

/// The same eleven lines through real scalac, so the test above cannot be
/// asserting something nsc rejects.
#[test]
fn scalac_agrees_cats_flat_map_then_and_timeout_to() {
    let (Some(sc), Some(cats)) = (scalac(), cats_effect_jars()) else {
        eprintln!("skip: scalac or cats jars not present");
        return;
    };
    let dir = tmp_dir("scalac-cats");
    let src = dir.join("user.scala");
    fs::write(&src, CATS_USER).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let cp = cats
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(":");
    let output = Command::new(sc)
        .args(["-cp", &cp])
        .args(["-d", out.to_str().unwrap()])
        .arg(&src)
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected the cats source:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&dir);
}
