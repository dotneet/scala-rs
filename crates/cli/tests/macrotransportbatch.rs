//! Macro AST transport and separately compiled macro declaration access.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const REFLECT: &str = "/tmp/scala-2.13.16/lib/scala-reflect.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";

#[test]
fn dynamic_macro_types_parse_and_preserve_constant_refinements() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    let implementation = root.join("implementation");
    compile("macroparse_impl", true, &implementation, &base, true);
    let cp = format!("{}:{base}", implementation.display());
    for nsc in [true, false] {
        let out = root.join(format!("use-{nsc}"));
        compile("macroparse_use", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"1\n42\n");
        compile(
            "macroparse_bad",
            nsc,
            &root.join(format!("bad-{nsc}")),
            &cp,
            false,
        );
        compile(
            "macroparse_syntax_bad",
            nsc,
            &root.join(format!("syntax-bad-{nsc}")),
            &cp,
            false,
        );
    }
    fs::remove_dir_all(root).unwrap();
}
fn compile(name: &str, nsc: bool, out: &Path, cp: &str, accepted: bool) {
    fs::create_dir(out).unwrap();
    let mut cmd = Command::new(if nsc {
        NSC
    } else {
        env!("CARGO_BIN_EXE_scala-rs")
    });
    if !nsc {
        cmd.args(["compile", "--scala-library", JAR]);
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("{name}.scala"));
    let r = cmd
        .arg(source)
        .args(["-cp", cp, "-d"])
        .arg(out)
        .output()
        .unwrap();
    assert_eq!(
        r.status.success(),
        accepted,
        "{name} nsc={nsc}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    if name == "macrotransport_owner_use" {
        let stdout = String::from_utf8_lossy(&r.stdout);
        let stderr = String::from_utf8_lossy(&r.stderr);
        for owner in ["field", "local", "definitions"] {
            assert!(
                stdout.contains(&format!("owner 日本語 {owner}")),
                "{stdout}"
            );
            assert!(
                stderr.contains(&format!("owner stderr 日本語 {owner}")),
                "{stderr}"
            );
        }
    }
    if !accepted {
        let error = String::from_utf8_lossy(&r.stderr);
        assert!(
            if name == "macrotransport_bad" {
                error.contains("cannot be accessed")
            } else if name == "macrotransport_owner_bad" {
                error.contains("recursive value field needs type")
                    && error.contains("recursive value local needs type")
            } else if name == "macromethod_info_bad" {
                error.contains("recursive method hidden needs result type")
            } else if name == "macrotransport_fields_bad" {
                error.matches("integer storage is forbidden").count() == 4
            } else if name == "refined_bundle_context_bad" {
                if nsc {
                    error.contains("macro bundles must be concrete monomorphic classes")
                } else {
                    error.contains("macro implementation reference has wrong shape")
                }
            } else if name == "macroassociated_bad" || name == "macrooutput_fit_bad" {
                error.contains("could not find implicit value") || error.contains("type mismatch")
            } else if name == "macrotransport_bundle_bad" {
                error.contains("could not find implicit value") || error.contains("type mismatch")
            } else if name == "refined_bundle_bad" {
                error.contains("bundle constructor rejected")
            } else if name == "macroparse_syntax_bad" {
                error.contains("ParseException")
            } else {
                error.contains("type mismatch") || error.contains("no matching overload")
            },
            "wrong rejection: {error}"
        );
    }
}
fn run(out: &Path, cp: &str) -> Vec<u8> {
    let r = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            "Main",
        ])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    r.stdout
}
fn root() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "macrotransport-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}
#[test]
fn bidirectional_macro_transport_and_access() {
    let root = root();
    let implementation = root.join("implementation");
    let base_cp = format!("{JAR}:{REFLECT}");
    compile("macrotransport_impl", true, &implementation, &base_cp, true);
    let impl_cp = format!("{}:{base_cp}", implementation.display());
    let expected = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/expected/macrotransport_use.txt"),
    )
    .unwrap();
    let mut reference = None;
    for producer in [true, false] {
        let api = root.join(format!("api-{producer}"));
        compile("macrotransport_api", producer, &api, &impl_cp, true);
        let cp = format!("{}:{impl_cp}", api.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_use", consumer, &output, &cp, true);
            let stdout = run(&output, &cp);
            assert_eq!(stdout, expected);
            let nsc_stdout = reference.get_or_insert_with(|| stdout.clone());
            assert_eq!(&stdout, nsc_stdout);
            for bad in ["macrotransport_bad", "macrotransport_arg_bad"] {
                compile(
                    bad,
                    consumer,
                    &root.join(format!("{bad}-{producer}-{consumer}")),
                    &cp,
                    false,
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn formerly_refused_shapes_execute_once_and_match_scalac() {
    let root = root();
    let expected = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/expected/macrotransport_legacy.txt"),
    )
    .unwrap();
    for producer in [true, false] {
        let base_cp = format!("{JAR}:{REFLECT}");
        let eg = root.join(format!("eg-{producer}"));
        let ex = root.join(format!("ex-{producer}"));
        compile("eg_impl", producer, &eg, &base_cp, true);
        compile("ex_impl", producer, &ex, &base_cp, true);
        let cp = format!("{}:{}:{base_cp}", eg.display(), ex.display());
        // API roundtrips are checked above. Explicit dependent signatures of
        // scala-rs-produced implementation methods are not nsc-readable yet;
        // here compare executable transport using each supported producer.
        for consumer in [true, false].into_iter().filter(|&nsc| producer || !nsc) {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_legacy", consumer, &output, &cp, true);
            assert_eq!(run(&output, &cp), expected);
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inferred_implementation_materializes_qualified_context_tag() {
    let root = root();
    for producer in [true, false] {
        let cp = format!("{JAR}:{REFLECT}");
        let implementation = root.join(format!("implementation-{producer}"));
        compile(
            "macrotransport_inferred",
            producer,
            &implementation,
            &cp,
            true,
        );
        let cp = format!("{}:{cp}", implementation.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_inferred_use", consumer, &output, &cp, true);
            assert_eq!(run(&output, &cp), b"42\n");
            compile(
                "macrotransport_inferred_bad",
                consumer,
                &root.join(format!("bad-{producer}-{consumer}")),
                &cp,
                false,
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_implicit_macro_materialization_tries_lower_priority_candidates() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    let producer = root.join("producer");
    compile(
        "implicit_materialization_retry",
        true,
        &producer,
        &base,
        true,
    );
    let cp = format!("{}:{base}", producer.display());
    for nsc in [true, false] {
        let out = root.join(format!("use-{nsc}"));
        compile("implicit_materialization_retry_use", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"42\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn separate_expansions_keep_distinct_local_class_identities() {
    let root = root();
    for producer in [true, false] {
        let cp = format!("{JAR}:{REFLECT}");
        let implementation = root.join(format!("implementation-{producer}"));
        compile(
            "macrotransport_class_identity",
            producer,
            &implementation,
            &cp,
            true,
        );
        let cp = format!("{}:{cp}", implementation.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile(
                "macrotransport_class_identity_use",
                consumer,
                &output,
                &cp,
                true,
            );
            assert_eq!(run(&output, &cp), b"true\ntrue\n");
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reflected_storage_fields_drive_macro_rejection() {
    let root = root();
    for producer in [true, false] {
        let cp = format!("{JAR}:{REFLECT}");
        let implementation = root.join(format!("implementation-{producer}"));
        compile(
            "macrotransport_fields",
            producer,
            &implementation,
            &cp,
            true,
        );
        let cp = format!("{}:{cp}", implementation.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_fields_use", consumer, &output, &cp, true);
            assert_eq!(run(&output, &cp), b"true\ntrue\ntrue\ntrue\nok\nok\n");
            compile(
                "macrotransport_fields_bad",
                consumer,
                &root.join(format!("bad-{producer}-{consumer}")),
                &cp,
                false,
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_ownership_and_macro_console_match_scalac() {
    let root = root();
    let implementation = root.join("implementation");
    let cp = format!("{JAR}:{REFLECT}");
    compile(
        "macrotransport_owner_impl",
        true,
        &implementation,
        &cp,
        true,
    );
    let cp = format!("{}:{cp}", implementation.display());
    let expected = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/expected/macrotransport_owner_use.txt"),
    )
    .unwrap();
    for producer in [true, false] {
        let api = root.join(format!("api-{producer}"));
        compile("macrotransport_owner_api", producer, &api, &cp, true);
        let cp = format!("{}:{cp}", api.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("macrotransport_owner_use", consumer, &output, &cp, true);
            assert_eq!(run(&output, &cp), expected);
            compile(
                "macrotransport_owner_bad",
                consumer,
                &root.join(format!("bad-{producer}-{consumer}")),
                &cp,
                false,
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn macro_bundle_metadata_and_expansion_interoperate_with_scalac() {
    let root = root();
    let base_cp = format!("{JAR}:{REFLECT}");
    let expected = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/expected/refined_bundle_use.txt"),
    )
    .unwrap();
    for producer in [true, false] {
        let api = root.join(format!("bundle-{producer}"));
        compile("refined_bundle_impl", producer, &api, &base_cp, true);
        let cp = format!("{}:{base_cp}", api.display());
        for consumer in [true, false] {
            let output = root.join(format!("use-{producer}-{consumer}"));
            compile("refined_bundle_use", consumer, &output, &cp, true);
            assert_eq!(run(&output, &cp), expected);
            compile(
                "refined_bundle_bad",
                consumer,
                &root.join(format!("bad-{producer}-{consumer}")),
                &cp,
                false,
            );
        }
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn macro_bundle_rejects_unrelated_context_refinements() {
    let root = root();
    for ours in [true, false] {
        compile(
            "refined_bundle_context_bad",
            ours,
            &root.join(format!("context-{ours}")),
            &format!("{JAR}:{REFLECT}"),
            false,
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inferred_macro_arguments_and_prefix_with_runtime_tag() {
    let root = root();
    let implementation = root.join("implementation");
    let base_cp = format!("{JAR}:{REFLECT}");
    compile(
        "macrotransport_prefix_tag",
        true,
        &implementation,
        &base_cp,
        true,
    );
    let cp = format!("{}:{base_cp}", implementation.display());
    for consumer in [true, false] {
        let output = root.join(format!("use-{consumer}"));
        compile(
            "macrotransport_prefix_tag_use",
            consumer,
            &output,
            &cp,
            true,
        );
        assert_eq!(run(&output, &cp), b"Payload\nString\ndone\n");
        compile(
            "macrotransport_prefix_tag_bad",
            consumer,
            &root.join(format!("bad-{consumer}")),
            &cp,
            false,
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_nested_declarations_aliases_and_access_boundaries_match_scalac() {
    let root = root();
    let implementation = root.join("implementation");
    let base_cp = format!("{JAR}:{REFLECT}");
    compile(
        "macrotransport_mirror_features",
        true,
        &implementation,
        &base_cp,
        true,
    );
    let cp = format!("{}:{base_cp}", implementation.display());
    for nsc in [true, false] {
        let output = root.join(format!("use-{nsc}"));
        compile(
            "macrotransport_mirror_features_use",
            nsc,
            &output,
            &cp,
            true,
        );
        assert_eq!(
            run(&output, &cp),
            b"true:String:true:Outer:mirrorfixture:true\n"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn binary_macro_bundle_preserves_literal_type_arguments() {
    let root = root();
    let implementation = root.join("implementation");
    let base_cp = format!("{JAR}:{REFLECT}");
    compile(
        "macrotransport_bundle",
        true,
        &implementation,
        &base_cp,
        true,
    );
    let cp = format!("{}:{base_cp}", implementation.display());
    for nsc in [true, false] {
        let output = root.join(format!("use-{nsc}"));
        compile("macrotransport_bundle_use", nsc, &output, &cp, true);
        assert_eq!(run(&output, &cp), b"100\nready\n");
        compile(
            "macrotransport_bundle_bad",
            nsc,
            &root.join(format!("bad-{nsc}")),
            &cp,
            false,
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn natural_number_bundle_matches_scalac_at_runtime() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let jar = ["Library/Caches/Coursier/v1", ".cache/coursier/v1"].into_iter()
        .map(|root| PathBuf::from(&home).join(root).join("https/repo1.maven.org/maven2/com/chuusai/shapeless_2.13/2.3.10/shapeless_2.13-2.3.10.jar"))
        .find(|jar| jar.is_file());
    let Some(jar) = jar else {
        eprintln!("skip: shapeless 2.3.10 is not cached");
        return;
    };
    let root = root();
    // The published bundle has signatures referencing nsc internals. The jar
    // is a link dependency of the macro, not a source-compilation fallback.
    let compiler = "/tmp/scala-2.13.16/lib/scala-compiler.jar";
    if !Path::new(compiler).is_file() {
        return;
    }
    let cp = format!("{JAR}:{REFLECT}:{compiler}:{}", jar.display());
    for nsc in [true, false] {
        let out = root.join(format!("out-{nsc}"));
        compile("macrotransport_natural", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"0\n1\n2\n10\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn binary_parent_loading_preserves_higher_kinded_parameters() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let mut jars = Vec::new();
    for artifact in [
        "io/circe/circe-core_2.13/0.14.7/circe-core_2.13-0.14.7.jar",
        "io/circe/circe-numbers_2.13/0.14.7/circe-numbers_2.13-0.14.7.jar",
        "org/typelevel/cats-core_2.13/2.11.0/cats-core_2.13-2.11.0.jar",
        "org/typelevel/cats-kernel_2.13/2.11.0/cats-kernel_2.13-2.11.0.jar",
    ] {
        let jar = ["Library/Caches/Coursier/v1", ".cache/coursier/v1"]
            .into_iter()
            .map(|cache| {
                PathBuf::from(&home)
                    .join(cache)
                    .join("https/repo1.maven.org/maven2")
                    .join(artifact)
            })
            .find(|jar| jar.is_file());
        let Some(jar) = jar else {
            eprintln!("skip: {artifact} is not cached");
            return;
        };
        jars.push(jar.to_string_lossy().into_owned());
    }
    let root = root();
    let cp = format!("{JAR}:{}", jars.join(":"));
    for nsc in [true, false] {
        let out = root.join(format!("out-{nsc}"));
        compile("macrotransport_binary_kinds", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"\"ok\"\nList(42)\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_binary_class_type_trees_round_trip_through_macros() {
    let root = root();
    let implementation = root.join("implementation");
    let base_cp = format!("{JAR}:{REFLECT}");
    compile(
        "macrotransport_nested_binary",
        true,
        &implementation,
        &base_cp,
        true,
    );
    let cp = format!("{}:{base_cp}", implementation.display());
    for nsc in [true, false] {
        let out = root.join(format!("out-{nsc}"));
        compile("macrotransport_nested_binary_use", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"true\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn parameterless_macro_receiver_infers_from_selected_member() {
    let root = root();
    let base_cp = format!("{JAR}:{REFLECT}");
    let implementation = root.join("receiver-implementation");
    compile(
        "macrotransport_receiver",
        true,
        &implementation,
        &base_cp,
        true,
    );
    let cp = format!("{}:{base_cp}", implementation.display());
    for consumer in [true, false] {
        let output = root.join(format!("receiver-use-{consumer}"));
        compile("macrotransport_receiver_use", consumer, &output, &cp, true);
        assert_eq!(run(&output, &cp), b"String\nString\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn generic_selection_probe_expands_each_macro_receiver_once() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    let implementation = root.join("implementation");
    compile("macrochain_probe", true, &implementation, &base, true);
    let cp = format!("{}:{base}", implementation.display());
    for nsc in [true, false] {
        let out = root.join(format!("use-{nsc}"));
        fs::create_dir(&out).unwrap();
        let mut cmd = Command::new(if nsc {
            NSC
        } else {
            env!("CARGO_BIN_EXE_scala-rs")
        });
        if !nsc {
            cmd.args(["compile", "--scala-library", JAR]);
        }
        let result = cmd
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/macrochain_probe_use.scala"),
            )
            .args(["-cp", &cp, "-d"])
            .arg(&out)
            .output()
            .unwrap();
        let messages = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.status.success(), "nsc={nsc}: {messages}");
        assert_eq!(
            messages.matches("chain-probe-expanded").count(),
            4,
            "nsc={nsc}: {messages}"
        );
        assert_eq!(run(&out, &cp), b"4\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn implicit_macro_context_reports_active_search() {
    let root = root();
    let base_cp = format!("{JAR}:{REFLECT}");
    let implementation = root.join("implicit-context-implementation");
    compile(
        "macrotransport_open_implicits",
        true,
        &implementation,
        &base_cp,
        true,
    );
    let cp = format!("{}:{base_cp}", implementation.display());
    for consumer in [true, false] {
        let output = root.join(format!("implicit-context-use-{consumer}"));
        compile(
            "macrotransport_open_implicits_use",
            consumer,
            &output,
            &cp,
            true,
        );
        assert_eq!(run(&output, &cp), b"1\nEvidenceInfo[String]\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_macro_contexts_preserve_identity_and_restore_the_active_stack() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    let implementation = root.join("implementation");
    compile("macrocontexts", true, &implementation, &base, true);
    let cp = format!("{}:{base}", implementation.display());
    for nsc in [true, false] {
        let out = root.join(format!("use-{nsc}"));
        compile("macrocontexts_use", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"inner,inner,outer\ninner,inner\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeated_parameter_types_round_trip_through_macro_reflection() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    let implementation = root.join("implementation");
    compile("macrorepeated", true, &implementation, &base, true);
    let cp = format!("{}:{base}", implementation.display());
    for nsc in [true, false] {
        let out = root.join(format!("use-{nsc}"));
        compile("macrorepeated_use", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"3\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn binary_macro_application_symbol_selects_same_arity_overload() {
    let root = root();
    let base_cp = format!("{JAR}:{REFLECT}");
    let implementation = root.join("overloaded-macro-implementation");
    compile(
        "macrotransport_overload",
        true,
        &implementation,
        &base_cp,
        true,
    );
    let cp = format!("{}:{base_cp}", implementation.display());
    for consumer in [true, false] {
        let output = root.join(format!("overloaded-macro-use-{consumer}"));
        compile("macrotransport_overload_use", consumer, &output, &cp, true);
        assert_eq!(run(&output, &cp), b"Throwable\nInt\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn whitebox_implicit_infers_an_associated_output_in_its_expansion() {
    let root = root();
    let base_cp = format!("{JAR}:{REFLECT}");
    let implementation = root.join("associated-implementation");
    compile("macroassociated", true, &implementation, &base_cp, true);
    let cp = format!("{}:{base_cp}", implementation.display());
    for consumer in [true, false] {
        let output = root.join(format!("associated-use-{consumer}"));
        compile("macroassociated_use", consumer, &output, &cp, true);
        assert_eq!(run(&output, &cp), b"Int\n");
        let rejected = root.join(format!("associated-bad-{consumer}"));
        compile("macroassociated_bad", consumer, &rejected, &cp, false);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn implicit_macro_queries_expand_on_the_same_engine_and_restore_outer_splices() {
    let root = root();
    let base_cp = format!("{JAR}:{REFLECT}");
    let implementation = root.join("nested-query-implementation");
    compile("macronestedquery", true, &implementation, &base_cp, true);
    let cp = format!("{}:{base_cp}", implementation.display());
    for consumer in [true, false] {
        let output = root.join(format!("nested-query-use-{consumer}"));
        compile("macronestedquery_use", consumer, &output, &cp, true);
        assert_eq!(run(&output, &cp), b"Int\nString\n1\n42\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deeply_nested_generic_implicit_macro_queries_complete() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    let implementation = root.join("deep-query-implementation");
    compile("macronestedquery_deep", false, &implementation, &base, true);
    let cp = format!("{}:{base}", implementation.display());
    let output = root.join("deep-query-use");
    compile("macronestedquery_deep_use", false, &output, &cp, true);
    assert_eq!(run(&output, &cp), b"17\n");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn macro_tags_preserve_unapplied_binary_alias_constructors() {
    let root = root();
    let base_cp = format!("{JAR}:{REFLECT}");
    let implementation = root.join("alias-constructor-implementation");
    compile(
        "macroaliasconstructor",
        true,
        &implementation,
        &base_cp,
        true,
    );
    let cp = format!("{}:{base_cp}", implementation.display());
    for consumer in [true, false] {
        let output = root.join(format!("alias-constructor-use-{consumer}"));
        compile("macroaliasconstructor_use", consumer, &output, &cp, true);
        assert_eq!(run(&output, &cp), b"String\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compiler_reflection_helpers_preserve_companions_access_and_encoded_names() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let jar = ["Library/Caches/Coursier/v1", ".cache/coursier/v1"].into_iter()
        .map(|root| PathBuf::from(&home).join(root).join("https/repo1.maven.org/maven2/com/chuusai/shapeless_2.13/2.3.13/shapeless_2.13-2.3.13.jar"))
        .find(|jar| jar.is_file());
    let Some(jar) = jar else {
        eprintln!("skip: shapeless 2.3.13 is not cached");
        return;
    };
    let compiler = "/tmp/scala-2.13.16/lib/scala-compiler.jar";
    if !Path::new(compiler).is_file() {
        return;
    }
    let root = root();
    let base = format!("{JAR}:{REFLECT}:{compiler}:{}", jar.display());
    let producer = root.join("producer");
    compile("macroreflection_helpers", true, &producer, &base, true);
    let cp = format!("{}:{base}", producer.display());
    for nsc in [true, false] {
        let out = root.join(format!("consumer-{nsc}"));
        compile("macroreflection_helpers_use", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"Entry\nBinaryEntry\n7\n");
        let generic = root.join(format!("generic-{nsc}"));
        compile("macroreflection_generic", nsc, &generic, &base, true);
        assert_eq!(run(&generic, &base), b"Entry(record,7)\n");
        let sum = root.join(format!("sum-{nsc}"));
        compile("macroreflection_sum", nsc, &sum, &base, true);
        assert_eq!(run(&sum, &base), b"Added(7)\nCleared\n");
        let labelled = root.join(format!("labelled-{nsc}"));
        compile("macroreflection_labelled", nsc, &labelled, &base, true);
        assert_eq!(run(&labelled, &base), b"Entry(record,7)\n");
        let companion = root.join(format!("source-companion-{nsc}"));
        compile("macroreflection_source_companion", nsc, &companion, &base, true);
        assert_eq!(run(&companion, &base), b"Entry(42)\n");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn constructor_reflection_preserves_multiple_param_lists_and_accessors() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    for producer in [true, false] {
        let implementation = root.join(format!("constructor-implementation-{producer}"));
        compile(
            "macroreflection_constructor_params",
            producer,
            &implementation,
            &base,
            true,
        );
        let cp = format!("{}:{base}", implementation.display());
        for consumer in [true, false] {
            let output = root.join(format!("constructor-use-{producer}-{consumer}"));
            compile(
                "macroreflection_constructor_params_use",
                consumer,
                &output,
                &cp,
                true,
            );
            assert_eq!(
                run(&output, &cp),
                b"ctor=1[false:true]/1[false:false]/1[true:false];apply=1[false:true]/1[false:false]/1[true:false];copy=1[false:Int:true]/1[false:String:true]/1[true:Ordering[Int]:true]\nctor=1[false:true]/2[true:false,true:false];apply=1[false:true]/2[true:false,true:false];copy=1[false:A:true]/2[true:Ordering[A]:true,true:Numeric[A]:true]\n1:body:1\n2\n",
                "producer={producer} consumer={consumer}"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_implicit_macro_derivation_keeps_associated_types_and_stable_symbols() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let artifacts = [
        "com/chuusai/shapeless_2.13/2.3.13/shapeless_2.13-2.3.13.jar",
        "io/circe/circe-core_2.13/0.14.7/circe-core_2.13-0.14.7.jar",
        "io/circe/circe-generic_2.13/0.14.7/circe-generic_2.13-0.14.7.jar",
        "io/circe/circe-numbers_2.13/0.14.7/circe-numbers_2.13-0.14.7.jar",
        "org/typelevel/cats-core_2.13/2.11.0/cats-core_2.13-2.11.0.jar",
        "org/typelevel/cats-kernel_2.13/2.11.0/cats-kernel_2.13-2.11.0.jar",
    ];
    let mut jars = Vec::new();
    for artifact in artifacts {
        let jar = ["Library/Caches/Coursier/v1", ".cache/coursier/v1"]
            .into_iter()
            .map(|cache| {
                PathBuf::from(&home)
                    .join(cache)
                    .join("https/repo1.maven.org/maven2")
                    .join(artifact)
            })
            .find(|jar| jar.is_file());
        let Some(jar) = jar else {
            eprintln!("skip: {artifact} is not cached");
            return;
        };
        jars.push(jar);
    }
    let compiler = "/tmp/scala-2.13.16/lib/scala-compiler.jar";
    if !Path::new(compiler).is_file() {
        return;
    }
    let root = root();
    let cp = format!(
        "{JAR}:{REFLECT}:{compiler}:{}",
        jars.iter()
            .map(|jar| jar.to_string_lossy())
            .collect::<Vec<_>>()
            .join(":")
    );
    for nsc in [true, false] {
        let out = root.join(format!("nested-derivation-{nsc}"));
        compile("macroreflection_nested_derivation", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"true\n");
    }
    // An attributed This in a binary nested companion must retain its module
    // path even though that module is not an enclosing owner at the call site.
    for producer in [true, false] {
        let model = root.join(format!("binary-model-{producer}"));
        compile("macroreflection_binary_model", producer, &model, &cp, true);
        let binary_cp = format!("{}:{cp}", model.display());
        for consumer in [true, false] {
            let out = root.join(format!("binary-derivation-{producer}-{consumer}"));
            compile(
                "macroreflection_binary_derivation",
                consumer,
                &out,
                &binary_cp,
                true,
            );
            assert_eq!(run(&out, &binary_cp), b"true\n");
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn whitebox_output_drives_nested_implicit_search_once_per_callsite() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    let producer = root.join("producer");
    compile("macrooutput_fit", true, &producer, &base, true);
    let cp = format!("{}:{base}", producer.display());
    for nsc in [true, false] {
        let out = root.join(format!("use-{nsc}"));
        fs::create_dir(&out).unwrap();
        let mut cmd = Command::new(if nsc {
            NSC
        } else {
            env!("CARGO_BIN_EXE_scala-rs")
        });
        if !nsc {
            cmd.args(["compile", "--scala-library", JAR]);
        }
        let result = cmd
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/macrooutput_fit_use.scala"),
            )
            .args(["-cp", &cp, "-d"])
            .arg(&out)
            .output()
            .unwrap();
        let messages = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.status.success(), "nsc={nsc}: {messages}");
        assert_eq!(
            messages.matches("output-fit-expanded").count(),
            3,
            "nsc={nsc}: {messages}"
        );
        assert_eq!(run(&out, &cp), b"Int!\nLong!\nDouble!\n");
        compile(
            "macrooutput_fit_bad",
            nsc,
            &root.join(format!("bad-{nsc}")),
            &cp,
            false,
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn generated_intersection_types_retain_constant_type_arguments() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    let producer = root.join("intersection-implementation");
    compile("macrointersection", true, &producer, &base, true);
    let cp = format!("{}:{base}", producer.display());
    for nsc in [true, false] {
        let output = root.join(format!("intersection-use-{nsc}"));
        compile("macrointersection_use", nsc, &output, &cp, true);
        assert_eq!(run(&output, &cp), b"true\n");
        compile(
            "macrointersection_bad",
            nsc,
            &root.join(format!("intersection-bad-{nsc}")),
            &cp,
            false,
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn method_reflection_completes_pending_inferred_signatures() {
    let root = root();
    let base = format!("{JAR}:{REFLECT}");
    let producer = root.join("producer");
    compile("macromethod_info", true, &producer, &base, true);
    let cp = format!("{}:{base}", producer.display());
    for nsc in [true, false] {
        let out = root.join(format!("use-{nsc}"));
        compile("macromethod_info_use", nsc, &out, &cp, true);
        assert_eq!(run(&out, &cp), b"42\n");
        compile(
            "macromethod_info_bad",
            nsc,
            &root.join(format!("bad-{nsc}")),
            &cp,
            false,
        );
    }
    fs::remove_dir_all(root).unwrap();
}
