//! Infer a lambda's result before trying an implicit conversion.
use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, path::PathBuf, process::Command};

#[test]
fn lambda_result_inference_matches_scalac() {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip lambda result inference: Scala/JVM toolchain unavailable");
        return;
    };
    let root = TestDir::new("lambda-result-inference");
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/lambda_result_inference");
    for (name, expected) in [
        ("direct_runtime", Some("42\n")),
        ("view_runtime", Some("Some(42)\n")),
        ("bounded_bad", None),
    ] {
        let source = fixtures.join(format!("{name}.scala"));
        for ours in [false, true] {
            let out = root.join(format!("{name}-{ours}"));
            fs::create_dir(&out).unwrap();
            let compiled = if ours {
                CompileCommand::new(&source, &out)
                    .scala_library(library)
                    .run()
            } else {
                CompileOutcome::from_output(
                    Command::new(scalac)
                        .arg(&source)
                        .arg("-d")
                        .arg(&out)
                        .output()
                        .unwrap(),
                )
            };
            assert_eq!(
                compiled.success(),
                expected.is_some(),
                "{name}, ours={ours}: {}",
                compiled.diagnostics()
            );
            if let Some(expected) = expected {
                let run = RunCommand::new("Main")
                    .classpath(format!("{}:{}", out.display(), library.display()))
                    .run();
                run.assert_success(&format!("{name}, ours={ours}"));
                assert_eq!(run.stdout_string(), expected, "{name}, ours={ours}");
            }
        }
    }
}
