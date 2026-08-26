//! End-to-end CLI tests against `tests/fixtures`.

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
    let p = std::env::temp_dir().join(format!("scala-rs-e2e-{tag}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
    assert!(
        out.join("Main$.class").is_file(),
        "Main$.class missing in {}",
        out.display()
    );
    out
}

fn compile_fixture(name: &str) -> PathBuf {
    compile_fixture_with(name, &[])
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_java(out: &Path) -> String {
    let output = Command::new("java")
        .args(["-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn check(name: &str) {
    let out = compile_fixture(name);
    if java_available() {
        let got = run_java(&out);
        let exp = expected_stdout(name);
        assert_eq!(got, exp, "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn cli_help() {
    let output = Command::new(bin()).arg("--help").output().unwrap();
    assert!(output.status.success());
    let s = String::from_utf8_lossy(&output.stdout);
    assert!(s.contains("compile"));
    assert!(s.contains("Scala 2.13"));
}

#[test]
fn fixtures_hello() {
    check("hello");
}
#[test]
fn fixtures_arithmetic() {
    check("arithmetic");
}
#[test]
fn fixtures_class_methods() {
    check("class_methods");
}
#[test]
fn fixtures_case_match() {
    check("case_match");
}
#[test]
fn fixtures_factorial() {
    check("factorial");
}
#[test]
fn fixtures_trait_impl() {
    check("trait_impl");
}
#[test]
fn fixtures_while_loop() {
    check("while_loop");
}
#[test]
fn fixtures_string_interp() {
    check("string_interp");
}
#[test]
fn fixtures_list_for() {
    check("list_for");
}
#[test]
fn fixtures_option_for() {
    check("option_for");
}
#[test]
fn fixtures_lazy_val() {
    check("lazy_val");
}
#[test]
fn fixtures_implicits() {
    check("implicits");
}
#[test]
fn fixtures_generic_id() {
    check("generic_id");
}
#[test]
fn fixtures_defaults() {
    check("defaults");
}
#[test]
fn fixtures_byname() {
    check("byname");
}
#[test]
fn fixtures_trait_concrete() {
    check("trait_concrete");
}
#[test]
fn fixtures_trait_linearize() {
    check("trait_linearize");
}
#[test]
fn fixtures_try_catch() {
    check("try_catch");
}
#[test]
fn fixtures_nested_class() {
    check("nested_class");
}
#[test]
fn fixtures_nested_object() {
    check("nested_object");
}
#[test]
fn fixtures_super() {
    check("super");
}
#[test]
fn fixtures_sealed_match() {
    check("sealed_match");
}
#[test]
fn fixtures_unapply() {
    check("unapply");
}
#[test]
fn fixtures_value_class() {
    check("value_class");
}
#[test]
fn fixtures_predef() {
    check("predef");
}
#[test]
fn fixtures_unapply_seq() {
    check("unapply_seq");
}
#[test]
fn fixtures_trait_val() {
    check("trait_val");
}
#[test]
fn fixtures_abstract_override() {
    check("abstract_override");
}
#[test]
fn fixtures_predef_more() {
    check("predef_more");
}
#[test]
fn fixtures_sealed_non_exhaustive_is_warning() {
    check("sealed_non_exhaustive");
}

#[test]
fn fatal_warnings_makes_non_exhaustive_fail() {
    let src = fixtures_dir().join("sealed_non_exhaustive.scala");
    let out = tmp_dir("fatal-warnings");
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "-Xfatal-warnings",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(!status.success(), "expected -Xfatal-warnings to fail");
    let _ = fs::remove_dir_all(&out);
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

#[test]
fn scala_library_dual_run_hello() {
    dual_run_fixture("hello");
}

#[test]
fn scala_library_dual_run_option_for() {
    dual_run_fixture("option_for");
}

#[test]
fn scala_library_dual_run_list_for() {
    dual_run_fixture("list_for");
}

#[test]
fn scala_library_dual_run_predef() {
    dual_run_fixture("predef");
}

#[test]
fn scala_library_dual_run_unapply() {
    dual_run_fixture("unapply");
}

const LIBRARY_COLLIDERS: &[&str] = &[
    "scala/Option.class",
    "scala/Some.class",
    "scala/Some$.class",
    "scala/None$.class",
    "scala/Function0.class",
    "scala/Function1.class",
    "scala/Tuple2.class",
    "scala/NotImplementedError.class",
    "scala/collection/immutable/List.class",
    "scala/collection/immutable/$colon$colon.class",
    "scala/collection/immutable/Nil$.class",
    "scala/collection/immutable/List$.class",
    "scala/runtime/ArrowAssoc.class",
];

fn assert_no_private_stdlib(out: &Path) {
    for rel in LIBRARY_COLLIDERS {
        let p = out.join(rel);
        assert!(
            !p.is_file(),
            "library ABI must not emit {} (would collide with scala-library.jar)",
            p.display()
        );
    }
}

fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_no_private_stdlib(&out);
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -cp out:scala-library failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn cli_run_hello() {
    if !java_available() {
        return;
    }
    let src = fixtures_dir().join("hello.scala");
    let output = Command::new(bin())
        .args(["run", src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("hello")
    );
}

#[test]
fn parse_dump_contains_module() {
    let src = fixtures_dir().join("hello.scala");
    let output = Command::new(bin())
        .args(["compile", src.to_str().unwrap(), "--parse"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let s = String::from_utf8_lossy(&output.stdout);
    assert!(s.contains("Module Main"), "{s}");
}
