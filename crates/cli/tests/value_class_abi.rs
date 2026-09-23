//! Value-class and constant ABI shared with scalac under separate compilation.
//!
//! Each case compiles a library and a client with every pairing of the two
//! compilers and requires the program to print what scalac's own build prints:
//! a scala-rs client against a scalac library, and a scalac client against a
//! scala-rs library.

use crate::support::{toolchain, TestDir};
use std::{fs, path::Path, process::Command};

fn output_text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Compile `lib` and `client` in all three pairings and compare the runs.
fn check_separate(label: &str, lib: &str, client: &str) {
    let tools = toolchain();
    let (Some(library), Some(java), Some(scalac)) =
        (tools.scala_library(), tools.java(), tools.scalac())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new(label);
    let lib_src = dir.join("Lib.scala");
    let use_src = dir.join("Use.scala");
    fs::write(&lib_src, lib).unwrap();
    fs::write(&use_src, client).unwrap();
    let out = |name: &str| {
        let p = dir.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    };
    let nsc = |src: &Path, cp: Option<&Path>, dest: &Path| {
        let mut c = Command::new(scalac);
        if let Some(cp) = cp {
            c.arg("-cp").arg(cp);
        }
        let r = c.arg("-d").arg(dest).arg(src).output().unwrap();
        assert!(
            r.status.success(),
            "scalac {}:\n{}",
            src.display(),
            output_text(&r)
        );
    };
    let ours = |src: &Path, cp: Option<&Path>, dest: &Path| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
        c.arg("compile")
            .arg(src)
            .arg("--scala-library")
            .arg(library);
        if let Some(cp) = cp {
            c.arg("-cp").arg(cp);
        }
        let r = c.arg("-d").arg(dest).output().unwrap();
        assert!(
            r.status.success(),
            "scala-rs {}:\n{}",
            src.display(),
            output_text(&r)
        );
    };
    let run = |dirs: &[&Path]| {
        let mut cp: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
        cp.push(library.display().to_string());
        let r = Command::new(java)
            .args(["-Xverify:all", "-cp", &cp.join(":"), "Main"])
            .output()
            .unwrap();
        output_text(&r)
    };
    let lib_sc = out("lib-sc");
    let lib_rs = out("lib-rs");
    nsc(&lib_src, None, &lib_sc);
    ours(&lib_src, None, &lib_rs);
    let use_sc = out("use-sc");
    nsc(&use_src, Some(&lib_sc), &use_sc);
    let expected = run(&[&use_sc, &lib_sc]);

    let use_rs = out("use-rs");
    ours(&use_src, Some(&lib_sc), &use_rs);
    assert_eq!(
        run(&[&use_rs, &lib_sc]),
        expected,
        "scala-rs client against a scalac library"
    );

    let use_sc2 = out("use-sc2");
    nsc(&use_src, Some(&lib_rs), &use_sc2);
    assert_eq!(
        run(&[&use_sc2, &lib_rs]),
        expected,
        "scalac client against a scala-rs library"
    );

    let use_rs2 = out("use-rs2");
    ours(&use_src, Some(&lib_rs), &use_rs2);
    assert_eq!(
        run(&[&use_rs2, &lib_rs]),
        expected,
        "scala-rs client against a scala-rs library"
    );
}

/// nsc erases `Wrap[Int]` for `class Wrap[A](val a: A) extends AnyVal` to
/// `java.lang.Integer`, not `int`: the underlying type is a type parameter,
/// and the class's own extension methods work on the boxed form.
#[test]
fn generic_value_class_at_a_primitive_erases_to_the_box() {
    check_separate(
        "vc-generic-primitive",
        r#"package lib
final class Wrap[A](val a: A) extends AnyVal { def get: A = a; def map[B](f: A => B): Wrap[B] = new Wrap(f(a)) }
object Api {
  def wrapped: Wrap[Int] = new Wrap(5)
  def take(w: Wrap[Int]): Int = w.get * 2
  def dbl: Wrap[Double] = new Wrap(1.5)
  def unit: Wrap[Unit] = new Wrap(())
  def str: Wrap[String] = new Wrap("s")
  def list: List[Wrap[Int]] = List(new Wrap(1), new Wrap(2))
}
"#,
        r#"import lib._
object Main {
  def main(args: Array[String]): Unit = {
    println(Api.wrapped.map(_ + 1).get)
    println(Api.take(new Wrap(21)))
    val w = Api.wrapped; println(w.get + 1)
    println(Api.dbl.get * 2); println(Api.unit.get); println(Api.str.get)
    println(Api.list.map(_.get).sum)
    val any: Any = Api.wrapped; println(any.asInstanceOf[Wrap[Int]].get)
    println(Api.take(Api.wrapped.map(x => x * 3)))
    println(classOf[Api.type].getMethods.filter(m => Set("wrapped", "take", "dbl", "unit")(m.getName)).map(_.toString).sorted.mkString("\n"))
  }
}
"#,
    );
}
