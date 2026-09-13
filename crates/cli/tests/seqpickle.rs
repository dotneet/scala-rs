//! Writer-to-reader regression for a case class whose parameter is the
//! `scala.Seq` alias. The library and consumer are compiled separately so the
//! second invocation has to recover the first invocation's pickle.

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
    let path = PathBuf::from("/private/tmp").join(format!(
        "scala-rs-seqpickle-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn compile(src: &Path, out: &Path, jar: &Path, classpath: Option<&Path>) {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    if let Some(cp) = classpath {
        cmd.args(["-cp", cp.to_str().unwrap()]);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
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
fn separate_compilation_recovers_named_seq_parameter() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library pickle roundtrip: jar not obtainable");
        return;
    };

    let root = tmp_dir("named");
    let lib_out = root.join("lib");
    let use_out = root.join("use");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&use_out).unwrap();

    compile(
        &fixtures_dir().join("seqpickle_named_lib.scala"),
        &lib_out,
        &jar,
        None,
    );
    compile(
        &fixtures_dir().join("seqpickle_named_use.scala"),
        &use_out,
        &jar,
        Some(&lib_out),
    );

    let cp = format!(
        "{}:{}:{}",
        use_out.display(),
        lib_out.display(),
        jar.display()
    );
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "owner:0\n");

    let _ = fs::remove_dir_all(root);
}
