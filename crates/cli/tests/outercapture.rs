//! Regression coverage for a lambda constructing an anonymous class whose
//! constructor-only initializer reads the enclosing receiver.
//!
//! The hidden \`$outer\` constructor parameter is part of the Scala ABI even
//! when scalac elides the anonymous class's physical \`$outer\` field. The
//! lambda must nevertheless capture and pass the receiver when that
//! constructor executes later.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn scala_library() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.is_file().then_some(path)
}

fn scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-outercapture-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn source(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.scala"))
}

fn compile_rs(name: &str, out: &Path, extra: &[&str]) -> Output {
    Command::new(bin())
        .args([
            "compile",
            source(name).to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .args(extra)
        .output()
        .expect("run scala-rs compile")
}

fn compile_scalac(name: &str, out: &Path, jar: &Path, scalac: &Path) -> Output {
    Command::new(scalac)
        .args([
            "-classpath",
            jar.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            source(name).to_str().unwrap(),
        ])
        .output()
        .expect("run scalac")
}

fn run_main(out: &Path, jar: Option<&Path>) -> Output {
    let cp = match jar {
        Some(jar) => format!("{}:{}", out.display(), jar.display()),
        None => out.display().to_string(),
    };
    Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run Main")
}

fn javap(out: &Path, class: &str) -> String {
    let output = Command::new("javap")
        .args(["-p", "-s", "-classpath", out.to_str().unwrap(), class])
        .output()
        .expect("run javap");
    assert!(
        output.status.success(),
        "javap {class} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("javap output is UTF-8")
}

fn assert_run(out: &Path, jar: Option<&Path>) {
    let output = run_main(out, jar);
    assert!(
        output.status.success(),
        "java -Xverify:all failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "outer-capture\nconstant\n"
    );
}

#[test]
fn constructor_only_outer_capture_matches_scalac_runtime_and_abi() {
    let (Some(jar), Some(scalac)) = (scala_library(), scalac()) else {
        eprintln!("skip outer-capture regression: scala-library or scalac is unavailable");
        return;
    };

    let ours_private = tmp_dir("ours-private");
    let output = compile_rs("oc_outer_capture", &ours_private, &["--no-scala-library"]);
    assert!(
        output.status.success(),
        "scala-rs private-runtime compile failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_run(&ours_private, None);
    let _ = fs::remove_dir_all(&ours_private);

    let ours = tmp_dir("ours");
    let output = compile_rs(
        "oc_outer_capture",
        &ours,
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(
        output.status.success(),
        "scala-rs library compile failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_run(&ours, Some(&jar));

    let reference = tmp_dir("scalac");
    let output = compile_scalac("oc_outer_capture", &reference, &jar, &scalac);
    assert!(
        output.status.success(),
        "real scalac compile failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_run(&reference, Some(&jar));

    // Both compilers retain the hidden constructor argument required by the
    // source-level outer read, but neither needs a physical \`$outer\` field
    // because the read occurs only during eager initialization. The control
    // anonymous class has the same constructor ABI and receives null.
    for (name, out) in [("scala-rs", &ours), ("scalac", &reference)] {
        let anon = javap(out, "OcOuter$$anon$1");
        assert!(
            anon.contains("OcOuter$$anon$1(OcOuter, java.lang.String)"),
            "{name} lost the outer/capture constructor ABI:\n{anon}"
        );
        assert!(
            !anon.lines().any(|line| line.contains("$outer")),
            "{name} retained an unnecessary $outer field:\n{anon}"
        );

        let control = javap(out, "OcOuter$$anon$2");
        assert!(
            control.contains("OcOuter$$anon$2(OcOuter)"),
            "{name} changed the no-capture constructor ABI:\n{control}"
        );
        assert!(
            !control.lines().any(|line| line.contains("$outer")),
            "{name} over-retained a no-capture $outer field:\n{control}"
        );
    }

    let _ = fs::remove_dir_all(&ours);
    let _ = fs::remove_dir_all(&reference);
}

#[test]
fn constructor_only_outer_capture_negative_is_rejected_by_both_compilers() {
    let Some(jar) = scala_library() else {
        eprintln!("skip outer-capture negative: scala-library is unavailable");
        return;
    };
    let Some(scalac) = scalac() else {
        eprintln!("skip outer-capture negative: scalac is unavailable");
        return;
    };

    let ours = tmp_dir("bad-ours");
    let output = compile_rs(
        "oc_outer_capture_bad",
        &ours,
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(
        !output.status.success(),
        "scala-rs accepted an unresolved capture: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let reference = tmp_dir("bad-scalac");
    let output = compile_scalac("oc_outer_capture_bad", &reference, &jar, &scalac);
    assert!(
        !output.status.success(),
        "real scalac accepted an unresolved capture"
    );

    let _ = fs::remove_dir_all(ours);
    let _ = fs::remove_dir_all(reference);
}
