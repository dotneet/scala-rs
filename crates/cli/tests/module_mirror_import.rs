//! An object-only binary's JVM forwarder is not a Scala type import.

#[path = "support/temp_nonce.rs"]
mod temp_nonce;

use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn object_import_does_not_hide_its_same_named_type_alias() {
    let scalac = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    let library = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if !scalac.is_file() || !library.is_file() {
        eprintln!("skip: Scala 2.13 toolchain is not installed in /tmp");
        return;
    }
    let nonce = temp_nonce::unique_stamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    let root = std::env::temp_dir().join(format!("scala-rs-module-mirror-import-{nonce}"));
    let lib = root.join("lib");
    let out = root.join("out");
    let reader = root.join("reader");
    for dir in [&lib, &out, &reader] {
        fs::create_dir_all(dir).unwrap();
    }
    let lib_src = root.join("Library.scala");
    let use_src = root.join("Use.scala");
    let reader_src = root.join("Reader.scala");
    fs::write(
        &lib_src,
        "package mirroralias\nobject Factory { type Factory[F[_]] = F[Int] }\n",
    )
    .unwrap();
    fs::write(
        &use_src,
        r#"package mirroralias
import mirroralias.Factory
import mirroralias.Factory.Factory
object Use {
  val module = Factory
  val value: Factory[Option] = Some(1)
}
"#,
    )
    .unwrap();
    fs::write(
        &reader_src,
        "package mirroralias\nobject Reader { val value: Option[Int] = Use.value }\n",
    )
    .unwrap();

    let compiled_lib = Command::new(&scalac)
        .args(["-d", lib.to_str().unwrap(), lib_src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        compiled_lib.status.success(),
        "library: {}{}",
        String::from_utf8_lossy(&compiled_lib.stderr),
        String::from_utf8_lossy(&compiled_lib.stdout)
    );
    let cp = format!("{}:{}", lib.display(), library.display());
    let compiled_use = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args([
            "compile",
            use_src.to_str().unwrap(),
            "--scala-library",
            library.to_str().unwrap(),
            "-cp",
            &cp,
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        compiled_use.status.success(),
        "scala-rs: {}{}",
        String::from_utf8_lossy(&compiled_use.stderr),
        String::from_utf8_lossy(&compiled_use.stdout)
    );
    let cp = format!("{}:{}", lib.display(), out.display());
    let compiled_reader = Command::new(&scalac)
        .args([
            "-cp",
            &cp,
            "-d",
            reader.to_str().unwrap(),
            reader_src.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        compiled_reader.status.success(),
        "reader: {}{}",
        String::from_utf8_lossy(&compiled_reader.stderr),
        String::from_utf8_lossy(&compiled_reader.stdout)
    );
    let _ = fs::remove_dir_all(root);
}
