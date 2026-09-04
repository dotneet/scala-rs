//! Value classes that arrive from `-cp`, not from the source being compiled.
//!
//! slick's twelve run programs all stopped at
//!
//! ```text
//! VerifyError: Type integer is not assignable to 'java/lang/Object'
//!   Location: slick/cats/Database$$anon$265.$anonfun$3 @11: checkcast
//! ```
//!
//! from `fs2.Stream.fromIterator[IO](it, chunkSize = 1)`.
//! `fs2.Stream.PartiallyAppliedFromIterator` is a value class over a
//! `Boolean`, so `fs2/Stream$.fromIterator` really does have the descriptor
//! `()Z` and nsc compiles the application as
//! `PartiallyAppliedFromIterator$.MODULE$.apply$extension(Z, …)`. scala-rs
//! emitted `checkcast fs2/Stream$PartiallyAppliedFromIterator` on the boolean
//! and called `apply` on it as an instance method.
//!
//! The reason is that `extends AnyVal` exists **only in the pickle**: a value
//! class's class file has `java/lang/Object` for a superclass and no
//! interfaces, so nothing in the class file distinguishes it from an ordinary
//! final class, and `SymbolTable::is_value_class` -- which the whole of
//! erasure and the `$extension` call path hang off -- answered no.
//!
//! This test builds a stand-in library with real scalac
//! (`tests/fixtures/cpvalueclass_lib.scala`) covering the shapes that matter:
//! a top-level value class over a primitive and over a reference, one that
//! also extends a universal trait, and -- the fs2 shape -- one nested in an
//! object, which gets no static forwarders at all, reached through a method
//! whose descriptor is the underlying type.

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
        "scala-rs-cpvalueclass-{tag}-{}-{nanos}-{seq}",
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
                .join("cpvalueclass_lib.scala")
                .to_str()
                .unwrap(),
        ])
        .status()
        .expect("run scalac on the stand-in library");
    assert!(status.success(), "real scalac failed on cpvalueclass_lib");
    lib
}

/// The same client, compiled by both compilers against the same
/// scalac-built library, must print the same bytes.
#[test]
fn calls_into_a_binary_value_class_match_scalac() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip cpvalueclass: scalac or scala-library not obtainable");
        return;
    };
    let lib = build_lib(&scalac);
    let src = fixtures_dir().join("cpvalueclass_use.scala");

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
    assert!(status.success(), "real scalac failed on cpvalueclass_use");
    let expected = run_main(&format!(
        "{}:{}:{}",
        ref_out.display(),
        lib.display(),
        jar.display()
    ))
    .expect("scalac-built cpvalueclass_use runs");

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
        "scala-rs failed on cpvalueclass_use: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let actual = run_main(&format!(
        "{}:{}:{}",
        ours.display(),
        lib.display(),
        jar.display()
    ))
    .expect("our cpvalueclass_use runs");

    assert_eq!(actual, expected, "stdout differs from real scalac");
    let _ = fs::remove_dir_all(&lib);
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&ours);
}

/// The call shape itself, so a change that keeps the stdout by some other
/// route still has to say so out loud.
///
/// A **top-level** binary value class is reached through nsc's own static
/// forwarder on the class (`Meters.describe$extension`); a **nested** one has
/// no forwarders, so the call goes through the companion module and the module
/// has to be pushed *before* the receiver -- there is no way to insert it
/// under several argument slots afterwards.
#[test]
fn the_call_goes_to_the_extension_method() {
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip cpvalueclass javap: scalac or scala-library not obtainable");
        return;
    };
    let lib = build_lib(&scalac);
    let ours = tmp_dir("javap");
    let out = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join("cpvalueclass_use.scala")
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
    assert!(out.status.success(), "scala-rs failed on cpvalueclass_use");

    let javap = Command::new("javap")
        .args(["-p", "-c", "-cp", ours.to_str().unwrap(), "Main$"])
        .output();
    let Ok(javap) = javap else {
        eprintln!("skip cpvalueclass javap: no javap");
        return;
    };
    if !javap.status.success() {
        eprintln!("skip cpvalueclass javap: javap failed");
        return;
    }
    let text = String::from_utf8_lossy(&javap.stdout).into_owned();
    for want in [
        // The method really returns the underlying `int` ...
        "Method cpvc/Factory$.make:(I)I",
        // ... and every call on the result is an `$extension`, never an
        // instance method on a `cpvc/Meters` that was never allocated.
        "Method cpvc/Meters.describe$extension:(I)Ljava/lang/String;",
        "Method cpvc/Meters.plus$extension:(II)I",
        "Method cpvc/Name.shout$extension:(Ljava/lang/String;)Ljava/lang/String;",
        // The nested one, through its companion module.
        "Field cpvc/Holder$Partial$.MODULE$",
        "apply$extension:(ZLscala/collection/Iterator;I)Ljava/lang/String;",
    ] {
        assert!(text.contains(want), "missing {want:?} in Main$:\n{text}");
    }
    // The shape this test exists to forbid: the underlying value cast to the
    // value class and the method called on it as an instance.
    for unwanted in [
        "Method cpvc/Meters.describe:()Ljava/lang/String;",
        "Method cpvc/Name.shout:()Ljava/lang/String;",
        "Method cpvc/Holder$Partial.apply:",
    ] {
        assert!(
            !text.contains(unwanted),
            "found {unwanted:?}: the value class is still called as an instance:\n{text}"
        );
    }
    let _ = fs::remove_dir_all(&lib);
    let _ = fs::remove_dir_all(&ours);
}
