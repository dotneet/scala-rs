//! Regression tests for JVM-valid parent constructor calls with defaults.

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

fn tmp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("scala-rs-vsql-{nanos}-{}", std::process::id()))
}

fn run_java(out: &Path) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("run java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn temurin17_home() -> Option<PathBuf> {
    let home = PathBuf::from("/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home");
    home.join("bin/java").is_file().then_some(home)
}

fn scalac() -> Option<PathBuf> {
    let scalac = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    scalac.is_file().then_some(scalac)
}

fn jdk17_command(cmd: &mut Command, home: &Path) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut jdk_path = home.join("bin").into_os_string();
    jdk_path.push(":");
    jdk_path.push(path);
    cmd.env("JAVA_HOME", home).env("PATH", jdk_path);
}

fn compile_scalac(src: &Path, out: &Path, cp: &[&Path], home: &Path, jar: &Path) {
    compile_scalac_with_flags(src, out, cp, home, jar, &[]);
}

fn compile_scalac_with_flags(
    src: &Path,
    out: &Path,
    cp: &[&Path],
    home: &Path,
    jar: &Path,
    flags: &[&str],
) {
    let mut cmd = Command::new(scalac().unwrap());
    let classpath = cp
        .iter()
        .map(|p| p.to_str().unwrap())
        .chain(std::iter::once(jar.to_str().unwrap()))
        .collect::<Vec<_>>()
        .join(":");
    cmd.args(flags).args([
        "-classpath",
        &classpath,
        "-d",
        out.to_str().unwrap(),
        src.to_str().unwrap(),
    ]);
    jdk17_command(&mut cmd, home);
    let output = cmd.output().expect("run scalac");
    assert!(
        output.status.success(),
        "scalac failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn compile_scala_rs_output(
    src: &Path,
    out: &Path,
    cp: &[&Path],
    jar: &Path,
) -> std::process::Output {
    let mut cmd = Command::new(bin());
    let classpath = cp
        .iter()
        .map(|p| p.to_str().unwrap())
        .collect::<Vec<_>>()
        .join(":");
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
        "-cp",
        &classpath,
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    cmd.output().expect("run scala-rs compile")
}

fn compile_scala_rs(src: &Path, out: &Path, cp: &[&Path], jar: &Path) {
    let output = compile_scala_rs_output(src, out, cp, jar);
    assert!(
        output.status.success(),
        "scala-rs failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_java17(out: &Path, parents: &[&Path], jar: &Path, home: &Path) -> String {
    let classpath = std::iter::once(out)
        .chain(parents.iter().copied())
        .chain(std::iter::once(jar))
        .map(|p| p.to_str().unwrap())
        .collect::<Vec<_>>()
        .join(":");
    let mut cmd = Command::new(home.join("bin/java"));
    cmd.args(["-Xverify:all", "-cp", &classpath, "Main"]);
    let output = cmd.output().expect("run java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn parent_default_constructor_is_verified() {
    let out = tmp_dir();
    fs::create_dir_all(&out).unwrap();
    let src = fixtures_dir().join("vsql_parent.scala");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out), "jdbc:test:user:password\n");
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn qualified_library_class_term_uses_companion() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let out = tmp_dir();
    fs::create_dir_all(&out).unwrap();
    let src = fixtures_dir().join("vsql_factory.scala");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let run = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{}", out.display(), jar.display()),
            "Main",
        ])
        .output()
        .expect("run java");
    assert!(
        run.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "scala.collection.Factory$:scala.collection.immutable.LazyList$\n"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn parent_defaults_interoperate_across_scalac_and_scala_rs() {
    let (Some(jar), Some(home), Some(_)) = (scala_library_jar(), temurin17_home(), scalac()) else {
        return;
    };
    let root = tmp_dir();
    let nsc_base = root.join("nsc-base");
    let rs_child = root.join("rs-child");
    let rs_base = root.join("rs-base");
    let nsc_child = root.join("nsc-child");
    for out in [&nsc_base, &rs_child, &rs_base, &nsc_child] {
        fs::create_dir_all(out).unwrap();
    }
    let fixtures = fixtures_dir();
    compile_scalac(
        &fixtures.join("vsql_nsc_base.scala"),
        &nsc_base,
        &[],
        &home,
        &jar,
    );
    compile_scala_rs(
        &fixtures.join("vsql_rs_child.scala"),
        &rs_child,
        &[&nsc_base],
        &jar,
    );
    assert_eq!(
        run_java17(&rs_child, &[&nsc_base], &jar, &home),
        "jdbc:nsc:user:password\n"
    );

    compile_scala_rs(&fixtures.join("vsql_rs_base.scala"), &rs_base, &[], &jar);
    compile_scalac(
        &fixtures.join("vsql_nsc_child.scala"),
        &nsc_child,
        &[&rs_base],
        &home,
        &jar,
    );
    assert_eq!(
        run_java17(&nsc_child, &[&rs_base], &jar, &home),
        "jdbc:rs:user:password\n"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn external_constructor_defaults_are_typed_and_companion_backed() {
    let (Some(jar), Some(home), Some(_)) = (scala_library_jar(), temurin17_home(), scalac()) else {
        return;
    };
    let root = tmp_dir();
    let nsc_base = root.join("nsc-base");
    let rs_child = root.join("rs-child");
    let rs_bad = root.join("rs-bad");
    let rs_overload_bad = root.join("rs-overload-bad");
    for out in [&nsc_base, &rs_child, &rs_bad, &rs_overload_bad] {
        fs::create_dir_all(out).unwrap();
    }
    let fixtures = fixtures_dir();
    // `-Xno-forwarders` leaves constructor default getters only on the
    // companion, which also covers a nested class whose classfile has no
    // ScalaSignature of its own.
    compile_scalac_with_flags(
        &fixtures.join("vsql_external_ctor_base.scala"),
        &nsc_base,
        &[],
        &home,
        &jar,
        &["-Xno-forwarders"],
    );
    compile_scala_rs(
        &fixtures.join("vsql_external_ctor_child.scala"),
        &rs_child,
        &[&nsc_base],
        &jar,
    );
    assert_eq!(
        run_java17(&rs_child, &[&nsc_base], &jar, &home),
        "42\n42\ncurried:42\n42\n"
    );

    // The getter's result is checked against the substituted constructor
    // parameter type. `Int` must not be silently accepted as `String`.
    let bad = compile_scala_rs_output(
        &fixtures.join("vsql_external_ctor_bad.scala"),
        &rs_bad,
        &[&nsc_base],
        &jar,
    );
    assert!(
        !bad.status.success(),
        "invalid generic default was accepted"
    );
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert!(
        diagnostics.contains("found: Int") && diagnostics.contains("required: String"),
        "unexpected diagnostic: {diagnostics}"
    );

    let overload_bad = compile_scala_rs_output(
        &fixtures.join("vsql_external_ctor_overload_bad.scala"),
        &rs_overload_bad,
        &[&nsc_base],
        &jar,
    );
    assert!(!overload_bad.status.success());
    let overload_diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&overload_bad.stdout),
        String::from_utf8_lossy(&overload_bad.stderr)
    );
    assert!(
        overload_diagnostics.contains("no matching overload for constructor VSqlOverloadedBase"),
        "unexpected overload diagnostic: {overload_diagnostics}"
    );
    let _ = fs::remove_dir_all(&root);
}
