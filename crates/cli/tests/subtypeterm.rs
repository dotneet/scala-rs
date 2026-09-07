//! That `SymbolTable::is_sub_type` terminates in reasonable time on a deep,
//! diamond-dense hierarchy — and that it still answers the same.
//!
//! The walk over a class's parents was bounded by a depth counter alone. **A
//! depth bound bounds the depth of the recursion tree, not its size.** With two
//! parents per node the tree is `2^depth`, and `any()` short-circuits only on
//! `true`, so a `false` — the answer overload resolution and implicit search
//! ask for most of the time — took every path.
//!
//! The shape that reaches it needs no cycle at all:
//!
//! ```text
//! trait A(n) extends A(n-1) with B(n-1)
//! trait B(n) extends A(n-1) with B(n-1)
//! ```
//!
//! is legal, acyclic Scala that scalac 2.13.16 compiles in about two seconds.
//! Before the fix this compiler took 4 s at 22 levels, 19 s at 24 and 74 s at
//! 26, doubling with every level added; the depth bound of 200 never fired,
//! because the depth is only 26. `linearize`'s guard cannot help here either —
//! it stops a walk re-entering a class it is *already* visiting, and nothing
//! here is re-entered. What the walk needed was a memo, so that the same
//! question reached down a different path is answered once.
//!
//! A termination guard is cheap to get wrong in the other direction: this one
//! buys termination by answering `false` for a re-entrant question, and a lost
//! `true` changes overload and implicit selection with no diagnostic at all.
//! So `tests/fixtures/subtypeterm_diamond.scala` is *run*, and what it prints
//! is compared against real scalac's own output in the e2e sweep.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
        "scala-rs-subtypeterm-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

/// Compile `args`, giving up after `secs`. `None` means it did not finish.
fn compile_within(args: &[&str], secs: u64) -> Option<(Duration, String)> {
    let out = tmp_dir("within");
    let mut child = Command::new(bin())
        .args(args)
        .args(["-d", out.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn scala-rs");
    let start = Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None if start.elapsed() > Duration::from_secs(secs) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_dir_all(&out);
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let elapsed = start.elapsed();
    let done = child.wait_with_output().expect("wait_with_output");
    let _ = fs::remove_dir_all(&out);
    Some((
        elapsed,
        format!(
            "{}{}",
            String::from_utf8_lossy(&done.stderr),
            String::from_utf8_lossy(&done.stdout)
        ),
    ))
}

/// `levels` levels of the doubling hierarchy above, ending in a question whose
/// answer is `false` and whose right-hand side is not a class — the
/// class-against-class "no" is already answered linearly by `class_reaches`, so
/// asking one would not exercise the walk.
fn diamond_source(levels: u32) -> String {
    let mut s = String::from("trait A0\ntrait B0\n");
    for i in 1..=levels {
        s.push_str(&format!("trait A{i} extends A{} with B{}\n", i - 1, i - 1));
        s.push_str(&format!("trait B{i} extends A{} with B{}\n", i - 1, i - 1));
    }
    s.push_str("object Main {\n  def main(args: Array[String]): Unit = {\n");
    s.push_str(&format!("    val x: A{levels} = null\n"));
    s.push_str("    val d: Double = x\n    println(d)\n  }\n}\n");
    s
}

/// 34 levels: `2^34` paths, which before the fix was an extrapolated five
/// hours. Ten seconds is two orders of magnitude more than it now takes, and a
/// regression fails this test instead of hanging the suite.
#[test]
fn deep_diamond_hierarchy_terminates() {
    let dir = tmp_dir("deep");
    let src = dir.join("Deep.scala");
    fs::write(&src, diamond_source(34)).unwrap();
    let path = src.to_str().unwrap().to_string();
    let Some((elapsed, diags)) = compile_within(&["compile", &path, "--no-scala-library"], 60)
    else {
        panic!("compiling a 34-level diamond hierarchy did not terminate within 60s");
    };
    assert!(
        elapsed < Duration::from_secs(10),
        "a 34-level diamond hierarchy took {elapsed:?}; the parent walk is \
         exponential again"
    );
    assert!(
        diags.contains("type mismatch"),
        "expected the one type mismatch, got:\n{diags}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The cost must grow with the *size* of the hierarchy, not with `2^depth`.
/// Eight more levels is 256x the paths and 1.3x the traits, so a walk that is
/// still exponential shows up here as a blown budget even if the absolute
/// numbers on a loaded machine are noisy.
#[test]
fn diamond_cost_does_not_double_per_level() {
    let dir = tmp_dir("scale");
    let mut timings = Vec::new();
    for levels in [22u32, 30] {
        let src = dir.join(format!("D{levels}.scala"));
        fs::write(&src, diamond_source(levels)).unwrap();
        let path = src.to_str().unwrap().to_string();
        let Some((elapsed, _)) = compile_within(&["compile", &path, "--no-scala-library"], 120)
        else {
            panic!("compiling a {levels}-level diamond hierarchy did not terminate within 120s");
        };
        timings.push((levels, elapsed));
    }
    let (_, small) = timings[0];
    let (_, big) = timings[1];
    // Pre-fix this ratio was ~256 -- 30 levels took an extrapolated 17 minutes,
    // so pre-fix this test does not reach the assertion at all, it times out
    // above. That makes the generous constant here free: it is only guarding
    // against a milder regression, and a loaded machine must not fail it.
    assert!(
        big < small * 20 + Duration::from_secs(5),
        "22 levels took {small:?} but 30 levels took {big:?}; the parent walk \
         still looks exponential in the depth"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The 22-level fixture is rejected for exactly the reason scalac rejects it,
/// and for no other: the guard must not invent errors by losing a `true`.
#[test]
fn diamond_bad_fixture_matches_scalac_error_count() {
    let src = fixtures_dir().join("subtypeterm_diamond_bad.scala");
    let path = src.to_str().unwrap().to_string();
    let Some((_, diags)) = compile_within(&["compile", &path, "--no-scala-library"], 60) else {
        panic!("compiling subtypeterm_diamond_bad did not terminate within 60s");
    };
    // scalac 2.13.16 on this file:
    //   subtypeterm_diamond_bad.scala:67: error: type mismatch;
    //    found   : A22
    //    required: Double
    //   1 error
    assert_eq!(
        diags.matches("error:").count(),
        1,
        "expected exactly the one error scalac reports, got:\n{diags}"
    );
    assert!(
        diags.contains("type mismatch"),
        "expected a type mismatch, got:\n{diags}"
    );
    assert!(
        diags.contains("subtypeterm_diamond_bad.scala:67:"),
        "expected the mismatch at line 67, as scalac reports it, got:\n{diags}"
    );
}
