//! A type tag for a class that is a member of an object with a companion
//! trait, read from the class path after another class file named it
//! (`InnerClasses` gives `X` as the outer class of a member of the object
//! `X` too, and the class-file read put a twin of the class in the trait).
//! The tag must select the class from the object: from the trait the mirror
//! found nothing (`ScalaReflectionException: type ChapterId in p.Cond not
//! found`), and the type printed as the projection `p.Cond#ChapterId`.
//! A class only an expanded alias names (`Code` in `Check`) is not yet a
//! member of the object when the tag is built; its pickle still says it is
//! declared there.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, path::Path, process::Command};

const LIB: &str = r#"
package p
sealed trait Cond
object Cond {
  case class ChapterId(value: Int) extends AnyVal
  case class Chapter(code: String, id: ChapterId) extends Cond
  case class Code(value: String) extends AnyVal
}
"#;

const SVC: &str = r#"
package q
import p.Cond._
object Svc {
  def check(id: ChapterId, ids: Seq[ChapterId]): Boolean = ids.contains(id)
  case class Holder(id: ChapterId, c: Chapter)
  type Check[F[_]] = (ChapterId, Code) => F[Boolean]
}
"#;

const USE: &str = r#"
import scala.reflect.runtime.universe._
import q.Svc._
object Main {
  def tag[A](implicit t: TypeTag[A]): String = t.tpe.toString
  def main(args: Array[String]): Unit = {
    println(check(p.Cond.ChapterId(1), Seq(p.Cond.ChapterId(1))))
    println(tag[Holder => p.Cond.ChapterId])
    println(tag[(p.Cond.ChapterId, Int)])
    println(typeTag[Check[Option]].tpe.dealias)
  }
}
"#;

fn scalac(scalac: &Path, cp: &str, out: &Path, src: &Path) {
    CompileOutcome::from_output(
        Command::new(scalac)
            .arg("-cp")
            .arg(cp)
            .arg("-d")
            .arg(out)
            .arg(src)
            .output()
            .unwrap(),
    )
    .assert_success("scalac");
}

#[test]
fn type_tag_selects_an_object_member_from_the_object() {
    let tools = toolchain();
    let (Some(scalac_bin), Some(library), Some(reflect), Some(_)) = (
        tools.scalac(),
        tools.scala_library(),
        tools.scala_reflect(),
        tools.java(),
    ) else {
        eprintln!("skip: scalac, scala-library, scala-reflect or Java is unavailable");
        return;
    };
    let dir = TestDir::new("reified-object-member-owner");
    let srcs: Vec<_> = [("Lib.scala", LIB), ("Svc.scala", SVC), ("Use.scala", USE)]
        .iter()
        .map(|(n, t)| {
            let p = dir.join(n);
            fs::write(&p, t).unwrap();
            p
        })
        .collect();
    let lr = format!("{}:{}", library.display(), reflect.display());
    let mut outputs = Vec::new();
    for ours in [false, true] {
        let tag = if ours { "ours" } else { "theirs" };
        let outs: Vec<_> = ["lib", "svc", "use"]
            .iter()
            .map(|n| {
                let p = dir.join(format!("{n}-{tag}"));
                fs::create_dir_all(&p).unwrap();
                p
            })
            .collect();
        for i in 0..3 {
            let cp = std::iter::once(lr.clone())
                .chain(outs[..i].iter().map(|o| o.display().to_string()))
                .collect::<Vec<_>>()
                .join(":");
            if ours {
                CompileCommand::new(&srcs[i], &outs[i])
                    .classpath(&cp)
                    .scala_library(library)
                    .run()
                    .assert_success("scala-rs compile");
            } else {
                scalac(scalac_bin, &cp, &outs[i], &srcs[i]);
            }
        }
        let cp = outs
            .iter()
            .map(|o| o.display().to_string())
            .chain(std::iter::once(lr.clone()))
            .collect::<Vec<_>>()
            .join(":");
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success(&format!("run {tag}"));
        outputs.push(run.stdout_string());
    }
    assert_eq!(
        outputs[0],
        "true\nq.Svc.Holder => p.Cond.ChapterId\n(p.Cond.ChapterId, Int)\n\
         (p.Cond.ChapterId, p.Cond.Code) => Option[Boolean]\n"
    );
    assert_eq!(outputs[1], outputs[0]);
}
