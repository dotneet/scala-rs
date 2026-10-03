//! A `DelayedInit` template's `delayedInit$body` closure class is named and
//! packaged as scalac names it: `p/C$delayedInit$body` for a class and
//! `p/O$delayedInit$body` for an object, not a flattened `p$C$...` in the
//! default package.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::path::Path;
use std::{fs, process::Command};

const SOURCE: &str = r#"
package p
trait MyApp extends DelayedInit {
  private var body: () => Unit = null
  def delayedInit(b: => Unit): Unit = body = () => b
  def main(args: Array[String]): Unit = body()
}
object O extends MyApp { println("o") }
class C extends MyApp { println("c") }
object Outer { object Inner extends MyApp { println("i") } }
object Main { def main(a: Array[String]): Unit = { O.main(a); new C().main(a); Outer.Inner.main(a) } }
"#;

fn delayed_bodies(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.to_string_lossy().contains("delayedInit$body") {
                out.push(path.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn delayed_init_body_classes_match_scalac_names() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("delayed-init-names");
    let source = dir.join("D.scala");
    fs::write(&source, SOURCE).unwrap();
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    CompileCommand::new(&source, &ours)
        .classpath(library)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let output = Command::new(scalac)
        .arg("-d")
        .arg(&theirs)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "scalac failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = delayed_bodies(&theirs);
    assert_eq!(expected.len(), 3, "scalac output: {expected:?}");
    assert_eq!(delayed_bodies(&ours), expected);

    let run = Command::new(java)
        .arg("-Xverify:all")
        .arg("-cp")
        .arg(format!("{}:{}", ours.display(), library.display()))
        .arg("p.Main")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "run failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "o\nc\ni\n");
}
