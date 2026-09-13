//! A ScalaSignature val accessor is represented as a Term by the eager
//! classpath scan, while the complete signature exposes the same JVM getter
//! as a zero-argument Method.  Completing the class must remove only the
//! origin-less classpath Term; source and prelude terms are not stale.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn scala_library() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let p = PathBuf::from(format!(
        "/private/tmp/scala-rs-stale-term-method-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn run_scalac(scalac: &Path, args: &[&str]) {
    let out = Command::new(scalac).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "scalac failed:\n{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}

/// `Concrete.profile` is a Scala `val`, but its getter is also a normal JVM
/// method.  The downstream expression uses the value as a receiver and then
/// calls an API method, which catches either a duplicate overload or the
/// classpath term's erased type.
#[test]
fn classpath_val_accessor_is_replaced_by_pickled_method() {
    let (Some(scala_library), Some(scalac)) = (scala_library(), scalac()) else {
        eprintln!("skip stale term/method regression: Scala 2.13.16 toolchain is absent");
        return;
    };
    let root = temp_dir();
    let lib_src = root.join("lib.scala");
    let app_src = root.join("app.scala");
    let lib_classes = root.join("lib-classes");
    let app_classes = root.join("app-classes");
    fs::create_dir_all(&lib_classes).unwrap();
    fs::create_dir_all(&app_classes).unwrap();

    fs::write(
        &lib_src,
        r#"package stale

class Api {
  def filter(value: String): String = value
}

class Profile {
  def blockingApi: Api = new Api
}

class Concrete {
  val profile: Profile = new Profile
}
"#,
    )
    .unwrap();
    fs::write(
        &app_src,
        r#"import stale.Concrete
import stale.Profile
import stale.Api

object Syntax {
  implicit class ApiOps(private val api: Api) {
    def extensionFilter(value: String): String = api.filter(value)
  }
}

object Main {
  import Syntax._

  def main(args: Array[String]): Unit = {
    val concrete = new Concrete
    val profile: Profile = concrete.profile
    val api: Api = profile.blockingApi
    println(api.extensionFilter("ok"))
  }
}
"#,
    )
    .unwrap();

    run_scalac(
        &scalac,
        &[
            "-d",
            lib_classes.to_str().unwrap(),
            lib_src.to_str().unwrap(),
        ],
    );

    let classpath = format!("{}:{}", lib_classes.display(), scala_library.display());
    let out = Command::new(bin())
        .args([
            "compile",
            app_src.to_str().unwrap(),
            "-d",
            app_classes.to_str().unwrap(),
            "-cp",
            &classpath,
            "--scala-library",
            scala_library.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "scala-rs rejected a classpath val accessor:\n{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(app_classes.join("Main.class").is_file());
    let run_cp = format!(
        "{}:{}:{}",
        app_classes.display(),
        lib_classes.display(),
        scala_library.display()
    );
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &run_cp, "Main"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "generated val-accessor client failed verification/runtime:\n{}{}",
        String::from_utf8_lossy(&run.stderr),
        String::from_utf8_lossy(&run.stdout)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "ok\n");

    let _ = fs::remove_dir_all(root);
}
