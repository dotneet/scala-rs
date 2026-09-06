//! A macro binding forces inference of its implementation during signatures.
//! The materialized TypeCreator inside that body must nevertheless be typed
//! completely before the inferred implementation tree is cached.
//!
//! An nsc adapter invokes the provider through Java reflection to isolate its
//! JVM body from our still incomplete macro/dependent-result ScalaSignature.
//! Both consumers execute each compiler's implementation. This does not prove
//! the direct Scala macro-declaration ABI, which has separate recorded failures.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn inferred_macro_local_template_supports_cross_compilation() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-lazy-template-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let scalac = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    assert!(
        scalac.is_file() && jar.is_file(),
        "requires scalac 2.13.16 and scala-library"
    );
    let reflect = "/tmp/scala-2.13.16/lib/scala-reflect.jar";
    let provider = root.join("Provider.scala");
    let consumer = root.join("Consumer.scala");
    let facade = root.join("Bridge.scala");
    fs::write(
        &facade,
        r#"
import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context
object Bridge {
  def dispatch(c: Context): c.Expr[Int] = {
    val cls = Class.forName("Provider$")
    val module = cls.getField("MODULE$").get(null)
    cls.getMethod("impl", classOf[Context]).invoke(module, c).asInstanceOf[c.Expr[Int]]
  }
  def seven: Int = macro dispatch
}
"#,
    )
    .unwrap();
    fs::write(
        &provider,
        r#"
import scala.reflect.macros.blackbox.Context
import scala.language.experimental.macros
object Provider {
  def impl(c: Context) = { import c.universe._; c.Expr[Int](q"7") }
  def seven: Int = macro impl
}
"#,
    )
    .unwrap();
    fs::write(
        &consumer,
        "object Main { def main(args: Array[String]): Unit = println(Bridge.seven) }",
    )
    .unwrap();
    for provider_rs in [false, true] {
        let classes = root.join(if provider_rs {
            "rs-provider"
        } else {
            "nsc-provider"
        });
        fs::create_dir_all(&classes).unwrap();
        let mut cmd = if provider_rs {
            let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
            c.args(["compile", "--scala-library"]).arg(&jar);
            c
        } else {
            let mut c = Command::new(&scalac);
            c.arg("-J-Xverify:all");
            c
        };
        let cp = format!("{}:{reflect}", jar.display());
        let result = cmd
            .arg(&provider)
            .arg("-d")
            .arg(&classes)
            .args(["-cp", &cp])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "provider rs={provider_rs}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let cp = format!("{}:{}:{reflect}", classes.display(), jar.display());
        let result = Command::new(&scalac)
            .arg("-J-Xverify:all")
            .arg(&facade)
            .arg("-d")
            .arg(&classes)
            .args(["-cp", &cp])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "macro facade for provider rs={provider_rs}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        for consumer_rs in [false, true] {
            let out = root.join(format!("consumer-{provider_rs}-{consumer_rs}"));
            fs::create_dir_all(&out).unwrap();
            let mut cmd = if consumer_rs {
                let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                c.args(["compile", "--scala-library"]).arg(&jar);
                c
            } else {
                let mut c = Command::new(&scalac);
                c.arg("-J-Xverify:all");
                c
            };
            let cp = format!("{}:{}:{reflect}", classes.display(), jar.display());
            let result = cmd
                .arg(&consumer)
                .arg("-d")
                .arg(&out)
                .args(["-cp", &cp])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "provider rs={provider_rs}, consumer rs={consumer_rs}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let cp = format!("{}:{cp}", out.display());
            let result = Command::new("java")
                .args(["-Xverify:all", "-cp", &cp, "Main"])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "provider rs={provider_rs}, consumer rs={consumer_rs}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&result.stdout), "7\n");
        }
    }
    let _ = fs::remove_dir_all(root);
}
