//! Named calls must see written overloads on a binary nested companion.

use crate::support::{toolchain, TestDir};
use std::{fs, process::Command};

#[test]
fn binary_nested_case_companion_loads_written_apply_names() {
    let Some(library) = toolchain().scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    let dir = TestDir::new("nested-companion-named-apply");
    let producer = dir.join("Producer.scala");
    let consumer = dir.join("Consumer.scala");
    let classes = dir.join("classes");
    let result = dir.join("result");
    fs::create_dir(&classes).unwrap();
    fs::create_dir(&result).unwrap();
    fs::write(
        &producer,
        r#"package example
object Outer {
  final case class Entry(data: String)
  object Entry {
    def apply(count: Int, label: String): Entry = new Entry(s"$count:$label")
  }
}
"#,
    )
    .unwrap();
    fs::write(
        &consumer,
        r#"package example
object Consumer {
  val written = Outer.Entry(label = "item", count = 2)
  val generated = Outer.Entry(data = "other")
}
"#,
    )
    .unwrap();

    let compiler = env!("CARGO_BIN_EXE_scala-rs");
    let produced = Command::new(compiler)
        .args(["compile"])
        .arg(&producer)
        .arg("-d")
        .arg(&classes)
        .arg("--scala-library")
        .arg(library)
        .output()
        .unwrap();
    assert!(
        produced.status.success(),
        "producer failed:\n{}{}",
        String::from_utf8_lossy(&produced.stdout),
        String::from_utf8_lossy(&produced.stderr)
    );

    let cp = std::env::join_paths([classes.as_path(), library]).unwrap();
    let consumed = Command::new(compiler)
        .args(["compile"])
        .arg(&consumer)
        .arg("-cp")
        .arg(cp)
        .arg("--scala-library")
        .arg(library)
        .arg("-d")
        .arg(&result)
        .output()
        .unwrap();
    assert!(
        consumed.status.success(),
        "consumer failed:\n{}{}",
        String::from_utf8_lossy(&consumed.stdout),
        String::from_utf8_lossy(&consumed.stderr)
    );
}
