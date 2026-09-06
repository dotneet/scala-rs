//! JVM descriptors must retain binary class identity even under scala/.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn java_descriptor_identity_matches_scalac() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-java-identity-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let provider = root.join("provider");
    fs::create_dir_all(&provider).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/multi/java_descriptor_identity");
    let mut sources: Vec<_> = ["List", "String", "FunctionThing", "Provider"]
        .iter()
        .map(|name| fixtures.join(format!("scala/custom/{name}.java")))
        .collect();
    sources.extend([
        fixtures.join("String.java"),
        fixtures.join("DefaultProvider.java"),
    ]);
    let result = Command::new("javac")
        .arg("-d")
        .arg(&provider)
        .args(&sources)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let jar = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    for valid in [true, false] {
        for ours in [false, true] {
            let out = root.join(format!("{valid}-{ours}"));
            fs::create_dir_all(&out).unwrap();
            let mut cmd = if ours {
                let mut cmd = Command::new(env!("CARGO_BIN_EXE_scala-rs"));
                cmd.args(["compile", "--scala-library", jar]);
                cmd
            } else {
                Command::new("/tmp/scala-2.13.16/bin/scalac")
            };
            let result = cmd
                .arg("-cp")
                .arg(&provider)
                .arg(fixtures.join(if valid { "Main.scala" } else { "Bad.scala" }))
                .arg("-d")
                .arg(&out)
                .output()
                .unwrap();
            let stderr = String::from_utf8_lossy(&result.stderr);
            assert_eq!(
                result.status.success(),
                valid,
                "valid={valid} ours={ours}: {stderr}"
            );
            if valid {
                let cp = format!("{}:{}:{jar}", out.display(), provider.display());
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
                    "default-string\ndefault-string\nlist\nlist\nstring\nfunction\nlist\n"
                );
            } else {
                assert!(
                    stderr.contains("type mismatch") || stderr.contains("no matching overload"),
                    "ours={ours}: {stderr}"
                );
                assert!(stderr.contains("Bad.scala:2:"), "ours={ours}: {stderr}");
            }
        }
    }
    fs::remove_dir_all(root).unwrap();
}
