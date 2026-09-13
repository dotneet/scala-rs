//! A scala-rs-written `Aux` pickle must be consumable by real scalac.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const LIB_SOURCE: &str = r#"
package probe
trait P[M[_]] { type F[_] }
class E[A]
class EParallel extends P[E] { type F[A] = List[A] }
object P {
  type Aux[M[_], F0[_]] = P[M] { type F[x] = F0[x] }
  implicit val eParallel: P.Aux[E, List] = new EParallel
  def apply[M[_]](implicit p: P[M], d: DummyImplicit): P.Aux[M, p.F] = p
}
object Lib { def p[M[_], F0[_]]: P.Aux[M, F0] = null }
"#;

const USE_SOURCE: &str = r#"
package probe
object Use {
  type E[A] = Either[String, A]
  def take[M[_], F0[_]](x: P.Aux[M, F0]): Unit = ()
  class C extends P[E] { type F[A] = List[A] }
  val c: P.Aux[E, List] = new C
  val x: P.Aux[E, List] = Lib.p[E, List]
  take[E, List](Lib.p[E, List])
  val selected: P.Aux[probe.E, List] = P[probe.E]
}
"#;

const KINDPROJ_LIB_SOURCE: &str = r#"
package probe
trait Holder[F[_]]
object Lib { def p: Holder[Either[String, *]] = null }
"#;

const KINDPROJ_USE_SOURCE: &str = r#"
package probe
object Use {
  type E[A] = Either[String, A]
  val x: Holder[E] = Lib.p
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
fn real_scalac_reads_aux_pickle() {
    let Some(library) = cached_library() else {
        eprintln!("skip Aux scalac reader probe: scala-library jar unavailable");
        return;
    };
    let Some(scalac) = cached_scalac() else {
        eprintln!("skip Aux scalac reader probe: scalac unavailable");
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let root =
        std::env::temp_dir().join(format!("scala-rs-auxpickle-{}-{stamp}", std::process::id()));
    let lib_src = root.join("auxlib.scala");
    let use_src = root.join("auxuse.scala");
    let lib_out = root.join("lib");
    let use_out = root.join("use");
    fs::create_dir_all(&lib_out).expect("Aux writer output directory");
    fs::create_dir_all(&use_out).expect("Aux reader output directory");
    fs::write(&lib_src, LIB_SOURCE).expect("write auxlib.scala");
    fs::write(&use_src, USE_SOURCE).expect("write auxuse.scala");

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
        "scala-rs Aux writer failed: {}{}",
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
        "real scalac could not read scala-rs Aux pickle (status={}): {}{}",
        reader.status,
        String::from_utf8_lossy(&reader.stdout),
        String::from_utf8_lossy(&reader.stderr)
    );
    assert!(
        use_out.join("probe/Use.class").is_file(),
        "real scalac produced no Use.class"
    );
    let _ = fs::remove_dir_all(root);
}

/// A standalone kind-projector lambda is represented in nsc pickles as a
/// direct POLYtpe.  A refinement alias reference is not a stable ABI: real
/// scalac cannot use it after the writer's classfile has been loaded.
#[test]
fn real_scalac_reads_kindproj_polytype_pickle() {
    let Some(library) = cached_library() else {
        eprintln!("skip kind-projector scalac reader probe: scala-library jar unavailable");
        return;
    };
    let Some(scalac) = cached_scalac() else {
        eprintln!("skip kind-projector scalac reader probe: scalac unavailable");
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let root = std::env::temp_dir().join(format!(
        "scala-rs-kindproj-pickle-{}-{stamp}",
        std::process::id()
    ));
    let lib_src = root.join("kindprojlib.scala");
    let use_src = root.join("kindprojuse.scala");
    let lib_out = root.join("lib");
    let use_out = root.join("use");
    fs::create_dir_all(&lib_out).expect("kind-projector writer output directory");
    fs::create_dir_all(&use_out).expect("kind-projector reader output directory");
    fs::write(&lib_src, KINDPROJ_LIB_SOURCE).expect("write kindprojlib.scala");
    fs::write(&use_src, KINDPROJ_USE_SOURCE).expect("write kindprojuse.scala");

    let writer = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            lib_src.to_str().unwrap(),
            "-d",
            lib_out.to_str().unwrap(),
            "--scala-library",
            library.to_str().unwrap(),
            "-Ykind-projector",
        ])
        .output()
        .expect("run scala-rs kind-projector writer");
    assert!(
        writer.status.success(),
        "scala-rs kind-projector writer failed: {}{}",
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
        .expect("run real scalac kind-projector reader");
    assert!(
        reader.status.success(),
        "real scalac could not read scala-rs kind-projector POLYtpe pickle (status={}): {}{}",
        reader.status,
        String::from_utf8_lossy(&reader.stdout),
        String::from_utf8_lossy(&reader.stderr)
    );
    assert!(
        use_out.join("probe/Use.class").is_file(),
        "real scalac produced no kind-projector Use.class"
    );
    let _ = fs::remove_dir_all(root);
}
