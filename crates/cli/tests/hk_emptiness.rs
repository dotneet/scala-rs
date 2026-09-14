//! Higher-kinded method bounds must survive a scalac pickle boundary.

use crate::support;

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn fixtures() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn diagnostics(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn compile_scala_rs(source: &Path, out: &Path, provider: &Path, library: &Path) -> Output {
    Command::new(support::scala_rs())
        .arg("compile")
        .arg(source)
        .args(["-d", out.to_str().unwrap(), "--scala-library"])
        .arg(library)
        .args(["-cp", provider.to_str().unwrap()])
        .output()
        .expect("run scala-rs")
}

#[test]
fn scalac_higher_kinded_emptiness_bounds_select_and_reject_like_nsc() {
    let toolchain = support::toolchain();
    let (Some(scalac), Some(java), Some(library)) = (
        toolchain.scalac(),
        toolchain.java(),
        toolchain.scala_library(),
    ) else {
        eprintln!("skip higher-kinded Emptiness binary test: Scala/JVM toolchain unavailable");
        return;
    };

    let root = support::TestDir::new("hk-emptiness");
    let provider = root.join("provider");
    let consumer = root.join("consumer");
    let bad = root.join("bad");
    fs::create_dir_all(&provider).unwrap();
    fs::create_dir_all(&consumer).unwrap();
    fs::create_dir_all(&bad).unwrap();

    let built = Command::new(scalac)
        .arg(fixtures().join("hk_emptiness_binary_lib.scala"))
        .args(["-d", provider.to_str().unwrap()])
        .output()
        .expect("run scalac provider");
    assert!(
        built.status.success(),
        "scalac provider failed:\n{}",
        diagnostics(&built)
    );

    let selected = compile_scala_rs(
        &fixtures().join("hk_emptiness_binary_use.scala"),
        &consumer,
        &provider,
        library,
    );
    assert!(
        selected.status.success(),
        "scala-rs consumer failed:\n{}",
        diagnostics(&selected)
    );

    let classpath = env::join_paths([consumer.as_path(), provider.as_path(), library])
        .expect("runtime classpath");
    let ran = Command::new(java)
        .args(["-Xverify:all", "-cp"])
        .arg(classpath)
        .arg("Main")
        .output()
        .expect("run consumer");
    assert!(
        ran.status.success(),
        "consumer failed:\n{}",
        diagnostics(&ran)
    );
    assert_eq!(ran.stdout, b"option\niterable\njava\n");

    let rejected = compile_scala_rs(
        &fixtures().join("hk_emptiness_binary_bad.scala"),
        &bad,
        &provider,
        library,
    );
    assert!(
        !rejected.status.success(),
        "scala-rs accepted Option through an Iterable-only bound"
    );
    let messages = diagnostics(&rejected);
    assert!(
        messages.contains("no implicit") && !messages.contains("ambiguous implicit"),
        "unexpected negative diagnostic: {messages}"
    );
}
