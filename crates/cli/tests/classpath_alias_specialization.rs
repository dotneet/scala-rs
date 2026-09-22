//! An inherited alias is specialized for its receiver without changing the
//! generic declaration that other receivers inherit.

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

fn temp_dir() -> PathBuf {
    let nonce = temp_nonce::unique_stamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    std::env::temp_dir().join(format!("scala-rs-alias-specialization-{nonce}"))
}

#[test]
fn classpath_alias_is_not_specialized_on_its_generic_owner() {
    if !PathBuf::from(SCALA_LIBRARY).is_file() || !PathBuf::from(SCALAC).is_file() {
        eprintln!("skip: Scala 2.13 toolchain is not installed in /tmp");
        return;
    }

    let root = temp_dir();
    let lib_src = root.join("AliasLibrary.scala");
    let use_src = root.join("AliasUse.scala");
    let lib = root.join("lib");
    let out = root.join("out");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&out).unwrap();
    fs::write(
        &lib_src,
        r#"package aliaslibrary

trait GenericCompanion[A] { type ValueType = A }

class ImportedEntity
class TargetEntity

object Imported extends GenericCompanion[ImportedEntity]
object Target extends GenericCompanion[TargetEntity]
"#,
    )
    .unwrap();
    fs::write(
        &use_src,
        r#"package aliasclient

import aliaslibrary.Imported

object Main {
  val imported: Imported.ValueType = new aliaslibrary.ImportedEntity
  val target: aliaslibrary.Target.ValueType = new aliaslibrary.TargetEntity
  def acceptsTarget(value: aliaslibrary.Target.ValueType): aliaslibrary.TargetEntity = value
  val checked: aliaslibrary.TargetEntity = acceptsTarget(target)
}
"#,
    )
    .unwrap();

    let compiled_library = Command::new(SCALAC)
        .arg(&lib_src)
        .args(["-d", lib.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        compiled_library.status.success(),
        "compile library: {}",
        String::from_utf8_lossy(&compiled_library.stderr)
    );

    let classpath = format!("{}:{SCALA_LIBRARY}", lib.display());
    let compiled_use = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args(["compile", "--scala-library", SCALA_LIBRARY])
        .arg(&use_src)
        .args(["-cp", &classpath, "-d", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        compiled_use.status.success(),
        "compile consumer: {}{}",
        String::from_utf8_lossy(&compiled_use.stderr),
        String::from_utf8_lossy(&compiled_use.stdout)
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn applied_alias_preserves_a_projected_type_parameter_across_classfiles() {
    if !PathBuf::from(SCALA_LIBRARY).is_file() || !PathBuf::from(SCALAC).is_file() {
        eprintln!("skip: Scala 2.13 toolchain is not installed in /tmp");
        return;
    }

    let root = temp_dir();
    let lib_src = root.join("Library.scala");
    let use_src = root.join("Use.scala");
    let lib = root.join("lib");
    let out = root.join("out");
    let scalac_out = root.join("scalac-out");
    for dir in [&lib, &out, &scalac_out] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(
        &lib_src,
        r#"package projectedalias

trait Identity[T]
trait Entity { type Id <: Identity[?] }
case class Identified[+Id <: Identity[?], +T <: Entity](id: Id, value: T)
object Entity { type WithId[+T <: Entity] = Identified[T#Id, T] }
class ItemId extends Identity[Item]
class Item extends Entity { type Id = ItemId }
trait Repository { def store(item: Entity.WithId[Item]): Unit }
"#,
    )
    .unwrap();
    fs::write(
        &use_src,
        r#"package projectedalias

object Use {
  def save(repository: Repository, item: Identified[ItemId, Item]): Unit =
    repository.store(item)
}
"#,
    )
    .unwrap();

    let producer = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            lib_src.to_str().unwrap(),
            "--scala-library",
            SCALA_LIBRARY,
        ])
        .args(["-d", lib.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        producer.status.success(),
        "producer: {}",
        String::from_utf8_lossy(&producer.stderr)
    );

    let consumer = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            use_src.to_str().unwrap(),
            "--scala-library",
            SCALA_LIBRARY,
        ])
        .args(["-cp", lib.to_str().unwrap(), "-d", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        consumer.status.success(),
        "consumer: {}",
        String::from_utf8_lossy(&consumer.stderr)
    );

    let scala_consumer = Command::new(SCALAC)
        .args([
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            scalac_out.to_str().unwrap(),
        ])
        .arg(&use_src)
        .output()
        .unwrap();
    assert!(
        scala_consumer.status.success(),
        "Scala 2 consumer: {}",
        String::from_utf8_lossy(&scala_consumer.stderr)
    );

    let _ = fs::remove_dir_all(root);
}
