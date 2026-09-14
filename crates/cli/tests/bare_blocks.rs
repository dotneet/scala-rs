//! Adjacent bare blocks must remain separate statements while macro calls in
//! their final expressions still expand through the JVM bridge.

use crate::support;
use std::fs;

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn compile(
    source: &Path,
    out: &Path,
    classpath: &str,
    scala_library: &Path,
) -> support::CompileOutcome {
    support::CompileCommand::new(source, out)
        .classpath(classpath)
        .scala_library(scala_library)
        .run()
}

#[test]
fn macro_calls_in_adjacent_bare_blocks_expand() {
    let toolchain = support::toolchain();
    let (Some(java), Some(scala_library), Some(scala_reflect)) = (
        toolchain.java(),
        toolchain.scala_library(),
        toolchain.scala_reflect(),
    ) else {
        eprintln!("skip adjacent bare-block macro test: macro prerequisites unavailable");
        return;
    };

    let root = support::TestDir::new("bare-blocks-macro");
    let impls = root.join("impls");
    let uses = root.join("uses");
    fs::create_dir_all(&impls).unwrap();
    fs::create_dir_all(&uses).unwrap();

    let implementation = compile(
        &fixture("eg_impl.scala"),
        &impls,
        scala_reflect.to_str().unwrap(),
        scala_library,
    );
    assert!(
        implementation.success(),
        "compile eg_impl failed: {}{}",
        String::from_utf8_lossy(implementation.stdout()),
        String::from_utf8_lossy(implementation.stderr())
    );

    let use_source = uses.join("bare_blocks.scala");
    fs::write(
        &use_source,
        r#"
import scala.language.experimental.macros

object AdjacentBlocks {
  def plus(x: Int): Int = macro EgImpl.plusImpl
}

object Main {
  def it(name: String)(body: => Unit): Unit = body

  def main(args: Array[String]): Unit = {
    it("adjacent") {
      { println(AdjacentBlocks.plus(1)); AdjacentBlocks.plus(2) }
      { println(AdjacentBlocks.plus(3)); AdjacentBlocks.plus(4) }
    }
  }
}
"#,
    )
    .unwrap();

    // The compiler accepts a path-list classpath as a single argument; keep
    // the implementation jar and scala-reflect visible to the use run.
    let cp = format!("{}:{}", impls.display(), scala_reflect.display());
    let uses_output = compile(&use_source, &uses, &cp, scala_library);
    assert!(
        uses_output.success(),
        "compile adjacent bare blocks failed: {}{}",
        String::from_utf8_lossy(uses_output.stdout()),
        String::from_utf8_lossy(uses_output.stderr())
    );

    let run = Command::new(java)
        .args([
            "-Xverify:all",
            "-cp",
            &format!(
                "{}:{}:{}",
                uses.display(),
                impls.display(),
                scala_library.display()
            ),
            "Main",
        ])
        .output()
        .expect("run adjacent bare blocks");
    assert!(
        run.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "2\n4\n");
}
