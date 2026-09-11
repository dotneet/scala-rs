//! Inference roots behind the last typelevel/cats core errors
//! (`tests/cats_measure.sh`, 27 at `9cac778e`), each reduced to a standalone
//! program and checked against scalac 2.13.16.
//!
//! `catsr_infer.scala` runs, and must print what scalac's build prints;
//! `catsr_bad.scala` must be rejected on exactly the lines marked
//! `// error`, which are the lines scalac rejects.
//!
//! Fixture prefix: `catsr_`.

use std::collections::BTreeSet;
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
        "scala-rs-catsr-{tag}-{}-{nanos}-{seq}",
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

/// Compile `src` into `out` with scalac (`Some`) or scala-rs (`None`).
fn compile(src: &Path, out: &Path, jar: &Path, scalac_path: Option<&Path>) -> std::process::Output {
    match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    }
}

fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(&src, &out, &jar, scalac_path);
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let cp = format!("{}:{}", out.display(), jar.display());
    let run = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        run.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// The lines of `name` that end in `// error`.
fn marked_lines(name: &str) -> BTreeSet<usize> {
    let src = fs::read_to_string(fixtures_dir().join(format!("{name}.scala"))).unwrap();
    src.lines()
        .enumerate()
        .filter(|(_, l)| l.trim_end().ends_with("// error"))
        .map(|(i, _)| i + 1)
        .collect()
}

/// The lines of `name` a compiler reported an error on.
fn error_lines(name: &str, scalac_path: Option<&Path>) -> Option<BTreeSet<usize>> {
    let jar = scala_library_jar()?;
    let src = fixtures_dir().join(format!("{name}.scala"));
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(&src, &out, &jar, scalac_path);
    assert!(!output.status.success(), "{name} compiled");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let file = format!("{name}.scala:");
    let mut lines = BTreeSet::new();
    let mut after_error = scalac_path.is_some();
    for l in text.lines() {
        if scalac_path.is_none() {
            // scala-rs prints `error: …` and then `  --> path:line:col`.
            if l.starts_with("error") {
                after_error = true;
                continue;
            }
            if !after_error {
                continue;
            }
        } else if !l.contains(": error:") {
            continue;
        }
        if let Some(at) = l.find(&file) {
            let rest = &l[at + file.len()..];
            let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = n.parse() {
                lines.insert(n);
            }
            if scalac_path.is_none() {
                after_error = false;
            }
        }
    }
    let _ = fs::remove_dir_all(&dir);
    Some(lines)
}

#[test]
fn catsr_infer_runs() {
    check_runs("catsr_infer", None);
}

#[test]
fn scalac_agrees_catsr_infer() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("catsr_infer", Some(&sc));
}

#[test]
fn catsr_bad_is_rejected_on_the_marked_lines() {
    let Some(got) = error_lines("catsr_bad", None) else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    assert_eq!(got, marked_lines("catsr_bad"));
}

#[test]
fn scalac_agrees_catsr_bad() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let Some(got) = error_lines("catsr_bad", Some(&sc)) else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    assert_eq!(got, marked_lines("catsr_bad"));
}
