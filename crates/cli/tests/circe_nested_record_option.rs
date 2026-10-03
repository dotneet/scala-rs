//! A shapeless derivation reached through queries the running derivation
//! macros make of the call site. Those queries search from their own query
//! depth, and the recursive `hconsZipWithKeys` rule for a record of two or
//! more fields needs a distinct instance at each level there as well. With
//! one instance shared by every level, the tail's `ZwkOut` had to unify with
//! `... :: ZwkOut`, `LabelledGeneric[Id]` failed, and circe fell back to
//! deriving `Option[R]` as a sealed family: `None` came out as
//! `{"None":null}` instead of `null`.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, path::PathBuf, process::Command};

const SOURCE: &str = r#"
import io.circe.generic.auto._
import io.circe.syntax._
case class Id(kind: String, n: Int)
case class Path(ids: Seq[Id])
case class R(p: Path)
case class Detail(r: Option[R] = None)
case class Cur(detail: Detail)
object Main {
  def main(args: Array[String]): Unit = {
    println(Cur(Detail()).asJson.noSpaces)
    println(Cur(Detail(Some(R(Path(Seq(Id("k", 1))))))).asJson.noSpaces)
  }
}
"#;

const EXPECTED: &str = "{\"detail\":{\"r\":null}}\n\
    {\"detail\":{\"r\":{\"p\":{\"ids\":[{\"kind\":\"k\",\"n\":1}]}}}}\n";

fn cached_jars() -> Option<Vec<PathBuf>> {
    let home = std::env::var_os("HOME")?;
    [
        "io/circe/circe-core_2.13/0.14.7/circe-core_2.13-0.14.7.jar",
        "io/circe/circe-generic_2.13/0.14.7/circe-generic_2.13-0.14.7.jar",
        "io/circe/circe-numbers_2.13/0.14.7/circe-numbers_2.13-0.14.7.jar",
        "org/typelevel/cats-core_2.13/2.11.0/cats-core_2.13-2.11.0.jar",
        "org/typelevel/cats-kernel_2.13/2.11.0/cats-kernel_2.13-2.11.0.jar",
        "com/chuusai/shapeless_2.13/2.3.13/shapeless_2.13-2.3.13.jar",
    ]
    .into_iter()
    .map(|artifact| {
        ["Library/Caches/Coursier/v1", ".cache/coursier/v1"]
            .into_iter()
            .map(|cache| {
                PathBuf::from(&home)
                    .join(cache)
                    .join("https/repo1.maven.org/maven2")
                    .join(artifact)
            })
            .find(|jar| jar.is_file())
    })
    .collect()
}

#[test]
fn nested_record_option_encodes_none_as_null() {
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
    let Some(jars) = cached_jars() else {
        eprintln!("skip: the circe jars are not cached");
        return;
    };
    let dir = TestDir::new("circe-nested-record-option");
    let source = dir.join("Main.scala");
    fs::write(&source, SOURCE).unwrap();
    let out = |name: &str| {
        let p = dir.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    };
    let (ours, theirs) = (out("ours"), out("theirs"));
    let libs = std::env::join_paths(&jars).unwrap();
    CompileOutcome::from_output(
        Command::new(scalac)
            .arg("-cp")
            .arg(&libs)
            .arg("-d")
            .arg(&theirs)
            .arg(&source)
            .output()
            .unwrap(),
    )
    .assert_success("scalac");
    let cp = std::env::join_paths(jars.iter().map(PathBuf::as_path).chain([reflect])).unwrap();
    CompileCommand::new(&source, &ours)
        .classpath(&cp)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    for classes in [&theirs, &ours] {
        let cp = std::env::join_paths(
            [classes.as_path(), library]
                .into_iter()
                .chain(jars.iter().map(PathBuf::as_path)),
        )
        .unwrap();
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success(&format!("run {}", classes.display()));
        assert_eq!(run.stdout_string(), EXPECTED, "{}", classes.display());
    }
}
