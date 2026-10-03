//! A trait's constant `final val` is a `default` method answering the
//! constant, with no mixin setter and no field in the classes mixing the
//! trait in -- nsc's shape. With an abstract accessor and setter instead, a
//! class scalac compiled against our trait died in `$init$` with
//! `AbstractMethodError` on the setter it had no reason to implement.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package l
trait T { final val N = 5; final val S = "s"; val plain: Int = 2; def get = N + plain }
class Impl extends T
"#;

const USE: &str = r#"
object M {
  def main(args: Array[String]): Unit = {
    val x = new l.T {}
    val y = new l.Impl
    println(x.N + " " + x.S + " " + x.get + " " + y.N + " " + y.get)
    println(classOf[l.Impl].getDeclaredFields.map(_.getName).sorted.mkString(","))
  }
}
"#;

#[test]
fn trait_constant_vals_are_default_methods() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("trait-constant-val");
    let lib = dir.join("T.scala");
    let use_ = dir.join("M.scala");
    fs::write(&lib, LIB).unwrap();
    fs::write(&use_, USE).unwrap();
    let names = [
        "lib-ours",
        "lib-theirs",
        "use-scalac",
        "use-ours",
        "use-theirs",
        "both",
    ];
    for n in names {
        fs::create_dir_all(dir.join(n)).unwrap();
    }
    let scalac_run = |src: &std::path::Path, cp: &std::path::Path, out: &std::path::Path| {
        let output = Command::new(scalac)
            .arg("-cp")
            .arg(cp)
            .arg("-d")
            .arg(out)
            .arg(src)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let ours = |srcs: &[&std::path::Path], cp: &str, out: &std::path::Path| {
        let mut c = CompileCommand::new(srcs[0], out);
        for s in &srcs[1..] {
            c = c.arg(s);
        }
        c.classpath(cp)
            .scala_library(library)
            .run()
            .assert_success("scala-rs compile");
    };
    let lib_ours = dir.join("lib-ours");
    let lib_theirs = dir.join("lib-theirs");
    ours(&[&lib], &library.display().to_string(), &lib_ours);
    scalac_run(&lib, library, &lib_theirs);
    scalac_run(&use_, &lib_ours, &dir.join("use-scalac"));
    scalac_run(&use_, &lib_theirs, &dir.join("use-theirs"));
    let cp = |p: &std::path::Path| format!("{}:{}", p.display(), library.display());
    ours(&[&use_], &cp(&lib_ours), &dir.join("use-ours"));
    ours(
        &[&lib, &use_],
        &library.display().to_string(),
        &dir.join("both"),
    );
    let run = |dirs: &[std::path::PathBuf]| {
        let mut cp = dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>();
        cp.push(library.display().to_string());
        let out = Command::new(java)
            .args(["-Xverify:all", "-cp"])
            .arg(cp.join(":"))
            .arg("M")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let expected = run(&[dir.join("use-theirs"), lib_theirs.clone()]);
    assert_eq!(expected, "5 s 7 5 7\nplain\n");
    assert_eq!(run(&[dir.join("use-scalac"), lib_ours.clone()]), expected);
    assert_eq!(run(&[dir.join("use-ours"), lib_ours.clone()]), expected);
    assert_eq!(run(&[dir.join("both")]), expected);
}
