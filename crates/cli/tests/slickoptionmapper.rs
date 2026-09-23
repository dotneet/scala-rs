//! Slick's `OptionMapper2` must infer the operation result before adapting it
//! to an expected `Rep[Option[T]]` result.

use crate::support::{toolchain, TestDir};
use std::{fs, path::PathBuf, process::Command};

fn slick_jars() -> Option<Vec<PathBuf>> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let roots = [
        home.join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        home.join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ];
    let wanted = [
        ("com/typesafe/slick/slick_2.13", "slick_2.13", Some("3.4.1")),
        ("com/typesafe/config", "config", None),
        ("org/slf4j/slf4j-api", "slf4j-api", None),
        (
            "org/reactivestreams/reactive-streams",
            "reactive-streams",
            None,
        ),
    ];
    wanted
        .into_iter()
        .map(|(rel, prefix, pinned)| {
            roots.iter().find_map(|root| {
                let entries = fs::read_dir(root.join(rel)).ok()?;
                entries.flatten().find_map(|entry| {
                    let version = entry.file_name().to_string_lossy().into_owned();
                    if pinned.is_some_and(|pin| pin != version) {
                        return None;
                    }
                    let jar = entry.path().join(format!("{prefix}-{version}.jar"));
                    jar.is_file().then_some(jar)
                })
            })
        })
        .collect()
}

fn compile(src: &std::path::Path, out: &std::path::Path, cp: &str, scalac: bool) -> (bool, String) {
    let Some(library) = toolchain().scala_library() else {
        return (false, "scala-library unavailable".to_string());
    };
    let mut command = if scalac {
        let Some(scalac) = toolchain().scalac() else {
            return (false, "scalac unavailable".to_string());
        };
        let mut command = Command::new(scalac);
        command.args(["-cp", cp]);
        command.arg("-d").arg(out).arg(src);
        command
    } else {
        let mut command = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
        command
            .args(["compile"])
            .arg(src)
            .arg("-d")
            .arg(out)
            .args(["-cp", cp, "--scala-library"])
            .arg(library);
        command
    };
    let output = command.output().expect("run compiler");
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}

#[test]
fn option_mapper_result_is_inferred_before_option_result_adaptation() {
    let (Some(library), Some(jars)) = (toolchain().scala_library(), slick_jars()) else {
        eprintln!("skip: scala-library or Slick 3.4.1 is unavailable");
        return;
    };
    let Some(scalac) = toolchain().scalac() else {
        eprintln!("skip: scalac is unavailable");
        return;
    };
    let _ = (library, scalac);
    let cp = jars
        .iter()
        .map(|jar| jar.display().to_string())
        .collect::<Vec<_>>()
        .join(":");
    let dir = TestDir::new("slick-optionmapper");
    let src = dir.join("OptionMapper.scala");
    fs::write(
        &src,
        r#"
import slick.jdbc.MySQLProfile.api._
object OptionMapper {
  def contains(target: Rep[String], value: String): Rep[Option[Boolean]] =
    target like s"%$value%"

  def both(target: Rep[Option[String]]): Rep[Option[Boolean]] =
    target.nonEmpty && true
}
"#,
    )
    .unwrap();
    let ours = dir.join("ours");
    fs::create_dir_all(&ours).unwrap();
    let (ok, diagnostics) = compile(&src, &ours, &cp, false);
    assert!(ok, "scala-rs failed:\n{diagnostics}");

    let oracle = dir.join("oracle");
    fs::create_dir_all(&oracle).unwrap();
    let (ok, diagnostics) = compile(&src, &oracle, &cp, true);
    assert!(ok, "scalac failed:\n{diagnostics}");
}
