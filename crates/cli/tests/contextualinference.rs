//! Contextual method values, overload clauses, inferred overrides and parent varargs.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";

#[test]
fn synthetic_case_apply_keeps_projected_parameter_prefixes() {
    matrix(
        &[
            ("projected_case_apply", true),
            ("projected_case_apply_bad", false),
        ],
        false,
    );
}

#[test]
fn projected_method_and_class_bounds_use_instantiated_receivers() {
    matrix(
        &[
            ("projected_bounds", true),
            ("projected_bounds_bad", false),
            ("projected_class_bounds_bad", false),
        ],
        false,
    );
}

#[test]
fn conditional_arguments_infer_types_from_both_branches() {
    matrix(&[("branch_wildcard", true)], false);
}

#[test]
fn invariant_results_refine_broad_evidence_without_accepting_bad_arguments() {
    matrix(
        &[("prototype_merge", true), ("prototype_merge_bad", false)],
        false,
    );
}

#[test]
fn binary_nominal_callbacks_infer_lambda_result_types() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("nominal-callback-{stamp}"));
    fs::create_dir(&root).unwrap();
    let source = root.join("Callbacks.java");
    fs::write(
        &source,
        r#"
public class Callbacks {
  public static <B> scala.collection.immutable.Seq<B> map(scala.Function1<String, B> f) {
    return new scala.collection.immutable.$colon$colon<B>(f.apply("abc"), scala.collection.immutable.List$.MODULE$.<B>empty());
  }
  public static <B> B combine(scala.Function2<String, String, B> f) {
    return f.apply("ab", "cde");
  }
}
"#,
    )
    .unwrap();
    let result = Command::new("javac")
        .args(["-cp", JAR, "-d"])
        .arg(&root)
        .arg(source)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let cp = format!("{JAR}:{}", root.display());
    matrix_cp(
        &[("nominal_callback", true), ("nominal_callback_bad", false)],
        false,
        &cp,
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_static_companions_keep_implicit_helper_paths() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let mut cp = format!(
        "{JAR}:/tmp/scala-2.13.16/lib/scala-reflect.jar:/tmp/scala-2.13.16/lib/scala-compiler.jar"
    );
    for artifact in [
        "com/github/pureconfig/pureconfig-core_2.13/0.17.10/pureconfig-core_2.13-0.17.10.jar",
        "com/github/pureconfig/pureconfig-generic_2.13/0.17.10/pureconfig-generic_2.13-0.17.10.jar",
        "com/github/pureconfig/pureconfig-generic-base_2.13/0.17.10/pureconfig-generic-base_2.13-0.17.10.jar",
        "com/chuusai/shapeless_2.13/2.3.13/shapeless_2.13-2.3.13.jar",
        "com/typesafe/config/1.4.5/config-1.4.5.jar",
    ] {
        let Some(jar) = ["Library/Caches/Coursier/v1", ".cache/coursier/v1"]
            .into_iter()
            .map(|cache| {
                PathBuf::from(&home)
                    .join(cache)
                    .join("https/repo1.maven.org/maven2")
                    .join(artifact)
            })
            .find(|path| path.is_file())
        else {
            eprintln!("skip: dependency is not cached: {artifact}");
            return;
        };
        cp.push(':');
        cp.push_str(jar.to_str().unwrap());
    }
    matrix_cp(&[("nested_static_helpers", true)], false, &cp);
}

