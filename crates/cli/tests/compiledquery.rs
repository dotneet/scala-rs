//! Regression test for dependent result types from imported Scala signatures.
//!
//! The result of `Parameters.apply` uses a type projection from its implicit
//! `Shape` parameter. Its classfile pickle names that local projection with
//! an unqualified alias. The reader must recover the projection so the method
//! remains applicable instead of exposing it as an unapplied function.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("scala-rs-{tag}-{nanos}"));
    fs::create_dir_all(&path).expect("create temporary directory");
    path
}

fn scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.exists().then_some(path)
}

fn scala_library() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.exists().then_some(path)
}

fn compile_with_scalac(scalac: &Path, source: &str, out: &Path, cp: Option<&Path>) {
    let mut command = Command::new(scalac);
    command.args([source, "-d", out.to_str().unwrap()]);
    if let Some(cp) = cp {
        command.args(["-cp", cp.to_str().unwrap()]);
    }
    let output = command.output().expect("run scalac");
    assert!(
        output.status.success(),
        "scalac failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn source_wildcard_alias_keeps_its_path_identity_across_file_order() {
    let (Some(scalac), Some(scala_library)) = (scalac(), scala_library()) else {
        eprintln!("skip wildcard alias regression: scalac or scala-library unavailable");
        return;
    };
    let use_source = fixtures_dir().join("compiledquery_order_use.scala");
    let shape_source = fixtures_dir().join("compiledquery_order_shape.scala");
    let scala_rs = bin();
    let mut outputs = Vec::new();
    for (compiler, is_scala_rs) in [(&scalac, false), (&scala_rs, true)] {
        let out = tmp_dir("compiled-query-order");
        let mut command = Command::new(compiler);
        if is_scala_rs {
            command.arg("compile");
        }
        command.args([
            use_source.to_str().unwrap(),
            shape_source.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-Xsource:3-cross",
        ]);
        if is_scala_rs {
            command.args(["--scala-library", scala_library.to_str().unwrap()]);
        }
        let output = command
            .output()
            .expect("compile ordered wildcard alias sources");
        assert!(
            output.status.success(),
            "{} rejected ordered wildcard alias sources: {}{}",
            compiler.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let cp = format!("{}:{}", out.display(), scala_library.display());
        let runtime = Command::new("java")
            .args(["-Xverify:all", "-cp", &cp, "compiledqueryorder.Main"])
            .output()
            .expect("run ordered wildcard alias output");
        assert!(
            runtime.status.success(),
            "{} emitted invalid ordered wildcard alias classes: {}",
            compiler.display(),
            String::from_utf8_lossy(&runtime.stderr)
        );
        outputs.push(String::from_utf8_lossy(&runtime.stdout).into_owned());
        let _ = fs::remove_dir_all(out);
    }
    assert_eq!(outputs, ["Parameters\n", "Parameters\n"]);
}

#[test]
fn reads_dependent_projection_from_scalac_classfiles() {
    let (Some(scalac), Some(scala_library)) = (scalac(), scala_library()) else {
        eprintln!("skip compiled-query regression: scalac or scala-library unavailable");
        return;
    };
    let lib = tmp_dir("compiled-query-lib");
    let user = tmp_dir("compiled-query-user");
    compile_with_scalac(
        &scalac,
        fixtures_dir()
            .join("compiledquery_lib.scala")
            .to_str()
            .unwrap(),
        &lib,
        None,
    );
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join("compiledquery_use.scala")
                .to_str()
                .unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            user.to_str().unwrap(),
            "--scala-library",
            scala_library.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "scala-rs rejected dependent projection from classfiles: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let runtime_cp = format!(
        "{}:{}:{}",
        user.display(),
        lib.display(),
        scala_library.display()
    );
    let runtime = Command::new("java")
        .args(["-cp", &runtime_cp, "Main"])
        .output()
        .expect("run compiled dependent projection");
    assert!(
        runtime.status.success(),
        "compiled-query runtime failed: {}{}",
        String::from_utf8_lossy(&runtime.stdout),
        String::from_utf8_lossy(&runtime.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&runtime.stdout).trim(), "true");

    let bad_scalac = tmp_dir("compiled-query-bad-scalac");
    let bad_source = fixtures_dir().join("compiledquery_bad.scala");
    let bad_scalac_output = Command::new(&scalac)
        .args([
            bad_source.to_str().unwrap(),
            "-d",
            bad_scalac.to_str().unwrap(),
        ])
        .args(["-cp", lib.to_str().unwrap()])
        .output()
        .expect("run scalac negative compiled-query check");
    assert!(
        !bad_scalac_output.status.success(),
        "scalac accepted invalid compiled-query uses: {}{}",
        String::from_utf8_lossy(&bad_scalac_output.stdout),
        String::from_utf8_lossy(&bad_scalac_output.stderr)
    );

    let bad = tmp_dir("compiled-query-bad");
    let bad_output = Command::new(bin())
        .args([
            "compile",
            bad_source.to_str().unwrap(),
            "-cp",
            lib.to_str().unwrap(),
            "-d",
            bad.to_str().unwrap(),
            "--scala-library",
            scala_library.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs negative compiled-query check");
    assert!(
        !bad_output.status.success(),
        "scala-rs accepted invalid compiled-query uses: {}{}",
        String::from_utf8_lossy(&bad_output.stdout),
        String::from_utf8_lossy(&bad_output.stderr)
    );
    let _ = fs::remove_dir_all(lib);
    let _ = fs::remove_dir_all(user);
    let _ = fs::remove_dir_all(bad_scalac);
    let _ = fs::remove_dir_all(bad);
}
