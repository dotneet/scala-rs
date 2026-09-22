//! Writer-to-reader regressions for inferred and explicitly declared module
//! singleton types, including the `Either.type` value-class shape used by
//! cats' `EitherObjectOps`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const LIB_SOURCE: &str = r#"
package modulepickle
final class EitherObjectOps(private val either: Either.type) extends AnyVal {
  def value: Either.type = either
}
object Lib {
  val Alias = Predef
  val TypedPredef: Predef.type = Predef
  val TypedEither: Either.type = Either
}
"#;

const USE_SOURCE: &str = r#"
package modulepickle
object Use {
  val c: Predef.type = Lib.Alias
  val p: Predef.type = Lib.TypedPredef
  val e: Either.type = Lib.TypedEither
  val ops = new EitherObjectOps(Either)
  val fromOps: Either.type = ops.value
}
"#;

fn cached_library() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.is_file().then_some(path)
}

fn cached_scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

#[test]
fn real_scalac_reads_inferred_and_declared_module_singletons() {
    let Some(library) = cached_library() else {
        eprintln!("skip module singleton scalac reader probe: scala-library jar unavailable");
        return;
    };
    let Some(scalac) = cached_scalac() else {
        eprintln!("skip module singleton scalac reader probe: scalac unavailable");
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let root = std::env::temp_dir().join(format!(
        "scala-rs-modulepickle-{}-{stamp}",
        std::process::id()
    ));
    let lib_src = root.join("modulelib.scala");
    let use_src = root.join("moduleuse.scala");
    let lib_out = root.join("lib");
    let use_out = root.join("use");
    fs::create_dir_all(&lib_out).expect("module writer output directory");
    fs::create_dir_all(&use_out).expect("module reader output directory");
    fs::write(&lib_src, LIB_SOURCE).expect("write modulelib.scala");
    fs::write(&use_src, USE_SOURCE).expect("write moduleuse.scala");

    let writer = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            lib_src.to_str().unwrap(),
            "-d",
            lib_out.to_str().unwrap(),
            "--scala-library",
            library.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs module writer");
    assert!(
        writer.status.success(),
        "scala-rs module writer failed: {}{}",
        String::from_utf8_lossy(&writer.stdout),
        String::from_utf8_lossy(&writer.stderr)
    );

    let classpath = format!("{}:{}", lib_out.display(), library.display());
    let reader = Command::new(scalac)
        .args([
            "-classpath",
            &classpath,
            "-d",
            use_out.to_str().unwrap(),
            use_src.to_str().unwrap(),
        ])
        .output()
        .expect("run real scalac module reader");
    assert!(
        reader.status.success(),
        "real scalac could not read module singleton pickle (status={}): {}{}",
        reader.status,
        String::from_utf8_lossy(&reader.stdout),
        String::from_utf8_lossy(&reader.stderr)
    );
    assert!(
        use_out.join("modulepickle/Use.class").is_file(),
        "real scalac produced no module Use.class"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn native_directory_forwarder_preserves_inherited_higher_kinded_arguments() {
    let Some(library) = cached_library() else {
        return;
    };
    let Some(scalac) = cached_scalac() else {
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("scala-rs-module-hk-{}-{stamp}", std::process::id()));
    let provider = root.join("provider");
    fs::create_dir_all(&provider).unwrap();
    let lib = root.join("library.scala");
    fs::write(&lib, r#"
package mappinglib
trait Mapping[F[_],G[_]] { def apply[A](value:F[A]):G[A] }
object Mapping { def id[F[_]]:Mapping[F,F] = new Mapping[F,F] { def apply[A](value:F[A]):F[A] = value } }
trait Definition[F[_]] { val id:Mapping[F,F] = Mapping.id[F] }
object Lists extends Definition[List]
object Tables extends Tables
trait Tables { case class Row(id:Int, label:String, group:Int=0, count:Int) }
"#).unwrap();
    let native = env!("CARGO_BIN_EXE_scala-rs");
    let result = Command::new(native)
        .arg("compile")
        .arg(&lib)
        .arg("--scala-library")
        .arg(&library)
        .arg("-d")
        .arg(&provider)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let cp = format!("{}:{}", provider.display(), library.display());
    for accepted in [true, false] {
        let source = root.join(format!("client-{accepted}.scala"));
        fs::write(
            &source,
            if accepted {
                r#"case class Result(label: String, count: Int)
object Main {
 val local = Result(label = "ok", count = 42)
 val row = mappinglib.Tables.Row(0, label=local.label, count=local.count, group=1)
 val id: mappinglib.Mapping[List,List] = mappinglib.Lists.id
 def main(args:Array[String]):Unit = println(id(List(row.count)).head)
}"#
            } else {
                "object Main { val wrong: mappinglib.Mapping[Option,Option] = mappinglib.Lists.id }"
            },
        )
        .unwrap();
        for ours in [false, true] {
            let output = root.join(format!("client-{accepted}-{ours}"));
            fs::create_dir_all(&output).unwrap();
            let mut command = Command::new(if ours {
                std::path::Path::new(native)
            } else {
                scalac.as_path()
            });
            if ours {
                command.arg("compile").arg("--scala-library").arg(&library);
            }
            let result = command
                .arg(&source)
                .arg("-cp")
                .arg(&cp)
                .arg("-d")
                .arg(&output)
                .output()
                .unwrap();
            assert_eq!(
                result.status.success(),
                accepted,
                "ours={ours}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            if accepted {
                let result = Command::new("java")
                    .arg("-Xverify:all")
                    .arg("-cp")
                    .arg(format!("{}:{cp}", output.display()))
                    .arg("Main")
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
            }
        }
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn imported_package_aliases_survive_native_module_boundaries() {
    let Some(library) = cached_library() else {
        return;
    };
    let Some(scalac) = cached_scalac() else {
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "scala-rs-package-alias-{}-{stamp}",
        std::process::id()
    ));
    let dependency = root.join("dependency");
    let provider = root.join("provider");
    fs::create_dir_all(&dependency).unwrap();
    fs::create_dir_all(&provider).unwrap();
    let lib = root.join("library.scala");
    fs::write(&lib, r#"
package aliaslib {
 trait Mapping[F[_],G[_]] { def apply[A](value:F[A]):G[A] }
 object Mapping { def id[F[_]]:Mapping[F,F] = new Mapping[F,F] { def apply[A](value:F[A]):F[A] = value } }
 trait Aliases { type Box[A] = List[A] }
 object Handler { val value = 42 }
 object Levels {
  final case class Level(number:Int) extends AnyVal
  val Info:Level = Level(42)
 }
}
package object aliaslib extends aliaslib.Aliases {
 type ~>[F[_],G[_]] = Mapping[F,G]
 type Handler = Int => String
}
"#).unwrap();
    let result = Command::new(&scalac)
        .arg(&lib)
        .arg("-cp")
        .arg(&library)
        .arg("-d")
        .arg(&dependency)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let source = root.join("provider.scala");
    fs::write(
        &source,
        r#"
package provider
import aliaslib.{Handler => Callback}
trait Endpoint { def handler:Callback }
object LevelsSource { def read(value:aliaslib.Levels.Level):Int = value.number }
import aliaslib.{~>, Box, Mapping}
object Transforms {
 trait Definition[F[_]] { val id:F ~> F = Mapping.id[F] }
 object Lists extends Definition[List]
 val boxed:Box[Int] = List(42)
}
"#,
    )
    .unwrap();
    let native = env!("CARGO_BIN_EXE_scala-rs");
    let cp = format!("{}:{}", dependency.display(), library.display());
    let result = Command::new(native)
        .arg("compile")
        .arg(&source)
        .arg("--scala-library")
        .arg(&library)
        .arg("-cp")
        .arg(&cp)
        .arg("-d")
        .arg(&provider)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let cp = format!("{}:{cp}", provider.display());
    for (case, accepted, rejected) in [
        ("valid", true, ""),
        ("constructor", false, "object Main { val wrong:aliaslib.Mapping[Option,Option] = provider.Transforms.Lists.id }"),
        ("alias", false, "object Main { val wrong:aliaslib.Handler = aliaslib.Handler }"),
        ("value-class", false, "object Main { val wrong = provider.LevelsSource.read(42) }"),
    ] {
        let source = root.join(format!("client-{case}.scala"));
        fs::write(&source, if accepted { r#"
object Main {
 def handler(endpoint:provider.Endpoint):Int => String = endpoint.handler
 val callback:aliaslib.Handler = _.toString
 val endpoint = new provider.Endpoint { def handler:aliaslib.Handler = callback }
 val id:aliaslib.Mapping[List,List] = provider.Transforms.Lists.id
 val boxed:List[Int] = provider.Transforms.boxed
 def main(args:Array[String]):Unit = {
  assert(provider.LevelsSource.read(aliaslib.Levels.Info) == boxed.head)
  println(handler(endpoint)(id(List(boxed.head)).head))
 }
}"# } else { rejected }).unwrap();
        for ours in [false, true] {
            let output = root.join(format!("client-{case}-{ours}"));
            fs::create_dir_all(&output).unwrap();
            let mut command = Command::new(if ours { std::path::Path::new(native) } else { scalac.as_path() });
            if ours { command.arg("compile").arg("--scala-library").arg(&library); }
            let result = command.arg(&source).arg("-cp").arg(&cp).arg("-d").arg(&output).output().unwrap();
            assert_eq!(result.status.success(), accepted, "ours={ours}: {}", String::from_utf8_lossy(&result.stderr));
            if accepted {
                let result = Command::new("java").arg("-Xverify:all").arg("-cp")
                    .arg(format!("{}:{cp}", output.display())).arg("Main").output().unwrap();
                assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
                assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
            }
        }
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn package_object_java_return_survives_separate_compilation() {
    let Some(library) = cached_library() else {
        return;
    };
    let Some(scalac) = cached_scalac() else {
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "scala-rs-package-object-java-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let producer_src = root.join("producer.scala");
    let consumer_src = root.join("consumer.scala");
    fs::write(
        &producer_src,
        r#"
package clocklib {
  package object api {
    def current = new java.sql.Timestamp(123L)
  }
}
"#,
    )
    .unwrap();
    fs::write(
        &consumer_src,
        r#"
import clocklib.api._

object Main {
  def main(args: Array[String]): Unit = {
    val now: java.sql.Timestamp = current
    println(now.getTime)
  }
}
"#,
    )
    .unwrap();

    let compile = |ours: bool, src: &PathBuf, out: &PathBuf, cp: Option<&str>| {
        let native = env!("CARGO_BIN_EXE_scala-rs");
        let mut command = Command::new(if ours {
            native
        } else {
            scalac.to_str().unwrap()
        });
        if ours {
            command.arg("compile");
        }
        if let Some(cp) = cp {
            command.arg("-cp").arg(cp);
        }
        command.arg(src).arg("-d").arg(out);
        if ours {
            command.arg("--scala-library").arg(&library);
        }
        command.output().unwrap()
    };

    for producer_ours in [false, true] {
        let producer_out = root.join(format!("producer-{producer_ours}"));
        fs::create_dir_all(&producer_out).unwrap();
        let producer = compile(producer_ours, &producer_src, &producer_out, None);
        assert!(
            producer.status.success(),
            "producer={producer_ours}: {}{}",
            String::from_utf8_lossy(&producer.stdout),
            String::from_utf8_lossy(&producer.stderr)
        );

        let original = fs::read_to_string(&consumer_src).unwrap();
        for import_style in ["_", "{current}"] {
            fs::write(
                &consumer_src,
                original.replace("api._", &format!("api.{import_style}")),
            )
            .unwrap();
            for consumer_ours in [false, true] {
                let consumer_out = root.join(format!(
                    "consumer-{producer_ours}-{consumer_ours}-{import_style}"
                ));
                fs::create_dir_all(&consumer_out).unwrap();
                let cp = format!("{}:{}", producer_out.display(), library.display());
                let consumer = compile(consumer_ours, &consumer_src, &consumer_out, Some(&cp));
                assert!(
                    consumer.status.success(),
                    "producer={producer_ours}, consumer={consumer_ours}: {}{}",
                    String::from_utf8_lossy(&consumer.stdout),
                    String::from_utf8_lossy(&consumer.stderr)
                );
                let output = Command::new("java")
                    .args([
                        "-Xverify:all",
                        "-cp",
                        &format!(
                            "{}:{}:{}",
                            consumer_out.display(),
                            producer_out.display(),
                            library.display()
                        ),
                        "Main",
                    ])
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "producer={producer_ours}, consumer={consumer_ours}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(String::from_utf8_lossy(&output.stdout), "123\n");
            }
        }
        fs::write(&consumer_src, original).unwrap();
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn nested_companions_keep_outer_type_arguments() {
    let Some(library) = cached_library() else {
        return;
    };
    let Some(scalac) = cached_scalac() else {
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("nested-companion-{stamp}"));
    fs::create_dir_all(&root).unwrap();
    let lib = root.join("Library.scala");
    let use_src = root.join("Use.scala");
    let bad_src = root.join("Bad.scala");
    fs::write(
        &lib,
        r#"
package nestedcompanion
trait Evidence[A] { def label: String }
case class Config(value: String)
trait Base[A] {
  protected type Alias = A
  case class Entry(value: String)(implicit val evidence: Evidence[Alias]) {
    def render: String = value + ":" + evidence.label
  }
  object Entry {
    def apply(config: Config)(implicit evidence: Evidence[Alias]): Entry = new Entry(config.value)
  }
}
"#,
    )
    .unwrap();
    fs::write(
        &use_src,
        r#"
import nestedcompanion._
trait Derived[X] extends Base[X] {
  def build(value: String)(implicit evidence: Evidence[X]): Entry = Entry(value)(evidence)
  def configured(value: Config)(implicit evidence: Evidence[X]): Entry = Entry(value)
}
object Main {
  def main(args: Array[String]): Unit = {
    implicit val evidence: Evidence[Int] = new Evidence[Int] { def label: String = "int" }
    val builder = new Derived[Int] {}
    println(builder.build("v").render)
    println(builder.configured(Config("c")).render)
  }
}
"#,
    )
    .unwrap();
    fs::write(
        &bad_src,
        r#"
import nestedcompanion._
trait Wrong extends Base[String] {
  def bad(evidence: Evidence[Int]): Entry = Entry("bad")(evidence)
}
"#,
    )
    .unwrap();
    let compile = |ours: bool, sources: &[&PathBuf], out: &PathBuf, cp: &str| {
        fs::create_dir_all(out).unwrap();
        let mut command = Command::new(if ours {
            PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
        } else {
            scalac.clone()
        });
        if ours {
            command.args(["compile", "--scala-library"]).arg(&library);
        }
        command
            .args(["-cp", cp, "-d"])
            .arg(out)
            .args(sources)
            .output()
            .unwrap()
    };
    let run = |out: &PathBuf, cp: &str| {
        let output = Command::new("java")
            .args([
                "-Xverify:all",
                "-cp",
                &format!("{}:{cp}", out.display()),
                "Main",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"v:int\nc:int\n");
    };
    let library_cp = library.to_str().unwrap();
    for ours in [false, true] {
        let out = root.join(format!("together-{ours}"));
        let result = compile(ours, &[&lib, &use_src], &out, library_cp);
        assert!(
            result.status.success(),
            "together ours={ours}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        run(&out, library_cp);
        let result = compile(
            ours,
            &[&lib, &bad_src],
            &root.join(format!("bad-together-{ours}")),
            library_cp,
        );
        assert!(
            !result.status.success(),
            "accepted mismatched evidence ours={ours}"
        );
    }
    for producer_ours in [false, true] {
        let producer_out = root.join(format!("producer-{producer_ours}"));
        let result = compile(producer_ours, &[&lib], &producer_out, library_cp);
        assert!(
            result.status.success(),
            "producer ours={producer_ours}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let cp = format!("{}:{library_cp}", producer_out.display());
        for consumer_ours in [false, true] {
            let out = root.join(format!("consumer-{producer_ours}-{consumer_ours}"));
            let result = compile(consumer_ours, &[&use_src], &out, &cp);
            assert!(
                result.status.success(),
                "producer={producer_ours}, consumer={consumer_ours}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            run(&out, &cp);
            let result = compile(
                consumer_ours,
                &[&bad_src],
                &root.join(format!("bad-{producer_ours}-{consumer_ours}")),
                &cp,
            );
            assert!(
                !result.status.success(),
                "accepted mismatched binary evidence"
            );
        }
    }
    let _ = fs::remove_dir_all(root);
}
