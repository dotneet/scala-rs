//! Default type imports remain available after same-named terms are loaded.
use crate::support;

use std::{fs, path::PathBuf, process::Command};

#[test]
fn default_type_imports_after_term_completion_match_scalac() {
    let toolchain = support::toolchain();
    let (Some(scalac), Some(jar), Some(_java)) = (
        toolchain.scalac(),
        toolchain.scala_library(),
        toolchain.java(),
    ) else {
        eprintln!("skip default type imports differential test: Scala/JVM toolchain unavailable");
        return;
    };
    let root = support::TestDir::new("default-type-imports");
    let positive = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/default_type_imports.scala");
    for (name, source, accepted) in [
        ("positive", fs::read_to_string(positive).unwrap(), true),
        ("cold", fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cold_module_apply.scala")).unwrap(), true),
        (
            "result",
            "object Bad { def bad[A](a: A): scala.collection.immutable.Stream[Int] = scala.collection.immutable.Stream.apply(a) }".into(),
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
                let result = Command::new(toolchain.java().unwrap())
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
                    "List(1)\nList(a)\n"
                );
            } else {
                let diagnostic = String::from_utf8_lossy(result.stderr());
                assert!(diagnostic.contains("type mismatch"), "{name}, ours={ours}: {diagnostic}");
            }
        }
    }
}
