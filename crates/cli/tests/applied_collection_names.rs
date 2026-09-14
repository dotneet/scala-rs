//! Applied collection names use source-level name resolution, including qualified names.
use crate::support;

use std::{fs, path::PathBuf, process::Command};

#[test]
fn applied_collection_names_match_scalac() {
    let toolchain = support::toolchain();
    let (Some(scalac), Some(jar), Some(_java)) = (
        toolchain.scalac(),
        toolchain.scala_library(),
        toolchain.java(),
    ) else {
        eprintln!("skip applied collection differential test: Scala/JVM toolchain unavailable");
        return;
    };
    let root = support::TestDir::new("applied-collection-names");
    let positive = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/applied_collection_names.scala");
    for (name, source, accepted) in [
        ("positive", fs::read_to_string(positive).unwrap(), true),
        (
            "result",
            "package custom { class List[A](val value: A) }; object Bad { val x: custom.List[String] = new custom.List[Int](7) }".into(),
            false,
        ),
        (
            "parameter",
            "package custom { class Option[A](val value: A) }; object Bad { val x: custom.Option[String] = new custom.Option[Int](7) }".into(),
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
                let result = support::RunCommand::new("Main").classpath(&cp).run();
                result.assert_success(&format!("ours={ours}"));
                assert_eq!(
                    result.stdout_string(),
                    "7\noption\n9\n3\n"
                );
            } else {
                let diagnostic = String::from_utf8_lossy(result.stderr());
                assert!(diagnostic.contains("type mismatch"), "{name}, ours={ours}: {diagnostic}");
            }
        }
    }
}
