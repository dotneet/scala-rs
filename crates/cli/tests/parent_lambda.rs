//! Parent-constructor lambdas need a prototype and may capture only an
//! initialized receiver. Exercise the inferred implicit conversion at runtime.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn parent_lambda_inference_and_outer_capture_match_scalac() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-parent-lambda-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let scalac = "/tmp/scala-2.13.16/bin/scalac";
    let positive =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/parent_lambda.scala");
    for (name, source, accepted) in [
        ("positive", fs::read_to_string(positive).unwrap(), true),
        (
            "result",
            "class Base(f: Int => String)\nclass Bad extends Base(x => x + 1)".into(),
            false,
        ),
        (
            "parameter",
            "class Base(f: Int => String)\nclass Bad extends Base((x: String) => x)".into(),
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
                    "false\ntrue\ntrue\nfalse\n"
                );
            } else {
                assert!(!String::from_utf8_lossy(&result.stderr).contains("unresolved apply"));
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
