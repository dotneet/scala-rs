//! Inherited abstract results guide inference without fixing its final type.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
const NSC: &str = "/tmp/scala-2.13.16/bin/scalac";
fn root() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "absresult-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}
fn fixture(n: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(n)
}
fn check(r: &Output) {
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
}
fn compile(files: &[PathBuf], out: &Path, cp: &str, oracle: bool, private: bool) -> Output {
    fs::create_dir_all(out).unwrap();
    let mut c = if oracle {
        Command::new(NSC)
    } else {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
        c.arg("compile");
        if private {
            c.arg("--no-scala-library");
        } else {
            c.args(["--scala-library", JAR]);
        }
        c
    };
    c.args(files)
        .arg("-d")
        .arg(out)
        .args(["-cp", cp])
        .output()
        .unwrap()
}
fn run(out: &Path, cp: &str) -> Output {
    Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            "Main",
        ])
        .output()
        .unwrap()
}
#[test]
fn abstract_results_match_scalac_from_source_and_binary() {
    let p = root();
    let lib = p.join("lib");
    check(&compile(
        &[fixture("absresult_lib.scala")],
        &lib,
        JAR,
        true,
        false,
    ));
    let cp = format!("{}:{JAR}", lib.display());
    let nsc = p.join("nsc");
    check(&compile(
        &[fixture("absresult.scala")],
        &nsc,
        &cp,
        true,
        false,
    ));
    let expected = run(&nsc, &cp);
    check(&expected);
    assert_eq!(
        expected.stdout,
        fs::read(fixture("expected/absresult.txt")).unwrap()
    );
    for binary in [false, true] {
        let files = if binary {
            vec![fixture("absresult.scala")]
        } else {
            vec![fixture("absresult.scala"), fixture("absresult_lib.scala")]
        };
        let path = if binary { &cp } else { JAR };
        let out = p.join(if binary { "binary" } else { "source" });
        check(&compile(&files, &out, path, false, false));
        let actual = run(&out, path);
        check(&actual);
        assert_eq!(actual.stdout, expected.stdout);
    }
    for oracle in [false, true] {
        let bad = compile(
            &[fixture("absresult_bad.scala")],
            &p.join(if oracle { "bad-nsc" } else { "bad-ours" }),
            &cp,
            oracle,
            false,
        );
        assert!(
            !bad.status.success(),
            "accepted incompatible abstract implementation"
        );
        assert!(
            String::from_utf8_lossy(&bad.stderr).contains("type mismatch"),
            "{bad:?}"
        );
    }
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn trivial_self_reference_warning_matches_scalac() {
    let p = root();
    for oracle in [false, true] {
        for bad in [false, true] {
            let out = p.join(format!("warn-{oracle}-{bad}"));
            fs::create_dir_all(&out).unwrap();
            let mut c = if oracle {
                Command::new(NSC)
            } else {
                let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                c.args(["compile", "--scala-library", JAR]);
                c
            };
            let r = c
                .arg(fixture(if bad {
                    "absresult_selfwarn_bad.scala"
                } else {
                    "absresult_selfwarn.scala"
                }))
                .arg("-Xfatal-warnings")
                .arg("-d")
                .arg(&out)
                .output()
                .unwrap();
            if bad {
                assert!(!r.status.success());
                assert_eq!(
                    String::from_utf8_lossy(&r.stderr)
                        .matches("does nothing other than call itself recursively")
                        .count(),
                    4
                );
            } else {
                check(&r);
                let result = run(&out, JAR);
                check(&result);
                assert_eq!(result.stdout, b"7\n8\n");
            }
        }
    }
}

#[test]
fn inherited_dependent_result_uses_implementation_parameters() {
    let p = root();
    for oracle in [false, true] {
        let out = p.join(format!("dependent-{oracle}"));
        check(&compile(
            &[fixture("absresult_dependent.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        assert_eq!(result.stdout, b"dependent\n");
        let bad = compile(
            &[fixture("absresult_dependent_bad.scala")],
            &p.join(format!("dependent-bad-{oracle}")),
            JAR,
            oracle,
            false,
        );
        assert!(!bad.status.success());
    }
}

#[test]
fn nothing_result_bridges_universal_methods() {
    let p = root();
    for oracle in [false, true] {
        let out = p.join(format!("nothing-bridge-{oracle}"));
        check(&compile(
            &[fixture("absresult_nothing_bridge.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        assert_eq!(result.stdout, b"match error preserved\n");
    }
}

#[test]
fn inherited_result_alias_substitutes_ancestor_parameters() {
    let p = root();
    for oracle in [false, true] {
        let out = p.join(format!("alias-{oracle}"));
        check(&compile(
            &[fixture("absresult_alias.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        assert_eq!(result.stdout, b"alias\n");
    }
}

#[test]
fn inherited_type_parameter_is_seen_from_anonymous_receiver() {
    let p = root();
    for oracle in [false, true] {
        let out = p.join(format!("inherited-param-{oracle}"));
        check(&compile(
            &[fixture("absresult_inherited_param.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        assert_eq!(result.stdout, b"prefix\n");
    }
}

#[test]
fn colon_type_operators_associate_right() {
    let p = root();
    for oracle in [false, true] {
        let out = p.join(format!("infix-{oracle}"));
        check(&compile(
            &[fixture("absresult_infix.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        assert_eq!(result.stdout, b"7\nright\ntrue\n");
        let bad = compile(
            &[fixture("absresult_infix_bad.scala")],
            &p.join(format!("infix-bad-{oracle}")),
            JAR,
            oracle,
            false,
        );
        assert!(!bad.status.success());
    }
}

#[test]
fn implicit_candidate_completion_preserves_runtime_evidence() {
    let p = root();
    let mut stdout = None;
    for oracle in [false, true] {
        let out = p.join(format!("completion-{oracle}"));
        check(&compile(
            &[fixture("absresult_completion.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        assert_eq!(result.stdout, b"completed\n");
        if let Some(previous) = &stdout {
            assert_eq!(previous, &result.stdout);
        }
        stdout = Some(result.stdout);
    }
}

#[test]
fn recursive_implicit_result_variables_are_distinct() {
    let p = root();
    for oracle in [false, true] {
        let out = p.join(format!("recursive-{oracle}"));
        check(&compile(
            &[fixture("absresult_recursive.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        assert_eq!(result.stdout, b"Cons(1,Cons(2,End))\n");
        let bad = compile(
            &[fixture("absresult_recursive_bad.scala")],
            &p.join(format!("recursive-bad-{oracle}")),
            JAR,
            oracle,
            false,
        );
        assert!(!bad.status.success());
    }
}

#[test]
fn shrinking_implicit_derivation_can_exceed_eight_levels() {
    let p = root();
    let mut outputs = Vec::new();
    for oracle in [false, true] {
        let out = p.join(format!("recursive-deep-{oracle}"));
        check(&compile(
            &[fixture("absresult_recursive_deep.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        outputs.push(result.stdout);
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0], b"Cons(1,Cons(2,Cons(3,Cons(4,Cons(5,Cons(6,Cons(7,Cons(8,Cons(9,Cons(10,Cons(11,Cons(12,End))))))))))))\n");
}

#[test]
fn infer_override_applies_to_methods_fields_and_setters() {
    let p = root();
    for oracle in [false, true] {
        for bad in [false, true] {
            let out = p.join(format!("infer-override-{oracle}-{bad}"));
            fs::create_dir_all(&out).unwrap();
            let mut c = if oracle {
                Command::new(NSC)
            } else {
                let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                c.args(["compile", "--scala-library", JAR]);
                c
            };
            let r = c
                .arg(fixture(if bad {
                    "absresult_infer_override_bad.scala"
                } else {
                    "absresult_infer_override.scala"
                }))
                .args(["-Xsource:3", "-Xsource-features:infer-override"])
                .arg("-d")
                .arg(&out)
                .output()
                .unwrap();
            if bad {
                assert!(!r.status.success());
            } else {
                check(&r);
                let result = run(&out, JAR);
                check(&result);
                assert_eq!(result.stdout, b"method\nfield\n42\nconstant\n");
            }
        }
    }
}

#[test]
fn infer_override_keeps_blackbox_results_at_the_inherited_type() {
    let p = root();
    let lib = p.join("macro-lib");
    check(&compile(
        &[fixture("absresult_infer_macro_lib.scala")],
        &lib,
        JAR,
        true,
        false,
    ));
    let cp = format!(
        "{}:{JAR}:/tmp/scala-2.13.16/lib/scala-reflect.jar",
        lib.display()
    );
    for oracle in [false, true] {
        let out = p.join(format!("macro-use-{oracle}"));
        fs::create_dir_all(&out).unwrap();
        let mut c = if oracle {
            Command::new(NSC)
        } else {
            let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
            c.args(["compile", "--scala-library", JAR]);
            c
        };
        check(
            &c.arg(fixture("absresult_infer_macro.scala"))
                .args(["-Xsource:3", "-Xsource-features:infer-override"])
                .arg("-cp")
                .arg(&cp)
                .arg("-d")
                .arg(&out)
                .output()
                .unwrap(),
        );
        let result = run(&out, &cp);
        check(&result);
        assert_eq!(result.stdout, b"macro\nmacro\n");
        let sig = Command::new("javap")
            .args(["-p", "-classpath"])
            .arg(&out)
            .arg("Use")
            .output()
            .unwrap();
        check(&sig);
        let sig = String::from_utf8_lossy(&sig.stdout);
        assert!(sig.contains("java.lang.Object method()"));
        assert!(sig.contains("java.lang.Object field()"));
        assert!(!sig.contains("java.lang.String method()"));
        assert!(!sig.contains("java.lang.String field()"));
    }
}

#[test]
fn infer_override_is_ignored_without_source_three() {
    let p = root();
    for oracle in [false, true] {
        let out = p.join(format!("feature-without-source-{oracle}"));
        fs::create_dir_all(&out).unwrap();
        let mut c = if oracle {
            Command::new(NSC)
        } else {
            let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
            c.args(["compile", "--scala-library", JAR]);
            c
        };
        let r = c
            .arg(fixture("absresult_infer_override_bad.scala"))
            .arg("-Xsource-features:infer-override")
            .arg("-d")
            .arg(&out)
            .output()
            .unwrap();
        check(&r);
        assert!(String::from_utf8_lossy(&r.stderr).contains("requires -Xsource:3"));
    }
}

#[test]
fn parameterless_receiver_variables_survive_selection() {
    let p = root();
    let mut outputs = Vec::new();
    for oracle in [false, true] {
        let out = p.join(format!("receiver-{oracle}"));
        check(&compile(
            &[fixture("absresult_receiver.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        outputs.push(result.stdout);
        let bad = compile(
            &[fixture("absresult_receiver_bad.scala")],
            &p.join(format!("receiver-bad-{oracle}")),
            JAR,
            oracle,
            false,
        );
        assert!(!bad.status.success());
        assert!(String::from_utf8_lossy(&bad.stderr).contains("type mismatch"));
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0], b"value\nargument\n");
}

#[test]
fn inherited_result_matches_the_exact_overload() {
    let p = root();
    let mut outputs = Vec::new();
    for oracle in [false, true] {
        let out = p.join(format!("overload-{oracle}"));
        check(&compile(
            &[fixture("absresult_overload.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        outputs.push(result.stdout);
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0], b"true\ntrue\n");
}

#[test]
fn inherited_ordering_result_emits_a_sam() {
    let p = root();
    let mut outputs = Vec::new();
    for oracle in [false, true] {
        let out = p.join(format!("ordering-{oracle}"));
        check(&compile(
            &[fixture("absresult_ordering.scala")],
            &out,
            JAR,
            oracle,
            false,
        ));
        let result = run(&out, JAR);
        check(&result);
        outputs.push(result.stdout);
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0], b"-1\n1\n-1\n-1\n");
}
