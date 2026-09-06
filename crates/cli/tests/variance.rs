//! Cross-unit Scala variance ABI checks.
//!
//! A JVM `Signature` records generic bounds but not Scala declaration
//! variance. A separately compiled nsc consumer therefore reads the
//! `ScalaSignature` pickle for `+A`, `-A`, and nested `F[+X]` metadata.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn scala_library() -> PathBuf {
    PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar")
}

fn scalac() -> PathBuf {
    PathBuf::from("/tmp/scala-2.13.16/bin/scalac")
}

fn temurin17() -> PathBuf {
    PathBuf::from("/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home")
}

fn tmp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let out = std::env::temp_dir().join(format!("scala-rs-variance-{nanos}"));
    fs::create_dir_all(&out).expect("create variance temp directory");
    out
}

fn with_temurin17(cmd: &mut Command) {
    let home = temurin17();
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let path = format!(
        "{}:{}",
        home.join("bin").display(),
        old_path.to_string_lossy()
    );
    cmd.env("JAVA_HOME", home).env("PATH", path);
}

fn compile_scala_rs(src: &Path, out: &Path, jar: &Path) -> Output {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    with_temurin17(&mut cmd);
    cmd.output().expect("run scala-rs producer")
}

fn compile_scalac(src: &Path, out: &Path, cp: &Path, jar: &Path) -> Output {
    let classpath = format!("{}:{}", cp.display(), jar.display());
    let mut cmd = Command::new(scalac());
    cmd.args([
        "-Xno-forwarders",
        "-classpath",
        &classpath,
        "-d",
        out.to_str().unwrap(),
        src.to_str().unwrap(),
    ]);
    with_temurin17(&mut cmd);
    cmd.output().expect("run scalac")
}

fn diagnostics(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn run_java(classes: &Path, producer: &Path, jar: &Path) -> String {
    let classpath = format!(
        "{}:{}:{}",
        classes.display(),
        producer.display(),
        jar.display()
    );
    let mut cmd = Command::new(temurin17().join("bin/java"));
    cmd.args(["-Xverify:all", "-cp", &classpath, "VarianceExternalGood"]);
    with_temurin17(&mut cmd);
    let out = cmd.output().expect("run variance consumer");
    assert!(out.status.success(), "java failed: {}", diagnostics(&out));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn nsc_consumers_preserve_class_method_and_hk_variance() {
    let jar = scala_library();
    if !jar.is_file() || !scalac().is_file() || !temurin17().join("bin/java").is_file() {
        eprintln!("skip: Scala 2.13.16 or Temurin 17 fixture is unavailable");
        return;
    }
    let root = tmp_dir();
    let producer = fixtures_dir().join("variance_external_producer.scala");
    let good = fixtures_dir().join("variance_external_consumer_good.scala");
    let bad = fixtures_dir().join("variance_external_consumer_bad.scala");
    let nsc_producer = root.join("nsc-producer");
    let ours_producer = root.join("ours-producer");
    let nsc_good = root.join("nsc-good");
    let ours_good = root.join("ours-good");
    let nsc_bad = root.join("nsc-bad");
    let ours_bad = root.join("ours-bad");
    for out in [
        &nsc_producer,
        &ours_producer,
        &nsc_good,
        &ours_good,
        &nsc_bad,
        &ours_bad,
    ] {
        fs::create_dir_all(out).unwrap();
    }

    let out = compile_scalac(&producer, &nsc_producer, &root, &jar);
    assert!(
        out.status.success(),
        "nsc producer failed: {}",
        diagnostics(&out)
    );
    let out = compile_scala_rs(&producer, &ours_producer, &jar);
    assert!(
        out.status.success(),
        "scala-rs producer failed: {}",
        diagnostics(&out)
    );

    // nsc is the reference producer/consumer pair, and must accept the same
    // class variance, method type parameter, and higher-kinded variance shape.
    let out = compile_scalac(&good, &nsc_good, &nsc_producer, &jar);
    assert!(
        out.status.success(),
        "nsc consumer failed: {}",
        diagnostics(&out)
    );
    assert_eq!(run_java(&nsc_good, &nsc_producer, &jar), "dog:dog\n");

    // The real interop boundary: nsc reads scala-rs's pickle and must accept
    // both legal assignments and the nested `F[+X]` kind.
    let out = compile_scalac(&good, &ours_good, &ours_producer, &jar);
    assert!(
        out.status.success(),
        "nsc consumer rejected scala-rs producer: {}",
        diagnostics(&out)
    );
    assert_eq!(run_java(&ours_good, &ours_producer, &jar), "dog:dog\n");

    for (consumer, producer_out) in [(&nsc_bad, &nsc_producer), (&ours_bad, &ours_producer)] {
        let out = compile_scalac(&bad, consumer, producer_out, &jar);
        let text = diagnostics(&out);
        assert!(
            !out.status.success(),
            "illegal variance was accepted: {text}"
        );
        for expected in [
            "VarianceSource[VarianceDog]",
            "VarianceSink[VarianceAnimal]",
            "VarianceBox[VarianceDog]",
        ] {
            assert!(
                text.contains(expected),
                "missing {expected:?} in variance diagnostics: {text}"
            );
        }
    }

    let _ = fs::remove_dir_all(root);
}
