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
    // Private-runtime fixtures must not auto-link a discovered scala-library jar.
    compile_fixture_with(name, &["--no-scala-library"])
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
    assert!(s.contains("--no-scala-library"));
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

/// `compile` with no flags should auto-link a discovered scala-library jar
/// (same as `run`), so the private runtime is not emitted.
#[test]
fn compile_auto_links_discovered_scala_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip compile autodetect: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("hello.scala");
    let out = tmp_dir("compile-autolink");
    let status = Command::new(bin())
        .args(["compile", src.to_str().unwrap(), "-d", out.to_str().unwrap()])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile (auto-link) failed: {status}");
    assert_no_private_stdlib(&out);
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -cp out:scala-library failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("hello")
    );
    let _ = fs::remove_dir_all(&out);
}

/// `--no-scala-library` must still emit the private runtime even when a jar
/// would otherwise be auto-found.
#[test]
fn compile_no_scala_library_emits_private_runtime() {
    let src = fixtures_dir().join("hello.scala");
    let out = tmp_dir("compile-private");
    let status = Command::new(bin())
        .args([
            "compile",
            "--no-scala-library",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile --no-scala-library");
    assert!(
        status.success(),
        "compile --no-scala-library failed: {status}"
    );
    assert!(
        out.join("scala/Option.class").is_file(),
        "expected private scala/Option.class under {}",
        out.display()
    );
    let _ = fs::remove_dir_all(&out);
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
fn fixtures_anonymous() {
    check("anonymous");
}
#[test]
fn fixtures_eta() {
    check("eta");
}
#[test]
fn fixtures_existentials() {
    check("existentials");
}
#[test]
fn fixtures_implicit_specific() {
    check("implicit_specific");
}
#[test]
fn fixtures_lambda_lift() {
    check("lambda_lift");
}
#[test]
fn fixtures_view_bounds() {
    check("view_bounds");
}
#[test]
fn fixtures_implicit_inherited() {
    check("implicit_inherited");
}
#[test]
fn fixtures_implicit_nested() {
    check("implicit_nested");
}
#[test]
fn fixtures_defaults_still_run() {
    check("defaults");
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

fn compile_fails(name: &str, needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
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
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_implicit_ambiguous_is_error() {
    compile_fails("implicit_ambiguous", "ambiguous implicit");
}

#[test]
fn fixtures_implicit_ambiguous_parents_is_error() {
    compile_fails("implicit_ambiguous_parents", "ambiguous implicit");
}

#[test]
fn fixtures_existential_bounds_is_error() {
    compile_fails("existential_bounds", "unimplemented");
}

#[test]
fn fixtures_view_bounds_class_is_error() {
    compile_fails("view_bounds_class", "view bound");
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

#[test]
fn scala_library_dual_run_unapply_seq() {
    dual_run_fixture("unapply_seq");
}

#[test]
fn scala_library_dual_run_iterator() {
    dual_run_fixture("iterator");
}

#[test]
fn scala_library_dual_run_predef_more() {
    dual_run_fixture("predef_more");
}

#[test]
fn scala_library_dual_run_map() {
    dual_run_fixture("map");
}

#[test]
fn scala_library_dual_run_vector() {
    dual_run_fixture("vector");
}

#[test]
fn scala_library_dual_run_int_ops() {
    dual_run_fixture("int_ops");
}

#[test]
fn scala_library_dual_run_string_ops() {
    dual_run_fixture("string_ops");
}

#[test]
fn scala_library_dual_run_list_apply() {
    dual_run_fixture("list_apply");
}

#[test]
fn scala_library_dual_run_set() {
    dual_run_fixture("set");
}

#[test]
fn scala_library_dual_run_long_ops() {
    dual_run_fixture("long_ops");
}

#[test]
fn scala_library_dual_run_seq() {
    dual_run_fixture("seq");
}

#[test]
fn scala_library_dual_run_either() {
    dual_run_fixture("either");
}

#[test]
fn scala_library_dual_run_float_ops() {
    dual_run_fixture("float_ops");
}

#[test]
fn scala_library_dual_run_string_ops2() {
    dual_run_fixture("string_ops2");
}

#[test]
fn scala_library_dual_run_anonymous() {
    dual_run_fixture("anonymous");
}

#[test]
fn scala_library_dual_run_eta() {
    dual_run_fixture("eta");
}

#[test]
fn scala_library_dual_run_try_util() {
    dual_run_fixture("try_util");
}

#[test]
fn scala_library_dual_run_existentials() {
    dual_run_fixture("existentials");
}

#[test]
fn scala_library_dual_run_implicit_specific() {
    dual_run_fixture("implicit_specific");
}

#[test]
fn scala_library_dual_run_lambda_lift() {
    dual_run_fixture("lambda_lift");
}

#[test]
fn scala_library_dual_run_view_bounds() {
    dual_run_fixture("view_bounds");
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
    "scala/Predef$.class",
    "scala/collection/StringOps.class",
    "scala/collection/WithFilter.class",
    "scala/collection/Iterator.class",
    "scala/Option$WithFilter.class",
    "scala/collection/immutable/Map.class",
    "scala/collection/immutable/Map$.class",
    "scala/collection/immutable/Vector.class",
    "scala/collection/immutable/Vector$.class",
    "scala/Predef$any2stringadd.class",
    "scala/Predef$ArrowAssoc.class",
    "scala/runtime/RichInt.class",
    "scala/runtime/RichLong.class",
    "scala/runtime/RichDouble.class",
    "scala/runtime/RichChar.class",
    "scala/collection/immutable/Range.class",
    "scala/collection/immutable/Set.class",
    "scala/collection/immutable/Set$.class",
    "scala/collection/immutable/Seq.class",
    "scala/collection/immutable/Seq$.class",
    "scala/collection/immutable/LazyList.class",
    "scala/collection/immutable/LazyList$.class",
    "scala/runtime/RichFloat.class",
    "scala/util/Either.class",
    "scala/util/Left.class",
    "scala/util/Right.class",
    "scala/util/Left$.class",
    "scala/util/Right$.class",
    "scala/util/Try.class",
    "scala/util/Try$.class",
    "scala/util/Success.class",
    "scala/util/Success$.class",
    "scala/util/Failure.class",
    "scala/util/Failure$.class",
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
fn scala_library_flag_without_path_uses_env() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip autodetect: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("hello.scala");
    let out = tmp_dir("autodetect");
    let status = Command::new(bin())
        .env("SCALA_LIBRARY_JAR", &jar)
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
        ])
        .status()
        .expect("compile --scala-library without path");
    assert!(status.success(), "autodetect --scala-library failed");
    assert_no_private_stdlib(&out);
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
fn cli_run_uses_auto_found_scala_library() {
    if !java_available() {
        return;
    }
    let Some(_) = scala_library_jar() else {
        eprintln!("skip run autodetect: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("int_ops.scala");
    let output = Command::new(bin())
        .args(["run", src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "run without --scala-library should use auto-found jar: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("int_ops")
    );
}

#[test]
fn cli_run_no_scala_library_uses_private_runtime() {
    if !java_available() {
        return;
    }
    let src = fixtures_dir().join("hello.scala");
    let output = Command::new(bin())
        .args(["run", "--no-scala-library", src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "run --no-scala-library failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

#[test]
fn scala_signature_on_compiled_object() {
    let out = compile_fixture("hello");
    if !javap_available() {
        let _ = fs::remove_dir_all(&out);
        return;
    }
    let output = Command::new("javap")
        .args(["-v", "-p", out.join("Main$.class").to_str().unwrap()])
        .output()
        .expect("javap");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("ScalaSignature") && text.contains("bytes"),
        "expected ScalaSignature annotation in javap -v, got {text}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn separate_compilation_against_classfiles() {
    if !java_available() {
        return;
    }
    let lib_src = fixtures_dir().join("separate_lib.scala");
    let main_src = fixtures_dir().join("separate_main.scala");
    let out_lib = tmp_dir("separate-lib");
    let out_main = tmp_dir("separate-main");
    let status = Command::new(bin())
        .args([
            "compile",
            "--no-scala-library",
            lib_src.to_str().unwrap(),
            "-d",
            out_lib.to_str().unwrap(),
        ])
        .status()
        .expect("compile Lib");
    assert!(status.success(), "compile separate_lib failed");
    assert!(
        out_lib.join("Lib$.class").is_file(),
        "Lib$.class missing in {}",
        out_lib.display()
    );
    let status = Command::new(bin())
        .args([
            "compile",
            "--no-scala-library",
            main_src.to_str().unwrap(),
            "-d",
            out_main.to_str().unwrap(),
            "-cp",
            out_lib.to_str().unwrap(),
        ])
        .status()
        .expect("compile Main against Lib classfiles");
    assert!(
        status.success(),
        "compile separate_main against Lib classfiles failed"
    );
    let cp = format!("{}:{}", out_main.display(), out_lib.display());
    let output = Command::new("java")
        .args(["-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("separate")
    );
    let _ = fs::remove_dir_all(&out_lib);
    let _ = fs::remove_dir_all(&out_main);
}

fn classfile_major(path: &Path) -> Option<u16> {
    let b = fs::read(path).ok()?;
    if b.len() < 8 || b[0..4] != [0xca, 0xfe, 0xba, 0xbe] {
        return None;
    }
    Some(u16::from_be_bytes([b[6], b[7]]))
}

#[test]
fn classfiles_are_java8_major_52() {
    let out = compile_fixture("while_loop");
    let main = out.join("Main$.class");
    let major = classfile_major(&main).expect("read classfile major");
    assert_eq!(major, 52, "expected Java 8 classfile major 52, got {major}");
    if java_available() {
        let output = Command::new("java")
            .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
            .output()
            .expect("java -Xverify:all");
        assert!(
            output.status.success(),
            "java -Xverify:all failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected_stdout("while_loop")
        );
    }
    if javap_available() {
        let output = Command::new("javap")
            .args(["-v", "-p", main.to_str().unwrap()])
            .output()
            .expect("javap");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            text.contains("StackMapTable") || text.contains("stack_map"),
            "expected StackMapTable in while_loop Main$, got {text}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    None
}

/// scalac 2.13 against our classfiles, if `scalac` is on PATH.
/// This environment does not ship scalac; see README.
#[test]
fn scalac_typechecks_against_our_classfiles_if_present() {
    let Some(scalac) = find_scalac() else {
        eprintln!(
            "scalac not installed; skipping scalac-vs-our-classfiles (documented in README)"
        );
        return;
    };
    if !java_available() {
        return;
    }
    let lib_src = fixtures_dir().join("separate_lib.scala");
    let out_lib = tmp_dir("scalac-cp-lib");
    let status = Command::new(bin())
        .args([
            "compile",
            "--no-scala-library",
            lib_src.to_str().unwrap(),
            "-d",
            out_lib.to_str().unwrap(),
        ])
        .status()
        .expect("compile Lib for scalac");
    assert!(status.success());
    let probe = tmp_dir("scalac-probe");
    let src = probe.join("UseLib.scala");
    fs::write(
        &src,
        r#"
object UseLib {
  def main(args: Array[String]): Unit = {
    val s: String = Lib.greet("Scala")
    val n: Int = Lib.magic
    val x: Int = Lib.id(42)
    val b: String = new Box("hi").get
  }
}
"#,
    )
    .unwrap();
    let output = Command::new(&scalac)
        .args([
            "-classpath",
            out_lib.to_str().unwrap(),
            "-d",
            probe.to_str().unwrap(),
            src.to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    assert!(
        output.status.success(),
        "scalac failed to typecheck against our classfiles: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out_lib);
    let _ = fs::remove_dir_all(&probe);
}
