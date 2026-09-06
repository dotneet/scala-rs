//! Typed patterns over FunctionN subclasses use function variance and erasure consistently.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn function_subclass_patterns_match_scalac() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-function-pattern-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let scalac = "/tmp/scala-2.13.16/bin/scalac";
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
            let mut cmd = if ours {
                let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                c.args(["compile", "--scala-library", jar]);
                c
            } else {
                Command::new(scalac)
            };
            let result = cmd.arg(&src).arg("-d").arg(&out).output().unwrap();
            assert_eq!(
                result.status.success(),
                accepted,
                "{name}, ours={ours}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            if accepted {
                let cp = format!("{}:{jar}", out.display());
                let result = Command::new("java")
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
                let diagnostic = String::from_utf8_lossy(&result.stderr);
                assert!(diagnostic.contains("incompatible"), "{name}, ours={ours}: {diagnostic}");
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
