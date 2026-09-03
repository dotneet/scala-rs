//! E2E tests for the `agent/dbio` slice: slick's `slick/jdbc/JdbcActionComponent.scala`
//! and `slick/dbio/DBIOAction.scala`.
//!
//! Four roots, all of them upstream of several of the reported errors:
//!
//! * a **parent constructor** took no named arguments. `class MultiInsertAction(…)
//!   extends SimpleJdbcProfileAction[MultiInsertResult](_name = …, statements = …)`
//!   typed each `name = value` as an assignment to a variable that does not
//!   exist, so one call produced `not found: value _name`, `not found: value
//!   statements` *and* `no matching overload for constructor
//!   SimpleJdbcProfileAction with arguments (Unit, Unit)` from the two `Unit`s
//!   they left behind. `new C(b = 2, a = 1)` already reordered; the `extends`
//!   path simply never called it.
//! * a `private[this]` member is **not inherited** (SLS 5.2), so an
//!   unqualified reference to one can only ever mean its own class's `this`.
//!   Reading it through the class we happen to be inside turned slick's
//!   `private[this] def superZip[R2, E2](…): DBIOAction[(R, R2), …]`, referenced
//!   from an anonymous `SynchronousDatabaseAction.Fused[(R, R2), …]`, into
//!   `DBIOAction[((R, R2), R2), …]` -- and `superAsTry` into `Try[Try[R]]`.
//!   With a *public* member scalac reports the very mismatch we did, so the
//!   fix is exactly the `private[this]` case and nothing wider.
//! * `Either.getOrElse` / `Try.getOrElse` were declared `(=> Any): Any`, which
//!   is not a widening but an erasure of the result: slick's
//!   `inv.results(…).getOrElse(throw …)` then reported `map` / `pr` / `close`
//!   `is not a member of Any`, three errors from one signature. nsc has
//!   `getOrElse[B1 >: B](or: => B1): B1` (`crates/typer/src/prelude_dbio.rs`).
//! * a `[B >: A]` lower bound was dropped whenever it still mentioned *any*
//!   type parameter after being read through the receiver -- including a
//!   parameter of the enclosing method, which is a fixed type at the call
//!   site. `def f[T](e: Either[Int, It[T]]) = e.getOrElse(throw …)` therefore
//!   solved `B1` to the argument's `Nothing`, while the same code spelled
//!   `It[String]` compiled. Only the *owner's* and the *method's own*
//!   parameters make a bound unusable.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All new fixtures
//! use the `db` prefix.

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
        "scala-rs-dbio-{tag}-{}-{nanos}-{seq}",
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

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
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
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

/// `-Xverify:all`: a signature read differently would be a `VerifyError` here,
/// not a silent difference in the output.
fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(e) => format!("{}:{}", out.display(), e),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_fails(name: &str, extra: &[&str], needle: &str) {
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
    assert!(
        err.contains(needle),
        "expected {name} error to contain {needle:?}, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through the real scalac 2.13.16: the recorded expectation,
/// scalac's stdout and ours all have to agree.
fn real_scalac_dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff {name}: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let jar_s = jar.to_str().unwrap();
    let reference = run_java(&ref_out, Some(jar_s));
    assert_eq!(
        reference,
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );

    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        reference,
        "stdout differs from real scalac for {name}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

/// The parent-constructor named arguments, the `private[this]` outer member
/// and the `[B >: A]` bound whose `A` mentions the caller's type parameter all
/// work without the library ABI: the private runtime has to accept the same
/// program and print the same thing.
#[test]
fn fixtures_db_private_runtime() {
    let out = compile_fixture_with("db", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout("db"),
            "stdout mismatch for private-runtime db"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_db_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run db: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("db", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("db"),
        "stdout mismatch for library-ABI db"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_db() {
    real_scalac_dual_run("db");
}

/// `Either` / `Try` are library-ABI only (`prelude::add_either` runs under
/// `library_abi`), so their `getOrElse` widening is tested there.
#[test]
fn fixtures_db_lib_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run db_lib: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("db_lib", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("db_lib"),
        "stdout mismatch for library-ABI db_lib"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn real_scalac_dual_run_db_lib() {
    real_scalac_dual_run("db_lib");
}

/// The private runtime backs no `scala.util.Either`, so the fixture has to be
/// diagnosed there, not quietly accepted.
#[test]
fn fixtures_db_lib_without_library_is_error() {
    compile_fails("db_lib", &["--no-scala-library"], "Either");
}

/// Placing named arguments in a parent constructor must not swallow a wrong
/// name: scalac 2.13.16 reports `unknown parameter name: stmt` here, and so
/// do we, in both modes.
#[test]
fn fixtures_db_bad_unknown_parameter_name() {
    compile_fails(
        "db_bad",
        &["--no-scala-library"],
        "unknown parameter name: stmt",
    );
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "db_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "unknown parameter name: stmt",
    );
}
