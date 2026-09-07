//! E2E test for the `agent/implguard` slice: a wildcard import must not
//! memoize a refusal for a class whose pickle is not readable *yet*.
//!
//! `Typer::import_wildcard` asks the pickle for the implicit members of every
//! class it walks. `PickleSupply::complete_named` declines a class this
//! compiler is not reading yet -- but it records the name in `tried` first,
//! and `tried` is permanent. The `adopt_binary_class` that reads the same
//! class a moment later gets the memo instead of the pickled signature, and
//! since nothing in bytecode records `implicit`, every `implicit def` the
//! class declares ends up in the implicit scope as a plain method that can
//! never be selected. Guarding the walk with `PickleSupply::pickle_readable`
//! is the fix; a class the walk skips is picked up by a later walk of the
//! same import, once something has adopted it.
//!
//! The library half is compiled by **real scalac**, which is the only setting
//! the defect appears in -- the memo is about reading someone else's pickle.
//! `docs/gitbucket.md` has the full account and the real case (blocking-slick's
//! eleven `Query` conversions under `import profile.blockingApi._`).
//!
//! Kept out of `crates/cli/tests/e2e.rs` on purpose; see `.agent-brief.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn multi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/implicit_wildcard_binary")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-implguard-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

/// Compile `Lib_1.scala` with real scalac into a fresh directory.
fn build_library(scalac: &Path, tag: &str) -> PathBuf {
    let out = tmp_dir(tag);
    let status = Command::new(scalac)
        .arg("-d")
        .arg(&out)
        .arg("-cp")
        .arg(&out)
        .arg(multi_dir().join("Lib_1.scala"))
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac Lib_1.scala failed");
    out
}

fn run_java(out: &Path, cp_extra: &str, main: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The conversions a wildcard-imported jar class declares are `implicit`, and
/// they fire.
///
/// This is the whole slice: with the guard reverted the same fixture reports
/// "value described is not a member of 3" and "value naming is not a member
/// of 3", because the wildcard walk memoized a refusal before anything had
/// adopted `Profile#API`.
#[test]
fn multi_implicit_wildcard_conversions_from_classfiles() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip implicit_wildcard_binary: scala-library jar or scalac not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let dir = multi_dir();
    let out = build_library(&scalac, "ok");
    let output = Command::new(bin())
        .args(["compile"])
        .arg(dir.join("Main_1.scala"))
        .arg("-cp")
        .arg(&out)
        .arg("-d")
        .arg(&out)
        .args(["--scala-library", jar_s])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compiling Main_1.scala against scalac's class files failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar_s, "Main"),
        fs::read_to_string(dir.join("expected.txt")).unwrap()
    );
    let _ = fs::remove_dir_all(&out);
}

/// The guard supplies the members the pickle marks `implicit`, and only
/// those: a plain `def` on the same class is still not a conversion.
#[test]
fn multi_implicit_wildcard_plain_def_is_not_a_conversion() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip implicit_wildcard_binary bad: scala-library jar or scalac not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let dir = multi_dir();
    let out = build_library(&scalac, "bad");
    let output = Command::new(bin())
        .args(["compile"])
        .arg(dir.join("Bad_1.scala"))
        .arg("-cp")
        .arg(&out)
        .arg("-d")
        .arg(&out)
        .args(["--scala-library", jar_s])
        .output()
        .expect("run scala-rs compile");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !output.status.success(),
        "a non-implicit def must not act as a conversion: {err}"
    );
    assert!(
        err.contains("value plainly is not a member of"),
        "expected `plainly` to be rejected, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
