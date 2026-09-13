//! A method-local definition lifted from a trait must stay private.
//!
//! Both traits below contain a local `loop` with the same erased descriptor and
//! are compiled as separate producer units, so each helper is named `loop$1`.
//! nsc writes these helpers as private interface methods. If scala-rs emits
//! them as public defaults, a real scalac consumer rejects the implementation
//! trait with an inherited name clash.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("{name}.scala"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "scala-rs-trait-local-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn scala_library_jar() -> Option<PathBuf> {
    let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    jar.is_file().then_some(jar)
}

fn scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

fn compile_rs(src: &Path, out: &Path, jar: &Path, cp: Option<&Path>) {
    let mut command = Command::new(bin());
    command.args([
        "compile",
        src.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    if let Some(cp) = cp {
        command.args(["-cp", cp.to_str().unwrap()]);
    }
    command.args(["-d", out.to_str().unwrap()]);
    let output = command.output().expect("scala-rs compile");
    assert!(
        output.status.success(),
        "scala-rs compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn local_trait_helpers_are_private_to_real_scalac_reader() {
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip local trait helper reader: scala-library or scalac unavailable");
        return;
    };
    let root = tmp_dir("reader");
    let lib = root.join("lib");
    let rs_use = root.join("rs-use");
    let use_out = root.join("use");
    fs::create_dir_all(&lib).unwrap();
    fs::create_dir_all(&rs_use).unwrap();
    fs::create_dir_all(&use_out).unwrap();
    compile_rs(&fixture("trait_local_applicative"), &lib, &jar, None);
    compile_rs(&fixture("trait_local_semigroupk"), &lib, &jar, None);

    // The same classfiles must remain usable by scala-rs's own separate
    // reader, and the private helpers must still be callable from their
    // trait defaults at runtime.
    compile_rs(&fixture("trait_local_use"), &rs_use, &jar, Some(&lib));
    let rs_cp = format!("{}:{}:{}", rs_use.display(), lib.display(), jar.display());
    let rs_run = Command::new("java")
        .args(["-Xverify:all", "-cp", &rs_cp, "Main"])
        .output()
        .expect("run scala-rs local trait reader probe");
    assert!(
        rs_run.status.success(),
        "scala-rs local trait consumer failed:\n{}",
        String::from_utf8_lossy(&rs_run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&rs_run.stdout),
        "List(1)\nList(2)\n"
    );

    let output = Command::new(&scalac)
        .args([
            fixture("trait_local_use").to_str().unwrap(),
            "-cp",
            &format!("{}:{}", lib.display(), jar.display()),
            "-d",
            use_out.to_str().unwrap(),
        ])
        .output()
        .expect("real scalac local trait reader");
    assert!(
        output.status.success(),
        "scalac rejected scala-rs local trait helpers:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let cp = format!("{}:{}:{}", use_out.display(), lib.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run local trait reader probe");
    assert!(
        run.status.success(),
        "real-scalac local trait consumer failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "List(1)\nList(2)\n");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn protected_this_trait_method_remains_a_super_target() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip protected[this] super target: scala-library unavailable");
        return;
    };
    let root = tmp_dir("protected-this-super");
    let out = root.join("out");
    fs::create_dir_all(&out).unwrap();
    compile_rs(&fixture("trait_protected_this_super"), &out, &jar, None);

    let cp = format!("{}:{}", out.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run protected[this] super probe");
    assert!(
        run.status.success(),
        "protected[this] super probe failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "profile/base\n");
    let _ = fs::remove_dir_all(root);
}
