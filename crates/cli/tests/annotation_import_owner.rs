//! An imported annotation must retain its resolved package in ScalaSignature.

use crate::support::{toolchain, TestDir};
use scala_rs_pickle::{read_pickle, scala_signature_bytes};
use std::{fs, process::Command};

#[test]
fn imported_annotation_has_qualified_pickle_owner() {
    let Some(library) = toolchain().scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    let dir = TestDir::new("annotation-import-owner");
    let marker = dir.join("Marker.scala");
    let target = dir.join("Target.scala");
    let classes = dir.join("classes");
    fs::create_dir(&classes).unwrap();
    fs::write(
        &marker,
        "package sample.marker\nclass Marker extends scala.annotation.StaticAnnotation\n",
    )
    .unwrap();
    fs::write(
        &target,
        "package sample\nimport sample.marker.Marker\n@Marker class Target\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args(["compile"])
        .args([&marker, &target])
        .arg("-cp")
        .arg(library)
        .arg("--scala-library")
        .arg(library)
        .arg("-d")
        .arg(&classes)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let class = fs::read(classes.join("sample/Target.class")).unwrap();
    let signature = scala_signature_bytes(&class).expect("ScalaSignature");
    let pickle = read_pickle(&signature).expect("valid ScalaSignature");
    assert!(
        (0..pickle.entries.len() as u32).any(|index| {
            pickle.sym_name(index) == Some("Marker")
                && pickle.sym_full_name(index).as_deref() == Some("sample.marker.Marker")
        }),
        "imported annotation must refer to sample.marker.Marker"
    );
}
