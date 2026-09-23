//! A deferred class val has an abstract accessor and no backing field.

#[path = "support/temp_nonce.rs"]
mod temp_nonce;

use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn abstract_class_val_is_implemented_by_a_binary_consumer() {
    let scalac = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    let library = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !scalac.is_file() || !library.is_file() {
        eprintln!("skip: Scala 2.13 toolchain is not installed in /tmp");
        return;
    }
    let nonce = temp_nonce::unique_stamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    let root = std::env::temp_dir().join(format!("scala-rs-abstract-val-{nonce}"));
    let producer = root.join("producer");
    let consumer = root.join("consumer");
    for dir in [&producer, &consumer] {
        fs::create_dir_all(dir).unwrap();
    }
    let base_src = root.join("Api.scala");
    let consumer_src = root.join("Main.scala");
    fs::write(
        &base_src,
        "package abstractval\nabstract class Api { val title: String; def count: Int }\n",
    )
    .unwrap();
    fs::write(
        &consumer_src,
        r#"package abstractval
class Impl extends Api {
  override val title: String = "ok"
  override def count: Int = 1
}
object Main {
  def main(args: Array[String]): Unit = println(new Impl().title + ":" + new Impl().count)
}
"#,
    )
    .unwrap();

    let produced = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            base_src.to_str().unwrap(),
            "--scala-library",
            library.to_str().unwrap(),
            "-d",
            producer.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        produced.status.success(),
        "producer: {}{}",
        String::from_utf8_lossy(&produced.stderr),
        String::from_utf8_lossy(&produced.stdout)
    );
    let classfile = producer.join("abstractval/Api.class");
    let javap = Command::new("javap")
        .args(["-p", classfile.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(javap.status.success());
    let shape = String::from_utf8_lossy(&javap.stdout);
    assert!(
        shape.contains("public abstract class abstractval.Api"),
        "{shape}"
    );
    assert!(
        shape.contains("public abstract java.lang.String title();"),
        "{shape}"
    );
    assert!(!shape.contains("java.lang.String title;"), "{shape}");

    let cp = format!("{}:{}", producer.display(), library.display());
    let consumed = Command::new(&scalac)
        .args([
            "-cp",
            &cp,
            "-d",
            consumer.to_str().unwrap(),
            consumer_src.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        consumed.status.success(),
        "consumer: {}{}",
        String::from_utf8_lossy(&consumed.stderr),
        String::from_utf8_lossy(&consumed.stdout)
    );
    let cp = format!(
        "{}:{}:{}",
        consumer.display(),
        producer.display(),
        library.display()
    );
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "abstractval.Main"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "runtime: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok:1");
    let _ = fs::remove_dir_all(root);
}
