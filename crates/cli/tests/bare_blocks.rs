//! Adjacent bare blocks must remain separate statements while macro calls in
//! their final expressions still expand through the JVM bridge.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SCALA_LIBRARY: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const SCALA_REFLECT: &str = "/tmp/scala-2.13.16/lib/scala-reflect.jar";

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn temp_dir(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "scala-rs-bare-blocks-{tag}-{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile(source: &Path, out: &Path, classpath: &str) -> Output {
    Command::new(bin())
        .args([
            "compile",
            source.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .args(["-cp", classpath])
        .args(["--scala-library", SCALA_LIBRARY])
        .output()
        .expect("run scala-rs compile")
}

#[test]
fn macro_calls_in_adjacent_bare_blocks_expand() {
    let java_available = Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false);
    if !Path::new(SCALA_LIBRARY).is_file() || !Path::new(SCALA_REFLECT).is_file() || !java_available
    {
        eprintln!("skip adjacent bare-block macro test: macro prerequisites unavailable");
        return;
    }

    let root = temp_dir("macro");
    let impls = root.join("impls");
    let uses = root.join("uses");
    fs::create_dir_all(&impls).unwrap();
    fs::create_dir_all(&uses).unwrap();

    let implementation = compile(&fixture("eg_impl.scala"), &impls, SCALA_REFLECT);
    assert!(
        implementation.status.success(),
        "compile eg_impl failed: {}{}",
        String::from_utf8_lossy(&implementation.stdout),
        String::from_utf8_lossy(&implementation.stderr)
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
    let cp = format!("{}:{}", impls.display(), SCALA_REFLECT);
    let uses_output = compile(&use_source, &uses, &cp);
    assert!(
        uses_output.status.success(),
        "compile adjacent bare blocks failed: {}{}",
        String::from_utf8_lossy(&uses_output.stdout),
        String::from_utf8_lossy(&uses_output.stderr)
    );

    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}:{}", uses.display(), impls.display(), SCALA_LIBRARY),
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

    let _ = fs::remove_dir_all(root);
}
