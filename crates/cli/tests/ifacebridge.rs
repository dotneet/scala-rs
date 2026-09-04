//! Bridges a class needs for members it inherits from *binary* interfaces.
//!
//! slick's `ExpandTables` died with
//!
//! ```text
//! ClassCastException: scala.collection.immutable.$colon$colon
//!   cannot be cast to scala.collection.immutable.IndexedSeq
//! ```
//!
//! from `ConstArray.toSeq.groupBy(…)`. `ConstArray.toSeq` returns
//! `new immutable.IndexedSeq[T] { def apply(i) = …; def length = … }`, and
//! `groupBy` goes through `IterableFactoryDefaults.newSpecificBuilder`, which
//! calls `iterableFactory()` at the **wide** descriptor
//! `()Lscala/collection/IterableFactory;`. `immutable.IndexedSeq` overrides
//! `iterableFactory` at the narrow `()Lscala/collection/SeqFactory;` and the
//! library's interfaces carry no bridge between the two, so the wide call
//! resolved to `immutable.Iterable`'s default — an `Iterable` factory, whose
//! builder is a `List` builder. nsc puts the bridge on the implementing class;
//! `crates/backend/src/ifacebridge.rs` now does too.
//!
//! The same rule, one level up, is why `filter` was an `AbstractMethodError`
//! on `fromSpecific` and why `toString` printed
//! `slick.util.ConstArray$$anon$630@281e3708`: a method inherited from the
//! superclass beats an interface default, and `java.lang.Object` is above
//! every class.
//!
//! This test cannot use `scala.collection` itself — scala-rs does not yet
//! accept `new immutable.IndexedSeq[T] { … }` outside a run that also reads
//! the collections from their class files. It builds a stand-in library with
//! real scalac instead (`tests/fixtures/ifacebridge_lib.scala`), which gives
//! the identical class-file shape: a covariant override with no bridge on the
//! interface, and a trait `toString` / `hashCode` / `equals`.

use std::fs;
use std::path::PathBuf;
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
        "scala-rs-ifacebridge-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    let which = Command::new("which").arg("scalac").output().ok()?;
    which
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&which.stdout).trim().to_string()))
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_main(cp: &str) -> Result<String, String> {
    let out = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Build the stand-in library with real scalac; returns its output directory.
fn build_lib(scalac: &PathBuf) -> PathBuf {
    let lib = tmp_dir("lib");
    let status = Command::new(scalac)
        .args([
            "-d",
            lib.to_str().unwrap(),
            fixtures_dir()
                .join("ifacebridge_lib.scala")
                .to_str()
                .unwrap(),
        ])
        .status()
        .expect("run scalac on the stand-in library");
    assert!(status.success(), "real scalac failed on ifacebridge_lib");
    lib
}

/// The same client, compiled by both compilers against the same
/// scalac-built library, must print the same bytes.
#[test]
fn inherited_binary_members_match_scalac() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip ifacebridge: scalac or scala-library not obtainable");
        return;
    };
    let lib = build_lib(&scalac);
    let src = fixtures_dir().join("ifacebridge_use.scala");

    let ref_out = tmp_dir("scalac");
    let status = Command::new(&scalac)
        .args([
            "-d",
            ref_out.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .status()
        .expect("run scalac on the client");
    assert!(status.success(), "real scalac failed on ifacebridge_use");
    let expected = run_main(&format!(
        "{}:{}:{}",
        ref_out.display(),
        lib.display(),
        jar.display()
    ))
    .expect("scalac-built ifacebridge_use runs");

    let ours = tmp_dir("rs");
    let out = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            ours.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        out.status.success(),
        "scala-rs failed on ifacebridge_use: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let actual = run_main(&format!(
        "{}:{}:{}",
        ours.display(),
        lib.display(),
        jar.display()
    ))
    .expect("our ifacebridge_use runs");

    assert_eq!(actual, expected, "stdout differs from real scalac");
    let _ = fs::remove_dir_all(&lib);
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&ours);
}

/// The bridge itself, so a future change that keeps the stdout by some other
/// route still has to say so out loud.
#[test]
fn the_class_carries_the_wide_descriptor() {
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip ifacebridge javap: scalac or scala-library not obtainable");
        return;
    };
    let lib = build_lib(&scalac);
    let ours = tmp_dir("javap");
    let out = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join("ifacebridge_use.scala")
                .to_str()
                .unwrap(),
            "-d",
            ours.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(out.status.success(), "scala-rs failed on ifacebridge_use");

    let javap = Command::new("javap")
        .args(["-p", "-cp", ours.to_str().unwrap(), "Impl"])
        .output();
    let Ok(javap) = javap else {
        eprintln!("skip ifacebridge javap: no javap");
        return;
    };
    if !javap.status.success() {
        eprintln!("skip ifacebridge javap: javap failed");
        return;
    }
    let text = String::from_utf8_lossy(&javap.stdout).into_owned();
    assert!(
        text.contains("ifb.Fac fac()"),
        "Impl needs the wide `fac()Lifb/Fac;` bridge:\n{text}"
    );
    assert!(
        text.contains("java.lang.String toString()"),
        "Impl needs a `toString` forwarder — Object's would win otherwise:\n{text}"
    );
    let _ = fs::remove_dir_all(&lib);
    let _ = fs::remove_dir_all(&ours);
}
