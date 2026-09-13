//! A scala-rs-written implicit containing source `List` must remain eligible
//! when a real Scala compiler consumes the signature in a separate run.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const LIB_SOURCE: &str = r#"
package listalias

trait Base[F[_]]
trait Sub[F[_]] extends Base[F]

object Base {
  implicit def forList: Sub[List] = null
}

abstract class Need[G[_]](implicit val instance: Base[G])
"#;

const USE_SOURCE: &str = r#"
package listalias

object Use {
  val inferred = new Need[List] {}
  val selected: Base[List] = Base.forList
}
"#;

fn cached(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    path.is_file().then_some(path)
}

#[test]
fn real_scalac_finds_list_implicit_from_scala_rs_pickle() {
    let Some(library) = cached("/tmp/scala-rs-lib/scala-library-2.13.16.jar") else {
        eprintln!("skip List alias implicit probe: scala-library jar unavailable");
        return;
    };
    let Some(scalac) = cached("/tmp/scala-2.13.16/bin/scalac") else {
        eprintln!("skip List alias implicit probe: scalac unavailable");
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let root = std::env::temp_dir().join(format!(
        "scala-rs-list-alias-implicit-{}-{stamp}",
        std::process::id()
    ));
    let lib_src = root.join("lib.scala");
    let use_src = root.join("use.scala");
    let lib_out = root.join("lib");
    let use_out = root.join("use");
    fs::create_dir_all(&lib_out).expect("writer output directory");
    fs::create_dir_all(&use_out).expect("reader output directory");
    fs::write(&lib_src, LIB_SOURCE).expect("write library source");
    fs::write(&use_src, USE_SOURCE).expect("write consumer source");

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
        .expect("run scala-rs writer");
    assert!(
        writer.status.success(),
        "scala-rs writer failed: {}{}",
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
        .expect("run real scalac reader");
    assert!(
        reader.status.success(),
        "real scalac failed to infer List implicit: {}{}",
        String::from_utf8_lossy(&reader.stdout),
        String::from_utf8_lossy(&reader.stderr)
    );
    assert!(
        use_out.join("listalias/Use.class").is_file(),
        "real scalac emitted no Use.class"
    );

    let _ = fs::remove_dir_all(root);
}
