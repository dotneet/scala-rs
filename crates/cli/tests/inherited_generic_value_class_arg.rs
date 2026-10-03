//! A value class passed to a member inherited from a generic parent whose
//! parameter is the parent's type parameter (`EntityList[ID].findById(id:
//! ID)`, seen from `UserList extends EntityList[UserId, User]`) goes in as
//! the box: the method takes `Object`. The member's parameter carried the
//! instantiation, so it was taken for one declared at the value class and
//! the argument was unboxed, and `findById(UserId(1))` matched nothing.
//! A parameter really declared at a generic value class keeps taking the
//! underlying value, also where the argument's own type is not known
//! (an implicit `Exp[Enc[A]]`, as circe's `Exported` is passed).

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
trait EntityId extends Any { def raw: Int }
case class UserId(value: Int) extends AnyVal with EntityId { def raw = value }
case class Code(value: String) extends AnyVal
case class User(id: UserId, name: String)
trait EntityList[ID <: EntityId, E] {
  def all: Seq[E]
  def idOf(e: E): ID
  def findById(id: ID): Option[E] = all.find(e => idOf(e) == id)
}
case class UserList(all: Seq[User]) extends EntityList[UserId, User] { def idOf(e: User) = e.id }
trait Keyed[K] { def keys: Seq[K]; def has(k: K): Boolean = keys.contains(k) }
case class Codes(keys: Seq[Code]) extends Keyed[Code]
final class Exp[T](val instance: T) extends AnyVal
trait Enc[A] { def f(a: A): String }
object Enc { implicit def imported[A](implicit e: Exp[Enc[A]]): Enc[A] = e.instance }
object Auto { implicit def exp[A]: Exp[Enc[A]] = new Exp(new Enc[A] { def f(a: A) = "auto:" + a }) }
"#;

const USE: &str = r#"
import lib._
case class Group(createdBy: UserId)
object Main {
  def main(args: Array[String]): Unit = {
    val l = UserList(Seq(User(UserId(1), "a"), User(UserId(2), "b")))
    println(l.findById(Group(UserId(2)).createdBy))
    println(l.findById(UserId(3)))
    println(Codes(Seq(Code("x"))).has(Code("x")))
    import Auto._
    println(implicitly[Enc[Int]].f(1))
  }
}
"#;

const EXPECTED: &str = "Some(User(UserId(2),b))\nNone\ntrue\nauto:1\n";

fn scalac(
    scalac: &std::path::Path,
    cp: &std::path::Path,
    out: &std::path::Path,
    src: &std::path::Path,
) {
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
fn value_class_argument_to_an_inherited_generic_member_is_boxed() {
    let tools = toolchain();
    let (Some(scalac_bin), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("inherited-generic-value-class-arg");
    let lib_src = dir.join("Lib.scala");
    let use_src = dir.join("Use.scala");
    fs::write(&lib_src, LIB).unwrap();
    fs::write(&use_src, USE).unwrap();
    let out = |name: &str| {
        let p = dir.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    };
    let (lib_ours, lib_theirs) = (out("lib-ours"), out("lib-theirs"));
    CompileCommand::new(&lib_src, &lib_ours)
        .scala_library(library)
        .run()
        .assert_success("scala-rs library");
    scalac(scalac_bin, library, &lib_theirs, &lib_src);
    for (lib, ours) in [
        (&lib_theirs, false),
        (&lib_theirs, true),
        (&lib_ours, false),
        (&lib_ours, true),
    ] {
        let client = out(&format!(
            "use-{}-{}",
            if ours { "ours" } else { "theirs" },
            lib.file_name().unwrap().to_string_lossy()
        ));
        if ours {
            CompileCommand::new(&use_src, &client)
                .classpath(lib)
                .scala_library(library)
                .run()
                .assert_success("scala-rs client");
        } else {
            scalac(scalac_bin, lib, &client, &use_src);
        }
        let cp = std::env::join_paths([client.as_path(), lib.as_path(), library]).unwrap();
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success(&format!("client {}", client.display()));
        assert_eq!(run.stdout_string(), EXPECTED, "{}", client.display());
    }
}
