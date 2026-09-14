//! Writer-to-reader regression for an abstract projection in a generic parent.
//!
//! `TableQuery[E]` inherits `Query[E, E#TableElementType, Seq]`. The writer
//! must retain the `E#` prefix so a separate scala-rs invocation can reduce
//! the element type after substituting a concrete table. The ordinary filter
//! and implicit witness checks the receiver substitution; the explicit
//! `Query[User, Int, Seq]` annotation checks the projected result type.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

fn temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-tablequery-parent-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn diagnostics(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn javap(classpath: &Path, class_name: &str) -> String {
    let output = Command::new("javap")
        .args(["-v", "-classpath"])
        .arg(classpath)
        .arg(class_name)
        .output()
        .expect("run javap");
    assert!(
        output.status.success(),
        "javap {class_name} failed:\n{}",
        diagnostics(&output)
    );
    String::from_utf8(output.stdout).expect("javap output is UTF-8")
}

fn assert_users_signature(text: &str, static_method: bool) {
    let method = if static_method {
        "public static tablequeryparent.TableQuery<tablequeryparent.User> users();"
    } else {
        "public tablequeryparent.TableQuery<tablequeryparent.User> users();"
    };
    assert!(
        text.contains(method),
        "missing typed users accessor `{method}` in:\n{text}"
    );
    assert!(
        text.contains("Signature:")
            && text.contains("()Ltablequeryparent/TableQuery<Ltablequeryparent/User;>;"),
        "missing generic users Signature in:\n{text}"
    );
}

fn assert_default_users_signature(text: &str) {
    assert!(
        text.contains("public default tablequeryparent.TableQuery<tablequeryparent.User> users();"),
        "missing typed default users accessor in:\n{text}"
    );
    assert!(
        text.contains("Signature:")
            && text.contains("()Ltablequeryparent/TableQuery<Ltablequeryparent/User;>;"),
        "missing generic default users Signature in:\n{text}"
    );
}

fn compile(source: &Path, out: &Path, classpath: Option<&Path>, library: &Path) -> Output {
    let mut command = Command::new(bin());
    command.args([
        "compile",
        source.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
        "--scala-library",
        library.to_str().unwrap(),
    ]);
    if let Some(classpath) = classpath {
        command.args(["-cp", classpath.to_str().unwrap()]);
    }
    command.output().expect("run scala-rs compile")
}

#[test]
fn generic_parent_projection_round_trips_with_concrete_element_type() {
    let Some(library) = scala_library_jar() else {
        eprintln!("skip TableQuery projection round trip: scala-library unavailable");
        return;
    };

    let root = temp_dir();
    let writer = root.join("writer");
    let reader = root.join("reader");
    fs::create_dir_all(&writer).unwrap();
    fs::create_dir_all(&reader).unwrap();

    let lib = compile(
        &fixtures_dir().join("tablequery_parent_lib.scala"),
        &writer,
        None,
        &library,
    );
    assert!(
        lib.status.success(),
        "scala-rs writer failed:\n{}",
        diagnostics(&lib)
    );

    // The accessor is inherited from a trait and the mirror class receives a
    // matching static forwarder. Both must retain TableQuery[User] in the
    // JVM Signature attribute; the erased descriptor alone makes a separate
    // consumer infer an unknown table element type.
    assert_default_users_signature(&javap(&writer, "tablequeryparent.UserTables"));
    assert_users_signature(&javap(&writer, "tablequeryparent.UserTableCatalog$"), false);
    assert_users_signature(&javap(&writer, "tablequeryparent.UserTableCatalog"), true);

    let use_stage = compile(
        &fixtures_dir().join("tablequery_parent_use.scala"),
        &reader,
        Some(&writer),
        &library,
    );
    assert!(
        use_stage.status.success(),
        "scala-rs reader failed:\n{}",
        diagnostics(&use_stage)
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn scalac_binary_parent_projection_is_adopted_lazily() {
    let (Some(library), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip scalac binary TableQuery projection: compiler unavailable");
        return;
    };

    let root = temp_dir();
    let writer = root.join("writer");
    let reader = root.join("reader");
    fs::create_dir_all(&writer).unwrap();
    fs::create_dir_all(&reader).unwrap();

    let lib = Command::new(scalac)
        .arg(fixtures_dir().join("tablequery_binary_lib.scala"))
        .args(["-d", writer.to_str().unwrap()])
        .output()
        .expect("run scalac");
    assert!(
        lib.status.success(),
        "scalac writer failed:\n{}",
        diagnostics(&lib)
    );

    let use_stage = compile(
        &fixtures_dir().join("tablequery_binary_use.scala"),
        &reader,
        Some(&writer),
        &library,
    );
    assert!(
        use_stage.status.success(),
        "scala-rs reader failed:\n{}",
        diagnostics(&use_stage)
    );

    let _ = fs::remove_dir_all(root);
}

/// GitBucket compiles in three layers: Slick first, its nested table classes
/// second, and the test sources last. The projected `TableElementType` must
/// therefore survive two independent scala-rs writer/reader boundaries.
#[test]
fn nested_table_projection_survives_two_binary_boundaries() {
    let (Some(library), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip layered TableQuery projection: compiler unavailable");
        return;
    };

    let root = temp_dir();
    let base = root.join("base");
    let base_jar = root.join("base.jar");
    let model = root.join("model");
    let consumer = root.join("consumer");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&model).unwrap();
    fs::create_dir_all(&consumer).unwrap();

    let base_stage = Command::new(scalac)
        .arg(fixtures_dir().join("tablequery_projected_base.scala"))
        .args(["-d", base.to_str().unwrap()])
        .output()
        .expect("run scalac");
    assert!(
        base_stage.status.success(),
        "scalac base writer failed:\n{}",
        diagnostics(&base_stage)
    );
    let jar_stage = Command::new("jar")
        .args(["--create", "--file"])
        .arg(&base_jar)
        .args(["-C", base.to_str().unwrap(), "."])
        .output()
        .expect("package base classes");
    assert!(
        jar_stage.status.success(),
        "jar packaging failed:\n{}",
        diagnostics(&jar_stage)
    );

    let model_stage = compile(
        &fixtures_dir().join("tablequery_projected_lib.scala"),
        &model,
        Some(&base_jar),
        &library,
    );
    assert!(
        model_stage.status.success(),
        "scala-rs model writer failed:\n{}",
        diagnostics(&model_stage)
    );

    let layered_cp = PathBuf::from(format!("{}:{}", base_jar.display(), model.display()));
    let consumer_stage = compile(
        &fixtures_dir().join("tablequery_projected_use.scala"),
        &consumer,
        Some(&layered_cp),
        &library,
    );
    assert!(
        consumer_stage.status.success(),
        "scala-rs layered reader failed:\n{}",
        diagnostics(&consumer_stage)
    );

    let _ = fs::remove_dir_all(root);
}
