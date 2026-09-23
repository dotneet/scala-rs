//! An imported annotation must retain its resolved package in ScalaSignature.

use crate::support::{toolchain, TestDir};
use scala_rs_pickle::read::{read_pickle, Entry};
use scala_rs_pickle::scala_signature_bytes;
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

#[test]
fn imported_binary_annotation_has_qualified_pickle_owner() {
    let Some(library) = toolchain().scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    let Some(javac) = toolchain().javac() else {
        eprintln!("skip: javac is unavailable");
        return;
    };
    let dir = TestDir::new("binary-annotation-import-owner");
    let java = dir.join("src/sample/marker/Trace.java");
    let target = dir.join("Target.scala");
    let classes = dir.join("classes");
    let dependency = dir.join("trace-api.jar");
    fs::create_dir_all(java.parent().unwrap()).unwrap();
    fs::create_dir_all(&classes).unwrap();
    fs::write(
        &java,
        "package sample.marker;\n\
         import java.lang.annotation.ElementType;\n\
         import java.lang.annotation.Retention;\n\
         import java.lang.annotation.RetentionPolicy;\n\
         import java.lang.annotation.Target;\n\
         @Retention(RetentionPolicy.RUNTIME)\n\
         @Target({ElementType.METHOD, ElementType.TYPE})\n\
         public @interface Trace {}\n",
    )
    .unwrap();
    let javac_output = Command::new(javac)
        .arg("-d")
        .arg(&classes)
        .arg(&java)
        .output()
        .unwrap();
    assert!(
        javac_output.status.success(),
        "javac failed:\n{}{}",
        String::from_utf8_lossy(&javac_output.stdout),
        String::from_utf8_lossy(&javac_output.stderr)
    );
    let jar_output = Command::new("jar")
        .args(["--create", "--file"])
        .arg(&dependency)
        .arg("-C")
        .arg(&classes)
        .arg("sample/marker/Trace.class")
        .output()
        .unwrap();
    assert!(
        jar_output.status.success(),
        "jar failed:\n{}{}",
        String::from_utf8_lossy(&jar_output.stdout),
        String::from_utf8_lossy(&jar_output.stderr)
    );
    fs::write(
        &target,
        "package sample\nimport sample.marker.Trace\nclass Target { @Trace def work = 1; @Trace object Nested }\n",
    )
    .unwrap();

    let classpath = dependency.display().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args(["compile"])
        .arg(&target)
        .arg("-cp")
        .arg(&classpath)
        .arg("--no-scala-library")
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
        pickle.entries.iter().any(|entry| {
            let Entry::SymAnnot { annot, .. } = entry else {
                return false;
            };
            let Some(Entry::TypeRefTpe { sym, .. }) = pickle.entry(annot.tpe) else {
                return false;
            };
            pickle.sym_full_name(*sym).as_deref() == Some("sample.marker.Trace")
        }),
        "imported binary annotation must refer to sample.marker.Trace"
    );

    let (Some(scalac), Some(reflect), Some(java)) = (
        toolchain().scalac(),
        toolchain().scala_reflect(),
        toolchain().java(),
    ) else {
        eprintln!("skip Scala reflection check: scalac, scala-reflect, or java is unavailable");
        return;
    };
    let reader = dir.join("ReadAnnotations.scala");
    fs::write(
        &reader,
        r#"
        package sample
        object ReadAnnotations {
          def main(args: Array[String]): Unit = {
            import scala.reflect.runtime.{universe => ru}
            val mirror = ru.runtimeMirror(getClass.getClassLoader)
            val target = mirror.staticClass("sample.Target").toType
            target.members.foreach(_.annotations)
            val method = target.decl(ru.TermName("work"))
            println(method.annotations.map(_.tree.tpe.typeSymbol.fullName).mkString(","))
          }
        }
        "#,
    )
    .unwrap();
    let runtime_cp = format!(
        "{}:{}:{}:{}",
        library.display(),
        reflect.display(),
        classes.display(),
        dependency.display()
    );
    let compiled = Command::new(scalac)
        .arg("-classpath")
        .arg(&runtime_cp)
        .arg("-d")
        .arg(&classes)
        .arg(&reader)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "reflection reader compile failed:\n{}{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );
    let reflected = Command::new(java)
        .arg("-cp")
        .arg(runtime_cp)
        .arg("sample.ReadAnnotations")
        .output()
        .unwrap();
    assert!(
        reflected.status.success(),
        "Scala reflection failed:\n{}{}",
        String::from_utf8_lossy(&reflected.stdout),
        String::from_utf8_lossy(&reflected.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&reflected.stdout).trim(),
        "sample.marker.Trace"
    );
}
