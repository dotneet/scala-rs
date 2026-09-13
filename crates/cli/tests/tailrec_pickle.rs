//! A static-forwarder ScalaSignature must preserve module-class ownership.
//!
//! Scala 2 puts the complete pickle for `object TailCalls` on
//! `TailCalls.class`, even though that file does not end in `$`; its nested
//! `TailRec` type is owned by the module class. This is deliberately a
//! writer-to-real-scalac-reader check: a JVM descriptor alone is still valid
//! when the pickle emits the wrong `ExtRef` owner, so only scalac's reader
//! catches the interoperability failure.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "scala-rs-tailrec-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn scala_library() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

fn status_message(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn compile_source(compiler: &Path, source: &Path, out: &Path, classpath: &str) -> String {
    let result = Command::new(compiler)
        .args([
            "compile",
            source.to_str().unwrap(),
            "--scala-library",
            "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
            "-cp",
            classpath,
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs");
    assert!(
        result.status.success(),
        "scala-rs rejected {}:\n{}",
        source.display(),
        status_message(&result)
    );
    status_message(&result)
}

#[test]
fn static_forwarder_nested_type_pickle_is_scalac_readable() {
    let (Some(jar), Some(scalac)) = (scala_library(), scalac()) else {
        eprintln!("skip TailRec pickle reader check: Scala 2.13.16 tools absent");
        return;
    };
    let root = temp_dir("reader");
    let lib_out = root.join("lib");
    let client_out = root.join("client");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&client_out).unwrap();
    let lib = root.join("TailLibRs.scala");
    let client = root.join("TailClient.scala");
    fs::write(
        &lib,
        "object TailLibRs {\n  def mk: scala.util.control.TailCalls.TailRec[Int] = scala.util.control.TailCalls.done(1)\n}\n",
    )
    .unwrap();
    fs::write(
        &client,
        "object TailClient {\n  val x: scala.util.control.TailCalls.TailRec[Int] = TailLibRs.mk\n  def main(args: Array[String]): Unit = println(x.result)\n}\n",
    )
    .unwrap();

    compile_source(
        Path::new(env!("CARGO_BIN_EXE_scala-rs")),
        &lib,
        &lib_out,
        jar.to_str().unwrap(),
    );
    let cp = format!("{}:{}", lib_out.display(), jar.display());
    let result = Command::new(scalac)
        .args([
            "-classpath",
            &cp,
            "-d",
            client_out.to_str().unwrap(),
            client.to_str().unwrap(),
        ])
        .output()
        .expect("run scalac reader");
    assert!(
        result.status.success(),
        "scalac could not read scala-rs TailRec pickle:\n{}",
        status_message(&result)
    );
    let runtime_cp = format!(
        "{}:{}:{}",
        client_out.display(),
        lib_out.display(),
        jar.display()
    );
    let runtime = Command::new("java")
        .args(["-Xverify:all", "-cp", &runtime_cp, "TailClient"])
        .output()
        .expect("run TailRec reader client");
    assert!(
        runtime.status.success(),
        "TailRec reader client failed at runtime:\n{}",
        status_message(&runtime)
    );
    assert_eq!(runtime.stdout, b"1\n");
    fs::remove_dir_all(root).unwrap();
}
