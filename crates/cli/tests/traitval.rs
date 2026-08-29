//! Runtime representation of a trait's `val` / `var`, and `case object`'s
//! synthetic members. Kept in its own file so it does not collide with the
//! parallel work landing in `e2e.rs`.

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
    // Two tests can share a tag, and the clock is not fine enough to
    // separate them: they ran in the same directory and each `java Main` saw
    // the other's half-written output.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-traitval-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Private-runtime mode: `--no-scala-library`.
fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout(name),
            "stdout mismatch for private-runtime {name}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Library-ABI mode: linked against the real scala-library 2.13.16 jar. The
/// expected file is real scalac's own output for the same source.
fn check_library(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
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

/// Reads the disassembly of one emitted class.
fn javap(out: &Path, class: &str) -> String {
    let output = Command::new("javap")
        .args(["-p", "-c", "-cp", out.to_str().unwrap(), class])
        .output()
        .expect("javap");
    assert!(
        output.status.success(),
        "javap {class} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn fixtures_tval_private_runtime() {
    check_private("tval");
}

#[test]
fn fixtures_tval_scala_library() {
    check_library("tval");
}

#[test]
fn fixtures_tval_bad_is_error() {
    compile_fails("tval_bad", "reassignment to val");
}

/// nsc names a trait `val`'s mixin setter after the owning trait
/// (`Named$_setter_$label_$eq`) and a trait `var`'s setter `n_$eq`. A class
/// that `override val`s one still implements the mixin setter — as a no-op, so
/// `Named$class.$init$` cannot clobber the override.
#[test]
fn trait_val_setters_follow_nsc_names() {
    let out = compile_fixture_with("tval", &["--no-scala-library"]);
    let named = javap(&out, "Named");
    assert!(
        named.contains("Named$_setter_$label_$eq"),
        "trait interface should declare nsc's mixin setter, got:\n{named}"
    );

    let plain = javap(&out, "Plain");
    assert!(
        plain.contains("Named$_setter_$label_$eq"),
        "implementing class should define the mixin setter, got:\n{plain}"
    );
    assert!(
        plain.contains("putfield"),
        "the mixin setter should store the field, got:\n{plain}"
    );

    // `Renamed override val label`: the setter is present but stores nothing.
    let renamed = javap(&out, "Renamed");
    let body = renamed
        .split("public void Named$_setter_$label_$eq")
        .nth(1)
        .unwrap_or_else(|| panic!("Renamed should still define the mixin setter:\n{renamed}"))
        .split("\n\n")
        .next()
        .unwrap_or("");
    assert!(
        !body.contains("putfield") && body.contains("return"),
        "an overridden trait val must get a no-op mixin setter, got:\n{body}"
    );

    // A trait `var` is a plain `n_$eq`, called (not `putfield`ed) from the
    // trait's own concrete methods.
    let counted = javap(&out, "Counted$class");
    assert!(
        counted.contains("count_$eq"),
        "a trait method assigning a `var` must call the setter, got:\n{counted}"
    );
    assert!(
        !counted.contains("putfield"),
        "a trait's static impl has no field to store into, got:\n{counted}"
    );

    let _ = fs::remove_dir_all(&out);
}

/// `case object Asc` gets nsc's constant-folded `toString` / `productPrefix` /
/// `hashCode` on the module class.
#[test]
fn case_object_members_are_on_the_module_class() {
    let out = compile_fixture_with("tval", &["--no-scala-library"]);
    let asc = javap(&out, "Asc$");
    for m in [
        "java.lang.String toString();",
        "java.lang.String productPrefix();",
        "int hashCode();",
        "int productArity();",
    ] {
        assert!(asc.contains(m), "Asc$ should define {m}, got:\n{asc}");
    }
    assert!(
        asc.contains("String Asc"),
        "toString/productPrefix should be folded to the object's name, got:\n{asc}"
    );
    let _ = fs::remove_dir_all(&out);
}