#[test]
fn inherited_binary_companion_overloads_keep_explicit_type_arguments() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("nested-companion-overloads-{stamp}"));
    fs::create_dir(&root).unwrap();
    let producer = root.join("Producer.scala");
    let consumer = root.join("Consumer.scala");
    fs::write(
        &producer,
        r#"
trait Protocol {
  trait Claim
  trait Show[T <: Claim]
  case class Endpoint[T <: Claim](n: Int)(implicit ev: Show[T]) {
    def apply(value: T): Int = n
  }
  object Endpoint {
    def apply[T <: Claim](s: String)(implicit ev: Show[T]): Endpoint[T] =
      Endpoint[T](s.length)
  }
}
"#,
    )
    .unwrap();
    fs::write(
        &consumer,
        r#"
trait Instances extends Protocol {
  case class Payload() extends Claim
  case class OtherPayload() extends Claim
  implicit val payloadShow: Show[Payload] = new Show[Payload] {}
  implicit val otherPayloadShow: Show[OtherPayload] = new Show[OtherPayload] {}
}
class Consumer extends Instances {
  def run(s: String): Int = Endpoint[Payload](s).apply(Payload())
}
object Main {
  def main(args: Array[String]): Unit = println(new Consumer().run("abc"))
}
"#,
    )
    .unwrap();
    for nsc in [true, false] {
        let compiler = if nsc {
            "/tmp/scala-2.13.16/bin/scalac"
        } else {
            env!("CARGO_BIN_EXE_scala-rs")
        };
        let out = root.join(if nsc { "nsc" } else { "native" });
        fs::create_dir(&out).unwrap();
        for source in [&producer, &consumer] {
            let mut command = Command::new(compiler);
            if !nsc {
                command.args(["compile", "--scala-library", JAR]);
            }
            let output = command
                .args(["-cp", &format!("{JAR}:{}", out.display()), "-d"])
                .arg(&out)
                .arg(source)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "nsc={nsc}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let output = Command::new("java")
            .args([
                "-Xverify:all",
                "-cp",
                &format!("{JAR}:{}", out.display()),
                "Main",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "nsc={nsc}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"3\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_derived_evidence_does_not_claim_an_implicit_view() {
    let Some(jars) = cached_circe() else {
        eprintln!("skip: Circe derivation jars are not cached");
        return;
    };
    let cp = format!(
        "{JAR}:{}",
        jars.iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join(":")
    );
    matrix_cp(&[("failed_derived_view", true)], false, &cp);
}

#[test]
fn three_field_configured_derivation_materializes_nested_evidence() {
    let Some(jars) = cached_circe() else {
        eprintln!("skip: Circe derivation jars are not cached");
        return;
    };
    let cp = format!(
        "{JAR}:{}",
        jars.iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join(":")
    );
    matrix_cp(&[("configured_three_fields", true)], false, &cp);
}

#[test]
fn explicit_macro_abort_in_view_evidence_remains_an_error() {
    let Some(jars) = cached_circe() else {
        eprintln!("skip: Scala macro jars are not cached");
        return;
    };
    let reflect = jars
        .iter()
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n == "scala-reflect-2.13.16.jar")
        })
        .unwrap();
    let compiler = jars
        .iter()
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n == "scala-compiler-2.13.16.jar")
        })
        .unwrap();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("view-macro-abort-{stamp}"));
    fs::create_dir(&root).unwrap();
    let producer = root.join("Producer.scala");
    let consumer = root.join("Consumer.scala");
    fs::write(
        &producer,
        r#"
import scala.language.experimental.macros
import scala.reflect.macros.blackbox
trait Show[A]
object Show { implicit def derived[A]: Show[A] = macro Macros.noShow[A] }
object Macros {
  def noShow[A: c.WeakTypeTag](c: blackbox.Context): c.Expr[Show[A]] =
    c.abort(c.enclosingPosition, "no instance")
}
class Box
object Box {
  implicit def boxApply(x: Box): ((Int, Int) => Int) => Int = f => f(2, 3)
}
trait Low {
  implicit def response[A: Show](a: A): String => String = identity
}
"#,
    )
    .unwrap();
    fs::write(
        &consumer,
        r#"
import scala.language.implicitConversions
object Main extends Low {
  val box = new Box
  val result = box { (x: Int, y: Int) => x + y }
}
"#,
    )
    .unwrap();
    let cp = format!("{JAR}:{}:{}", reflect.display(), compiler.display());
    let built = root.join("producer");
    fs::create_dir(&built).unwrap();
    let p = Command::new("/tmp/scala-2.13.16/bin/scalac")
        .args(["-cp", &cp, "-d"])
        .arg(&built)
        .arg(&producer)
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let cp = format!("{cp}:{}", built.display());
    for nsc in [true, false] {
        let output = root.join(if nsc { "nsc" } else { "native" });
        fs::create_dir(&output).unwrap();
        let mut cmd = Command::new(if nsc {
            "/tmp/scala-2.13.16/bin/scalac"
        } else {
            env!("CARGO_BIN_EXE_scala-rs")
        });
        if !nsc {
            cmd.args(["compile", "--scala-library", JAR]);
        }
        let p = cmd
            .args(["-cp", &cp, "-d"])
            .arg(&output)
            .arg(&consumer)
            .output()
            .unwrap();
        assert!(!p.status.success(), "nsc={nsc}");
        assert!(
            String::from_utf8_lossy(&p.stderr).contains("no instance"),
            "nsc={nsc}: {}",
            String::from_utf8_lossy(&p.stderr)
        );
    }
    fs::remove_dir_all(root).unwrap();
}

