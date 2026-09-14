//! Applied abstract type bounds participate in typed-pattern compatibility.
use crate::support;

use std::{fs, path::PathBuf, process::Command};

#[test]
fn abstract_pattern_bounds_match_scalac() {
    let toolchain = support::toolchain();
    let (Some(scalac), Some(jar)) = (toolchain.scalac(), toolchain.scala_library()) else {
        eprintln!("skip abstract pattern differential test: Scala toolchain unavailable");
        return;
    };
    let root = support::TestDir::new("abstract-pattern-bounds");
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/abstract_pattern_bounds");
    for (name, accepted) in [("t10272", true), ("t12077", true), ("bad", false)] {
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
            if !accepted {
                let diagnostic = result.diagnostics();
                assert!(
                    diagnostic.contains("incompatible"),
                    "{name}, ours={ours}: {diagnostic}"
                );
            }
        }
    }
}
