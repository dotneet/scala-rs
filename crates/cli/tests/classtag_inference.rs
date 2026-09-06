//! Inference must settle type arguments before constructing ClassTag evidence.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn classtag_inference_match_scalac() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-classtag-inference-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let scalac = "/tmp/scala-2.13.16/bin/scalac";
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/multi/classtag_inference");
    for (name, accepted) in [
        ("unresolved", false),
        ("abstract", false),
        ("explicit", true),
        ("no_tag", true),
        ("not_in_result", true),
        ("receiver", true),
        ("nested-array", true),
        ("nested-list", true),
        ("normal_call", true),
        ("runtime", true),
        ("lower_runtime", true),
        ("explicit_runtime", true),
        ("partial_runtime", true),
        ("t3859", true),
        ("t5692c", true),
        ("t5859", true),
    ] {
        let source = fs::read_to_string(fixtures.join(format!("{name}.scala"))).unwrap();
        let src = root.join(format!("{name}.scala"));
        fs::write(&src, source).unwrap();
        let mut reference_output = None;
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
            if accepted && name.ends_with("runtime") {
                let cp = format!("{}:{jar}", out.display());
                let ran = Command::new("java")
                    .args(["-Xverify:all", "-cp", &cp, "Main"])
                    .output()
                    .unwrap();
                assert!(
                    ran.status.success(),
                    "{}",
                    String::from_utf8_lossy(&ran.stderr)
                );
                if ours {
                    assert_eq!(
                        Some(ran.stdout),
                        reference_output,
                        "{name}: runtime differs"
                    );
                } else {
                    reference_output = Some(ran.stdout);
                }
            }
            if !accepted {
                let diagnostic = String::from_utf8_lossy(&result.stderr);
                assert!(
                    diagnostic.contains(if name == "unresolved" {
                        "unresolved spliceable type"
                    } else {
                        "ClassTag"
                    }),
                    "{name}, ours={ours}: {diagnostic}"
                );
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
