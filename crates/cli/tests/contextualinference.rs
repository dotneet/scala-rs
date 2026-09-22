//! Contextual method values, overload clauses, inferred overrides and parent varargs.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";

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
