//! Value classes with universal traits, the newline rule around `}` and a
//! following unary minus, and `X.type` resolution when the name is shadowed in
//! the type namespace. Kept in its own file so it does not collide with
//! `e2e.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
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
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-vcls-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn javap_available() -> bool {
    Command::new("javap")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn run_java(out: &Path, main: &str, cp_extra: Option<&Path>) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all {main} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile in both modes (private runtime and the real scala-library) and
/// check the program prints what nsc's build of it prints.
fn check_both_modes(name: &str, main: &str) {
    if !java_available() {
        return;
    }
    let expected = expected_stdout(name);
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, main, None),
        expected,
        "private runtime {name}"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, main, Some(&jar)),
        expected,
        "library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn check_library_mode(name: &str, main: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, main, Some(&jar)),
        expected_stdout(name),
        "library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn diagnostics(name: &str) -> String {
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
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{name} unexpectedly compiled");
    let _ = fs::remove_dir_all(&out);
    text
}

fn javap(out: &Path, class: &str) -> String {
    let output = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    assert!(output.status.success(), "javap {class} failed");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn fixtures_vcls() {
    check_both_modes("vcls", "Main");
}

#[test]
fn fixtures_vcls_newline_rules() {
    check_both_modes("vcls_nl", "Main");
}

#[test]
fn fixtures_vcls_arrays_and_collections() {
    // `Array.apply`/`List.apply` need the real library.
    check_library_mode("vcls_arr", "Main");
}

#[test]
fn fixtures_vcls_hnil_type() {
    check_library_mode("vcls_hnil", "hl.Main");
}

#[test]
fn vcls_bad_is_error() {
    let text = diagnostics("vcls_bad");
    for needle in [
        "value n is not a member of Univ",
        "value missingMember is not a member of Meters",
        "stable identifier required, but notStable found",
    ] {
        assert!(
            text.contains(needle),
            "expected {needle:?} in diagnostics, got:\n{text}"
        );
    }
}

/// nsc's shape for `final class Meters(val n: Int) extends AnyVal with Univ`:
/// the class really implements the interface, the methods have `$extension`
/// static twins over the underlying value, and `equals`/`hashCode` come from
/// that value rather than from object identity.
#[test]
fn vcls_classfile_matches_nsc_shape() {
    if !javap_available() {
        return;
    }
    let out = compile_fixture_with("vcls", &["--no-scala-library"]);
    let meters = javap(&out, "Meters");
    assert!(
        meters.contains("class Meters implements Univ"),
        "Meters must implement the universal trait:\n{meters}"
    );
    for sig in [
        "public java.lang.String describe();",
        "public static java.lang.String describe$extension(int);",
        "public static int plus$extension(int, int);",
        "public int hashCode();",
        "public static int hashCode$extension(int);",
        "public boolean equals(java.lang.Object);",
        "public static boolean equals$extension(int, java.lang.Object);",
    ] {
        assert!(meters.contains(sig), "missing {sig}:\n{meters}");
    }
    let univ = javap(&out, "Univ");
    assert!(
        univ.contains("interface Univ"),
        "Univ must be an interface:\n{univ}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The universal trait is reached through a real instance, so passing a value
/// class where the trait is expected has to box it as `new Meters(n)` -- an
/// `Integer` there is an `IncompatibleClassChangeError` at run time.
#[test]
fn vcls_boxes_into_the_value_class_not_its_underlying_box() {
    if !javap_available() {
        return;
    }
    let out = compile_fixture_with("vcls", &["--no-scala-library"]);
    let main = javap(&out, "Main$");
    let lines: Vec<&str> = main.lines().collect();
    let call = lines
        .iter()
        .position(|l| l.contains("twice:(LUniv;)"))
        .unwrap_or_else(|| panic!("no call to twice in:\n{main}"));
    let before = lines[call.saturating_sub(4)..call].join("\n");
    assert!(
        before.contains("// class Meters") && before.contains("Meters.\"<init>\":(I)V"),
        "the universal-trait argument must be `new Meters(n)`, got:\n{before}"
    );
    assert!(
        !before.contains("Integer.valueOf"),
        "the value class must not be boxed as an Integer:\n{before}"
    );
    let _ = fs::remove_dir_all(&out);
}
