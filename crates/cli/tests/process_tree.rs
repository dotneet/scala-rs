//! A curried method in a separately compiled class must retain its parameter
//! clauses when the consumer reads the producer's ScalaSignature.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("{name}.scala"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "scala-rs-process-tree-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn compile(src: &Path, out: &Path, jar: &Path, classpath: Option<&Path>) {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    if let Some(cp) = classpath {
        cmd.args(["-cp", cp.to_str().unwrap()]);
    }
    cmd.args(["-d", out.to_str().unwrap()]);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {} failed:\n{}{}",
        src.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn separate_compilation_preserves_curried_process_tree() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip processTree separate compilation: scala-library jar not obtainable");
        return;
    };
    let root = tmp_dir("curried");
    let lib = root.join("lib");
    let app = root.join("app");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&app).unwrap();

    compile(&fixture("process_tree_lib"), &lib, &jar, None);
    compile(&fixture("process_tree_app"), &app, &jar, Some(&lib));

    let cp = format!("{}:{}:{}", app.display(), lib.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "tree:7\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn separate_compilation_preserves_curried_process_tree_java_parameter_types() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip processTree Java separate compilation: scala-library jar not obtainable");
        return;
    };
    let root = tmp_dir("curried-java");
    let lib = root.join("lib");
    let app = root.join("app");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&app).unwrap();

    compile(&fixture("process_tree_java_lib"), &lib, &jar, None);
    compile(&fixture("process_tree_java_app"), &app, &jar, Some(&lib));

    let cp = format!("{}:{}:{}", app.display(), lib.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "ProcessTreeJavaMain"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java ProcessTreeJavaMain failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "tree:1\n");
    let _ = fs::remove_dir_all(root);
}
