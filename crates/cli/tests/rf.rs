//! Remaining `run` failures of the scala/scala corpus (the `agent/runfail`
//! slice).
//!
//! Fixture prefix: `rf_`. Every fixture here needs `scala-reflect.jar` on the
//! class path, the way `tests/scala_corpus.sh` runs the corpus's own `run`
//! tests, so the helpers add it.

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
        "scala-rs-rf-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

/// `scala-reflect.jar` and `scala-compiler.jar`, the two the corpus harness
/// adds for its reflection tests.
fn reflect_jars() -> Option<(PathBuf, PathBuf)> {
    let r = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    let c = PathBuf::from("/tmp/scala-2.13.16/lib/scala-compiler.jar");
    (r.is_file() && c.is_file()).then_some((r, c))
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{cp_extra}", out.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `name` with scala-rs (or with real scalac when `scalac_path` is
/// given), run it, and compare with the expected output.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some((reflect, compiler)) = reflect_jars() else {
        eprintln!("skip: scala-reflect / scala-compiler jars not present");
        return;
    };
    let cp = format!(
        "{}:{}:{}",
        jar.display(),
        reflect.display(),
        compiler.display()
    );
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = match scalac_path {
        Some(sc) => Command::new(sc)
            .args(["-classpath", &cp, "-d", out.to_str().unwrap()])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["-cp", &cp])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    };
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &cp), expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rf_booleanflag_runs() {
    check_runs("rf_booleanflag", None);
}

#[test]
fn scalac_agrees_rf_booleanflag() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("rf_booleanflag", Some(&sc));
}

/// Compile one fixture with scala-rs (or real scalac), `extra` on the class
/// path. Used for the two-stage macro fixtures, where the implementation has to
/// be a *separate run* from the call sites: that is the only arrangement in
/// which a macro expands at all.
fn compile_stage(
    name: &str,
    out: &Path,
    extra: &[&Path],
    scalac_path: Option<&Path>,
) -> std::process::Output {
    let jar = scala_library_jar().expect("scala-library");
    let (reflect, compiler) = reflect_jars().expect("reflect jars");
    let mut cp = format!("{}:{}", reflect.display(), compiler.display());
    for e in extra {
        cp.push(':');
        cp.push_str(&e.display().to_string());
    }
    let src = fixtures_dir().join(format!("{name}.scala"));
    match scalac_path {
        Some(sc) => Command::new(sc)
            .args(["-cp", &cp, "-d", out.to_str().unwrap()])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["-cp", &cp])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    }
}

fn runtime_classpath(dirs: &[&Path]) -> String {
    let jar = scala_library_jar().unwrap();
    let (reflect, compiler) = reflect_jars().unwrap();
    let mut cp = String::new();
    for d in dirs {
        cp.push_str(&d.display().to_string());
        cp.push(':');
    }
    format!(
        "{cp}{}:{}:{}",
        reflect.display(),
        compiler.display(),
        jar.display()
    )
}

/// `rf_wbimpl.scala` then `rf_wbuse.scala`: four whitebox macros whose
/// expansions are typed more precisely than their declared `Any`, and one
/// blackbox macro with the same expansion that is not.
fn whitebox_two_stage(scalac_path: Option<&Path>) {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    if reflect_jars().is_none() {
        eprintln!("skip: scala-reflect / scala-compiler jars not present");
        return;
    }
    let impls = tmp_dir("wbimpl");
    let uses = tmp_dir("wbuse");
    let out = compile_stage("rf_wbimpl", &impls, &[], scalac_path);
    assert!(
        out.status.success(),
        "compile rf_wbimpl failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let out = compile_stage("rf_wbuse", &uses, &[&impls], scalac_path);
    assert!(
        out.status.success(),
        "compile rf_wbuse failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let cp = runtime_classpath(&[&uses, &impls]);
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        run.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join("rf_wbuse.txt")).unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);

    // The other direction: the blackbox declaration beside them keeps `Any`, so
    // narrowing it at the call site must stay an error. A whitebox rule that
    // applied to every macro would accept this.
    let bad = tmp_dir("wbbad");
    let out = compile_stage("rf_wbbad", &bad, &[&impls], scalac_path);
    assert!(
        !out.status.success(),
        "rf_wbbad.scala was accepted; a blackbox expansion's type must not reach \
         the call site:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        said.contains("type mismatch"),
        "rf_wbbad.scala was rejected for the wrong reason: {said}"
    );

    let _ = fs::remove_dir_all(&impls);
    let _ = fs::remove_dir_all(&uses);
    let _ = fs::remove_dir_all(&bad);
}

#[test]
fn rf_whitebox_expansions_replace_the_declared_type() {
    whitebox_two_stage(None);
}

#[test]
fn scalac_agrees_rf_whitebox() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    whitebox_two_stage(Some(&sc));
}
