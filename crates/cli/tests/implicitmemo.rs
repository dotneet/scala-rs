//! Implicit search is memoized (`ImplicitMemo` in
//! `crates/typer/src/implicits.rs`). These tests are about the answers, not the
//! speed: a memo is only worth anything if the compiler cannot tell it is
//! there.
//!
//! Three things have to survive it, and each is a separate way the key could
//! have been wrong.
//!
//! 1. **The same wanted type reached twice in one derivation** has one answer,
//!    at whatever depth it is reached. `im_memo` asks for `Show[Int]` under a
//!    list, under an option, under both halves of a pair, and eight
//!    constructors deep.
//! 2. **The memo cannot outlive one search.** The candidates it was filled from
//!    are the ones in scope at that point, and a nearer binding of the same
//!    name is a different answer to the same question. `im_memo`'s `shadowed`
//!    is that, and it is checked by running the class files rather than by
//!    compiling them: a stale memo picks the wrong witness, which type-checks
//!    perfectly and prints `i1` where scalac prints `I1`.
//! 3. **A cut-off is not an answer that generalises.** `im_memo_bad` derives
//!    `Box[A]` from `Box[A]`, which nsc's `openImplicits` rule stops; that rule
//!    reads state the key does not carry, which is why an entry is only stored
//!    and reused where the open stack cannot have decided it. The same fixture
//!    checks that an ambiguity still comes back as an ambiguity -- a third
//!    result the memo has to carry besides "found" and "not found".
//!
//! Both fixtures are checked against real scalac 2.13.16: `im_memo`'s recorded
//! stdout is what nsc's own class files print, and `im_memo_bad` is rejected by
//! nsc for the same two reasons.

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
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-implicitmemo-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn compile(out: &Path, name: &str, extra: &[&str]) -> (bool, String) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let output = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .args(["-d", out.to_str().unwrap()])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

fn run_main(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ---------------------------------------------------------------- im_memo

/// The witnesses the memo hands back are the ones that have to *run*: picking
/// the wrong `Show[Int]` compiles either way and only differs in the output.
#[test]
fn im_memo_runs_against_the_jar() {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip im_memo: jar or java not present");
        return;
    };
    let out = tmp_dir("im_memo");
    let (ok, msgs) = compile(&out, "im_memo", &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "compile im_memo failed:\n{msgs}");
    assert_eq!(
        run_main(&out, &jar),
        expected_stdout("im_memo"),
        "im_memo picked different witnesses than scalac"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The expectation has to be what real scalac 2.13.16 prints, not what this
/// compiler happened to print when the fixture was written.
#[test]
fn im_memo_matches_real_scalac() {
    let (Some(scalac), Some(jar), true) = (find_scalac(), scala_library_jar(), java_available())
    else {
        eprintln!("skip im_memo real-scalac diff: scalac, jar or java not present");
        return;
    };
    let src = fixtures_dir().join("im_memo.scala");
    let out = tmp_dir("im_memo-nsc");
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile im_memo");
    assert_eq!(
        run_main(&out, &jar),
        expected_stdout("im_memo"),
        "recorded expectation for im_memo does not match real scalac"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------ im_memo_bad

#[test]
fn im_memo_bad_still_diverges_and_is_ambiguous() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip im_memo_bad: jar not present");
        return;
    };
    let out = tmp_dir("im_memo_bad");
    let (ok, msgs) = compile(
        &out,
        "im_memo_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected im_memo_bad to be rejected:\n{msgs}");
    assert!(
        msgs.contains("diverging implicit expansion") && msgs.contains("loop"),
        "im_memo_bad lost the divergence diagnostic:\n{msgs}"
    );
    assert!(
        msgs.contains("ambiguous implicit") && msgs.contains("t1") && msgs.contains("t2"),
        "im_memo_bad lost the ambiguity diagnostic:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn im_memo_bad_is_rejected_by_real_scalac_too() {
    let Some(scalac) = find_scalac() else {
        eprintln!("skip im_memo_bad real-scalac check: scalac not present");
        return;
    };
    let out = tmp_dir("im_memo_bad-nsc");
    let output = Command::new(&scalac)
        .args([
            fixtures_dir().join("im_memo_bad.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("scalac");
    let msgs = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !output.status.success(),
        "real scalac accepted im_memo_bad:\n{msgs}"
    );
    assert!(
        msgs.contains("diverging implicit expansion") && msgs.contains("ambiguous implicit"),
        "real scalac rejected im_memo_bad for other reasons:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}
