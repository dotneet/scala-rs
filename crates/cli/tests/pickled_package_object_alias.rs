//! An alias of a library's package object (`cats.Id`) in a signature is
//! pickled with the object's stable path as its prefix,
//! `<root>.cats.package.type#Id`, as nsc writes it. `ThisType(cats.package)`
//! read back as `cats.package.Id`, and Airframe, which tells bindings apart
//! by the names a type carries, found no binding for a constructor parameter
//! of `Box[cats.Id]` made with `bind[Box[Id]]`.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, path::PathBuf, process::Command};

const HOLDER: &str = r#"
trait Box[F[_]]
class Holder(val b: Box[cats.Id])
"#;

const READER: &str = r#"
import scala.reflect.runtime.universe._
object Reader {
  def main(args: Array[String]): Unit = {
    val ctor = typeOf[Holder].typeSymbol.asClass.primaryConstructor.asMethod
    ctor.paramLists.flatten.foreach(p => println(p.typeSignature + " " + showRaw(p.typeSignature)))
  }
}
"#;

fn cats_core() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    ["Library/Caches/Coursier/v1", ".cache/coursier/v1"]
        .into_iter()
        .map(|cache| {
            PathBuf::from(&home)
                .join(cache)
                .join("https/repo1.maven.org/maven2")
                .join("org/typelevel/cats-core_2.13/2.11.0/cats-core_2.13-2.11.0.jar")
        })
        .find(|jar| jar.is_file())
}

#[test]
fn package_object_alias_is_pickled_through_the_object() {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(reflect), Some(_)) = (
        tools.scalac(),
        tools.scala_library(),
        tools.scala_reflect(),
        tools.java(),
    ) else {
        eprintln!("skip: scalac, scala-library, scala-reflect or Java is unavailable");
        return;
    };
    let Some(cats) = cats_core() else {
        eprintln!("skip: cats-core is not cached");
        return;
    };
    let dir = TestDir::new("pickled-package-object-alias");
    let holder = dir.join("Holder.scala");
    let reader = dir.join("Reader.scala");
    fs::write(&holder, HOLDER).unwrap();
    fs::write(&reader, READER).unwrap();
    let out = |name: &str| {
        let p = dir.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    };
    let (ours, theirs, reader_out) = (out("ours"), out("theirs"), out("reader"));
    let scalac_run = |cp: &std::ffi::OsStr, dest: &std::path::Path, src: &std::path::Path| {
        CompileOutcome::from_output(
            Command::new(scalac)
                .arg("-cp")
                .arg(cp)
                .arg("-d")
                .arg(dest)
                .arg(src)
                .output()
                .unwrap(),
        )
        .assert_success("scalac");
    };
    scalac_run(cats.as_os_str(), &theirs, &holder);
    CompileCommand::new(&holder, &ours)
        .classpath(&cats)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let reader_cp = std::env::join_paths([theirs.as_path(), cats.as_path()]).unwrap();
    scalac_run(&reader_cp, &reader_out, &reader);
    let mut outputs = Vec::new();
    for classes in [&theirs, &ours] {
        let cp = std::env::join_paths([
            reader_out.as_path(),
            classes.as_path(),
            cats.as_path(),
            library,
            reflect,
        ])
        .unwrap();
        let run = RunCommand::new("Reader").classpath(cp).run();
        run.assert_success(&format!("run {}", classes.display()));
        outputs.push(run.stdout_string());
    }
    assert!(outputs[0].starts_with("Box[cats.Id] "), "{}", outputs[0]);
    assert_eq!(outputs[1], outputs[0]);
}
