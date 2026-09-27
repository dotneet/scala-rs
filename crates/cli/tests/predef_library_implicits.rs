//! Implicits of the library's `Predef` that the hand-written prelude does not
//! declare: `tuple2ToZippedOps` / `tuple3ToZippedOps` and the implicit classes
//! `Ensuring` and `StringFormat`. `(xs, ys).zipped`, `x.ensuring(p)` and
//! `d.formatted(f)` were "not a member"; the zipped view's own
//! `List[Int] => IterableOps[El1, Iterable, It1] with It1` then needed `List`'s
//! pickled parents to be answered by the identity.
//!
//! The program is compiled by scalac and by scala-rs and run; the output has
//! to agree.

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
        "scala-rs-predef-library-implicits-{tag}-{}-{nanos}-{seq}",
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

fn run_java(out: &Path, cp: &str, main: &str) -> String {
    let o = Command::new("java")
        .args([
            "-Xverify:all",
            "-cp",
            &format!("{}:{cp}", out.display()),
            main,
        ])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "{main} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Compile `fixture` with scala-rs (and with scalac when it is present)
/// against `cp`, run `main` from both, and compare the output with each
/// other and with `expected/<fixture>.txt`.
fn compiles_and_runs_like_scalac(fixture: &str, cp: Option<&Path>, main: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not present");
        return;
    };
    let src = fixtures_dir().join(format!("{fixture}.scala"));
    let ours = tmp_dir("ours");
    let mut cmd = Command::new(bin());
    cmd.arg("compile").arg(&src).arg("-d").arg(&ours);
    if let Some(cp) = cp {
        cmd.arg("-cp").arg(cp);
    }
    let o = cmd
        .arg("--scala-library")
        .arg(&jar)
        .output()
        .expect("run scala-rs compile");
    assert!(
        o.status.success(),
        "scala-rs rejected {fixture}: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let run_cp = match cp {
        Some(cp) => format!("{}:{}", cp.display(), jar.display()),
        None => jar.display().to_string(),
    };
    let actual = run_java(&ours, &run_cp, main);
    let _ = fs::remove_dir_all(&ours);
    let expected =
        fs::read_to_string(fixtures_dir().join(format!("expected/{fixture}.txt"))).unwrap();
    if let Some(scalac) = scalac() {
        let theirs = tmp_dir("scalac");
        let mut cmd = Command::new(scalac);
        cmd.env("JAVA_OPTS", "-Xmx2g -Xss8m");
        if let Some(cp) = cp {
            cmd.arg("-cp").arg(cp);
        }
        let o = cmd
            .arg("-d")
            .arg(&theirs)
            .arg(&src)
            .output()
            .expect("run scalac");
        assert!(o.status.success(), "scalac rejected {fixture}");
        assert_eq!(run_java(&theirs, &run_cp, main), expected);
        let _ = fs::remove_dir_all(&theirs);
    }
    assert_eq!(actual, expected);
}

#[test]
fn predef_zipped_ensuring_and_formatted_run_like_scalac() {
    compiles_and_runs_like_scalac("predef_library_implicits", None, "Main");
}
