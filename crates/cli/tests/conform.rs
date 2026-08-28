//! Differential conformance: run each program under scalac 2.13.16 and under
//! scala-rs, and require identical stdout.
//!
//! Programs live in `tests/conform`. Both compilers see the same source and the
//! same `scala-library-2.13.16.jar`, so a divergence is ours. Skipped when
//! scalac or the jar is not available.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn conform_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/conform")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-conform-{tag}-{}-{nanos}",
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

/// Compile `src` with both compilers and compare what `Main` prints.
fn same_as_scalac(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip conform {name}: scalac or scala-library not available");
        return;
    };
    let src = conform_dir().join(format!("{name}.scala"));
    assert!(src.is_file(), "missing conformance program {name}.scala");

    let ref_out = tmp_dir(&format!("{name}-scalac"));
    let status = Command::new(&scalac)
        .args(["-d", ref_out.to_str().unwrap(), src.to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed to compile {name}");
    let expected = run_main(&format!("{}:{}", ref_out.display(), jar.display()))
        .unwrap_or_else(|e| panic!("scalac-built {name} failed to run: {e}"));

    let ours = tmp_dir(name);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            ours.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "scala-rs failed to compile {name}");
    let actual = run_main(&format!("{}:{}", ours.display(), jar.display()))
        .unwrap_or_else(|e| panic!("our {name} failed to run: {e}"));

    assert_eq!(actual, expected, "stdout differs from scalac for {name}");
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&ours);
}

macro_rules! conform {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                same_as_scalac(stringify!($name));
            }
        )*
    };
}

conform!(
    basics,
    patterns,
    collections,
    generics,
    strings,
    options,
    classes,
    interp,
    chains,
    parse_forms,
    lambdas,
    self_type,
    decl_forms,
    layout,
    companion_extends
);
