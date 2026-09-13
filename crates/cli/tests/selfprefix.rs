//! A source self alias is another spelling of `this` in a ScalaSignature.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const LIB_SOURCE: &str = r#"
package selfprefix
trait Rep { self =>
  type A
  def value: A
  def pair(other: Rep): (self.A, other.A) = (value, other.value)
}
"#;

const USE_SOURCE: &str = r#"
package selfprefix
object Use {
  type IntRep = Rep { type A = Int }
  type StringRep = Rep { type A = String }
  def pair(x: IntRep, y: StringRep): (Int, String) = x.pair(y)
}
"#;

fn cached(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    path.is_file().then_some(path)
}

#[test]
fn real_scalac_reads_self_alias_as_this_prefix() {
    let Some(library) = cached("/tmp/scala-rs-lib/scala-library-2.13.16.jar") else {
        eprintln!("skip self-prefix probe: scala-library unavailable");
        return;
    };
    let Some(scalac) = cached("/tmp/scala-2.13.16/bin/scalac") else {
        eprintln!("skip self-prefix probe: scalac unavailable");
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let root = std::env::temp_dir().join(format!(
        "scala-rs-selfprefix-{}-{stamp}",
        std::process::id()
    ));
    let lib_src = root.join("lib.scala");
    let use_src = root.join("use.scala");
    let lib_out = root.join("lib");
    let use_out = root.join("use");
    fs::create_dir_all(&lib_out).expect("self-prefix writer output");
    fs::create_dir_all(&use_out).expect("self-prefix reader output");
    fs::write(&lib_src, LIB_SOURCE).expect("write self-prefix library");
    fs::write(&use_src, USE_SOURCE).expect("write self-prefix client");

    let writer = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            lib_src.to_str().unwrap(),
            "-d",
            lib_out.to_str().unwrap(),
            "--scala-library",
            library.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs self-prefix writer");
    assert!(
        writer.status.success(),
        "scala-rs self-prefix writer failed: {}{}",
        String::from_utf8_lossy(&writer.stdout),
        String::from_utf8_lossy(&writer.stderr)
    );

    let classpath = format!("{}:{}", lib_out.display(), library.display());
    let reader = Command::new(scalac)
        .args([
            "-classpath",
            &classpath,
            "-d",
            use_out.to_str().unwrap(),
            use_src.to_str().unwrap(),
        ])
        .output()
        .expect("run real scalac self-prefix reader");
    assert!(
        reader.status.success(),
        "real scalac could not read scala-rs self-prefix pickle: {}{}",
        String::from_utf8_lossy(&reader.stdout),
        String::from_utf8_lossy(&reader.stderr)
    );
    assert!(use_out.join("selfprefix/Use.class").is_file());
    let _ = fs::remove_dir_all(root);
}
