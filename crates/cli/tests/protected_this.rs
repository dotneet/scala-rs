//! Separate-compilation regression for generic `protected[this]` methods.
//!
//! The classfile contains both a JVM member and a ScalaSignature. The JVM
//! member is erased, while the pickle preserves the generic result needed by
//! a subclass. The classpath loader must use that richer declaration even
//! though the member is not part of the externally visible API.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn compile(source: &Path, out: &Path, library: &Path, classpath: Option<&Path>) {
    let mut cmd = Command::new(bin());
    cmd.args(["compile", source.to_str().unwrap(), "-d", out.to_str().unwrap()]);
    cmd.args(["--scala-library", library.to_str().unwrap()]);
    if let Some(cp) = classpath {
        cmd.args(["-cp", cp.to_str().unwrap()]);
    }
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compiling {} failed:\n{}{}",
        source.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn protected_this_generic_method_survives_separate_compilation() {
    let Some(library) = scala_library_jar() else {
        eprintln!("skip protected[this] regression: scala-library jar not present");
        return;
    };
    let root = std::env::temp_dir().join(format!(
        "scala-rs-protected-this-{}",
        std::process::id()
    ));
    let producer = root.join("producer.scala");
    let consumer = root.join("consumer.scala");
    let producer_out = root.join("producer-out");
    let consumer_out = root.join("consumer-out");
    fs::create_dir_all(&producer_out).unwrap();
    fs::create_dir_all(&consumer_out).unwrap();
    fs::write(
        &producer,
        "class Producer[A] {\n\
           protected[this] def next[B]: A = null.asInstanceOf[A]\n\
         }\n",
    )
    .unwrap();
    fs::write(
        &consumer,
        "class Derived extends Producer[String] {\n\
           def value: String = next[Int]\n\
         }\n",
    )
    .unwrap();

    compile(&producer, &producer_out, &library, None);
    compile(&consumer, &consumer_out, &library, Some(&producer_out));
    let _ = fs::remove_dir_all(root);
}
