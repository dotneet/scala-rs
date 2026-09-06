//! Regression coverage for trait `super` calls that terminate at Object.
//!
//! Scala's `Any.toString` is omitted from the explicit linearization, but a
//! trait that extends another trait can still call `super.toString`. The
//! class then needs the nsc-compatible `T$$super$toString` accessor, whose
//! legal JVM target is the nearest concrete superclass (or Object itself).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn scalac() -> Option<PathBuf> {
    let scalac = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    scalac.is_file().then_some(scalac)
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("scala-rs-super-object-{tag}-{nanos}"));
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

fn run_main(cp: &str, main: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, main])
        .output()
        .expect("run generated program");
    assert!(
        output.status.success(),
        "java {main} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_rs(
    sources: &[&Path],
    out: &Path,
    class_path: Option<&str>,
    jar: Option<&Path>,
) -> std::process::Output {
    let mut command = Command::new(bin());
    command.arg("compile");
    if let Some(jar) = jar {
        command.args(["--scala-library", jar.to_str().expect("jar path")]);
    } else {
        command.arg("--no-scala-library");
    }
    if let Some(class_path) = class_path {
        command.args(["-cp", class_path]);
    }
    for source in sources {
        command.arg(source);
    }
    command.args(["-d", out.to_str().expect("output path")]);
    command.output().expect("run scala-rs compile")
}

const SOURCE_MAIN: &str = r#"
trait Parent
trait Printable extends Parent {
  override def toString: String = "Printable(" + super.toString + ")"
}
class Direct extends Printable
class Bare
class C extends Bare with Printable
object Main {
  def main(args: Array[String]): Unit = {
    println(new Direct().toString.startsWith("Printable(Direct@"))
    println(new C().toString.startsWith("Printable(C@"))
  }
}
"#;

const PROVIDER: &str = r#"
trait Parent
trait Printable extends Parent {
  override def toString: String = "Printable(" + super.toString + ")"
}
"#;

const CLIENT: &str = r#"
class Direct extends Printable
class Bare
class C extends Bare with Printable
object ClientMain {
  def main(args: Array[String]): Unit = {
    println(new Direct().toString.startsWith("Printable(Direct@"))
    println(new C().toString.startsWith("Printable(C@"))
  }
}
"#;

#[test]
fn object_super_accessor_matches_nsc_in_both_interop_directions() {
    let Some((jar, scalac)) = scala_library_jar().zip(scalac()) else {
        eprintln!("skip Object super accessor interop: Scala 2.13.16 tools unavailable");
        return;
    };
    let dir = tmp_dir("interop");
    let source_main = dir.join("Main.scala");
    let provider = dir.join("Provider.scala");
    let client = dir.join("Client.scala");
    fs::write(&source_main, SOURCE_MAIN).expect("write source regression");
    fs::write(&provider, PROVIDER).expect("write provider");
    fs::write(&client, CLIENT).expect("write client");

    let source_out = dir.join("source-out");
    fs::create_dir_all(&source_out).expect("create source output");
    let source_output = compile_rs(&[&source_main], &source_out, None, None);
    assert!(
        source_output.status.success(),
        "source Object super regression failed:\n{}{}",
        String::from_utf8_lossy(&source_output.stdout),
        String::from_utf8_lossy(&source_output.stderr)
    );
    assert_eq!(
        run_main(source_out.to_str().unwrap(), "Main"),
        "true\ntrue\n"
    );

    let ours_provider = dir.join("ours-provider");
    fs::create_dir_all(&ours_provider).expect("create provider output");
    let provider_output = compile_rs(&[&provider], &ours_provider, None, Some(&jar));
    assert!(
        provider_output.status.success(),
        "scala-rs provider failed:\n{}{}",
        String::from_utf8_lossy(&provider_output.stdout),
        String::from_utf8_lossy(&provider_output.stderr)
    );
    let nsc_client = dir.join("nsc-client");
    fs::create_dir_all(&nsc_client).expect("create nsc client output");
    let provider_cp = format!("{}:{}", ours_provider.display(), jar.display());
    let nsc_client_output = Command::new(&scalac)
        .args([
            "-cp",
            &provider_cp,
            "-d",
            nsc_client.to_str().unwrap(),
            client.to_str().unwrap(),
        ])
        .output()
        .expect("run nsc client");
    assert!(
        nsc_client_output.status.success(),
        "nsc client over scala-rs provider failed:\n{}",
        String::from_utf8_lossy(&nsc_client_output.stderr)
    );
    let nsc_forward_cp = format!(
        "{}:{}:{}",
        nsc_client.display(),
        ours_provider.display(),
        jar.display()
    );
    assert_eq!(run_main(&nsc_forward_cp, "ClientMain"), "true\ntrue\n");

    let nsc_provider = dir.join("nsc-provider");
    fs::create_dir_all(&nsc_provider).expect("create nsc provider output");
    let nsc_provider_output = Command::new(&scalac)
        .args([
            "-d",
            nsc_provider.to_str().unwrap(),
            provider.to_str().unwrap(),
        ])
        .output()
        .expect("run nsc provider");
    assert!(
        nsc_provider_output.status.success(),
        "nsc provider failed:\n{}",
        String::from_utf8_lossy(&nsc_provider_output.stderr)
    );
    let nsc_control = dir.join("nsc-control");
    fs::create_dir_all(&nsc_control).expect("create nsc control output");
    let nsc_control_cp = format!("{}:{}", nsc_provider.display(), jar.display());
    let nsc_control_output = Command::new(&scalac)
        .args([
            "-cp",
            &nsc_control_cp,
            "-d",
            nsc_control.to_str().unwrap(),
            client.to_str().unwrap(),
        ])
        .output()
        .expect("run nsc control client");
    assert!(
        nsc_control_output.status.success(),
        "nsc control client failed:\n{}",
        String::from_utf8_lossy(&nsc_control_output.stderr)
    );
    let nsc_control_run_cp = format!(
        "{}:{}:{}",
        nsc_control.display(),
        nsc_provider.display(),
        jar.display()
    );
    assert_eq!(run_main(&nsc_control_run_cp, "ClientMain"), "true\ntrue\n");

    let ours_client = dir.join("ours-client");
    fs::create_dir_all(&ours_client).expect("create scala-rs client output");
    let provider_cp = format!("{}:{}", nsc_provider.display(), jar.display());
    let ours_client_output = compile_rs(&[&client], &ours_client, Some(&provider_cp), Some(&jar));
    assert!(
        ours_client_output.status.success(),
        "scala-rs client over nsc provider failed:\n{}{}",
        String::from_utf8_lossy(&ours_client_output.stdout),
        String::from_utf8_lossy(&ours_client_output.stderr)
    );
    let ours_reverse_cp = format!(
        "{}:{}:{}",
        ours_client.display(),
        nsc_provider.display(),
        jar.display()
    );
    assert_eq!(run_main(&ours_reverse_cp, "ClientMain"), "true\ntrue\n");

    let _ = fs::remove_dir_all(dir);
}
