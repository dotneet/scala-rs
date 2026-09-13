//! Writer-to-reader regressions for inferred and explicitly declared module
//! singleton types, including the `Either.type` value-class shape used by
//! cats' `EitherObjectOps`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const LIB_SOURCE: &str = r#"
package modulepickle
final class EitherObjectOps(private val either: Either.type) extends AnyVal {
  def value: Either.type = either
}
object Lib {
  val Alias = Predef
  val TypedPredef: Predef.type = Predef
  val TypedEither: Either.type = Either
}
"#;

const USE_SOURCE: &str = r#"
package modulepickle
object Use {
  val c: Predef.type = Lib.Alias
  val p: Predef.type = Lib.TypedPredef
  val e: Either.type = Lib.TypedEither
  val ops = new EitherObjectOps(Either)
  val fromOps: Either.type = ops.value
}
"#;

fn cached_library() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.is_file().then_some(path)
}

fn cached_scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

#[test]
fn real_scalac_reads_inferred_and_declared_module_singletons() {
    let Some(library) = cached_library() else {
        eprintln!("skip module singleton scalac reader probe: scala-library jar unavailable");
        return;
    };
    let Some(scalac) = cached_scalac() else {
        eprintln!("skip module singleton scalac reader probe: scalac unavailable");
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let root = std::env::temp_dir().join(format!(
        "scala-rs-modulepickle-{}-{stamp}",
        std::process::id()
    ));
    let lib_src = root.join("modulelib.scala");
    let use_src = root.join("moduleuse.scala");
    let lib_out = root.join("lib");
    let use_out = root.join("use");
    fs::create_dir_all(&lib_out).expect("module writer output directory");
    fs::create_dir_all(&use_out).expect("module reader output directory");
    fs::write(&lib_src, LIB_SOURCE).expect("write modulelib.scala");
    fs::write(&use_src, USE_SOURCE).expect("write moduleuse.scala");

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
        .expect("run scala-rs module writer");
    assert!(
        writer.status.success(),
        "scala-rs module writer failed: {}{}",
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
        .expect("run real scalac module reader");
    assert!(
        reader.status.success(),
        "real scalac could not read module singleton pickle (status={}): {}{}",
        reader.status,
        String::from_utf8_lossy(&reader.stdout),
        String::from_utf8_lossy(&reader.stderr)
    );
    assert!(
        use_out.join("modulepickle/Use.class").is_file(),
        "real scalac produced no module Use.class"
    );
    let _ = fs::remove_dir_all(root);
}
