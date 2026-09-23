//! An inherited binary alias keeps the declaring symbol and the receiver's
//! specialized prefix when written into a ScalaSignature.

#[path = "support/temp_nonce.rs"]
mod temp_nonce;

use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const SCALA_LIBRARY: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const SCALAC: &str = "/tmp/scala-2.13.16/bin/scalac";

#[test]
fn inherited_alias_uses_its_declaring_owner_in_a_consumer_pickle() {
    if !PathBuf::from(SCALA_LIBRARY).is_file() || !PathBuf::from(SCALAC).is_file() {
        eprintln!("skip: Scala 2.13 toolchain is not installed in /tmp");
        return;
    }

    let nonce = temp_nonce::unique_stamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    let root = std::env::temp_dir().join(format!("scala-rs-inherited-alias-owner-{nonce}"));
    let lib = root.join("lib");
    let producer = root.join("producer");
    let consumer = root.join("consumer");
    for dir in [&lib, &producer, &consumer] {
        fs::create_dir_all(dir).unwrap();
    }
    let lib_src = root.join("Library.scala");
    let producer_src = root.join("Producer.scala");
    let consumer_src = root.join("Consumer.scala");
    fs::write(
        &lib_src,
        r#"package inheritedalias
trait Aliases[T] { type Box[A] = Either[T, A] }
trait API extends Aliases[String]
"#,
    )
    .unwrap();
    fs::write(
        &producer_src,
        r#"package inheritedalias
abstract class Producer(val api: API) {
  import api._
  def action: Box[Int]
}
"#,
    )
    .unwrap();
    fs::write(
        &consumer_src,
        r#"package inheritedalias
class Consumer(api: API) extends Producer(api) {
  def action: Either[String, Int] = Right(1)
}
"#,
    )
    .unwrap();

    let library = Command::new(SCALAC)
        .args(["-d", lib.to_str().unwrap(), lib_src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        library.status.success(),
        "Scala library: {}{}",
        String::from_utf8_lossy(&library.stderr),
        String::from_utf8_lossy(&library.stdout)
    );
    let classpath = format!("{}:{SCALA_LIBRARY}", lib.display());
    let produced = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            producer_src.to_str().unwrap(),
            "--scala-library",
            SCALA_LIBRARY,
            "-cp",
            &classpath,
            "-d",
            producer.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        produced.status.success(),
        "scala-rs producer: {}{}",
        String::from_utf8_lossy(&produced.stderr),
        String::from_utf8_lossy(&produced.stdout)
    );
    let classpath = format!("{}:{}", lib.display(), producer.display());
    let consumed = Command::new(SCALAC)
        .args([
            "-cp",
            &classpath,
            "-d",
            consumer.to_str().unwrap(),
            consumer_src.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        consumed.status.success(),
        "Scala consumer: {}{}",
        String::from_utf8_lossy(&consumed.stderr),
        String::from_utf8_lossy(&consumed.stdout)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn inherited_nested_class_uses_its_declaring_owner_in_a_consumer_pickle() {
    if !PathBuf::from(SCALA_LIBRARY).is_file() || !PathBuf::from(SCALAC).is_file() {
        eprintln!("skip: Scala 2.13 toolchain is not installed in /tmp");
        return;
    }

    let nonce = temp_nonce::unique_stamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    let root = std::env::temp_dir().join(format!("scala-rs-inherited-nested-owner-{nonce}"));
    let lib = root.join("lib");
    let producer = root.join("producer");
    let consumer = root.join("consumer");
    for dir in [&lib, &producer, &consumer] {
        fs::create_dir_all(dir).unwrap();
    }
    let lib_src = root.join("Library.scala");
    let producer_src = root.join("Producer.scala");
    let consumer_src = root.join("Consumer.scala");
    fs::write(
        &lib_src,
        r#"package nestedclass
trait Tables { case class Row(id: Int) }
object Tables extends Tables
"#,
    )
    .unwrap();
    fs::write(
        &producer_src,
        r#"package nestedclass
class Producer { def row: Tables.Row = null }
"#,
    )
    .unwrap();
    fs::write(
        &consumer_src,
        r#"package nestedclass
class Consumer extends Producer { override def row: Tables.Row = null }
"#,
    )
    .unwrap();

    let library = Command::new(SCALAC)
        .args(["-d", lib.to_str().unwrap(), lib_src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        library.status.success(),
        "Scala library: {}{}",
        String::from_utf8_lossy(&library.stderr),
        String::from_utf8_lossy(&library.stdout)
    );
    let classpath = format!("{}:{SCALA_LIBRARY}", lib.display());
    let produced = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            producer_src.to_str().unwrap(),
            "--scala-library",
            SCALA_LIBRARY,
            "-cp",
            &classpath,
            "-d",
            producer.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        produced.status.success(),
        "scala-rs producer: {}{}",
        String::from_utf8_lossy(&produced.stderr),
        String::from_utf8_lossy(&produced.stdout)
    );
    let classpath = format!("{}:{}", lib.display(), producer.display());
    let consumed = Command::new(SCALAC)
        .args([
            "-cp",
            &classpath,
            "-d",
            consumer.to_str().unwrap(),
            consumer_src.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        consumed.status.success(),
        "Scala consumer: {}{}",
        String::from_utf8_lossy(&consumed.stderr),
        String::from_utf8_lossy(&consumed.stdout)
    );
    let _ = fs::remove_dir_all(root);
}
