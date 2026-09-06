//! Singleton bounds must survive both directions of separate compilation.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};
const JAR: &str = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
fn compile(ours: bool, source: &Path, out: &Path, cp: &str) -> Output {
    fs::create_dir_all(out).unwrap();
    let mut c = if ours {
        let mut c = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
        c.args(["compile", "--scala-library", JAR]);
        c
    } else {
        Command::new("/tmp/scala-2.13.16/bin/scalac")
    };
    c.arg(source)
        .arg("-d")
        .arg(out)
        .args(["-cp", cp])
        .output()
        .unwrap()
}
#[test]
fn singleton_bounds_survive_separate_compilation() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-singleton-metadata-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/singleton_metadata");
    for producer_ours in [false, true] {
        let provider = root.join(format!("provider-{producer_ours}"));
        let p = compile(
            producer_ours,
            &fixtures.join("provider.scala"),
            &provider,
            JAR,
        );
        assert!(
            p.status.success(),
            "producer ours={producer_ours}: {}",
            String::from_utf8_lossy(&p.stderr)
        );
        for consumer_ours in [false, true] {
            for bad in [false, true] {
                let out = root.join(format!("consumer-{producer_ours}-{consumer_ours}-{bad}"));
                let cp = format!("{}:{JAR}", provider.display());
                let p = compile(
                    consumer_ours,
                    &fixtures.join(if bad {
                        "consumer_bad.scala"
                    } else {
                        "consumer.scala"
                    }),
                    &out,
                    &cp,
                );
                assert_eq!(
                    p.status.success(),
                    !bad,
                    "producer ours={producer_ours}, consumer ours={consumer_ours}, bad={bad}: {}",
                    String::from_utf8_lossy(&p.stderr)
                );
                if bad {
                    let diagnostic = String::from_utf8_lossy(&p.stderr);
                    assert!(
                        diagnostic.contains("incompatible type in overriding"),
                        "producer ours={producer_ours}, consumer ours={consumer_ours}: {diagnostic}"
                    );
                }
                if !bad {
                    let p = Command::new("java")
                        .arg("-Xverify:all")
                        .arg("-cp")
                        .arg(format!("{}:{cp}", out.display()))
                        .arg("Main")
                        .output()
                        .unwrap();
                    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
                    assert_eq!(String::from_utf8_lossy(&p.stdout), "7\nbound\n");
                }
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
