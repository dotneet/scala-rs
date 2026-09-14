//! Typed patterns over FunctionN subclasses use function variance and erasure consistently.
use crate::support;

use std::{fs, path::PathBuf, process::Command};

#[test]
fn function_subclass_patterns_match_scalac() {
    let toolchain = support::toolchain();
    let (Some(scalac), Some(jar), Some(java)) = (
        toolchain.scalac(),
        toolchain.scala_library(),
        toolchain.java(),
    ) else {
        eprintln!("skip function-pattern differential test: Scala/JVM toolchain unavailable");
        return;
    };
    let root = support::TestDir::new("function-pattern");
    let positive = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/function_subclass_pattern.scala");
    for (name, source, accepted) in [
        ("positive", fs::read_to_string(positive).unwrap(), true),
        (
            "result",
            "final class Constant[A](a: A) extends (Any => A) { def apply(x: Any): A = a }; object Bad { def test(f: Any => Int) = f match { case c: Constant[String] => 1; case _ => 0 } }".into(),
            false,
        ),
        (
            "parameter",
            "final class Zero[A](a: A) extends (() => A) { def apply(): A = a }; object Bad { def test(f: () => Int) = f match { case c: Zero[String] => 1; case _ => 0 } }".into(),
            false,
        ),
    ] {
        let src = root.join(format!("{name}.scala"));
        fs::write(&src, source).unwrap();
        for ours in [false, true] {
            let out = root.join(format!("{name}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let result = if ours {
                support::CompileCommand::new(&src, &out)
                    .scala_library(jar)
                    .run()
            } else {
                support::CompileOutcome::from_output(
                    Command::new(scalac)
                        .arg(&src)
                        .arg("-d")
                        .arg(&out)
                        .output()
                        .unwrap(),
                )
            };
            assert_eq!(
                result.success(),
                accepted,
                "{name}, ours={ours}: {}",
                result.diagnostics()
            );
            if accepted {
                let cp = format!("{}:{}", out.display(), jar.display());
                let result = Command::new(java)
                    .args(["-Xverify:all", "-cp", &cp, "Main"])
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "ours={ours}: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&result.stdout),
                    "constant\nwildcard\nwrapped\nzero\n"
                );
            } else {
                let diagnostic = String::from_utf8_lossy(result.stderr());
                assert!(diagnostic.contains("incompatible"), "{name}, ours={ours}: {diagnostic}");
            }
        }
    }
}
