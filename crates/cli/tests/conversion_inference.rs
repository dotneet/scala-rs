//! Escaping conversion variables and implicit views followed by ClassTag evidence.
use crate::support;

use std::{fs, path::PathBuf, process::Command};

#[test]
fn conversion_inference_match_scalac() {
    let toolchain = support::toolchain();
    let (Some(scalac), Some(jar), Some(_java)) = (
        toolchain.scalac(),
        toolchain.scala_library(),
        toolchain.java(),
    ) else {
        eprintln!("skip conversion inference differential test: Scala/JVM toolchain unavailable");
        return;
    };
    let root = support::TestDir::new("conversion-inference");
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/conversion_inference");
    for (name, accepted) in [
        ("dependent", true),
        ("bounded_good", true),
        ("bounded_bad", false),
        ("runtime", true),
        ("flatten_runtime", true),
    ] {
        let source = fs::read_to_string(fixtures.join(format!("{name}.scala"))).unwrap();
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
            if name.ends_with("runtime") {
                let result = Command::new(toolchain.java().unwrap())
                    .arg("-Xverify:all")
                    .arg("-cp")
                    .arg(format!("{}:{}", out.display(), jar.display()))
                    .arg("Main")
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "{name}, ours={ours}: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(
                    String::from_utf8_lossy(&result.stdout),
                    if name == "runtime" {
                        "23\nhi\n"
                    } else {
                        "List(1, 2)\nList(1, 2)\nList(a, b)\n"
                    },
                    "{name}, ours={ours}"
                );
            }
            if !accepted {
                let diagnostic = String::from_utf8_lossy(result.stderr());
                assert!(
                    diagnostic.contains("type mismatch")
                        || diagnostic.contains("type arguments")
                        || diagnostic.contains("value foo"),
                    "{name}, ours={ours}: {diagnostic}"
                );
            }
        }
    }
}
