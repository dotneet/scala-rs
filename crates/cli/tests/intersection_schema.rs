//! A real-scalac reader regression for a refined method parameter.
//!
//! Slick's `data.schema` is supplied by an implicit conversion whose argument
//! is `Query[...] with TableQuery[...]`. The JVM descriptor erases that
//! intersection to `TableQuery`; the generic Signature must name the same
//! owner or scalac cannot use the conversion when it reads the class files.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn cached_library() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.is_file().then_some(path)
}

fn cached_scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = std::env::temp_dir().join(format!(
        "scala-rs-intersection-schema-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("intersection schema test directory");
    path
}

#[test]
fn real_scalac_reads_intersection_signature_for_data_schema() {
    let Some(library) = cached_library() else {
        eprintln!("skip intersection Signature scalac reader probe: scala-library unavailable");
        return;
    };
    let Some(scalac) = cached_scalac() else {
        eprintln!("skip intersection Signature scalac reader probe: scalac unavailable");
        return;
    };
    let root = temp_dir();
    let lib_out = root.join("lib");
    let use_out = root.join("use");
    fs::create_dir_all(&lib_out).expect("library output directory");
    fs::create_dir_all(&use_out).expect("reader output directory");

    let writer = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            fixtures_dir()
                .join("intersection_schema_lib.scala")
                .to_str()
                .unwrap(),
            "-d",
            lib_out.to_str().unwrap(),
            "--scala-library",
            library.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs intersection schema writer");
    assert!(
        writer.status.success(),
        "scala-rs intersection schema writer failed: {}{}",
        String::from_utf8_lossy(&writer.stdout),
        String::from_utf8_lossy(&writer.stderr)
    );

    let javap = Command::new("javap")
        .args(["-p", "-v", "-classpath", lib_out.to_str().unwrap(), "API"])
        .output()
        .expect("inspect intersection schema Signature");
    assert!(
        javap.status.success(),
        "javap could not inspect API.class: {}{}",
        String::from_utf8_lossy(&javap.stdout),
        String::from_utf8_lossy(&javap.stderr)
    );
    let javap_text = String::from_utf8_lossy(&javap.stdout);
    assert!(
        javap_text.contains("(LTableQuery<TT;>;)LTableQueryExtensionMethods<TT;>;"),
        "API's generic Signature must use the TableQuery erasure: {javap_text}"
    );

    let classpath = format!("{}:{}", lib_out.display(), library.display());
    let reader = Command::new(scalac)
        .args([
            "-classpath",
            &classpath,
            "-d",
            use_out.to_str().unwrap(),
            fixtures_dir()
                .join("intersection_schema_use.scala")
                .to_str()
                .unwrap(),
        ])
        .output()
        .expect("run real scalac intersection schema reader");
    assert!(
        reader.status.success(),
        "real scalac could not read intersection Signature for data.schema (status={}): {}{}",
        reader.status,
        String::from_utf8_lossy(&reader.stdout),
        String::from_utf8_lossy(&reader.stderr)
    );
    assert!(
        use_out.join("data.class").is_file(),
        "real scalac produced no data.class"
    );
    let _ = fs::remove_dir_all(root);
}
