//! E2E tests for the `agent/setapply` slice: a companion `apply` completed
//! from the jar a second time, next to the hand-written prelude copy of the
//! very same declaration.
//!
//! `object Set extends IterableFactory[Set]`'s `apply(elems: A*): Set[A]` is
//! hand-written in `crates/typer/src/prelude.rs` (`add_set`), on the module
//! class itself, so `Set(1, 2, 3)` still works under `--no-scala-library`.
//! It carries no `pickled_origin` -- only a member `PickleSupply` actually
//! installed from a pickle does.
//!
//! Selecting `Set[A]`'s own member `apply(A): Boolean` (from `SetOps`,
//! reached by calling a `Set` value like a function -- `xs(elem)`) is a
//! *different* member, on a different class, and nothing has ever completed
//! it. `Check::ensure_apply_supplied` completes it via
//! `PickleSupply::complete`, which -- so an instance-only override such as
//! `scala.math.BigDecimal`'s is not hidden by an empty class-side lookup --
//! *also* asks the companion module for `apply`, unconditionally, and unions
//! the two results. That companion ask is a genuine first ask (nothing had
//! asked the module directly before), so it re-reads `apply` from the jar and
//! installs a second copy of `IterableFactory.apply` right next to the
//! prelude's own.
//!
//! The two copies share an owner (the module class), so `drop_overridden`'s
//! override rule -- which only ever fires *across* owners -- does not apply.
//! And only the pickle-derived one carries a `pickled_origin`, so
//! `collapse_pickled_copies` (the `agent/ambigmap` fix) does not apply
//! either: it only merges when *both* sides have one. A later `Set(x)` then
//! sees both and is `ambiguous overload for apply` -- but only if something
//! had already forced the class-side `apply` to complete first. Order
//! dependent, like `agent/ambigmap` before it, and for the same underlying
//! reason: a pickled declaration reached twice is one member, not two, and
//! this time one of the two reaches was not through a pickle at all.
//!
//! The fix: before installing a pickle-derived member, decline it when the
//! class already carries a hand-written (`pickled_origin`-empty) member of
//! the same name whose erased parameters are identical. Prelude always wins
//! -- stated as one of `pickle_supply`'s three governing rules already, just
//! not enforced at this particular seam. Nothing is keyed on the name `Set`;
//! `Map`, `List` and `Seq` share the same completion path and are exercised
//! here too, though none of them reproduced the bug (their companions'
//! `apply` happens to get asked before any instance-side completion in the
//! programs that matter), so the fixture also acts as a non-regression net
//! for them.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `sa` prefix.

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
        "scala-rs-setapply-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: declining a duplicate changes *which* symbol the call
/// site picks, and so the owner and descriptor codegen writes. A wrong pick
/// shows up here as a verification failure rather than as a silent
/// difference.
fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
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

/// The regression itself: a member `apply` (via `Repo.hasTag`'s `xs(tag)`)
/// completes first, forcing the companion's `apply` to complete as a side
/// effect; a later bare `Set(...)` must still resolve to one candidate. The
/// reverse order, plus `Map` / `List` / `Seq`, ride along as a non-regression
/// net for the same completion path.
#[test]
fn fixtures_sa_setapply() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("sa_setapply", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("sa_setapply"),
        "stdout mismatch for library dual-run sa_setapply"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through **real scalac**, so the expected output is
/// pinned to what Scala 2.13.16 prints and not just to our own.
#[test]
fn real_scalac_dual_run_sa_setapply() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip real-scalac dual-run: jar or scalac not obtainable");
        return;
    };
    let dir = tmp_dir("sa_setapply-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("sa_setapply.scala"))
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac rejected sa_setapply:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run_java(&dir, Some(jar.to_str().unwrap())),
        expected_stdout("sa_setapply"),
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The private runtime has no `scala.collection` pickle at all, so bare
/// `Set` is not found -- a clean diagnostic, not a silent accept.
#[test]
fn sa_setapply_without_the_library_is_diagnosed() {
    compile_fails(
        "sa_setapply",
        &["--no-scala-library"],
        "not found: type Set",
    );
}

/// The decline is keyed on the erased *shape*, not on the name: two
/// genuinely different `apply` overloads a call cannot choose between stay
/// two, and the ambiguity is still reported -- as it is in scalac.
#[test]
fn fixtures_sa_setapply_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    compile_fails(
        "sa_setapply_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "ambiguous overload for apply",
    );
}

/// scalac rejects the `_bad` fixture too, for the same reason.
#[test]
fn real_scalac_rejects_sa_setapply_bad() {
    let Some(scalac) = scalac() else {
        eprintln!("skip: scalac not obtainable");
        return;
    };
    let dir = tmp_dir("sa_setapply_bad-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("sa_setapply_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(!out.status.success(), "scalac accepted sa_setapply_bad");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("ambiguous reference to overloaded definition"),
        "unexpected scalac error: {err}"
    );
    let _ = fs::remove_dir_all(&dir);
}
