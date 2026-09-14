//! A Scala classpath reader must preserve a generic accessor's stable path.
//!
//! The library and consumer are compiled separately so the consumer reads
//! `Config[P].profile` from classfiles/pickle, then uses `c.profile` both as a
//! wildcard-import prefix and as a singleton type prefix.

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

fn compile(src: &Path, out: &Path, jar: &Path, cp: Option<&Path>) {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    if let Some(cp) = cp {
        cmd.args(["-cp", cp.to_str().unwrap()]);
    }
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {} failed:\n{}{}",
        src.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generic_profile_accessor_is_a_stable_path_after_separate_compilation() {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !jar.is_file() {
        eprintln!("skip stable profile path: scala-library jar not obtainable");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "scala-rs-stable-profile-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let lib = root.join("lib");
    let use_ = root.join("use");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&use_).unwrap();

    compile(
        &fixtures_dir().join("stable_profile_lib.scala"),
        &lib,
        &jar,
        None,
    );
    compile(
        &fixtures_dir().join("stable_profile_use.scala"),
        &use_,
        &jar,
        Some(&lib),
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inherited_profile_accessor_preserves_a_narrowed_api_bound() {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !jar.is_file() {
        eprintln!("skip inherited profile API: scala-library jar not obtainable");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "scala-rs-inherited-profile-api-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    compile(
        &fixtures_dir().join("profile_api_inherited.scala"),
        &root,
        &jar,
        None,
    );

    fs::remove_dir_all(root).unwrap();
}
