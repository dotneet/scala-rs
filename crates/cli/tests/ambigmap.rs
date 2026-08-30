//! E2E tests for the `agent/ambigmap` slice: one pickled declaration reached
//! through two classes is one member, not an overload set.
//!
//! `PickleSupply` completes a library member on demand and installs it on the
//! class the lookup asked about, because that is where the typer has to find
//! it again. `map` is not declared by any of the collection classes the
//! prelude writes out -- `scala.collection.IterableOps` declares it, and every
//! `Seq`, `IndexedSeq` and `Set` inherits it -- so *which* class ends up
//! carrying the completed copy is decided by whichever receiver asks first,
//! which is a property of the program being compiled.
//!
//! A program that asks on a `scala.Seq` and then on a
//! `scala.collection.IndexedSeq` gets two copies of the one `IterableOps.map`:
//! one on `scala.collection.immutable.Seq`, one on
//! `scala.collection.IndexedSeq`. `scala.IndexedSeq` has both above it and
//! neither below the other, so `drop_overridden` cannot relate them; they
//! differ only in the vocabulary each copy was rewritten into (`Seq[B]` vs
//! `IndexedSeq[B]`), so specificity cannot separate them either. Every
//! `xs.map(f)` on such a receiver came out `ambiguous overload for map` -- 25
//! of slick's errors, and the same shape was waiting for `flatMap`, `filter`,
//! `partition` and `foldLeft`.
//!
//! `Symbol::pickled_origin` records which pickled declaration a completed
//! member stands for (its declaring class plus its erased descriptor, and
//! *not* the class it was installed on), and `drop_overridden` keeps only the
//! first copy of each. nsc sees one `IterableOps.map`; so does this.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `am` prefix.

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
        "scala-rs-ambigmap-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: collapsing two copies changes *which* symbol the call site
/// picks, and so the owner and descriptor codegen writes. A wrong pick shows
/// up here as a verification failure rather than as a silent difference.
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

/// The regression itself: `Seq`, then `collection.IndexedSeq`, then
/// `scala.IndexedSeq`. The order of the three blocks in the fixture is the
/// whole point -- swap them and the duplicate copy never arises.
#[test]
fn fixtures_am_pickledup() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("am_pickledup", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("am_pickledup"),
        "stdout mismatch for library dual-run am_pickledup"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through **real scalac**, so the expected output is pinned
/// to what Scala 2.13.16 prints and not just to our own.
#[test]
fn real_scalac_dual_run_am_pickledup() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip real-scalac dual-run: jar or scalac not obtainable");
        return;
    };
    let dir = tmp_dir("am_pickledup-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("am_pickledup.scala"))
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac rejected am_pickledup:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run_java(&dir, Some(jar.to_str().unwrap())),
        expected_stdout("am_pickledup"),
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The private runtime has no `scala.collection` at all -- no pickle, so no
/// completed members and nothing to collapse. It must say so rather than
/// quietly accept the program.
#[test]
fn am_pickledup_without_the_library_is_diagnosed() {
    compile_fails(
        "am_pickledup",
        &["--no-scala-library"],
        "not found: type Seq",
    );
}

/// The collapse is keyed on the pickled *declaration*, not on the name: two
/// genuinely different alternatives stay two, and an ambiguity between them
/// is still an error -- as it is in scalac.
#[test]
fn fixtures_am_pickledup_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    compile_fails(
        "am_pickledup_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "ambiguous overload for f",
    );
}

/// scalac rejects the `_bad` fixture too, for the same reason.
#[test]
fn real_scalac_rejects_am_pickledup_bad() {
    let Some(scalac) = scalac() else {
        eprintln!("skip: scalac not obtainable");
        return;
    };
    let dir = tmp_dir("am_pickledup_bad-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("am_pickledup_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(!out.status.success(), "scalac accepted am_pickledup_bad");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("ambiguous reference to overloaded definition"),
        "unexpected scalac error: {err}"
    );
    let _ = fs::remove_dir_all(&dir);
}
