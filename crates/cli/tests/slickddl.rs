//! Focused regression for Slick's `SchemaDescription` extension conversion.
//!
//! `import db.profile.api._` exposes `schemaActionExtensionMethods`, whose
//! parameter is declared through `RelationalProfile` while the receiver's
//! path-dependent `SchemaDescription` is inherited from `BasicProfile`.  The
//! two spellings describe the same profile family and must be compared after
//! being read through the same import prefix.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/slickddl.scala")
}

fn tmp_dir() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-slickddl-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn scala_library() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.is_file().then_some(path)
}

fn slick_classpath() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let root = PathBuf::from(home).join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2");
    let jars = [
        root.join("com/typesafe/slick/slick_2.13/3.4.1/slick_2.13-3.4.1.jar"),
        root.join("com/typesafe/config/1.4.9/config-1.4.9.jar"),
        root.join("org/slf4j/slf4j-api/2.0.18/slf4j-api-2.0.18.jar"),
        root.join("org/reactivestreams/reactive-streams/1.0.4/reactive-streams-1.0.4.jar"),
        root.join("org/scala-lang/scala-reflect/2.13.16/scala-reflect-2.13.16.jar"),
    ];
    jars.iter().all(|path| path.is_file()).then(|| {
        jars.iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(":")
    })
}

#[test]
fn schema_description_extension_resolves_through_profile_api_import() {
    let (Some(scala_library), Some(classpath)) = (scala_library(), slick_classpath()) else {
        eprintln!("skip slickddl: scala-library or Slick dependencies not cached");
        return;
    };
    let out = tmp_dir();
    let result = Command::new(bin())
        .args([
            "compile",
            fixture().to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-cp",
            &classpath,
            "--scala-library",
            scala_library.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        result.status.success(),
        "Slick DDL fixture failed to compile:\n{diagnostics}"
    );
    assert!(
        !diagnostics.contains("error:"),
        "unexpected diagnostics:\n{diagnostics}"
    );
    let class_count = fs::read_dir(&out).unwrap().count();
    assert!(class_count > 0, "fixture emitted no class files");
    let _ = fs::remove_dir_all(out);
}
