//! A concrete method mixed in from a trait implements an interface method whose
//! erased signature is narrower: `Helper[T <: Tok].search(t: T)` erases to
//! `search(Tok)`, and a class mixing `Helper[Admin]` into `UseCase` owes
//! `UseCase.search(Admin)` a bridge to it. Only the class's own methods were
//! considered, so the interface method stayed abstract (`AbstractMethodError`).
//! A same-named method of an unrelated trait must not be taken for one:
//! `Enc.apply(A)` is not `Dec.apply(StringBuilder)`, although `A` erases to
//! `Object`.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
class Tok(val n: Int); class Admin(n: Int) extends Tok(n)
trait UseCase { def search(t: Admin, q: String): String; def count(t: Admin): Int }
trait Helper[T <: Tok] { def search(t: T, q: String): String = q + t.n; def count(t: T): Int = t.n * 2 }
final case class Impl(x: Int) extends UseCase with Helper[Admin]
trait Enc[A] { def apply(a: A): String }
trait Dec[A] { def apply(c: StringBuilder): Either[String, A] }
trait TokEnc[T <: Tok] extends Enc[T] { final override def apply(a: T): String = "enc" + a.n }
trait TokDec[T <: Tok] extends Dec[T] { final override def apply(c: StringBuilder): Either[String, T] = Left("dec" + c) }
trait TokCodec[T <: Tok] extends TokEnc[T] with TokDec[T]
class Own extends UseCase with Helper[Admin] { override def count(t: Admin): Int = -t.n }
object Main {
  def main(args: Array[String]): Unit = {
    val a = new Admin(5)
    val us: List[UseCase] = List(Impl(1), new Own)
    println(us.map(u => u.search(a, "q") + " " + u.count(a)))
    val codec = new TokCodec[Admin] {}
    val e: Enc[Admin] = codec
    val d: Dec[Admin] = codec
    println((e(a), d(new StringBuilder("x"))))
  }
}
"#;

#[test]
fn mixed_in_method_bridges_a_narrower_interface_method() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("mixin-interface-bridge");
    let source = dir.join("Main.scala");
    fs::write(&source, SOURCE).unwrap();
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    CompileCommand::new(&source, &ours)
        .classpath(library)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let output = Command::new(scalac)
        .arg("-d")
        .arg(&theirs)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = |classes: &std::path::Path| {
        let out = Command::new(java)
            .args(["-Xverify:all", "-cp"])
            .arg(format!("{}:{}", classes.display(), library.display()))
            .arg("Main")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let expected = run(&theirs);
    assert_eq!(expected, "List(q5 10, q5 -5)\n(enc5,Left(decx))\n");
    assert_eq!(run(&ours), expected);
}
