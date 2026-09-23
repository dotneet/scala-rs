use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/withfilter_subtype.scala")
}

fn temp_out() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let out = std::env::temp_dir().join(format!("scala-rs-withfilter-subtype-{stamp}"));
    fs::create_dir_all(&out).unwrap();
    out
}

fn scala_library() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

#[test]
fn inherited_with_filter_keeps_declared_base_result() {
    let Some(jar) = scala_library() else {
        eprintln!("skip: scala-library unavailable");
        return;
    };
    let out = temp_out();
    let result = Command::new(bin())
        .args([
            "compile",
            fixture().to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "compile failed: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    let result = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "query\n");
    let _ = fs::remove_dir_all(out);
}

#[test]
fn indexed_with_filter_flat_map_accepts_seq_callback() {
    let Some(jar) = scala_library() else {
        eprintln!("skip: scala-library unavailable");
        return;
    };
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/withfilter_flatmap_iterable_once.scala");
    let out = temp_out();
    let compiled = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "compile failed: {}{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    let ran = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "1,3\n");
    let _ = fs::remove_dir_all(out);
}
