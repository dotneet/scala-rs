//! A later EitherT generator may widen the left type chosen by an earlier one.

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

fn tmp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path =
        std::env::temp_dir().join(format!("scala-rs-et-widen-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn coursier_jar(home: &Path, group: &str, artifact: &str, version: &str) -> Option<PathBuf> {
    for root in [
        home.join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        home.join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ] {
        let path = root
            .join(group)
            .join(version)
            .join(format!("{artifact}-{version}.jar"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn scala_library() -> Option<PathBuf> {
    let direct = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if direct.is_file() {
        return Some(direct);
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    coursier_jar(
        &home,
        "org/scala-lang/scala-library",
        "scala-library",
        "2.13.16",
    )
}

#[test]
fn later_eithert_left_map_widens_previous_generator() {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("skip: HOME is unavailable");
        return;
    };
    let Some(core) = coursier_jar(
        &home,
        "org/typelevel/cats-core_2.13",
        "cats-core_2.13",
        "2.13.0",
    ) else {
        eprintln!("skip: cats-core jar is not present in Coursier cache");
        return;
    };
    let Some(kernel) = coursier_jar(
        &home,
        "org/typelevel/cats-kernel_2.13",
        "cats-kernel_2.13",
        "2.13.0",
    ) else {
        eprintln!("skip: cats-kernel jar is not present in Coursier cache");
        return;
    };
    let Some(library) = scala_library() else {
        eprintln!("skip: scala-library jar is not present");
        return;
    };
    let source = fixtures_dir().join("et_left_widen.scala");
    let out = tmp_dir();
    let cp = format!("{}:{}", core.display(), kernel.display());
    let output = Command::new(bin())
        .args([
            "compile",
            source.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            &cp,
            "--scala-library",
            library.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    assert!(output.status.success(), "compile failed:\n{diagnostics}");
}
