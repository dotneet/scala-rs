//! Calls inherited from a package-private Java superclass must name the
//! visible receiver class in the JVM Methodref.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

#[test]
fn inherited_method_from_inaccessible_java_parent_links() {
    let root = std::env::temp_dir().join(format!(
        "scala-rs-jvm-access-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let provider = root.join("provider");
    let out = root.join("out");
    fs::create_dir_all(&provider).unwrap();
    fs::create_dir_all(&out).unwrap();
    let java_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/multi/jvm_inaccessible_parent/inaccessible");
    let java = Command::new("javac")
        .args([
            "-d",
            provider.to_str().unwrap(),
            java_dir.join("HiddenBase.java").to_str().unwrap(),
            java_dir.join("ParentApi.java").to_str().unwrap(),
            java_dir.join("ChildApi.java").to_str().unwrap(),
            java_dir.join("VisibleChild.java").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        java.status.success(),
        "javac failed:\n{}",
        String::from_utf8_lossy(&java.stderr)
    );

    let library = "/tmp/scala-rs-lib/scala-library-2.13.16.jar";
    let cp = format!("{}:{library}", provider.display());
    let compile = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            "--scala-library",
            library,
            fixture("jvm_inaccessible_parent.scala").to_str().unwrap(),
            "-cp",
            &cp,
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "scala-rs failed:\n{}{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run_cp = format!("{}:{cp}", out.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &run_cp, "jvmaccess.Main"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "java failed:\n{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "7\n11\n11\nvisible\n");

    let _ = fs::remove_dir_all(root);
}