fn cached_cats() -> Option<Vec<PathBuf>> {
    let home = std::env::var_os("HOME")?;
    let mut jars = Vec::new();
    for artifact in ["cats-core", "cats-kernel"] {
        let relative = format!(
            "https/repo1.maven.org/maven2/org/typelevel/{artifact}_2.13/2.11.0/{artifact}_2.13-2.11.0.jar"
        );
        let jar = ["Library/Caches/Coursier/v1", ".cache/coursier/v1"]
            .into_iter()
            .map(|cache| PathBuf::from(&home).join(cache).join(&relative))
            .find(|path| path.is_file())?;
        jars.push(jar);
    }
    Some(jars)
}

fn cached_circe() -> Option<Vec<PathBuf>> {
    let home = std::env::var_os("HOME")?;
    let artifacts = [
        "com/chuusai/shapeless_2.13/2.3.13/shapeless_2.13-2.3.13.jar",
        "io/circe/circe-core_2.13/0.14.7/circe-core_2.13-0.14.7.jar",
        "io/circe/circe-generic_2.13/0.14.7/circe-generic_2.13-0.14.7.jar",
        "io/circe/circe-generic-extras_2.13/0.14.3/circe-generic-extras_2.13-0.14.3.jar",
        "io/circe/circe-numbers_2.13/0.14.7/circe-numbers_2.13-0.14.7.jar",
        "org/typelevel/cats-core_2.13/2.11.0/cats-core_2.13-2.11.0.jar",
        "org/typelevel/cats-kernel_2.13/2.11.0/cats-kernel_2.13-2.11.0.jar",
        "org/scala-lang/scala-reflect/2.13.16/scala-reflect-2.13.16.jar",
        "org/scala-lang/scala-compiler/2.13.16/scala-compiler-2.13.16.jar",
    ];
    artifacts
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
                .find(|path| path.is_file())
        })
        .collect()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("ctxinfer_{name}.scala"))
}
fn matrix(names: &[(&str, bool)], warm: bool) {
    matrix_cp(names, warm, JAR);
}
fn matrix_cp(names: &[(&str, bool)], warm: bool, cp: &str) {
    matrix_ordered(names, warm.then_some("warm"), cp);
}
fn matrix_ordered(names: &[(&str, bool)], warm: Option<&str>, cp: &str) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "contextual-inference-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    for &(name, accepted) in names {
        let mut oracle = None;
        for nsc in [true, false] {
            for order in 0..if warm.is_some() { 3 } else { 1 } {
                let out = root.join(format!("{name}-{nsc}-{order}"));
                fs::create_dir(&out).unwrap();
                let mut c = Command::new(if nsc {
                    "/tmp/scala-2.13.16/bin/scalac"
                } else {
                    env!("CARGO_BIN_EXE_scala-rs")
                });
                if !nsc {
                    c.args(["compile", "--scala-library", JAR]);
                }
                c.args(["-cp", cp, "-d"]).arg(&out);
                if order == 1 {
                    c.arg(fixture(warm.unwrap()));
                }
                c.arg(fixture(name));
                if order == 2 {
                    c.arg(fixture(warm.unwrap()));
                }
                let p = c.output().unwrap();
                assert_eq!(
                    p.status.success(),
                    accepted,
                    "{name} nsc={nsc} order={order}: {}",
                    String::from_utf8_lossy(&p.stderr)
                );
                if accepted {
                    let p = Command::new("java")
                        .args([
                            "-Xverify:all",
                            "-cp",
                            &format!("{}:{cp}", out.display()),
                            "Main",
                        ])
                        .output()
                        .unwrap();
                    assert!(
                        p.status.success(),
                        "{name} nsc={nsc} order={order}: {}",
                        String::from_utf8_lossy(&p.stderr)
                    );
                    let expected = fs::read(
                        fixture(name)
                            .parent()
                            .unwrap()
                            .join("expected")
                            .join(format!("ctxinfer_{name}.txt")),
                    )
                    .unwrap();
                    assert_eq!(p.stdout, expected, "{name} nsc={nsc} order={order}");
                    if let Some(ref oracle) = oracle {
                        assert_eq!(&p.stdout, oracle);
                    } else {
                        oracle = Some(p.stdout);
                    }
                } else {
                    assert!(String::from_utf8_lossy(&p.stderr).contains("error"));
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_invariant_branch_joins_keep_common_parent_bounds() {
    matrix(&[("nested_join", true), ("nested_join_bad", false)], false);
}

#[test]
fn partially_applied_factories_keep_fixed_lambda_result_arguments() {
    let Some(jars) = cached_cats() else {
        eprintln!("skip: cats 2.11.0 is not cached");
        return;
    };
    let cp = format!("{JAR}:{}:{}", jars[0].display(), jars[1].display());
    matrix_cp(
        &[("partial_factory", true), ("partial_factory_bad", false)],
        false,
        &cp,
    );
}

#[test]
fn unapplied_higher_kinded_evidence_supports_eithert_parallel() {
    let Some(jars) = cached_cats() else {
        eprintln!("skip: cats 2.11.0 is not cached");
        return;
    };
    let cp = format!("{JAR}:{}:{}", jars[0].display(), jars[1].display());
    matrix_cp(&[("parallel_eithert", true)], false, &cp);
}

#[test]
fn singleton_lambda_results_infer_their_underlying_type_constructor() {
    matrix(
        &[
            ("singleton_constructor", true),
            ("singleton_constructor_bad", false),
        ],
        false,
    );
}

#[test]
fn open_receiver_lower_bounds_do_not_fix_lambda_result_variables() {
    matrix(
        &[
            ("receiver_lower_bound", true),
            ("receiver_lower_bound_bad", false),
        ],
        false,
    );
}

#[test]
fn fixed_formal_arguments_reach_nested_factories_without_a_result_prototype() {
    matrix(
        &[("fixed_prototype", true), ("fixed_prototype_bad", false)],
        false,
    );
}

#[test]
fn inferred_override_scope() {
    matrix(
        &[
            ("inferred_override", true),
            ("narrowed_override", true),
            ("override_overload", true),
            ("override_wrong_bad", false),
            ("override_ambiguous_bad", false),
        ],
        false,
    );
}

#[test]
fn implicit_method_values() {
    matrix(
        &[
            ("curried_eta", true),
            ("implicit_eta", true),
            ("eta_generic", true),
            ("eta_missing_bad", false),
            ("eta_wrong_input_bad", false),
            ("eta_bound_bad", false),
            ("eta_capture", true),
        ],
        false,
    );
}

#[test]
fn residual_overload_clauses() {
    matrix(
        &[
            ("implicit_overload", true),
            ("overload_reverse", true),
            ("overload_explicit_bad", false),
            ("overload_generic", true),
        ],
        false,
    );
}

#[test]
fn inherited_generic_overloaded_method_values() {
    matrix(
        &[
            ("overload_eta_generic", true),
            ("overload_eta_bound_bad", false),
        ],
        false,
    );
}

#[test]
fn nested_overloaded_receivers_keep_the_outer_type_arguments() {
    matrix(
        &[("chain_nested", true), ("chain_nested_bad", false)],
        false,
    );
}

#[test]
fn parent_repeated_arguments() {
    matrix(
        &[
            ("parent_repeated", true),
            ("parent_spread", true),
            ("parent_class", true),
            ("parent_wrong_bad", false),
            ("parent_tail_wrong_bad", false),
            ("parent_self_bad", false),
            ("parent_nested_self_bad", false),
            ("parent_function_self_bad", false),
            ("parent_self_byname", true),
            ("parent_self_byname_bad", false),
            ("parent_nulls", true),
        ],
        false,
    );
}

#[test]
fn binary_declaration_variance() {
    matrix(
        &[
            ("stream_empty", true),
            ("stream_widen", true),
            ("stream_narrow_bad", false),
        ],
        false,
    );
}

#[path = "support/temp_nonce.rs"]
mod temp_nonce;

#[test]
fn java_parent_uses_arrays() {
    let root = std::env::temp_dir().join(format!(
        "contextual-inference-java-{}",
        temp_nonce::unique_stamp(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    ));
    fs::create_dir(&root).unwrap();
    let source = fixture("java_parent")
        .parent()
        .unwrap()
        .join("ctxinfer_JavaParent.java");
    let p = Command::new("javac")
        .arg("-d")
        .arg(&root)
        .arg(source)
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    matrix_cp(
        &[("java_parent", true), ("java_parent_bad", false)],
        false,
        &format!("{}:{JAR}", root.display()),
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn covariant_results_minimize_materialized_tags() {
    matrix_cp(
        &[("tag_minimize", true), ("tag_abstract_bad", false)],
        false,
        &format!("{JAR}:/tmp/scala-2.13.16/lib/scala-reflect.jar"),
    );
}

#[test]
fn inferred_join_retains_independent_common_traits() {
    matrix(&[("intersection_join", true)], false);
}

#[test]
fn by_name_nothing_does_not_fix_fold_result_type() {
    matrix(
        &[("fold_nothing", true), ("fold_nothing_bad", false)],
        false,
    );
}

fn existential_alias_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/contextualinference")
        .join(format!("existential_alias_scope_{name}.scala"))
}

fn compile_existential_fixture(
    scalac: bool,
    source: &Path,
    out: &Path,
    cp: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(if scalac {
        "/tmp/scala-2.13.16/bin/scalac"
    } else {
        env!("CARGO_BIN_EXE_scala-rs")
    });
    if !scalac {
        command.args(["compile", "--scala-library", JAR]);
    }
    if let Some(cp) = cp {
        command.args(["-cp", cp]);
    }
    command.args(["-d"]).arg(out).arg(source);
    command.output().unwrap()
}

#[test]
fn existential_alias_scope_acceptance_matches_both_compilers() {
    let stamp = temp_nonce::unique_stamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    );
    let root = std::env::temp_dir().join(format!("existential-alias-scalac-{stamp}"));
    fs::create_dir(&root).unwrap();

    for compiler_scalac in [true, false] {
        for (name, accepted) in [
            ("good", true),
            ("explicit_bad", false),
            ("explicit_concrete_bad", false),
            ("correlation_bad", false),
            ("bound_bad", false),
        ] {
            let out = root.join(format!("{name}-{compiler_scalac}"));
            fs::create_dir(&out).unwrap();
            let output = compile_existential_fixture(
                compiler_scalac,
                &existential_alias_fixture(name),
                &out,
                Some(JAR),
            );
            assert_eq!(
                output.status.success(),
                accepted,
                "{name} compiler_scalac={compiler_scalac}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn existential_alias_signature_roundtrip_across_compilers() {
    let stamp = temp_nonce::unique_stamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    );
    let root = std::env::temp_dir().join(format!("existential-alias-roundtrip-{stamp}"));
    fs::create_dir(&root).unwrap();

    for producer_scalac in [true, false] {
        for consumer_scalac in [true, false] {
            let tag = format!("producer-{producer_scalac}-consumer-{consumer_scalac}");
            let producer_out = root.join(format!("{tag}-producer"));
            let consumer_out = root.join(format!("{tag}-consumer"));
            fs::create_dir(&producer_out).unwrap();
            fs::create_dir(&consumer_out).unwrap();

            let producer = compile_existential_fixture(
                producer_scalac,
                &existential_alias_fixture("producer"),
                &producer_out,
                Some(JAR),
            );
            assert!(
                producer.status.success(),
                "{tag}: producer compile failed: {}{}",
                String::from_utf8_lossy(&producer.stdout),
                String::from_utf8_lossy(&producer.stderr)
            );

            let cp = format!("{}:{JAR}", producer_out.display());
            let consumer = compile_existential_fixture(
                consumer_scalac,
                &existential_alias_fixture("consumer"),
                &consumer_out,
                Some(&cp),
            );
            assert!(
                consumer.status.success(),
                "{tag}: consumer compile failed: {}{}",
                String::from_utf8_lossy(&consumer.stdout),
                String::from_utf8_lossy(&consumer.stderr)
            );

            let runtime_cp = format!(
                "{}:{}:{JAR}",
                consumer_out.display(),
                producer_out.display()
            );
            let run = Command::new("java")
                .args([
                    "-Xverify:all",
                    "-cp",
                    &runtime_cp,
                    "existential_alias_scope.Main",
                ])
                .output()
                .unwrap();
            assert!(
                run.status.success(),
                "{tag}: runtime failed: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            assert_eq!(run.stdout, b"6\n", "{tag}: output differs");
        }
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn unit_adaptation_matches_scalac_and_native() {
    matrix(
        &[("unit_adaptation", true), ("unit_adaptation_bad", false)],
        false,
    );
}

#[test]
fn range_flatmap_uses_its_polymorphic_element_type() {
    matrix(
        &[("range_flatmap", true), ("range_flatmap_bad", false)],
        false,
    );
}

#[test]
fn contravariant_inputs_and_covariant_output_join_from_expected_type() {
    matrix(
        &[
            ("join_lub", true),
            ("join_lub_bad_reverse", false),
            ("join_lub_bad_inputs", false),
        ],
        false,
    );
}

#[test]
fn dependent_evidence_out_types_are_fitted_before_implicit_selection() {
    matrix(
        &[
            ("dependent_evidence", true),
            ("dependent_evidence_bad", false),
        ],
        false,
    );
}
