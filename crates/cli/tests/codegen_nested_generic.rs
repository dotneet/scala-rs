//! Nested generator definitions inherit the concrete type arguments of their
//! lexical outer class when an unannotated override is inferred.
//!
//! Slick's `AbstractSourceCodeGenerator` has this shape: a generic outer
//! generator declares `Code`, while a concrete source generator defines
//! nested classes whose inferred members return `String`, `Option[String]`,
//! and other outer type parameters. The expected type comes from an abstract
//! member inherited through the nested class, not from a direct parent edge.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "support/temp_nonce.rs"]
mod temp_nonce;

fn temp_dir() -> PathBuf {
    let wall_clock_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let stamp = temp_nonce::unique_stamp(wall_clock_nanos);
    let dir = std::env::temp_dir().join(format!("scala-rs-codegen-nested-{stamp}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn nested_inferred_members_use_outer_generator_arguments() {
    let dir = temp_dir();
    let source = dir.join("NestedGenerator.scala");
    let output = dir.join("classes");
    fs::write(
        &source,
        r#"
trait Helpers[Code, TermName, TypeName] {
  def docWithCode(code: Code): Code
}

abstract class Generator[Code, TermName, TypeName]
    extends Helpers[Code, TermName, TypeName] {
  abstract class Def {
    def code: Code
    def optional: Option[Code]
    def typeName: TypeName
  }
}

abstract class StringGenerator extends Generator[String, String, String] {
  def docWithCode(code: String): String = code

  abstract class StringDef extends Def {
    def code = "hello"
    def optional = Some("hello")
    def typeName = "name"
  }
}
"#,
    )
    .expect("write source");

    let result = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            source.to_str().expect("source path"),
            "--no-scala-library",
            "-d",
            output.to_str().expect("output path"),
        ])
        .output()
        .expect("run scala-rs");
    assert!(
        result.status.success(),
        "nested generic compilation failed:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(&dir).expect("remove temp dir");
}
