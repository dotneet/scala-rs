//! Macro implementations run with the compiler's own scala-reflect and
//! scala-compiler, as nsc runs them, without the user listing either jar.
//!
//! shapeless's `Generic` and `Lazy` macros reach into scala-compiler
//! (`scala.tools.nsc.Global`). A scalac user never puts that jar on the
//! classpath, and with only shapeless there the derivation below failed here
//! with nothing but "could not find implicit value of type Enc[R0]".

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-macro-runtime-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn shapeless_jar() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let jar = home.join(
        "Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/com/chuusai/shapeless_2.13/2.3.13/shapeless_2.13-2.3.13.jar",
    );
    jar.is_file().then_some(jar)
}

fn run_java(out: &Path, cp: &str) -> String {
    let o = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            "sg.Main",
        ])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "sg.Main failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn shapeless_derivation_needs_no_compiler_jars_on_the_classpath() {
    let (Some(jar), Some(shapeless)) = (scala_library_jar(), shapeless_jar()) else {
        eprintln!("skip: scala-library or shapeless 2.3.13 not present");
        return;
    };
    let src = fixtures_dir().join("macro_runtime_shapeless.scala");
    let ours = tmp_dir("ours");
    let o = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .arg("-d")
        .arg(&ours)
        .arg("-cp")
        .arg(&shapeless)
        .arg("--scala-library")
        .arg(&jar)
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "scala-rs rejected the derivation: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let run_cp = format!("{}:{}", shapeless.display(), jar.display());
    let actual = run_java(&ours, &run_cp);
    let _ = fs::remove_dir_all(&ours);
    let expected =
        fs::read_to_string(fixtures_dir().join("expected/macro_runtime_shapeless.txt")).unwrap();
    if let Some(scalac) = scalac() {
        let theirs = tmp_dir("scalac");
        let o = Command::new(scalac)
            .env("JAVA_OPTS", "-Xmx2g -Xss8m")
            .arg("-cp")
            .arg(&shapeless)
            .arg("-d")
            .arg(&theirs)
            .arg(&src)
            .output()
            .expect("run scalac");
        assert!(o.status.success(), "scalac rejected the derivation");
        assert_eq!(run_java(&theirs, &run_cp), expected);
        let _ = fs::remove_dir_all(&theirs);
    }
    assert_eq!(actual, expected);
}

/// Where no compiler jar of the library's version can be found at all, the
/// search that needed the macro says why it failed rather than only that it
/// found nothing.
#[test]
fn a_macro_that_cannot_run_is_named_in_the_missing_implicit() {
    let (Some(jar), Some(shapeless)) = (scala_library_jar(), shapeless_jar()) else {
        eprintln!("skip: scala-library or shapeless 2.3.13 not present");
        return;
    };
    // A version nothing on the machine has the compiler jars of.
    let lib_dir = tmp_dir("lib");
    let lib = lib_dir.join("scala-library-2.13.99.jar");
    fs::copy(&jar, &lib).unwrap();
    let out = tmp_dir("out");
    let o = Command::new(bin())
        .arg("compile")
        .arg(fixtures_dir().join("macro_runtime_shapeless.scala"))
        .arg("-d")
        .arg(&out)
        .arg("-cp")
        .arg(&shapeless)
        .arg("--scala-library")
        .arg(&lib)
        .output()
        .expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let _ = fs::remove_dir_all(&lib_dir);
    let _ = fs::remove_dir_all(&out);
    assert!(!o.status.success());
    assert!(
        text.contains("could not find implicit value of type Enc[R0]")
            && text.contains("no macro could be expanded to supply it")
            && text.contains("scala-reflect.jar is not on the classpath"),
        "{text}"
    );
}
