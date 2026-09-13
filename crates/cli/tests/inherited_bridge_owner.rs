//! A pickle must not re-own an inherited overload as a bridge of the child
//! trait.  Real scalac trusts that owner when it emits mixin forwarders; a
//! false bridge then links against a `$` helper which cannot exist.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("{name}.scala"))
}

fn scala_library() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.is_file().then_some(path)
}

fn scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

fn temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-inherited-bridge-owner-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn compile_producer(source: &Path, out: &Path, library: &Path) {
    let result = Command::new(bin())
        .args([
            "compile",
            source.to_str().unwrap(),
            "--scala-library",
            library.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("compile producer with scala-rs");
    assert!(
        result.status.success(),
        "scala-rs producer failed:\n{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn real_scalac_uses_the_traits_that_declared_inherited_overloads() {
    let (Some(library), Some(scalac)) = (scala_library(), scalac()) else {
        eprintln!("skip inherited bridge owner: Scala 2.13.16 toolchain unavailable");
        return;
    };
    let root = temp_dir();
    let producer = root.join("producer");
    let consumer = root.join("consumer");
    fs::create_dir_all(&producer).unwrap();
    fs::create_dir_all(&consumer).unwrap();

    compile_producer(&fixture("inherited_bridge_owner_lib"), &producer, &library);
    let cp = format!("{}:{}", producer.display(), library.display());
    let result = Command::new(&scalac)
        .args([
            fixture("inherited_bridge_owner_use").to_str().unwrap(),
            "-cp",
            &cp,
            "-d",
            consumer.to_str().unwrap(),
        ])
        .output()
        .expect("compile consumer with real scalac");
    assert!(
        result.status.success(),
        "real scalac consumer failed:\n{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let run_cp = format!(
        "{}:{}:{}",
        consumer.display(),
        producer.display(),
        library.display()
    );
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &run_cp, "inheritedbridgeowner.Main"])
        .output()
        .expect("run real-scalac consumer");
    assert!(
        run.status.success(),
        "consumer linked to a helper on the wrong trait:\n{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "inheritedbridgeowner.Impl\ninheritedbridgeowner.Impl\n"
    );

    let javap = Command::new("javap")
        .args([
            "-classpath",
            &run_cp,
            "-p",
            "-c",
            "inheritedbridgeowner.Impl",
        ])
        .output()
        .expect("javap consumer");
    assert!(javap.status.success());
    let text = String::from_utf8_lossy(&javap.stdout);
    assert!(
        text.contains("InterfaceMethod inheritedbridgeowner/Foldable.compose$"),
        "Foldable overload was not forwarded to its declaring trait:\n{text}"
    );
    assert!(
        text.contains("InterfaceMethod inheritedbridgeowner/Invariant.compose$"),
        "Invariant overload was not forwarded to its declaring trait:\n{text}"
    );
    assert!(
        !text.contains(
            "InterfaceMethod inheritedbridgeowner/Traverse.compose$:(Linheritedbridgeowner/Traverse;Linheritedbridgeowner/Foldable;)"
        ),
        "false Traverse-owned Foldable bridge survived:\n{text}"
    );

    let _ = fs::remove_dir_all(root);
}
